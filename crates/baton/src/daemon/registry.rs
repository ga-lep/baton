//! The set of live sessions, the single attached client and the child
//! process groups that must be terminated at shutdown.

use super::notifier::Notifier;
use super::session::{self, ClientSink, Cmd, SessionHandle};
use baton_core::config::Config;
use baton_core::hooks::{self, HookPayload};
use baton_core::notify_rule::ClientView;
use baton_core::paths;
use baton_proto::{DaemonMsg, SessionId, SessionInfo, Status};
use nix::unistd::Pid;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::oneshot;

/// Largest accepted terminal dimension (rows or columns).
pub const MAX_DIM: u16 = 1000;
/// Largest number of scrollback rows returned by one request.
pub const MAX_SCROLLBACK_COUNT: u32 = 10_000;
/// Terminal size used until a client says otherwise.
const DEFAULT_SIZE: (u16, u16) = (24, 80);
/// Capacity of the per-client outgoing message queue.
pub const CLIENT_QUEUE: usize = 4096;

/// Longest accepted hook event name, in bytes.
const MAX_EVENT_NAME: usize = 64;

/// Whether `name` looks like a hook event name: short ASCII alphanumerics.
fn valid_event_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_EVENT_NAME
        && name.bytes().all(|b| b.is_ascii_alphanumeric())
}

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
    sink: ClientSink,
}

struct Inner {
    sessions: Vec<SessionHandle>,
    size: (u16, u16),
    nudge: bool,
    attached: Option<Attached>,
    /// What the attached client shows; `None` while no client is attached.
    view: Option<ClientView>,
}

