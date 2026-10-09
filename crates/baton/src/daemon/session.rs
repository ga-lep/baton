//! One session: a PTY child, its authoritative screen and the attached client.

use super::notifier::{Notification, Notifier};
use super::persist::{Store, now_secs};
use super::registry::ChildGroups;
use super::{lifecycle, spawn, tailer};
use crate::term::screen::Screen;
use crate::term::vt100_screen::Vt100Screen;
use anyhow::{Result, bail};
use baton_core::config::SessionSpec;
use baton_core::hooks::HookPayload;
use baton_core::launch::{self, Rung};
use baton_core::notify_rule::{self, ClientView};
use baton_core::pricing::Pricing;
use baton_core::state::PersistedSession;
use baton_core::status::{self, Input};
use baton_proto::{DaemonMsg, Quota, SessionId, SessionInfo, Status, Usage};
use nix::unistd::Pid;
use portable_pty::PtySize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};
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
    /// The child of generation `generation` was reaped with `code`.
    Exited {
        generation: u64,
        code: i32,
    },
    /// Kill the child (if any) and launch it again, resuming the conversation.
    Restart,
    /// A hook fired in the child.
    Hook {
        event: String,
        payload: HookPayload,
    },
    /// What the attached client shows; `None` when no client is attached.
    SetView(Option<ClientView>),
    /// The user explicitly marks the session as seen.
    MarkViewed,
    /// The transcript tailer's latest result (`None`: unreadable, shown as `n/a`).
    Usage(Option<Usage>),
    /// The account's subscription quota, reported by this or a sibling session.
    Quota(Quota),
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
    /// The conversation id remembered from an earlier run, if any.
    pub resume_id: Option<String>,
    /// Where session metadata is persisted.
    pub store: &'a Arc<Store>,
    /// Price table and context windows for the usage summary.
    pub pricing: &'a Pricing,
}

/// What a child process needs to be (re)started, kept for the session's life.
struct Ctx {
    id: SessionId,
    spec: SessionSpec,
    sock: std::path::PathBuf,
    hooks: std::path::PathBuf,
    groups: Arc<ChildGroups>,
    cmd_tx: mpsc::UnboundedSender<Cmd>,
    out_tx: mpsc::Sender<(u64, Vec<u8>)>,
    store: Arc<Store>,
    hook_timeout: Duration,
}

/// The current child: its PTY master, input queue and process group.
struct Proc {
    master: Box<dyn portable_pty::MasterPty + Send>,
    input: std_mpsc::SyncSender<Vec<u8>>,
    pgid: Pid,
}

