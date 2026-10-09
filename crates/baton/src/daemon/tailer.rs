//! Polling transcript tailer: turns a session's transcript file into usage
//! updates.
//!
//! The transcript path comes from a hook payload, i.e. from outside Baton, so
//! it is only ever opened after it has been shown to be a regular file under
//! the session's Claude config directory (`<config dir>/projects/`). Any
//! problem makes the usage `None` (shown as `n/a`); the tailer never touches
//! the session's status.

use baton_core::config::SessionSpec;
use baton_core::pricing::Pricing;
use baton_core::transcript::UsageAccumulator;
use baton_proto::Usage;
use nix::fcntl::OFlag;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::watch;

/// How often the transcript is polled.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// Longest single line buffered; a longer one is dropped up to its newline.
pub const MAX_LINE: usize = 4 * 1024 * 1024;
/// Most bytes read from the file in one poll.
pub const MAX_POLL_READ: u64 = 1024 * 1024;

/// The directory a session's transcripts must live under: `projects/` in the
/// Claude config dir, which is `CLAUDE_CONFIG_DIR` from the profile env, else
/// from the daemon's own environment (which the child inherits), else
/// `~/.claude`. A relative value is taken relative to the repo, the child's cwd.
pub fn projects_root(
    spec: &SessionSpec,
    daemon_env: &dyn Fn(&str) -> Option<String>,
) -> Option<PathBuf> {
    let config_dir = match spec
        .env
        .get("CLAUDE_CONFIG_DIR")
        .cloned()
        .or_else(|| daemon_env("CLAUDE_CONFIG_DIR"))
        .filter(|v| !v.is_empty())
    {
        Some(dir) => spec.repo.join(dir),
        None => Path::new(&daemon_env("HOME").filter(|h| !h.is_empty())?).join(".claude"),
    };
    Some(config_dir.join("projects"))
}

/// Why a transcript path was not opened.
#[derive(Debug)]
enum Refusal {
    /// Does not exist (yet): wait silently.
    Missing,
    /// Must never be read.
    Refused(String),
}

/// Opens `path` for reading if it is a regular file under `root`.
fn open_checked(root: &Path, path: &Path) -> Result<File, Refusal> {
    let refuse = |why: &str| Refusal::Refused(why.to_owned());
    let canon_root = match root.canonicalize() {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Refusal::Missing),
        Err(e) => return Err(Refusal::Refused(format!("projects dir: {e}"))),
    };
    let canon = match path.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Refusal::Missing),
        Err(e) => return Err(Refusal::Refused(format!("canonicalize: {e}"))),
    };
    if !canon.starts_with(&canon_root) {
        return Err(refuse("outside the projects dir"));
    }
    // NOFOLLOW: the last component must not be a (newly swapped in) symlink;
    // NONBLOCK: opening a FIFO must not hang the poll.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags((OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK).bits())
        .open(&canon)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Refusal::Missing,
            _ => Refusal::Refused(format!("open: {e}")),
        })?;
    let meta = file
        .metadata()
        .map_err(|e| Refusal::Refused(format!("fstat: {e}")))?;
    if !meta.is_file() {
        return Err(refuse("not a regular file"));
    }
    // What was really opened (not what the path said a moment ago).
    let real = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))
        .map_err(|e| Refusal::Refused(format!("readlink: {e}")))?;
    if !real.starts_with(&canon_root) {
        return Err(refuse("opened file is outside the projects dir"));
    }
    Ok(file)
}

/// What is being tailed.
struct Tracked {
    path: PathBuf,
    /// `(device, inode)` of the file read so far.
    identity: Option<(u64, u64)>,
    offset: u64,
    partial: Vec<u8>,
    /// Inside an over-long line: skip to the next newline.
    discarding: bool,
    acc: UsageAccumulator,
}

impl Tracked {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            identity: None,
            offset: 0,
            partial: Vec::new(),
            discarding: false,
            acc: UsageAccumulator::new(),
        }
    }

    fn reset(&mut self) {
        *self = Self::new(std::mem::take(&mut self.path));
    }

    fn ingest(&mut self, mut chunk: &[u8], max_line: usize) {
        while !chunk.is_empty() {
            let Some(nl) = chunk.iter().position(|&b| b == b'\n') else {
                if !self.discarding {
                    if self.partial.len() + chunk.len() > max_line {
                        self.partial.clear();
                        self.discarding = true;
                    } else {
                        self.partial.extend_from_slice(chunk);
                    }
                }
                return;
            };
            let (head, rest) = chunk.split_at(nl);
            chunk = &rest[1..];
            if self.discarding {
                self.discarding = false;
            } else if self.partial.len() + head.len() <= max_line {
                self.partial.extend_from_slice(head);
                self.acc.feed_line(&String::from_utf8_lossy(&self.partial));
            }
            self.partial.clear();
        }
    }
}

