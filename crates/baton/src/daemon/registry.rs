//! The set of live sessions, the single attached client and the child
//! process groups that must be terminated at shutdown.

use super::session::{self, Cmd, SessionHandle};
use baton_core::config::Config;
use baton_core::paths;
use baton_proto::{DaemonMsg, SessionId, SessionInfo, Status};
use nix::unistd::Pid;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::{mpsc, oneshot};

/// Largest accepted terminal dimension (rows or columns).
pub const MAX_DIM: u16 = 1000;
/// Largest number of scrollback rows returned by one request.
pub const MAX_SCROLLBACK_COUNT: u32 = 10_000;
/// Terminal size used until a client says otherwise.
const DEFAULT_SIZE: (u16, u16) = (24, 80);
/// Capacity of the per-client outgoing message queue.
pub const CLIENT_QUEUE: usize = 4096;

/// A terminal size was zero or larger than [`MAX_DIM`].
#[derive(Debug, thiserror::Error)]
#[error("invalid terminal size {rows}x{cols}: each side must be 1..={MAX_DIM}")]
pub struct BadSize {
    rows: u16,
    cols: u16,
}

/// Checks a client-supplied terminal size.
///
/// # Errors
/// [`BadSize`] if either side is 0 or above [`MAX_DIM`].
pub fn validate_size(rows: u16, cols: u16) -> Result<(u16, u16), BadSize> {
    if (1..=MAX_DIM).contains(&rows) && (1..=MAX_DIM).contains(&cols) {
        Ok((rows, cols))
    } else {
        Err(BadSize { rows, cols })
    }
}

/// A process group was refused registration.
#[derive(Debug, thiserror::Error)]
#[error("refusing to track process group {0}")]
pub struct BadGroup(pub i32);

/// Process groups to terminate at shutdown.
#[derive(Default)]
pub struct ChildGroups {
    groups: Mutex<Vec<Pid>>,
}

impl ChildGroups {
    /// Records a child process group.
    ///
    /// # Errors
    /// [`BadGroup`] for `pgid <= 1` or the daemon's own process group, which
    /// must never be signalled.
    pub fn register(&self, pgid: Pid) -> Result<(), BadGroup> {
        if pgid.as_raw() <= 1 || pgid == nix::unistd::getpgrp() {
            return Err(BadGroup(pgid.as_raw()));
        }
        self.lock().push(pgid);
        Ok(())
    }

    /// Forgets a group, typically because its leader was reaped (the id may
    /// be reused by an unrelated process afterwards).
    pub fn unregister(&self, pgid: Pid) {
        self.lock().retain(|&g| g != pgid);
    }

    /// Takes all registered groups.
    pub fn take(&self) -> Vec<Pid> {
        std::mem::take(&mut *self.lock())
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Pid>> {
        self.groups.lock().unwrap_or_else(|e| e.into_inner())
    }
}

struct Attached {
    conn: u64,
    tx: mpsc::Sender<DaemonMsg>,
}

struct Inner {
    sessions: Vec<SessionHandle>,
    size: (u16, u16),
    nudge: bool,
    attached: Option<Attached>,
}

/// Registry of sessions and the attached client.
pub struct Registry {
    groups: Arc<ChildGroups>,
    inner: Mutex<Inner>,
}

/// Why `OpenProject` failed.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("cannot load config: {0}")]
    Config(String),
    #[error("unknown project \"{0}\"")]
    UnknownProject(String),
    #[error("{0}")]
    Spawn(String),
}

