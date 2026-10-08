//! One session: a PTY child, its authoritative screen and the attached client.

use super::notifier::{Notification, Notifier};
use super::registry::ChildGroups;
use super::spawn;
use crate::term::screen::Screen;
use crate::term::vt100_screen::Vt100Screen;
use anyhow::{Result, bail};
use baton_core::config::SessionSpec;
use baton_core::hooks::HookPayload;
use baton_core::notify_rule::{self, ClientView};
use baton_core::status::{self, Input};
use baton_proto::{DaemonMsg, SessionId, SessionInfo, Status};
use nix::unistd::Pid;
use portable_pty::PtySize;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

/// Chunks queued between the PTY reader thread and the session task.
const OUTPUT_QUEUE: usize = 64;
/// Writes queued for the PTY writer thread.
const INPUT_QUEUE: usize = 1024;
/// After the child exits, how long to wait for the last output to arrive.
const DRAIN_TIMEOUT: Duration = Duration::from_millis(100);

/// Requests to a session task.
pub enum Cmd {
    Input(Vec<u8>),
    Resize {
        rows: u16,
        cols: u16,
    },
    /// Send a snapshot to `tx`, then stream output to it.
    Attach {
        sink: ClientSink,
        nudge: bool,
    },
    Scrollback {
        start: usize,
        count: usize,
        reply: oneshot::Sender<Vec<Vec<u8>>>,
    },
    Exited(i32),
    /// A hook fired in the child.
    Hook {
        event: String,
        payload: HookPayload,
    },
    /// What the attached client shows; `None` when no client is attached.
    SetView(Option<ClientView>),
    /// The user explicitly marks the session as seen.
    MarkViewed,
}

/// The attached client's outgoing queue plus the switch that makes its
/// connection report an error and close when the queue overflows.
#[derive(Clone)]
pub struct ClientSink {
    pub tx: mpsc::Sender<DaemonMsg>,
    pub kick: CancellationToken,
}

impl ClientSink {
    /// Queues `msg`. Returns `false` when the client must be dropped: its
    /// queue is full (the connection is told via `kick`) or already closed.
    pub fn send(&self, msg: DaemonMsg) -> bool {
        match self.tx.try_send(msg) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.kick.cancel();
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }
}

/// Cheap handle to a running session task.
pub struct SessionHandle {
    pub id: SessionId,
    info: Arc<Mutex<SessionInfo>>,
    tx: mpsc::UnboundedSender<Cmd>,
}

impl SessionHandle {
    /// Current summary.
    pub fn info(&self) -> SessionInfo {
        self.info.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Queues a hook event for the session task, which updates the summary
    /// and the status in order with everything else.
    pub fn hook(&self, event: &str, payload: HookPayload) {
        self.send(Cmd::Hook {
            event: event.to_owned(),
            payload,
        });
    }

    /// Queues a request; ignored if the task has gone away.
    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }
}

/// What to start.
pub struct Launch<'a> {
    pub id: SessionId,
    pub spec: &'a SessionSpec,
    pub sock: &'a Path,
    /// The injected `hooks.json` passed to the child via `--settings`.
    pub hooks: &'a Path,
    /// `(rows, cols)`.
    pub size: (u16, u16),
    pub scrollback: usize,
    /// How long a live session may stay `Starting` before it is `Unknown`.
    pub hook_timeout: Duration,
    /// Whether desktop notifications are enabled (`notifications` in the config).
    pub notifications: bool,
    pub notifier: &'a Notifier,
}

