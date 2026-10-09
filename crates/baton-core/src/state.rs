//! Persisted per-session metadata (`state.json`).
//!
//! The file lives in the private state directory but is still parsed as
//! untrusted input: it is read tolerantly, bounded in size, never followed
//! through a symlink, and every value that later reaches a command line is
//! validated.

use crate::launch::is_uuid;
use baton_proto::Status;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// Current `state.json` format version.
pub const VERSION: u32 = 1;
/// Largest `state.json` that is read; anything bigger is treated as corrupt.
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
/// Longest accepted key, project, repo, profile or path.
const MAX_FIELD: usize = 4096;

/// What is remembered about one session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedSession {
    pub project: String,
    pub repo: String,
    pub profile: Option<String>,
    pub claude_session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub last_status: Status,
    /// Unix timestamp, seconds.
    pub updated_at: u64,
}

/// The whole file: `{version, sessions: {<baton id>: {...}}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub version: u32,
    pub sessions: BTreeMap<String, PersistedSession>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            version: VERSION,
            sessions: BTreeMap::new(),
        }
    }
}

/// The result of [`load`].
#[derive(Debug, PartialEq)]
pub struct Loaded {
    /// The sanitized state (empty if the file was missing or unusable).
    pub state: State,
    /// Why the file was set aside, for the log; `None` when it was fine or
    /// simply absent.
    pub problem: Option<String>,
    /// Keys of entries that could not be parsed and were left out; the rest
    /// of the file is still used.
    pub dropped: Vec<String>,
}

/// The file as first parsed: entries stay raw so that a bad one only costs
/// itself.
#[derive(Deserialize)]
struct RawState {
    version: u32,
    sessions: BTreeMap<String, serde_json::Value>,
}

fn clean(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_FIELD && !s.chars().any(char::is_control)
}

fn clean_opt(s: Option<String>) -> Option<String> {
    s.filter(|v| clean(v))
}

impl State {
    /// Drops entries with unusable keys or text and clears fields that would
    /// be unsafe to use later: a `claude_session_id` that is not a UUID and
    /// a relative `transcript_path`.
    fn sanitize(&mut self) {
        self.sessions
            .retain(|id, e| clean(id) && clean(&e.project) && clean(&e.repo));
        for e in self.sessions.values_mut() {
            e.profile = clean_opt(e.profile.take());
            e.claude_session_id = e.claude_session_id.take().filter(|id| is_uuid(id));
            e.transcript_path =
                clean_opt(e.transcript_path.take()).filter(|p| Path::new(p).is_absolute());
        }
    }
}

fn read_checked(path: &Path) -> Result<Option<(State, Vec<String>)>, String> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot stat: {e}")),
    };
    if !meta.file_type().is_file() {
        return Err("not a regular file".into());
    }
    // O_NOFOLLOW closes the window between the check above and the open.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| format!("cannot open: {e}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read: {e}"))?;
    if bytes.len() > MAX_BYTES {
        return Err(format!("larger than {MAX_BYTES} bytes"));
    }
    let raw: RawState = serde_json::from_slice(&bytes).map_err(|e| format!("invalid JSON: {e}"))?;
    if raw.version != VERSION {
        return Err(format!("unsupported version {}", raw.version));
    }
    let mut state = State::default();
    let mut dropped = Vec::new();
    for (id, value) in raw.sessions {
        match serde_json::from_value::<PersistedSession>(value) {
            Ok(entry) => {
                state.sessions.insert(id, entry);
            }
            Err(_) => dropped.push(id),
        }
    }
    Ok(Some((state, dropped)))
}

/// Loads `path` tolerantly. A missing file gives an empty state. A file that
/// cannot be used is renamed to `<path>.bad-<now>` (best effort), reported in
/// [`Loaded::problem`], and an empty state is returned; this never fails.
pub fn load(path: &Path, now: u64) -> Loaded {
    match read_checked(path) {
        Ok(None) => Loaded {
            state: State::default(),
            problem: None,
            dropped: Vec::new(),
        },
        Ok(Some((mut state, dropped))) => {
            state.sanitize();
            Loaded {
                state,
                problem: None,
                dropped,
            }
        }
        Err(why) => {
            let mut aside = path.as_os_str().to_owned();
            aside.push(format!(".bad-{now}"));
            let aside = PathBuf::from(aside);
            let moved = match std::fs::rename(path, &aside) {
                Ok(()) => format!("moved to {}", aside.display()),
                Err(e) => format!("could not be moved aside: {e}"),
            };
            Loaded {
                state: State::default(),
                problem: Some(format!("{}: {why}; {moved}", path.display())),
                dropped: Vec::new(),
            }
        }
    }
}