/// Registry of sessions and the attached client.
pub struct Registry {
    groups: Arc<ChildGroups>,
    notifier: Notifier,
    inner: Mutex<Inner>,
    /// Held for the whole of `open_project`, never together with `inner`
    /// while spawning.
    open_gate: Mutex<()>,
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
    pub fn new(groups: Arc<ChildGroups>, notifier: Notifier) -> Self {
        Self {
            groups,
            notifier,
            inner: Mutex::new(Inner {
                sessions: Vec::new(),
                size: DEFAULT_SIZE,
                nudge: true,
                attached: None,
                view: None,
            }),
            open_gate: Mutex::new(()),
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
        let hooks = paths::hooks_json_path();
        // Serializes opens so two requests cannot both spawn the same
        // session; the registry lock itself is not held while spawning.
        let _open = self.open_gate.lock().unwrap_or_else(|e| e.into_inner());
        let size = {
            let mut inner = self.lock();
            inner.nudge = config.attach_redraw_nudge;
            inner.size
        };
        let mut failures = Vec::new();
        let mut started = Vec::new();
        let mut ids = Vec::new();
        for spec in &project.sessions {
            let id = match SessionId::from_repo(&spec.project, &spec.repo) {
                Ok(id) => id,
                Err(e) => {
                    failures.push(format!("{}: {e}", spec.repo.display()));
                    continue;
                }
            };
            ids.push(id.clone());
            let live = self
                .lock()
                .sessions
                .iter()
                .find(|s| s.id == id)
                .map(SessionHandle::info)
                .filter(|i| !matches!(i.status, Status::Exited(_)));
            if live.is_some() {
                continue;
            }
            let launch = session::Launch {
                id: id.clone(),
                spec,
                sock: &sock,
                hooks: &hooks,
                size,
                scrollback: config.scrollback_lines,
                hook_timeout: Duration::from_secs(config.hook_timeout_secs),
                notifications: config.notifications,
                notifier: &self.notifier,
            };
            // No registry lock here: spawning a PTY can be slow.
            match session::start(&launch, &self.groups) {
                Ok(handle) => started.push(handle),
                Err(e) => failures.push(format!("{}: {e:#}", spec.repo.display())),
            }
        }
        let mut guard = self.lock();
        let inner = &mut *guard;
        let mut fresh = Vec::new();
        for handle in started {
            if inner.size != size {
                // The client resized while this session was being spawned.
                handle.send(Cmd::Resize {
                    rows: inner.size.0,
                    cols: inner.size.1,
                });
            }
            if inner.view.is_some() {
                handle.send(Cmd::SetView(inner.view.clone()));
            }
            fresh.push(handle.id.clone());
            match inner.sessions.iter().position(|s| s.id == handle.id) {
                Some(i) => inner.sessions[i] = handle,
                None => inner.sessions.push(handle),
            }
        }
        // Tell an attached client about the new sessions.
        if let Some(att) = &inner.attached
            && !fresh.is_empty()
        {
            let list = inner.sessions.iter().map(SessionHandle::info).collect();
            att.sink.send(DaemonMsg::SessionList(list));
            for s in inner.sessions.iter().filter(|s| fresh.contains(&s.id)) {
                s.send(Cmd::Attach {
                    sink: att.sink.clone(),
                    nudge: inner.nudge,
                });
            }
        }
        let result: Vec<SessionInfo> = ids
            .iter()
            .filter_map(|id| inner.sessions.iter().find(|s| &s.id == id))
            .map(SessionHandle::info)
            .collect();
        if failures.is_empty() {
            Ok(result)
        } else {
            Err(OpenError::Spawn(failures.join("; ")))
        }
    }

    /// Applies a hook event from a Claude child to its session. Events for
    /// unknown sessions or outside the closed event set are logged and dropped.
    pub fn hook(&self, session: &SessionId, event: &str, payload_json: &str) {
        if !valid_event_name(event) {
            // Client-supplied: `{:?}` escapes control characters; cap the echo.
            let shown: String = event.chars().take(MAX_EVENT_NAME).collect();
            tracing::debug!("dropping hook with invalid event name {shown:?}");
            return;
        }
        if !hooks::is_known_event(event) {
            tracing::debug!("dropping hook with unknown event {event:?}");
            return;
        }
        let payload = HookPayload::parse(payload_json);
        let inner = self.lock();
        match inner.sessions.iter().find(|s| &s.id == session) {
            Some(s) => s.hook(event, payload),
            // The id is client-supplied: `{:?}` escapes control characters.
            None => tracing::debug!(
                "dropping hook {event:?} for unknown session {:?}",
                session.0
            ),
        }
    }

    /// Records which session the attached client `conn` shows and tells every
    /// session whether it is on screen. Other connections are ignored.
    pub fn set_view(&self, conn: u64, on_screen: Option<SessionId>, terminal_focused: bool) {
        let mut inner = self.lock();
        if inner.attached.as_ref().is_none_or(|a| a.conn != conn) {
            return;
        }
        let view = ClientView {
            on_screen,
            terminal_focused,
        };
        for s in &inner.sessions {
            s.send(Cmd::SetView(Some(view.clone())));
        }
        inner.view = Some(view);
    }

    /// Marks a session as seen, as if the user had looked at it.
    ///
    /// # Errors
    /// If the session does not exist.
    pub fn mark_viewed(&self, id: &SessionId) -> Result<(), String> {
        let inner = self.lock();
        find(&inner, id)?.send(Cmd::MarkViewed);
        Ok(())
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
    pub fn attach(&self, conn: u64, rows: u16, cols: u16, sink: &ClientSink) {
        let mut inner = self.lock();
        if let Some(old) = inner.attached.replace(Attached {
            conn,
            sink: sink.clone(),
        }) {
            old.sink.send(DaemonMsg::Error {
                message: "replaced by another client".into(),
            });
        }
        inner.size = (rows, cols);
        // The new client has not said what it shows yet; assume it is focused.
        let view = ClientView {
            on_screen: None,
            terminal_focused: true,
        };
        inner.view = Some(view.clone());
        let list = inner.sessions.iter().map(SessionHandle::info).collect();
        sink.send(DaemonMsg::SessionList(list));
        for s in &inner.sessions {
            s.send(Cmd::SetView(Some(view.clone())));
            s.send(Cmd::Resize { rows, cols });
            s.send(Cmd::Attach {
                sink: sink.clone(),
                nudge: inner.nudge,
            });
        }
    }

    /// Forgets `conn` if it is the attached client.
    pub fn detach(&self, conn: u64) {
        let mut inner = self.lock();
        if inner.attached.as_ref().is_some_and(|a| a.conn == conn) {
            inner.attached = None;
            inner.view = None;
            for s in &inner.sessions {
                s.send(Cmd::SetView(None));
            }
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
    fn event_names_are_short_ascii_alphanumerics() {
        assert!(valid_event_name("PreToolUse"));
        assert!(valid_event_name(&"a".repeat(MAX_EVENT_NAME)));
        assert!(!valid_event_name(""));
        assert!(!valid_event_name(&"a".repeat(MAX_EVENT_NAME + 1)));
        for bad in ["Stop\n", "St op", "Stop\u{1b}[2J", "Sto-p", "Stöp"] {
            assert!(!valid_event_name(bad), "{bad:?}");
        }
    }

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