/// Spawns the child, its helper threads and the session task.
///
/// Must be called from within the tokio runtime.
///
/// # Errors
/// If the child cannot be started or its process group cannot be tracked.
pub fn start(l: &Launch<'_>, groups: &Arc<ChildGroups>) -> Result<SessionHandle> {
    let mut sp = spawn::spawn(l.spec, &l.id, l.sock, l.hooks, l.size)?;
    let pgid = match i32::try_from(sp.pid)
        .map_err(anyhow::Error::from)
        .and_then(|p| nix::unistd::getpgid(Some(Pid::from_raw(p))).map_err(Into::into))
    {
        Ok(p) => p,
        Err(e) => {
            reap(&mut sp.child);
            bail!("cannot determine process group: {e}");
        }
    };
    if let Err(e) = groups.register(pgid) {
        reap(&mut sp.child);
        bail!("{e}");
    }

    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>(OUTPUT_QUEUE);
    let (in_tx, in_rx) = std_mpsc::sync_channel::<Vec<u8>>(INPUT_QUEUE);

    let mut reader = sp.reader;
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break, // EIO once the child side is gone
            }
        }
    });
    let mut writer = sp.writer;
    std::thread::spawn(move || {
        for chunk in in_rx {
            if writer
                .write_all(&chunk)
                .and_then(|()| writer.flush())
                .is_err()
            {
                break;
            }
        }
    });
    let mut child = sp.child;
    let wait_tx = cmd_tx.clone();
    let wait_groups = Arc::clone(groups);
    std::thread::spawn(move || {
        let code = match child.wait() {
            Ok(status) => i32::try_from(status.exit_code()).unwrap_or(-1),
            Err(e) => {
                tracing::warn!("waiting for child: {e}");
                -1
            }
        };
        // Reaped: the group id may now be reused by an unrelated process.
        wait_groups.unregister(pgid);
        let _ = wait_tx.send(Cmd::Exited(code));
    });

    let started_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let info = Arc::new(Mutex::new(SessionInfo {
        id: l.id.clone(),
        project: l.spec.project.clone(),
        repo: l.spec.repo.display().to_string(),
        profile: Some(l.spec.profile.clone()),
        status: Status::Starting,
        claude_session_id: None,
        transcript_path: None,
        model: None,
        started_at,
        exit_code: None,
        usage: None,
    }));
    let task = Task {
        id: l.id.clone(),
        screen: Vt100Screen::new(l.size.0, l.size.1, l.scrollback),
        master: sp.master,
        input: in_tx,
        info: Arc::clone(&info),
        client: None,
        exited: false,
        status: Status::Starting,
        view: None,
        notifications: l.notifications,
        notifier: l.notifier.clone(),
        project: l.spec.project.clone(),
        repo_name: l.spec.repo.file_name().map_or_else(
            || l.spec.repo.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        ),
        deadline: Some(Instant::now() + l.hook_timeout),
    };
    tokio::spawn(task.run(cmd_rx, out_rx));
    Ok(SessionHandle {
        id: l.id.clone(),
        info,
        tx: cmd_tx,
    })
}

/// Kills and reaps a child that never became a tracked session.
fn reap(child: &mut Box<dyn portable_pty::Child + Send + Sync>) {
    let _ = child.kill();
    let _ = child.wait();
}

struct Task {
    id: SessionId,
    screen: Vt100Screen,
    master: Box<dyn portable_pty::MasterPty + Send>,
    input: std_mpsc::SyncSender<Vec<u8>>,
    info: Arc<Mutex<SessionInfo>>,
    client: Option<ClientSink>,
    exited: bool,
    status: Status,
    /// What the attached client shows; `None` when none is attached.
    view: Option<ClientView>,
    notifications: bool,
    notifier: Notifier,
    project: String,
    /// Last path component of the repo, for notification summaries.
    repo_name: String,
    /// When a session still `Starting` becomes `Unknown`; cleared once fired.
    deadline: Option<Instant>,
}

