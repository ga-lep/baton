//! The async TUI loop: terminal events, daemon frames and the render pacer.

use std::time::{Duration, Instant};

use anyhow::Result;
use baton_proto::{ClientMsg, Role};
use crossterm::event::EventStream;
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use tokio::sync::oneshot::Receiver;

use super::app::{App, Effect};
use super::terminal_guard::TerminalGuard;
use super::{editor, git, sidebar, ui};
use crate::client::{self, ClientError, Conn};

type Term = Terminal<CrosstermBackend<std::io::Stdout>>;

/// Sleep used when nothing is scheduled.
const IDLE: Duration = Duration::from_secs(3600);
/// How often the sessions' git branches are re-read.
const BRANCH_POLL: Duration = Duration::from_secs(2);

/// Receives from the connection, or never completes when there is none.
async fn recv(conn: &mut Option<Conn>) -> Result<baton_proto::DaemonMsg, ClientError> {
    match conn {
        Some(c) => c.recv().await,
        None => std::future::pending().await,
    }
}

fn host_rect(terminal: &Term) -> Result<Rect> {
    Ok(Rect::from((Default::default(), terminal.size()?)))
}

/// Result of trying to reach the daemon.
enum Link {
    Up(Conn),
    Mismatch(u32),
    Down,
}

/// Attaches a freshly handshaken connection with the panel size.
async fn attach(mut conn: Conn, size: (u16, u16)) -> Link {
    let msg = ClientMsg::Attach {
        rows: size.0,
        cols: size.1,
    };
    match conn.send(&msg).await {
        Ok(()) => Link::Up(conn),
        Err(_) => Link::Down,
    }
}

async fn link(result: Result<Conn, ClientError>, size: (u16, u16)) -> Result<Link> {
    match result {
        Ok(conn) => Ok(attach(conn, size).await),
        Err(ClientError::VersionMismatch { daemon_version }) => Ok(Link::Mismatch(daemon_version)),
        Err(ClientError::NotRunning | ClientError::Closed | ClientError::Io(_)) => Ok(Link::Down),
        Err(e) => Err(e.into()),
    }
}

/// Moves `app` and `conn` to the state `l` describes.
fn settle(l: Link, app: &mut App, conn: &mut Option<Conn>) {
    match l {
        Link::Up(c) => {
            app.on_connected();
            match sidebar::load_settings() {
                Ok(settings) => {
                    app.set_projects(settings.projects);
                    app.set_keymap(settings.keymap);
                    app.set_editor(settings.editor);
                }
                Err(e) => app.on_config_error(e),
            }
            *conn = Some(c);
        }
        Link::Mismatch(v) => {
            *conn = None;
            app.on_mismatch(v);
        }
        Link::Down => {
            *conn = None;
            app.on_disconnected();
        }
    }
}

/// Performs `effects`; returns `true` when the TUI should exit.
async fn apply(effects: Vec<Effect>, app: &mut App, conn: &mut Option<Conn>) -> Result<bool> {
    for effect in effects {
        match effect {
            Effect::Send(msg) => {
                if let Some(c) = conn
                    && c.send(&msg).await.is_err()
                {
                    *conn = None;
                    app.on_disconnected();
                }
            }
            Effect::Quit => {
                if let Some(c) = conn {
                    // Best effort: the sessions live on in the daemon either way.
                    let _ = c.send(&ClientMsg::Detach).await;
                }
                return Ok(true);
            }
            Effect::Reconnect => {
                let l = link(client::ensure_daemon(Role::Tui).await, app.size()).await?;
                settle(l, app, conn);
            }
            Effect::OpenEditor { template, path } => {
                if let Err(e) = editor::open(&template, &path) {
                    app.on_editor_error(e.to_string(), Instant::now());
                }
            }
            Effect::RestartDaemon => {
                let l = link(client::restart_daemon(Role::Tui).await, app.size()).await?;
                settle(l, app, conn);
            }
        }
    }
    Ok(false)
}

/// Runs the TUI: connect, take over the terminal, loop until quit.
///
/// # Errors
/// If the daemon cannot be started (before the screen is touched) or on
/// terminal I/O errors.
pub async fn run() -> Result<()> {
    // Connect before entering the alternate screen so failures print normally.
    let first = match client::ensure_daemon(Role::Tui).await {
        Err(e) if !matches!(e, ClientError::VersionMismatch { .. }) => return Err(e.into()),
        other => other,
    };
    let guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut app = App::new(host_rect(&terminal)?);
    let mut conn = None;
    let startup = crate::update::startup();
    app.update_available = startup.available;
    let result = match link(first, app.size()).await {
        Ok(l) => {
            settle(l, &mut app, &mut conn);
            drive(&mut app, &mut conn, &mut terminal, startup.pending).await
        }
        Err(e) => Err(e),
    };
    drop(terminal);
    drop(guard);
    result
}

/// Awaits the background update check, or never completes when there is none.
async fn update_result(rx: &mut Option<Receiver<Option<String>>>) -> Option<String> {
    match rx {
        // A dropped sender (thread died) counts as "nothing found".
        Some(r) => r.await.unwrap_or(None),
        None => std::future::pending().await,
    }
}

async fn drive(
    app: &mut App,
    conn: &mut Option<Conn>,
    terminal: &mut Term,
    mut update_rx: Option<Receiver<Option<String>>>,
) -> Result<()> {
    let mut events = EventStream::new();
    // When the branches were last read, and for how many sessions (a new
    // session gets its branch at once rather than at the next poll).
    let mut branches_read: Option<(Instant, usize)> = None;
    loop {
        app.expire(Instant::now());
        if branches_read.is_none_or(|(t, n)| t.elapsed() >= BRANCH_POLL || n != app.sessions.len())
        {
            app.refresh_branches(git::branch);
            branches_read = Some((Instant::now(), app.sessions.len()));
        }
        let until_poll = branches_read.map_or(Duration::ZERO, |(t, _)| {
            BRANCH_POLL.saturating_sub(t.elapsed())
        });
        let wait = app
            .next_deadline(Instant::now())
            .unwrap_or(IDLE)
            .min(until_poll);
        let effects = tokio::select! {
            ev = events.next() => match ev {
                Some(Ok(ev)) => app.on_event(ev, host_rect(terminal)?),
                Some(Err(e)) => return Err(e.into()),
                None => return Ok(()),
            },
            msg = recv(conn) => match msg {
                Ok(msg) => app.on_daemon(msg, Instant::now()),
                Err(e) => {
                    tracing::debug!("daemon connection lost: {e}");
                    *conn = None;
                    app.on_disconnected();
                    Vec::new()
                }
            },
            found = update_result(&mut update_rx) => {
                update_rx = None;
                if found.is_some() {
                    app.update_available = found;
                    app.pacer.mark_dirty();
                }
                Vec::new()
            },
            () = tokio::time::sleep(wait) => Vec::new(),
        };
        if apply(effects, app, conn).await? {
            return Ok(());
        }
        let now = Instant::now();
        if app.pacer.should_render(now) {
            terminal.draw(|f| ui::draw(f, app))?;
            app.pacer.rendered(Instant::now());
        }
    }
}