/// Writes `state` to `path` atomically: a private (0600) temp file in the
/// same directory, fsync, then rename over the target.
///
/// # Errors
/// On any I/O error; the previous file is left in place.
pub fn save(path: &Path, state: &State) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(format!(".tmp.{}", std::process::id()));
    let tmp = PathBuf::from(tmp_name);
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let written = f.write_all(&json).and_then(|()| f.sync_all());
    drop(f);
    let result = written.and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result?;
    if let Some(dir) = path.parent()
        && let Ok(d) = std::fs::File::open(dir)
    {
        let _ = d.sync_all(); // best effort: makes the rename durable
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use baton_proto::Status;
    use std::os::unix::fs::PermissionsExt;

    const ID: &str = "123e4567-e89b-42d3-a456-426614174000";

    fn entry() -> PersistedSession {
        PersistedSession {
            project: "x".into(),
            repo: "/tmp/r".into(),
            profile: Some("p".into()),
            claude_session_id: Some(ID.into()),
            transcript_path: Some("/home/u/.claude/projects/r/a.jsonl".into()),
            last_status: Status::Idle,
            updated_at: 42,
        }
    }

    fn state() -> State {
        let mut s = State::default();
        s.sessions.insert("x//tmp/r".into(), entry());
        s
    }

    #[test]
    fn save_then_load_round_trips_with_a_private_file() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("state.json");
        save(&p, &state()).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let loaded = load(&p, 1);
        assert_eq!(loaded.problem, None);
        assert_eq!(loaded.state, state());
        // No temp files are left behind.
        let names: Vec<_> = std::fs::read_dir(t.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["state.json"]);
    }

    #[test]
    fn file_format_matches_the_spec() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("state.json");
        save(&p, &state()).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["version"], 1);
        let s = &v["sessions"]["x//tmp/r"];
        for k in [
            "project",
            "repo",
            "profile",
            "claude_session_id",
            "transcript_path",
            "last_status",
            "updated_at",
        ] {
            assert!(s.get(k).is_some(), "missing {k}");
        }
    }

    #[test]
    fn missing_file_is_an_empty_state_without_a_problem() {
        let t = tempfile::tempdir().unwrap();
        let loaded = load(&t.path().join("state.json"), 1);
        assert_eq!(loaded.state, State::default());
        assert_eq!(loaded.problem, None);
    }

    #[test]
    fn corrupt_files_are_renamed_aside_and_start_empty() {
        for (i, junk) in [
            &b"{not json"[..],
            b"",
            b"[1,2,3]",
            b"\xff\xfe\x00",
            br#"{"version":99,"sessions":{}}"#,
            br#"{"version":1,"sessions":[1]}"#,
        ]
        .into_iter()
        .enumerate()
        {
            let t = tempfile::tempdir().unwrap();
            let p = t.path().join("state.json");
            std::fs::write(&p, junk).unwrap();
            let loaded = load(&p, 1234);
            assert_eq!(loaded.state, State::default(), "case {i}");
            assert!(loaded.problem.is_some(), "case {i}");
            assert!(!p.exists(), "case {i}");
            let bad = t.path().join("state.json.bad-1234");
            assert_eq!(std::fs::read(&bad).unwrap(), junk, "case {i}");
        }
    }

    #[test]
    fn one_malformed_entry_drops_only_that_entry() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("state.json");
        let good = serde_json::to_value(entry()).unwrap();
        let json = serde_json::json!({
            "version": 1,
            "sessions": {
                "x//tmp/r": good,
                "bad1": {"project": 5},
                "bad2": "nope",
                "bad3": null,
            }
        });
        std::fs::write(&p, json.to_string()).unwrap();
        let loaded = load(&p, 1);
        assert_eq!(loaded.problem, None);
        assert_eq!(loaded.state, state());
        assert_eq!(loaded.dropped, ["bad1", "bad2", "bad3"]);
        assert!(p.exists(), "the file is not set aside");
    }

    #[test]
    fn an_oversized_file_is_treated_as_corrupt() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("state.json");
        std::fs::write(&p, vec![b' '; MAX_BYTES + 1]).unwrap();
        let loaded = load(&p, 7);
        assert!(loaded.problem.is_some());
        assert!(t.path().join("state.json.bad-7").exists());
    }

    #[test]
    fn a_symlink_is_not_followed() {
        let t = tempfile::tempdir().unwrap();
        let target = t.path().join("elsewhere.json");
        save(&target, &state()).unwrap();
        let p = t.path().join("state.json");
        std::os::unix::fs::symlink(&target, &p).unwrap();
        let loaded = load(&p, 9);
        assert_eq!(loaded.state, State::default());
        assert!(loaded.problem.is_some());
        assert!(target.exists(), "the link target is untouched");
    }

    #[test]
    fn foreign_values_are_sanitized_not_trusted() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("state.json");
        let mut s = state();
        let e = s.sessions.get_mut("x//tmp/r").unwrap();
        e.claude_session_id = Some("--dangerously-skip-permissions".into());
        e.transcript_path = Some("relative/../path".into());
        s.sessions.insert(
            "bad\nid".into(),
            PersistedSession {
                project: "x".into(),
                ..entry()
            },
        );
        save(&p, &s).unwrap();
        let loaded = load(&p, 1);
        assert_eq!(loaded.problem, None);
        assert_eq!(loaded.state.sessions.len(), 1);
        let e = &loaded.state.sessions["x//tmp/r"];
        assert_eq!(e.claude_session_id, None);
        assert_eq!(e.transcript_path, None);
    }

    #[test]
    fn save_replaces_an_existing_file_atomically() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("state.json");
        std::fs::write(&p, b"old").unwrap();
        save(&p, &state()).unwrap();
        assert_eq!(load(&p, 1).state, state());
    }
}
