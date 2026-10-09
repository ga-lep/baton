//! Update-check logic: version comparison, release parsing, cache, enablement.
//!
//! Pure library code: no network access lives here.

use serde::{Deserialize, Serialize};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;

/// Longest accepted release URL.
const MAX_URL: usize = 200;
/// Longest accepted `ETag`.
const MAX_ETAG: usize = 200;
/// Largest cache file read.
const MAX_CACHE_BYTES: u64 = 64 * 1024;
/// Longest tag echoed in messages.
const MAX_TAG_SHOWN: usize = 40;

fn visible_ascii(s: &str) -> bool {
    s.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

/// Accepts a release URL only if it is a short, printable-ASCII `https://github.com/` URL.
pub fn sanitize_url(url: &str) -> Option<String> {
    (url.starts_with("https://github.com/") && url.len() <= MAX_URL && visible_ascii(url))
        .then(|| url.to_owned())
}

/// Accepts an `ETag` only if it is non-empty, short, visible ASCII.
pub fn sanitize_etag(etag: &str) -> Option<String> {
    (!etag.is_empty() && etag.len() <= MAX_ETAG && visible_ascii(etag)).then(|| etag.to_owned())
}

/// Makes untrusted text safe to print: control characters become `?` and
/// the result is capped in length.
fn printable(s: &str) -> String {
    let mut out: String = s
        .chars()
        .take(MAX_TAG_SHOWN)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    if s.chars().count() > MAX_TAG_SHOWN {
        out.push_str("...");
    }
    out
}

/// Cache lifetime after a successful check.
pub const OK_TTL_SECS: u64 = 24 * 60 * 60;
/// Cache lifetime after a failed check.
pub const FAIL_TTL_SECS: u64 = 6 * 60 * 60;

/// Result of comparing the running version with the latest release tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Running version is the latest, or newer.
    UpToDate,
    /// A newer release exists.
    Newer {
        /// The newer version, without a leading `v`.
        latest: String,
    },
    /// The versions could not be compared.
    Unknown(String),
}

/// Compares `current` with `latest_tag` (a leading `v` is stripped) under semver ordering.
pub fn compare(current: &str, latest_tag: &str) -> Outcome {
    let strip = |s: &str| s.strip_prefix('v').unwrap_or(s).to_owned();
    let cur = match semver::Version::parse(&strip(current)) {
        Ok(v) => v,
        Err(e) => {
            return Outcome::Unknown(format!(
                "bad current version \"{}\": {e}",
                printable(current)
            ));
        }
    };
    let latest = match semver::Version::parse(&strip(latest_tag)) {
        Ok(v) => v,
        Err(e) => {
            return Outcome::Unknown(format!(
                "unrecognised release tag \"{}\": {}",
                printable(latest_tag),
                printable(&e.to_string())
            ));
        }
    };
    if latest > cur {
        Outcome::Newer {
            latest: latest.to_string(),
        }
    } else {
        Outcome::UpToDate
    }
}

/// The fields of a GitHub release response that Baton uses.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    /// Git tag of the release, e.g. `v0.3.0`.
    pub tag_name: String,
    /// Web page of the release.
    #[serde(default)]
    pub html_url: Option<String>,
}

/// Parses a GitHub "latest release" response body; other fields are ignored.
///
/// # Errors
/// If the body is not JSON or lacks `tag_name`.
pub fn parse_release(body: &str) -> Result<Release, serde_json::Error> {
    let mut r: Release = serde_json::from_str(body)?;
    r.html_url = r.html_url.as_deref().and_then(sanitize_url);
    Ok(r)
}

/// On-disk record of the last update check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cache {
    /// Unix seconds of the check.
    pub checked_at: u64,
    /// Latest release tag, if the check succeeded.
    pub latest: Option<String>,
    /// Release page URL.
    pub html_url: Option<String>,
    /// `ETag` of the response, for conditional requests.
    pub etag: Option<String>,
    /// Whether the check succeeded.
    pub ok: bool,
}