/// Result of one poll.
#[derive(Debug, PartialEq)]
pub struct Poll {
    /// The new usage, if it differs from the previous poll's.
    pub update: Option<Option<Usage>>,
    /// More bytes are waiting: poll again at once.
    pub more: bool,
}

/// Synchronous core of the tailer; [`run`] drives it once a second.
pub struct Tailer {
    root: PathBuf,
    pricing: Pricing,
    tracked: Option<Tracked>,
    /// Last usage reported.
    last: Option<Usage>,
    /// A path already logged as refused, so a bad path is not logged every poll.
    logged_refusal: Option<PathBuf>,
    max_line: usize,
    max_read: u64,
}

impl Tailer {
    /// A tailer that only reads files under `root` (the `projects/` dir).
    pub fn new(root: PathBuf, pricing: Pricing) -> Self {
        Self {
            root,
            pricing,
            tracked: None,
            last: None,
            logged_refusal: None,
            max_line: MAX_LINE,
            max_read: MAX_POLL_READ,
        }
    }

    /// Follows `path` (a new `SessionStart` may change it); `None` stops tailing.
    /// A different path starts over.
    pub fn set_path(&mut self, path: Option<PathBuf>) {
        if self.tracked.as_ref().map(|t| &t.path) == path.as_ref() {
            return;
        }
        self.tracked = path.map(Tracked::new);
    }

    /// Reads whatever was appended since the last poll.
    pub fn poll(&mut self) -> Poll {
        let (usage, more) = match self.read() {
            Ok(r) => r,
            Err(Refusal::Missing) => (self.last.clone(), false),
            Err(Refusal::Refused(why)) => {
                if let Some(t) = self.tracked.as_mut() {
                    if self.logged_refusal.as_ref() != Some(&t.path) {
                        tracing::debug!(path = ?t.path, "transcript not read: {why}");
                        self.logged_refusal = Some(t.path.clone());
                    }
                    t.reset();
                }
                (None, false)
            }
        };
        let update = (usage != self.last).then(|| {
            self.last.clone_from(&usage);
            usage
        });
        Poll { update, more }
    }

    fn read(&mut self) -> Result<(Option<Usage>, bool), Refusal> {
        let Some(t) = self.tracked.as_mut() else {
            return Ok((None, false));
        };
        let mut file = open_checked(&self.root, &t.path)?;
        let meta = file
            .metadata()
            .map_err(|e| Refusal::Refused(format!("fstat: {e}")))?;
        let identity = (meta.dev(), meta.ino());
        if t.identity.is_some_and(|i| i != identity) || meta.len() < t.offset {
            t.reset(); // replaced or truncated
        }
        t.identity = Some(identity);
        file.seek(SeekFrom::Start(t.offset))
            .map_err(|e| Refusal::Refused(format!("seek: {e}")))?;
        let mut buf = Vec::new();
        (&file)
            .take(self.max_read)
            .read_to_end(&mut buf)
            .map_err(|e| Refusal::Refused(format!("read: {e}")))?;
        t.offset += buf.len() as u64;
        t.ingest(&buf, self.max_line);
        let more = t.offset < meta.len();
        Ok((Some(t.acc.snapshot(&self.pricing)), more))
    }
}

