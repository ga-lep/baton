//! Socket accept loop and per-connection protocol handling.

use super::notifier::{self, Notifier, SinkKind};
use super::registry::{CLIENT_QUEUE, ChildGroups, Registry, validate_size};
use super::session::ClientSink;
use baton_proto::{ClientMsg, DaemonMsg, MAX_INPUT, PROTOCOL_VERSION, decode, encode, framed};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;

/// A client must send `Hello` within this long.
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum simultaneous connections.
const MAX_CONNECTIONS: usize = 64;
/// How long to wait to deliver the slow-client notice before closing anyway.
const KICK_NOTICE_TIMEOUT: Duration = Duration::from_secs(2);

/// Shared daemon state.
pub struct State {
    /// Process groups to terminate at shutdown.
    pub groups: Arc<ChildGroups>,
    /// Live sessions and the attached client.
    pub registry: Registry,
    next_conn: AtomicU64,
}

impl State {
    /// Creates empty state.
    pub fn new() -> Self {
        let groups = Arc::new(ChildGroups::default());
        let kind = SinkKind::parse(std::env::var(notifier::SINK_ENV).ok().as_deref());
        // Without a usable state dir only the (stateless) D-Bus sink can work.
        let state_dir = baton_core::paths::state_dir().unwrap_or_default();
        let notifier = Notifier::spawn(kind.build(&state_dir));
        Self {
            registry: Registry::new(Arc::clone(&groups), notifier),
            groups,
            next_conn: AtomicU64::new(1),
        }
    }
}

/// Accepts connections until `shutdown` is cancelled.
pub async fn serve(listener: UnixListener, shutdown: CancellationToken, state: Arc<State>) {
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let stream = tokio::select! {
            () = shutdown.cancelled() => return,
            accepted = listener.accept() => match accepted {
                Ok((s, _)) => s,
                Err(e) => {
                    tracing::warn!("accept failed: {e}");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            },
        };
        if !same_user(&stream) {
            continue; // dropped: closes the connection
        }
        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
            tracing::warn!("too many connections; dropping one");
            continue;
        };
        let shutdown = shutdown.clone();
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            handle(stream, &shutdown, &state).await;
            drop(permit);
        });
    }
}

/// True if the peer's uid (SO_PEERCRED) equals ours.
fn same_user(stream: &UnixStream) -> bool {
    match stream.peer_cred() {
        Ok(cred) if cred.uid() == nix::unistd::getuid().as_raw() => true,
        Ok(cred) => {
            tracing::warn!(uid = cred.uid(), "rejecting connection from another user");
            false
        }
        Err(e) => {
            tracing::warn!("peer_cred failed: {e}");
            false
        }
    }
}

type Conn = tokio_util::codec::Framed<UnixStream, tokio_util::codec::LengthDelimitedCodec>;

async fn send(conn: &mut Conn, msg: &DaemonMsg) -> bool {
    match encode(msg) {
        Ok(b) => conn.send(b).await.is_ok(),
        Err(e) => {
            tracing::warn!("encoding reply: {e}");
            false
        }
    }
}

/// Reads and decodes one frame; `None` on EOF, I/O error or invalid frame.
async fn recv(conn: &mut Conn) -> Option<ClientMsg> {
    match conn.next().await? {
        Ok(frame) => match decode(&frame) {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::debug!("invalid frame: {e}");
                None
            }
        },
        Err(e) => {
            tracing::debug!("read error: {e}");
            None
        }
    }
}