impl Task {
    async fn run(
        mut self,
        mut cmds: mpsc::UnboundedReceiver<Cmd>,
        mut output: mpsc::Receiver<Vec<u8>>,
    ) {
        let mut output_open = true;
        loop {
            let deadline = self.deadline;
            tokio::select! {
                () = async {
                    match deadline {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => {
                    self.deadline = None;
                    self.apply(Input::HookTimeout);
                }
                cmd = cmds.recv() => match cmd {
                    None => return,
                    Some(Cmd::Exited(code)) => {
                        while let Ok(Some(chunk)) =
                            tokio::time::timeout(DRAIN_TIMEOUT, output.recv()).await
                        {
                            self.on_output(&chunk);
                        }
                        self.on_exit(code);
                    }
                    Some(cmd) => self.handle(cmd),
                },
                chunk = output.recv(), if output_open => match chunk {
                    Some(chunk) => self.on_output(&chunk),
                    None => output_open = false,
                },
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Input(bytes) => self.write(bytes),
            Cmd::Resize { rows, cols } => self.resize(rows, cols),
            Cmd::Attach { sink, nudge } => self.attach(sink, nudge),
            Cmd::Scrollback {
                start,
                count,
                reply,
            } => {
                let _ = reply.send(self.screen.scrollback_rows(start, count));
            }
            Cmd::Hook { event, payload } => self.on_hook(&event, payload),
            Cmd::SetView(view) => {
                self.view = view;
                self.apply(Input::Viewed);
            }
            Cmd::MarkViewed => self.apply(Input::Viewed),
            Cmd::Exited(_) => {} // handled in `run`
        }
    }

    fn write(&self, bytes: Vec<u8>) {
        if self.exited || bytes.is_empty() {
            return;
        }
        if let Err(std_mpsc::TrySendError::Full(_)) = self.input.try_send(bytes) {
            tracing::warn!(session = %self.id, "input queue full; dropping input");
        }
    }

    fn pty_resize(&self, rows: u16, cols: u16) {
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        if let Err(e) = self.master.resize(size) {
            tracing::debug!(session = %self.id, "pty resize: {e}");
        }
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        if self.screen.size() == (rows, cols) {
            return;
        }
        self.screen.resize(rows, cols);
        self.pty_resize(rows, cols);
    }

    fn attach(&mut self, sink: ClientSink, nudge: bool) {
        let (rows, cols) = self.screen.size();
        let snapshot = DaemonMsg::Snapshot {
            session: self.id.clone(),
            rows,
            cols,
            bytes: self.screen.snapshot(),
        };
        if !sink.send(snapshot) {
            return;
        }
        self.client = Some(sink);
        // A resize to cols-1 and back raises SIGWINCH twice so the child
        // repaints. Only the PTY is touched: the screen keeps its size.
        if nudge && !self.exited && cols > 1 {
            self.pty_resize(rows, cols - 1);
            self.pty_resize(rows, cols);
        }
    }

    fn on_output(&mut self, chunk: &[u8]) {
        let replies = self.screen.process(chunk);
        self.write(replies);
        self.forward(DaemonMsg::Output {
            session: self.id.clone(),
            bytes: chunk.to_vec(),
        });
    }

    fn on_exit(&mut self, code: i32) {
        self.exited = true;
        self.deadline = None;
        self.info
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .exit_code = Some(code);
        self.apply(Input::Exited(code));
    }

    fn on_hook(&mut self, event: &str, payload: HookPayload) {
        // The id is client-chosen text: `{:?}` keeps control characters out.
        tracing::debug!("hook {event:?} session={:?}", self.id.0);
        if event == "SessionStart" {
            // Every SessionStart replaces the previous values (`/clear` makes a
            // new conversation); invalid fields are dropped, not stored.
            let checked = payload.clone().validated();
            let mut info = self.info.lock().unwrap_or_else(|e| e.into_inner());
            info.claude_session_id = checked.session_id;
            info.transcript_path = checked.transcript_path;
            info.model = checked.model;
        }
        self.apply(Input::Hook {
            event,
            notification_type: payload.notification_type.as_deref(),
        });
    }

    /// Feeds `input` to the status machine; a session the client has on
    /// screen is viewed at once, so a `Stop` seen live never needs attention.
    fn apply(&mut self, input: Input<'_>) {
        let before = self.status;
        let (mut to, fx) = status::next(before, input);
        if fx.unrecognized {
            tracing::debug!(session = ?self.id.0, "ignoring unrecognized hook input {input:?}");
        }
        // Decided on the status before the viewed shortcut: a `Stop` seen live
        // in an unfocused terminal still deserves a notification.
        self.maybe_notify(before, to);
        if self.on_screen() {
            to = status::next(to, Input::Viewed).0;
        }
        if to == before {
            return;
        }
        self.status = to;
        self.info.lock().unwrap_or_else(|e| e.into_inner()).status = to;
        self.forward(DaemonMsg::StatusChanged {
            session: self.id.clone(),
            status: to,
        });
    }

    /// The attached client has this session on screen.
    fn on_screen(&self) -> bool {
        self.view
            .as_ref()
            .is_some_and(|v| v.on_screen.as_ref() == Some(&self.id))
    }

    fn maybe_notify(&self, before: Status, to: Status) {
        if !notify_rule::should_notify(before, to, self.view.as_ref(), &self.id, self.notifications)
        {
            return;
        }
        let Some(summary) = notify_rule::summary(&self.repo_name, to) else {
            return;
        };
        self.notifier.send(Notification {
            session: self.id.clone(),
            status: to,
            summary,
            body: notify_rule::sanitize(&self.project),
        });
    }

    fn forward(&mut self, msg: DaemonMsg) {
        let Some(sink) = &self.client else { return };
        if !sink.send(msg) {
            tracing::warn!(session = %self.id, "client too slow or gone; detaching it");
            self.client = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(n: u8) -> DaemonMsg {
        DaemonMsg::Output {
            session: SessionId("s".into()),
            bytes: vec![n],
        }
    }

    #[test]
    fn overflowing_queue_kicks_the_client() {
        let (tx, mut rx) = mpsc::channel(2);
        let sink = ClientSink {
            tx,
            kick: CancellationToken::new(),
        };
        assert!(sink.send(out(1)) && sink.send(out(2)));
        assert!(!sink.kick.is_cancelled());
        assert!(!sink.send(out(3)));
        assert!(sink.kick.is_cancelled());
        assert_eq!(rx.try_recv().ok(), Some(out(1)));
    }

    #[test]
    fn closed_queue_drops_without_kick() {
        let (tx, rx) = mpsc::channel(2);
        drop(rx);
        let sink = ClientSink {
            tx,
            kick: CancellationToken::new(),
        };
        assert!(!sink.send(out(1)));
        assert!(!sink.kick.is_cancelled());
    }
}