/// Polls the transcript named by `path` once a second and reports changes
/// as `Some(usage)` / `None` (`n/a`) to `report`. Ends when `report` returns
/// `false` or the path sender is gone.
pub async fn run(
    mut tailer: Tailer,
    mut path: watch::Receiver<Option<PathBuf>>,
    mut report: impl FnMut(Option<Usage>) -> bool,
) {
    tailer.set_path(path.borrow_and_update().clone());
    loop {
        // Blocking file I/O stays off the async threads.
        let polled = tokio::task::spawn_blocking(move || {
            let p = tailer.poll();
            (tailer, p)
        })
        .await;
        let Ok((t, poll)) = polled else { return };
        tailer = t;
        if let Some(update) = poll.update
            && !report(update)
        {
            return;
        }
        if poll.more {
            tokio::task::yield_now().await;
            continue;
        }
        tokio::select! {
            () = tokio::time::sleep(POLL_INTERVAL) => {}
            changed = path.changed() => {
                if changed.is_err() {
                    return;
                }
                tailer.set_path(path.borrow_and_update().clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct Fx {
        dir: tempfile::TempDir,
        root: PathBuf,
    }

    impl Fx {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("tmp");
            let root = dir.path().canonicalize().expect("canon").join("projects");
            std::fs::create_dir_all(root.join("slug")).expect("mkdir");
            Self { dir, root }
        }

        fn file(&self, name: &str) -> PathBuf {
            self.root.join("slug").join(name)
        }

        fn tailer(&self) -> Tailer {
            Tailer::new(self.root.clone(), Pricing::default())
        }

        fn append(&self, path: &Path, text: &str) {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("open");
            f.write_all(text.as_bytes()).expect("write");
        }
    }

    fn line(id: &str, input: u64) -> String {
        format!(
            "{{\"type\":\"assistant\",\"message\":{{\"id\":\"{id}\",\"model\":\"claude-x\",\"usage\":{{\"input_tokens\":{input},\"output_tokens\":1}}}}}}\n"
        )
    }

    fn input_of(p: &Poll) -> Option<u64> {
        p.update.as_ref()?.as_ref().map(|u| u.input)
    }

    #[test]
    fn a_missing_file_is_waited_for_silently_then_picked_up() {
        let fx = Fx::new();
        let f = fx.file("a.jsonl");
        let mut t = fx.tailer();
        t.set_path(Some(f.clone()));
        assert_eq!(
            t.poll(),
            Poll {
                update: None,
                more: false
            }
        );
        fx.append(&f, &line("m1", 7));
        assert_eq!(input_of(&t.poll()), Some(7));
    }

    #[test]
    fn appended_lines_are_read_from_the_offset_and_partial_lines_wait() {
        let fx = Fx::new();
        let f = fx.file("a.jsonl");
        fx.append(&f, &line("m1", 1));
        let mut t = fx.tailer();
        t.set_path(Some(f.clone()));
        assert_eq!(input_of(&t.poll()), Some(1));
        assert_eq!(t.poll().update, None, "unchanged values emit nothing");
        let second = line("m2", 10);
        let (a, b) = second.split_at(20);
        fx.append(&f, a);
        assert_eq!(t.poll().update, None, "a partial line is buffered");
        fx.append(&f, b);
        assert_eq!(input_of(&t.poll()), Some(11));
        // Same message id again: counted once, so nothing changes.
        fx.append(&f, &line("m2", 10));
        assert_eq!(t.poll().update, None);
    }

    #[test]
    fn truncation_and_path_changes_start_over() {
        let fx = Fx::new();
        let (a, b) = (fx.file("a.jsonl"), fx.file("b.jsonl"));
        fx.append(&a, &(line("m1", 5) + &line("m2", 5)));
        fx.append(&b, &line("n1", 3));
        let mut t = fx.tailer();
        t.set_path(Some(a.clone()));
        assert_eq!(input_of(&t.poll()), Some(10));
        std::fs::write(&a, line("m9", 2)).expect("truncate");
        assert_eq!(input_of(&t.poll()), Some(2));
        t.set_path(Some(b));
        assert_eq!(input_of(&t.poll()), Some(3));
        t.set_path(None);
        assert_eq!(t.poll().update, Some(None), "no path: n/a");
    }

    #[test]
    fn paths_outside_the_projects_dir_are_refused() {
        let fx = Fx::new();
        let good = fx.file("a.jsonl");
        fx.append(&good, &line("m1", 4));
        let outside = fx.dir.path().join("elsewhere.jsonl");
        std::fs::write(&outside, line("evil", 999)).expect("write");
        let mut t = fx.tailer();
        t.set_path(Some(good));
        assert_eq!(input_of(&t.poll()), Some(4));
        t.set_path(Some(outside.clone()));
        assert_eq!(t.poll().update, Some(None));
        assert_eq!(t.poll().update, None, "stays n/a");
        // `..` tricks resolve to the same place.
        t.set_path(Some(fx.root.join("slug/../../elsewhere.jsonl")));
        assert_eq!(t.poll().update, None);
        assert!(t.last.is_none());
    }

    #[test]
    fn a_symlink_to_a_file_elsewhere_is_refused() {
        let fx = Fx::new();
        let outside = fx.dir.path().join("secret.jsonl");
        std::fs::write(&outside, line("evil", 999)).expect("write");
        let link = fx.file("link.jsonl");
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");
        let mut t = fx.tailer();
        t.set_path(Some(link));
        let p = t.poll();
        assert_eq!(p.update, None);
        assert!(t.last.is_none());
        assert!(t.tracked.as_ref().is_some_and(|t| t.offset == 0));
    }

    #[test]
    fn only_regular_files_are_read() {
        let fx = Fx::new();
        let fifo = fx.file("fifo.jsonl");
        nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::S_IRWXU).expect("mkfifo");
        let dir = fx.file("dir.jsonl");
        std::fs::create_dir(&dir).expect("mkdir");
        for path in [fifo, dir] {
            let mut t = fx.tailer();
            t.set_path(Some(path));
            assert_eq!(
                t.poll(),
                Poll {
                    update: None,
                    more: false
                }
            );
        }
        // A prior good value turns into n/a when the path becomes a FIFO.
        let good = fx.file("a.jsonl");
        fx.append(&good, &line("m1", 4));
        let mut t = fx.tailer();
        t.set_path(Some(good));
        assert_eq!(input_of(&t.poll()), Some(4));
        t.set_path(Some(fx.file("fifo.jsonl")));
        assert_eq!(t.poll().update, Some(None));
    }

    #[test]
    fn an_over_long_line_is_dropped_and_reading_resyncs_at_the_newline() {
        let fx = Fx::new();
        let f = fx.file("a.jsonl");
        let mut t = fx.tailer();
        t.max_line = 200;
        t.set_path(Some(f.clone()));
        fx.append(&f, &line("m1", 1));
        fx.append(&f, &"x".repeat(5000));
        t.poll();
        assert!(t.tracked.as_ref().is_some_and(|t| t.partial.is_empty()));
        fx.append(&f, &"y".repeat(5000));
        fx.append(&f, "\n");
        t.poll();
        fx.append(&f, &line("m2", 20));
        assert_eq!(input_of(&t.poll()), Some(21));
    }

    #[test]
    fn each_poll_reads_a_bounded_amount() {
        let fx = Fx::new();
        let f = fx.file("a.jsonl");
        let one = line("m0", 1);
        for i in 0..10 {
            fx.append(&f, &line(&format!("m{i}"), 1));
        }
        let mut t = fx.tailer();
        t.max_read = one.len() as u64 * 2 + 3;
        t.set_path(Some(f));
        let mut polls = 0;
        loop {
            polls += 1;
            if !t.poll().more {
                break;
            }
            assert!(polls < 20);
        }
        assert!(polls >= 4, "{polls}");
        assert_eq!(t.last.as_ref().map(|u| u.input), Some(10));
    }

    #[test]
    fn projects_root_follows_profile_then_daemon_env_then_home() {
        use std::collections::BTreeMap;
        let spec = |env: &[(&str, &str)]| SessionSpec {
            project: "p".into(),
            repo: PathBuf::from("/repo"),
            profile: "d".into(),
            argv: vec![],
            args: vec![],
            env: env
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<BTreeMap<_, _>>(),
        };
        let env = |k: &str| match k {
            "CLAUDE_CONFIG_DIR" => Some("/daemon/cfg".to_owned()),
            "HOME" => Some("/home/u".to_owned()),
            _ => None,
        };
        let home_only = |k: &str| (k == "HOME").then(|| "/home/u".to_owned());
        let nothing = |_: &str| None;
        let profile = spec(&[("CLAUDE_CONFIG_DIR", "/prof/cfg")]);
        assert_eq!(
            projects_root(&profile, &env),
            Some("/prof/cfg/projects".into())
        );
        assert_eq!(
            projects_root(&spec(&[]), &env),
            Some("/daemon/cfg/projects".into())
        );
        assert_eq!(
            projects_root(&spec(&[]), &home_only),
            Some("/home/u/.claude/projects".into())
        );
        assert_eq!(projects_root(&spec(&[]), &nothing), None);
        let rel = spec(&[("CLAUDE_CONFIG_DIR", "cfg")]);
        assert_eq!(projects_root(&rel, &env), Some("/repo/cfg/projects".into()));
    }

    #[tokio::test]
    async fn the_runner_reports_changes_and_stops_when_dropped() {
        let fx = Fx::new();
        let f = fx.file("a.jsonl");
        fx.append(&f, &line("m1", 6));
        let (ptx, prx) = watch::channel(Some(f));
        let (otx, mut orx) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(run(fx.tailer(), prx, move |u| otx.send(u).is_ok()));
        let got = tokio::time::timeout(Duration::from_secs(5), orx.recv())
            .await
            .expect("timely");
        assert_eq!(got.flatten().map(|u| u.input), Some(6));
        drop(ptx);
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("stops")
            .expect("no panic");
    }
}