async fn handle(stream: UnixStream, shutdown: &CancellationToken, state: &State) {
    let mut conn = framed(stream);
    // Hello.role is informational only; authorization is the uid check.
    match tokio::time::timeout(HELLO_TIMEOUT, recv(&mut conn)).await {
        Ok(Some(ClientMsg::Hello { version, .. })) if version == PROTOCOL_VERSION => {
            let welcome = DaemonMsg::Welcome {
                version: PROTOCOL_VERSION,
                pid: std::process::id(),
            };
            if !send(&mut conn, &welcome).await {
                return;
            }
        }
        Ok(Some(ClientMsg::Hello { .. })) => {
            let _ = send(
                &mut conn,
                &DaemonMsg::VersionMismatch {
                    daemon_version: PROTOCOL_VERSION,
                },
            )
            .await;
            return;
        }
        _ => return,
    }
    let conn_id = state.next_conn.fetch_add(1, Ordering::Relaxed);
    // The stream channel exists once attached; only the registry and the
    // sessions hold senders, so a takeover closes it and ends this connection.
    let mut stream_rx = mpsc::channel::<DaemonMsg>(1).1;
    let mut attached = false;
    let mut kick = CancellationToken::new();
    loop {
        let msg = tokio::select! {
            biased;
            () = shutdown.cancelled() => break,
            () = kick.cancelled() => {
                // The queue overflowed: say why, then close so the client
                // notices and can reattach.
                let _ = tokio::time::timeout(
                    KICK_NOTICE_TIMEOUT,
                    send(&mut conn, &error("client too slow; disconnected")),
                )
                .await;
                break;
            }
            streamed = stream_rx.recv(), if attached => match streamed {
                Some(m) => {
                    if !send(&mut conn, &m).await {
                        break;
                    }
                    continue;
                }
                None => break,
            },
            m = recv(&mut conn) => match m {
                Some(m) => m,
                None => break,
            },
        };
        let reply = match msg {
            ClientMsg::Status => Some(DaemonMsg::DaemonStatus {
                pid: std::process::id(),
                version: PROTOCOL_VERSION,
                sessions: state.registry.list(),
            }),
            ClientMsg::Shutdown => {
                tracing::info!("shutdown requested");
                shutdown.cancel();
                break;
            }
            ClientMsg::Detach | ClientMsg::Hello { .. } => break, // Hello again: protocol violation
            ClientMsg::OpenProject { name } => match state.registry.open_project(&name) {
                // An attached client is pushed the full list; replying with
                // only this project's sessions would overwrite it.
                Ok(_) if attached => None,
                Ok(list) => Some(DaemonMsg::SessionList(list)),
                Err(e) => Some(error(e)),
            },
            ClientMsg::Attach { rows, cols } => match validate_size(rows, cols) {
                Ok((rows, cols)) => {
                    let (tx, rx) = mpsc::channel(CLIENT_QUEUE);
                    stream_rx = rx;
                    attached = true;
                    kick = CancellationToken::new();
                    let sink = ClientSink {
                        tx,
                        kick: kick.clone(),
                    };
                    state.registry.attach(conn_id, rows, cols, &sink);
                    None
                }
                Err(e) => Some(error(e)),
            },
            ClientMsg::Resize { rows, cols } => match validate_size(rows, cols) {
                Ok((rows, cols)) => {
                    state.registry.resize_all(rows, cols);
                    None
                }
                Err(e) => Some(error(e)),
            },
            ClientMsg::Input { bytes, .. } if bytes.len() > MAX_INPUT => Some(error(format!(
                "input of {} bytes exceeds the {MAX_INPUT}-byte limit",
                bytes.len()
            ))),
            ClientMsg::Input { session, bytes } => {
                state.registry.input(&session, bytes).err().map(error)
            }
            ClientMsg::GetScrollback {
                session,
                start,
                count,
            } => Some(
                match state.registry.scrollback(&session, start, count).await {
                    Ok(rows) => DaemonMsg::Scrollback {
                        session,
                        start,
                        rows,
                    },
                    Err(e) => error(e),
                },
            ),
            ClientMsg::Hook {
                baton_session,
                event,
                payload_json,
            } => {
                // Fire and forget: the hook client does not wait for a reply.
                state.registry.hook(&baton_session, &event, &payload_json);
                None
            }
            ClientMsg::ClientView {
                on_screen,
                terminal_focused,
            } => {
                state
                    .registry
                    .set_view(conn_id, on_screen, terminal_focused);
                None
            }
            ClientMsg::MarkViewed { session } => {
                state.registry.mark_viewed(&session).err().map(error)
            }
            _ => Some(error("not supported yet")),
        };
        if let Some(reply) = reply
            && !send(&mut conn, &reply).await
        {
            break;
        }
    }
    state.registry.detach(conn_id);
}

fn error(e: impl ToString) -> DaemonMsg {
    DaemonMsg::Error {
        message: e.to_string(),
    }
}