impl Cache {
    /// Loads the cache; a missing, empty or corrupt file yields `None`.
    pub fn load(path: &Path) -> Option<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(path)
            .ok()?;
        let meta = file.metadata().ok()?;
        if !meta.is_file() || meta.len() > MAX_CACHE_BYTES {
            return None;
        }
        let mut text = String::new();
        file.take(MAX_CACHE_BYTES + 1)
            .read_to_string(&mut text)
            .ok()?;
        if text.len() as u64 > MAX_CACHE_BYTES {
            return None;
        }
        let mut c: Self = serde_json::from_str(&text).ok()?;
        c.html_url = c.html_url.as_deref().and_then(sanitize_url);
        c.etag = c.etag.as_deref().and_then(sanitize_etag);
        Some(c)
    }

    /// Atomically writes the cache (private temp file plus rename) into the
    /// existing parent directory, which the caller must have prepared.
    ///
    /// The temp file is created exclusively with mode 0600 and a random
    /// name, and is removed if any step fails.
    ///
    /// # Errors
    /// On any I/O or serialisation failure.
    pub fn store(&self, path: &Path) -> std::io::Result<()> {
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let json = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
        tmp.write_all(&json)?;
        tmp.as_file().sync_all()?;
        tmp.persist(path).map_err(|e| e.error)?;
        Ok(())
    }

    /// Whether the cached result is recent enough to skip a new check at `now` (unix secs).
    pub fn is_fresh(&self, now: u64) -> bool {
        if self.checked_at > now {
            return false;
        }
        let ttl = if self.ok { OK_TTL_SECS } else { FAIL_TTL_SECS };
        now - self.checked_at < ttl
    }
}