/// Spawns the child, its helper threads and the session task.
///
/// Must be called from within the tokio runtime.
///
/// # Errors
/// If the child cannot be started or its process group cannot be tracked.
pub fn start(l: &Launch<'_>, groups: &Arc<ChildGroups>) -> Result<SessionHandle> {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (out_tx, out_rx) = mpsc::channel::<(u64, Vec<u8>)>(OUTPUT_QUEUE);
    let ctx = Ctx {
        id: l.id.clone(),
        spec: l.spec.clone(),
        sock: l.sock.to_path_buf(),
        hooks: l.hooks.to_path_buf(),
        groups: Arc::clone(groups),
        cmd_tx: cmd_tx.clone(),
        out_tx,
        store: Arc::clone(l.store),
        hook_timeout: l.hook_timeout,
    };
    let usage_path = spawn_tailer(l.spec, l.pricing, &cmd_tx);
    let rung = launch::first_rung(l.resume_id.as_deref());
    let generation = 1;
    let proc = launch_child(&ctx, &rung, generation, l.size)?;
    tracing::info!(session = ?l.id.0, launch = rung.label(), "launching");

    let started_at = now_secs();
    let info = Arc::new(Mutex::new(SessionInfo {
        id: l.id.clone(),
        project: l.spec.project.clone(),
        repo: l.spec.repo.display().to_string(),
        profile: Some(l.spec.profile.clone()),
        status: Status::Starting,
        claude_session_id: rung.known_session_id().map(str::to_owned),
        transcript_path: None,
        model: None,
        started_at,
        exit_code: None,
        usage: None,
        launch: Some(rung.label().to_owned()),
        quota: None,
    }));
    let task = Task {
        id: l.id.clone(),
        screen: Vt100Screen::new(l.size.0, l.size.1, l.scrollback),
        proc,
        generation,
        rung,
        launched_at: Instant::now(),
        saw_start: false,
        ctx,
        info: Arc::clone(&info),
        client: None,
        usage_path,
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
    task.persist();
    tokio::spawn(task.run(cmd_rx, out_rx));
    Ok(SessionHandle {
        id: l.id.clone(),
        info,
        tx: cmd_tx,
    })
}

/// Starts the transcript tailer for a session and returns the switch that
/// tells it which file to follow. Its results arrive as [`Cmd::Usage`]; it
/// ends with the session task.
fn spawn_tailer(
    spec: &SessionSpec,
    pricing: &Pricing,
    cmd_tx: &mpsc::UnboundedSender<Cmd>,
) -> watch::Sender<Option<PathBuf>> {
    let (path_tx, path_rx) = watch::channel(None);
    match tailer::projects_root(spec, &|k| std::env::var(k).ok()) {
        Some(root) => {
            let cmd_tx = cmd_tx.clone();
            tokio::spawn(tailer::run(
                tailer::Tailer::new(root, pricing.clone()),
                path_rx,
                move |usage| cmd_tx.send(Cmd::Usage(usage)).is_ok(),
            ));
        }
        None => tracing::debug!("no Claude config dir known; usage stays n/a"),
    }
    path_tx
}

/// Starts one child of generation `generation` with the arguments of `rung`, its
/// helper threads included. Output and the exit are tagged with `generation` so the
/// session can ignore a child it has already replaced.
fn launch_child(ctx: &Ctx, rung: &Rung, generation: u64, size: (u16, u16)) -> Result<Proc> {
    let mut sp = spawn::spawn(
        &ctx.spec,
        &ctx.id,
        &ctx.sock,
        &ctx.hooks,
        &rung.args(),
        size,
    )?;
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
    if let Err(e) = ctx.groups.register(pgid) {
        reap(&mut sp.child);
        bail!("{e}");
    }

    let (in_tx, in_rx) = std_mpsc::sync_channel::<Vec<u8>>(INPUT_QUEUE);
    let mut reader = sp.reader;
    let out_tx = ctx.out_tx.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if out_tx
                        .blocking_send((generation, buf[..n].to_vec()))
                        .is_err()
                    {
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
    let wait_tx = ctx.cmd_tx.clone();
    let wait_groups = Arc::clone(&ctx.groups);
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
        let _ = wait_tx.send(Cmd::Exited { generation, code });
    });
    Ok(Proc {
        master: sp.master,
        input: in_tx,
        pgid,
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
    proc: Proc,
    /// Generation of the current child; bumped at every relaunch.
    generation: u64,
    /// The rung the current child was started with.
    rung: Rung,
    launched_at: Instant,
    /// A `SessionStart` hook arrived from the current child.
    saw_start: bool,
    ctx: Ctx,
    info: Arc<Mutex<SessionInfo>>,
    client: Option<ClientSink>,
    /// Which transcript the tailer follows.
    usage_path: watch::Sender<Option<PathBuf>>,
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
        mut output: mpsc::Receiver<(u64, Vec<u8>)>,
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
                    // A child that was replaced (restart) is not this session's business.
                    Some(Cmd::Exited { generation, .. }) if generation != self.generation => {}
                    Some(Cmd::Exited { code, .. }) => {
                        while let Ok(Some((generation, chunk))) =
                            tokio::time::timeout(DRAIN_TIMEOUT, output.recv()).await
                        {
                            if generation == self.generation {
                                self.on_output(&chunk);
                            }
                        }
                        self.on_child_exit(code);
                    }
                    Some(Cmd::Restart) => self.restart().await,
                    Some(cmd) => self.handle(cmd),
                },
                chunk = output.recv(), if output_open => match chunk {
                    Some((generation, chunk)) if generation == self.generation => self.on_output(&chunk),
                    Some(_) => {}
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
            Cmd::Usage(usage) => {
                self.info.lock().unwrap_or_else(|e| e.into_inner()).usage = usage.clone();
                self.forward(DaemonMsg::UsageUpdated {
                    session: self.id.clone(),
                    usage,
                });
            }
            Cmd::Quota(quota) => {
                self.info.lock().unwrap_or_else(|e| e.into_inner()).quota = Some(quota);
                self.forward(DaemonMsg::QuotaUpdated {
                    session: self.id.clone(),
                    quota,
                });
            }
            Cmd::Exited { .. } | Cmd::Restart => {} // handled in `run`
        }
    }

    fn write(&self, bytes: Vec<u8>) {
        if self.exited || bytes.is_empty() {
            return;
        }
        if let Err(std_mpsc::TrySendError::Full(_)) = self.proc.input.try_send(bytes) {
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
        if let Err(e) = self.proc.master.resize(size) {
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

    /// The current child exited. An early failure moves down the launch
    /// ladder instead of ending the session while a rung is left.
    fn on_child_exit(&mut self, code: i32) {
        if launch::is_early_failure(code, self.launched_at.elapsed(), self.saw_start)
            && let Some(next) = self.rung.next(|| uuid::Uuid::new_v4().to_string())
        {
            tracing::info!(
                session = ?self.id.0,
                code,
                from = self.rung.label(),
                to = next.label(),
                "launch attempt failed early; trying the next rung"
            );
            match self.relaunch(next) {
                Ok(()) => return,
                Err(e) => tracing::warn!(session = ?self.id.0, "cannot relaunch: {e:#}"),
            }
        }
        self.on_exit(code);
    }

    /// Replaces the child: kills the old process group (if it is still ours
    /// and alive), then launches again from the first rung for the known
    /// conversation, keeping the screen object.
    async fn restart(&mut self) {
        let pgid = self.proc.pgid;
        // `contains` is false once the leader was reaped: the id may then
        // belong to an unrelated process and must not be signalled.
        if !self.exited && self.ctx.groups.contains(pgid) {
            tracing::info!(session = ?self.id.0, "restart: terminating the child's group");
            lifecycle::terminate_groups(&[pgid], super::KILL_GRACE).await;
        }
        let known = self
            .info
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .claude_session_id
            .clone();
        let rung = launch::first_rung(known.as_deref());
        if let Err(e) = self.relaunch(rung) {
            tracing::warn!(session = ?self.id.0, "restart failed: {e:#}");
            self.forward(DaemonMsg::Error {
                message: format!("restart failed: {e:#}"),
            });
        }
    }

    /// Starts a new child with `rung` in place of the current one.
    fn relaunch(&mut self, rung: Rung) -> Result<()> {
        let generation = self.generation + 1;
        let proc = launch_child(&self.ctx, &rung, generation, self.screen.size())?;
        tracing::info!(session = ?self.id.0, launch = rung.label(), "launching");
        self.proc = proc;
        self.generation = generation;
        self.exited = false;
        self.saw_start = false;
        self.launched_at = Instant::now();
        self.deadline = Some(self.launched_at + self.ctx.hook_timeout);
        // RIS: the new child starts on a clean screen (the replies are moot).
        self.screen.process(b"\x1bc");
        {
            let mut info = self.info.lock().unwrap_or_else(|e| e.into_inner());
            info.exit_code = None;
            info.launch = Some(rung.label().to_owned());
            // `--continue` does not know the id: keep the remembered one (and
            // its transcript) until a SessionStart reports the real one.
            info.claude_session_id =
                launch::id_after_launch(info.claude_session_id.as_deref(), &rung);
            if matches!(rung, Rung::Fresh(_)) {
                info.transcript_path = None;
                self.usage_path.send_replace(None);
            }
        }
        self.rung = rung;
        self.apply(Input::Spawn);
        self.persist();
        // The client's copy of the screen is stale: send it a fresh snapshot.
        if let Some(sink) = self.client.clone() {
            self.attach(sink, false);
        }
        Ok(())
    }

    /// Records the session in `state.json` (written back after a debounce).
    fn persist(&self) {
        let info = self.info.lock().unwrap_or_else(|e| e.into_inner()).clone();
        self.ctx.store.put(
            &self.id,
            PersistedSession {
                project: info.project,
                repo: info.repo,
                profile: info.profile,
                claude_session_id: info.claude_session_id,
                transcript_path: info.transcript_path,
                last_status: self.status,
                updated_at: now_secs(),
            },
        );
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
            let path = info.transcript_path.as_deref().map(PathBuf::from);
            drop(info);
            self.usage_path.send_if_modified(|cur| {
                let changed = *cur != path;
                *cur = path;
                changed
            });
            self.saw_start = true;
            self.persist();
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
        self.persist();
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
