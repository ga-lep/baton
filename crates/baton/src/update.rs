//! Update check: an HTTP fetcher for the latest GitHub release and the
//! cache-aware entry point built on `baton_core::update`.

use baton_core::paths;
use baton_core::update::{Cache, Outcome, compare, parse_release};
use std::io::Read as _;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Default endpoint; overridable with `BATON_UPDATE_URL` (used by tests).
const DEFAULT_URL: &str = "https://api.github.com/repos/ga-lep/baton/releases/latest";
/// Largest response body accepted.
const MAX_BODY: u64 = 1024 * 1024;

/// Timeout for the background check of the TUI.
pub const TUI_TIMEOUT: Duration = Duration::from_secs(2);

/// Timeout for explicit checks and `baton doctor`.
pub const CLI_TIMEOUT: Duration = Duration::from_secs(3);

/// Result of one HTTP request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    /// A fresh release description.
    Release {
        /// Release tag, e.g. `v0.3.0`.
        tag: String,
        /// Release page.
        html_url: Option<String>,
        /// Response `ETag`.
        etag: Option<String>,
    },
    /// `304 Not Modified`: the cached release is still current.
    NotModified,
}

/// A completed check: the comparison plus the release page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// Comparison of the running version with the latest release.
    pub outcome: Outcome,
    /// Release page, when known.
    pub html_url: Option<String>,
}

/// Performs one unauthenticated `GET` of the latest release.
///
/// # Errors
/// A one-line, user-readable reason.
pub fn fetch(url: &str, timeout: Duration, etag: Option<&str>) -> Result<Fetched, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .https_only(https_only(url))
        .max_redirects(3)
        .http_status_as_error(false)
        .build()
        .into();
    let mut req = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", format!("baton/{}", env!("CARGO_PKG_VERSION")));
    if let Some(etag) = etag {
        req = req.header("If-None-Match", etag);
    }
    let mut resp = req.call().map_err(|e| e.to_string())?;
    match resp.status().as_u16() {
        200 => {}
        304 => return Ok(Fetched::NotModified),
        404 => return Err("no public release found".into()),
        403 | 429 => return Err("rate limited".into()),
        code => return Err(format!("HTTP {code}")),
    }
    let etag = resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .and_then(baton_core::update::sanitize_etag);
    let mut body = String::new();
    resp.body_mut()
        .as_reader()
        .take(MAX_BODY + 1)
        .read_to_string(&mut body)
        .map_err(|e| format!("reading response: {e}"))?;
    if body.len() as u64 > MAX_BODY {
        return Err("response too large".into());
    }
    let release = parse_release(&body).map_err(|e| format!("bad response: {e}"))?;
    Ok(Fetched::Release {
        tag: release.tag_name,
        html_url: release.html_url,
        etag,
    })
}

