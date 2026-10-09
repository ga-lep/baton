//! Update-check logic: version comparison, release parsing, cache, enablement.
//!
//! Pure library code: no network access lives here.

use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::path::Path;

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
        Err(e) => return Outcome::Unknown(format!("bad current version \"{current}\": {e}")),
    };
    let latest = match semver::Version::parse(&strip(latest_tag)) {
        Ok(v) => v,
        Err(e) => {
            return Outcome::Unknown(format!("unrecognised release tag \"{latest_tag}\": {e}"));
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
    serde_json::from_str(body)
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
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Atomically writes the cache (temp file plus rename), creating the parent dir.
    ///
    /// # Errors
    /// On any I/O or serialisation failure.
    pub fn store(&self, path: &Path) -> std::io::Result<()> {
        let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
        if let Some(dir) = dir {
            std::fs::create_dir_all(dir)?;
        }
        let mut tmp = tempfile_in(dir.unwrap_or(Path::new(".")), path)?;
        let json = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        tmp.0.write_all(&json)?;
        tmp.0.sync_all()?;
        std::fs::rename(&tmp.1, path)
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

/// Creates a uniquely named temp file next to `target`.
fn tempfile_in(dir: &Path, target: &Path) -> std::io::Result<(std::fs::File, std::path::PathBuf)> {
    let name = target
        .file_name()
        .map_or_else(|| "cache".into(), |n| n.to_string_lossy().into_owned());
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)?;
    Ok((f, tmp))
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
            html_url: ok.then(|| "http://x/r".to_owned()),
            etag: Some("\"abc\"".into()),
            ok,
        }
    }

    #[test]
    fn cache_round_trips_and_creates_parent() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a/b/update-check.json");
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
        std::fs::write(&p, "").unwrap();
        assert_eq!(Cache::load(&p), None);
        std::fs::write(&p, "{ nope").unwrap();
        assert_eq!(Cache::load(&p), None);
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