impl Registry {
    /// Creates an empty registry.
    pub fn new(groups: Arc<ChildGroups>) -> Self {
        Self {
            groups,
            inner: Mutex::new(Inner {
                sessions: Vec::new(),
                size: DEFAULT_SIZE,
                nudge: true,
                attached: None,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Summaries of all sessions, in creation order.
    pub fn list(&self) -> Vec<SessionInfo> {
        self.lock()
            .sessions
            .iter()
            .map(SessionHandle::info)
            .collect()
    }

    /// Re-reads the config and spawns the project's sessions that are not
    /// already live. Returns all sessions of the project.
    ///
    /// # Errors
    /// If the config is unreadable, the project is unknown, or any spawn
    /// fails (the sessions that did start stay up).
    pub fn open_project(&self, name: &str) -> Result<Vec<SessionInfo>, OpenError> {
        let path = paths::config_file().map_err(|e| OpenError::Config(e.to_string()))?;
        let config = Config::load(&path, &|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
            .map_err(|e| OpenError::Config(e.to_string()))?;
        let project = config
            .projects
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| OpenError::UnknownProject(name.to_owned()))?;
        let sock = paths::socket_path();
        let mut guard = self.lock();
        let inner = &mut *guard;
        inner.nudge = config.attach_redraw_nudge;
        let mut failures = Vec::new();
        let mut result = Vec::new();
        let mut fresh = Vec::new();
        for spec in &project.sessions {
            let id = match SessionId::from_repo(&spec.project, &spec.repo) {
                Ok(id) => id,
                Err(e) => {
                    failures.push(format!("{}: {e}", spec.repo.display()));
                    continue;
                }
            };
            let existing = inner.sessions.iter().position(|s| s.id == id);
            if let Some(i) = existing
                && !matches!(inner.sessions[i].info().status, Status::Exited(_))
            {
                result.push(inner.sessions[i].info());
                continue;
            }
            let launch = session::Launch {
                id: id.clone(),
                spec,
                sock: &sock,
                size: inner.size,
                scrollback: config.scrollback_lines,
            };
            match session::start(&launch, &self.groups) {
                Ok(handle) => {
                    result.push(handle.info());
                    match existing {
                        Some(i) => inner.sessions[i] = handle,
                        None => inner.sessions.push(handle),
                    }
                    fresh.push(id);
                }
                Err(e) => failures.push(format!("{}: {e:#}", spec.repo.display())),
            }
        }
        // Tell an attached client about the new sessions.
        if let Some(att) = &inner.attached
            && !fresh.is_empty()
        {
            let list = inner.sessions.iter().map(SessionHandle::info).collect();
            let _ = att.tx.try_send(DaemonMsg::SessionList(list));
            for s in inner.sessions.iter().filter(|s| fresh.contains(&s.id)) {
                s.send(Cmd::Attach {
                    tx: att.tx.clone(),
                    nudge: inner.nudge,
                });
            }
        }
        if failures.is_empty() {
            Ok(result)
        } else {
            Err(OpenError::Spawn(failures.join("; ")))
        }
    }

    /// Forwards keyboard bytes to a session.
    ///
    /// # Errors
    /// If the session does not exist.
    pub fn input(&self, id: &SessionId, bytes: Vec<u8>) -> Result<(), String> {
        let inner = self.lock();
        let s = find(&inner, id)?;
        s.send(Cmd::Input(bytes));
        Ok(())
    }

    /// Resizes every session; a no-op where the size is unchanged.
    pub fn resize_all(&self, rows: u16, cols: u16) {
        let mut inner = self.lock();
        inner.size = (rows, cols);
        for s in &inner.sessions {
            s.send(Cmd::Resize { rows, cols });
        }
    }

    /// Makes `conn` the single attached client, replacing any previous one.
    ///
    /// Queues `SessionList`, then one `Snapshot` per session (in order),
    /// after which output streams to `tx`.
    pub fn attach(&self, conn: u64, rows: u16, cols: u16, tx: &mpsc::Sender<DaemonMsg>) {
        let mut inner = self.lock();
        if let Some(old) = inner.attached.replace(Attached {
            conn,
            tx: tx.clone(),
        }) {
            let _ = old.tx.try_send(DaemonMsg::Error {
                message: "replaced by another client".into(),
            });
        }
        inner.size = (rows, cols);
        let list = inner.sessions.iter().map(SessionHandle::info).collect();
        let _ = tx.try_send(DaemonMsg::SessionList(list));
        for s in &inner.sessions {
            s.send(Cmd::Resize { rows, cols });
            s.send(Cmd::Attach {
                tx: tx.clone(),
                nudge: inner.nudge,
            });
        }
    }

    /// Forgets `conn` if it is the attached client.
    pub fn detach(&self, conn: u64) {
        let mut inner = self.lock();
        if inner.attached.as_ref().is_some_and(|a| a.conn == conn) {
            inner.attached = None;
        }
    }

    /// Scrollback rows `start..start+count` (count clamped) of a session.
    ///
    /// # Errors
    /// If the session does not exist or has stopped.
    pub async fn scrollback(
        &self,
        id: &SessionId,
        start: u32,
        count: u32,
    ) -> Result<Vec<Vec<u8>>, String> {
        let (reply, rx) = oneshot::channel();
        {
            let inner = self.lock();
            find(&inner, id)?.send(Cmd::Scrollback {
                start: start as usize,
                count: count.min(MAX_SCROLLBACK_COUNT) as usize,
                reply,
            });
        }
        rx.await.map_err(|_| "session stopped".to_owned())
    }
}

fn find<'a>(inner: &'a Inner, id: &SessionId) -> Result<&'a SessionHandle, String> {
    inner
        .sessions
        .iter()
        .find(|s| &s.id == id)
        .ok_or_else(|| format!("unknown session \"{id}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_outside_1_to_1000_are_rejected() {
        assert_eq!(validate_size(24, 80).unwrap(), (24, 80));
        assert!(validate_size(1, 1).is_ok());
        assert!(validate_size(1000, 1000).is_ok());
        for (r, c) in [
            (0, 80),
            (24, 0),
            (1001, 80),
            (24, 1001),
            (0, 0),
            (u16::MAX, 1),
        ] {
            assert!(validate_size(r, c).is_err(), "{r}x{c}");
        }
    }

    #[test]
    fn groups_reject_init_negative_and_own_group() {
        let g = ChildGroups::default();
        for raw in [i32::MIN, -5, 0, 1] {
            assert!(g.register(Pid::from_raw(raw)).is_err(), "{raw}");
        }
        assert!(g.register(nix::unistd::getpgrp()).is_err());
        assert!(g.take().is_empty());
    }

    #[test]
    fn groups_unregister_once_reaped() {
        let g = ChildGroups::default();
        let (a, b) = (Pid::from_raw(i32::MAX - 1), Pid::from_raw(i32::MAX - 2));
        g.register(a).unwrap();
        g.register(b).unwrap();
        g.unregister(a);
        assert_eq!(g.take(), vec![b]);
        assert!(g.take().is_empty());
    }
}