/// Whether `url` must be https: always, except for plain-http loopback
/// endpoints (`127.0.0.1` / `localhost`, optionally with a port), used by tests.
fn https_only(url: &str) -> bool {
    ["http://127.0.0.1", "http://localhost"].iter().all(|p| {
        url.strip_prefix(p)
            .is_none_or(|rest| !(rest.is_empty() || rest.starts_with([':', '/'])))
    })
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn from_cache(cache: &Cache) -> Result<Checked, String> {
    match &cache.latest {
        Some(tag) => Ok(Checked {
            outcome: compare(env!("CARGO_PKG_VERSION"), tag),
            html_url: cache.html_url.clone(),
        }),
        None => Err("no release information cached".into()),
    }
}

/// Checks for a newer release, using the cache unless `force` is set.
///
/// A fresh cache is answered without any network access; a fresh failed
/// check yields an error without retrying. The cache is updated after a
/// network attempt (write failures are ignored).
///
/// # Errors
/// A one-line, user-readable reason.
pub fn check(force: bool, timeout: Duration) -> Result<Checked, String> {
    let path = paths::update_cache_path().map_err(|e| format!("no state dir: {e}"))?;
    // Create/validate the state dir like the daemon does; if it is unusable
    // the cache is simply not written.
    let dir_ok = paths::ensure_state_dir().is_ok();
    let now = now_secs();
    let cached = Cache::load(&path);
    if !force && let Some(c) = cached.as_ref().filter(|c| c.is_fresh(now)) {
        return if c.ok {
            from_cache(c)
        } else {
            Err("a recent check failed; retrying later".into())
        };
    }
    let url = std::env::var("BATON_UPDATE_URL")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| DEFAULT_URL.to_owned());
    let etag = cached
        .as_ref()
        .filter(|c| c.ok)
        .and_then(|c| c.etag.as_deref());
    let result = fetch(&url, timeout, etag).and_then(|f| match f {
        Fetched::Release {
            tag,
            html_url,
            etag,
        } => Ok(Cache {
            checked_at: now,
            latest: Some(tag),
            html_url,
            etag,
            ok: true,
        }),
        Fetched::NotModified => cached
            .clone()
            .filter(|c| c.ok && c.latest.is_some())
            .map(|c| Cache {
                checked_at: now,
                ..c
            })
            .ok_or_else(|| "unexpected 304 without a cached release".to_owned()),
    });
    match result {
        Ok(cache) => {
            if dir_ok {
                let _ = cache.store(&path);
            }
            from_cache(&cache)
        }
        Err(reason) => {
            if dir_ok {
                let _ = Cache {
                    checked_at: now,
                    latest: None,
                    html_url: None,
                    etag: None,
                    ok: false,
                }
                .store(&path);
            }
            Err(reason)
        }
    }
}

/// What the TUI knows about updates at startup.
#[derive(Debug)]
pub struct Startup {
    /// A newer version (without the leading `v`) known from the cache.
    pub available: Option<String>,
    /// Result of a background fetch, when the cache was stale.
    pub pending: Option<tokio::sync::oneshot::Receiver<Option<String>>>,
}

/// Reads the cache (a tiny file) and, when it is stale, starts a detached
/// background fetch. Does nothing when automatic checks are disabled.
pub fn startup() -> Startup {
    let none = Startup {
        available: None,
        pending: None,
    };
    let flag = crate::cmd::version::config_flag();
    if !baton_core::update::enabled(flag, &|k| std::env::var(k).ok()) {
        return none;
    }
    let Ok(path) = paths::update_cache_path() else {
        return none;
    };
    let cached = Cache::load(&path);
    let available = cached
        .as_ref()
        .filter(|c| c.ok)
        .and_then(|c| from_cache(c).ok())
        .and_then(|c| newer(&c.outcome));
    let fresh = cached.is_some_and(|c| c.is_fresh(now_secs()));
    Startup {
        available,
        pending: (!fresh).then(|| spawn_background(TUI_TIMEOUT)),
    }
}

fn newer(outcome: &Outcome) -> Option<String> {
    match outcome {
        Outcome::Newer { latest } => Some(latest.clone()),
        Outcome::UpToDate | Outcome::Unknown(_) => None,
    }
}

/// Runs [`check`] on a detached thread; the receiver yields the newer
/// version, if any. Errors are only logged: the TUI owns the terminal.
pub fn spawn_background(timeout: Duration) -> tokio::sync::oneshot::Receiver<Option<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let found = match check(false, timeout) {
            Ok(c) => newer(&c.outcome),
            Err(e) => {
                tracing::debug!("update check failed: {e}");
                None
            }
        };
        let _ = tx.send(found);
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_policy() {
        assert!(https_only("https://api.github.com/x"));
        assert!(https_only("http://example.com/x"));
        assert!(https_only("http://127.0.0.1.evil.com/x"));
        assert!(https_only("http://localhost.evil.com/x"));
        assert!(https_only("http://user@127.0.0.1/x"));
        assert!(!https_only("http://127.0.0.1:8080/x"));
        assert!(!https_only("http://127.0.0.1/x"));
        assert!(!https_only("http://localhost:1/x"));
        assert!(!https_only("http://localhost"));
    }
}