/// Whether automatic update checks are enabled.
///
/// False when `config_flag` is false or `BATON_NO_UPDATE_CHECK` is set to a
/// non-empty value other than `0`.
pub fn enabled(config_flag: bool, getenv: &dyn Fn(&str) -> Option<String>) -> bool {
    if !config_flag {
        return false;
    }
    !matches!(getenv("BATON_NO_UPDATE_CHECK"), Some(v) if !v.is_empty() && v != "0")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn newer(v: &str) -> Outcome {
        Outcome::Newer { latest: v.into() }
    }

    #[test]
    fn compare_table() {
        let cases = [
            ("0.1.0", "v0.2.0", newer("0.2.0")),
            ("0.2.0", "v0.2.0", Outcome::UpToDate),
            ("0.2.0", "0.2.0", Outcome::UpToDate),
            ("0.3.0-rc.1", "v0.2.0", Outcome::UpToDate),
            ("0.2.0", "v0.3.0-rc.1", newer("0.3.0-rc.1")),
            ("0.3.0-rc.1", "v0.3.0", newer("0.3.0")),
        ];
        for (cur, tag, want) in cases {
            assert_eq!(compare(cur, tag), want, "{cur} vs {tag}");
        }
    }

    #[test]
    fn compare_unknown() {
        assert!(matches!(compare("0.2.0", "nightly"), Outcome::Unknown(_)));
        assert!(matches!(compare("junk", "v1.0.0"), Outcome::Unknown(_)));
    }

    const REAL: &str = r#"{
      "url": "https://api.github.com/repos/ga-lep/baton/releases/1",
      "html_url": "https://github.com/ga-lep/baton/releases/tag/v0.2.0",
      "id": 1, "tag_name": "v0.2.0", "draft": false, "prerelease": false,
      "assets": [{"name": "baton.tar.gz", "size": 3}], "body": "notes"
    }"#;

    #[test]
    fn parse_release_extracts_fields() {
        let r = parse_release(REAL).unwrap();
        assert_eq!(r.tag_name, "v0.2.0");
        assert_eq!(
            r.html_url.as_deref(),
            Some("https://github.com/ga-lep/baton/releases/tag/v0.2.0")
        );
    }

    #[test]
    fn parse_release_requires_tag_name() {
        assert!(parse_release(r#"{"html_url":"x"}"#).is_err());
        assert!(parse_release("not json").is_err());
    }

    fn cache(checked_at: u64, ok: bool) -> Cache {
        Cache {
            checked_at,
            latest: ok.then(|| "v0.3.0".to_owned()),
            html_url: ok.then(|| "https://github.com/ga-lep/baton/releases/tag/v0.3.0".to_owned()),
            etag: Some("\"abc\"".into()),
            ok,
        }
    }

    #[test]
    fn cache_round_trips_and_leaves_no_temp_files() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("update-check.json");
        let c = cache(1000, true);
        c.store(&p).unwrap();
        assert_eq!(Cache::load(&p), Some(c));
        let leftovers: Vec<_> = std::fs::read_dir(p.parent().unwrap()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "temp file must be renamed away");
    }

    #[test]
    fn cache_load_tolerates_bad_files() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.json");
        assert_eq!(Cache::load(&p), None);
        std::fs::write(&p, vec![b' '; 70 * 1024]).unwrap();
        assert_eq!(Cache::load(&p), None, "oversized");
        std::fs::remove_file(&p).unwrap();
        std::fs::create_dir(&p).unwrap();
        assert_eq!(Cache::load(&p), None, "not a regular file");
        std::fs::remove_dir(&p).unwrap();
        std::fs::write(&p, "").unwrap();
        assert_eq!(Cache::load(&p), None);
        std::fs::write(&p, "{ nope").unwrap();
        assert_eq!(Cache::load(&p), None);
    }

    #[test]
    fn store_fails_without_parent_and_does_not_create_it() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("missing/c.json");
        assert!(cache(1, true).store(&p).is_err());
        assert!(!d.path().join("missing").exists());
    }

    #[test]
    fn store_cleans_up_temp_file_on_rename_failure() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.json");
        std::fs::create_dir(&p).unwrap(); // rename over a directory fails
        assert!(cache(1, true).store(&p).is_err());
        let names: Vec<_> = std::fs::read_dir(d.path()).unwrap().collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[test]
    fn store_uses_private_mode() {
        use std::os::unix::fs::PermissionsExt as _;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.json");
        cache(1, true).store(&p).unwrap();
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn url_validator() {
        let ok = "https://github.com/ga-lep/baton/releases/tag/v1.0.0";
        assert_eq!(sanitize_url(ok).as_deref(), Some(ok));
        for bad in [
            "http://github.com/x",
            "https://evil.example/x",
            "https://github.com/\x1b]8;;x\x07",
            "https://github.com/a b",
            "https://github.com/é",
            &format!("https://github.com/{}", "a".repeat(300)),
        ] {
            assert_eq!(sanitize_url(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn etag_validator() {
        assert_eq!(sanitize_etag("W/\"abc\"").as_deref(), Some("W/\"abc\""));
        for bad in ["", "a\x1bb", "a\nb", "é", &"a".repeat(300)] {
            assert_eq!(sanitize_etag(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn unknown_message_is_escaped_and_capped() {
        let Outcome::Unknown(m) = compare("0.2.0", "\x1b[2J\x1b]0;pwn\x07") else {
            panic!("expected unknown")
        };
        assert!(!m.chars().any(char::is_control), "{m:?}");
        let Outcome::Unknown(m) = compare("0.2.0", &"z".repeat(500)) else {
            panic!("expected unknown")
        };
        assert!(m.len() < 200, "{}", m.len());
    }

    #[test]
    fn parse_release_drops_bad_url() {
        let r = parse_release(r#"{"tag_name":"v1.0.0","html_url":"https://evil/x"}"#).unwrap();
        assert_eq!(r.html_url, None);
    }

    #[test]
    fn load_sanitises_old_cache_files() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.json");
        let bad = Cache {
            html_url: Some("https://github.com/\x1b[2J".into()),
            etag: Some("\x07".into()),
            ..cache(1, true)
        };
        std::fs::write(&p, serde_json::to_vec(&bad).unwrap()).unwrap();
        let c = Cache::load(&p).unwrap();
        assert_eq!((c.html_url, c.etag), (None, None));
    }

    #[test]
    fn freshness_rules() {
        let ok = cache(1000, true);
        assert!(ok.is_fresh(1000 + OK_TTL_SECS - 1));
        assert!(!ok.is_fresh(1000 + OK_TTL_SECS));
        let bad = cache(1000, false);
        assert!(bad.is_fresh(1000 + FAIL_TTL_SECS - 1));
        assert!(!bad.is_fresh(1000 + FAIL_TTL_SECS));
        assert!(!cache(2000, true).is_fresh(1000), "future is stale");
        assert!(!cache(2000, false).is_fresh(1000), "future is stale");
    }

    fn env<'a>(v: Option<&'a str>) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            (k == "BATON_NO_UPDATE_CHECK")
                .then(|| v.map(str::to_owned))
                .flatten()
        }
    }

    #[test]
    fn enabled_rules() {
        assert!(enabled(true, &env(None)));
        assert!(enabled(true, &env(Some(""))));
        assert!(enabled(true, &env(Some("0"))));
        assert!(!enabled(true, &env(Some("1"))));
        assert!(!enabled(true, &env(Some("yes"))));
        assert!(!enabled(false, &env(None)));
        assert!(!enabled(false, &env(Some("0"))));
    }
}
