//! Socket accept loop and per-connection protocol handling.

use baton_proto::{ClientMsg, DaemonMsg, PROTOCOL_VERSION, decode, encode, framed};
use futures_util::{SinkExt, StreamExt};
use nix::unistd::Pid;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// A client must send `Hello` within this long.
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum simultaneous connections.
const MAX_CONNECTIONS: usize = 64;

/// Shared daemon state.
#[derive(Default)]
pub struct State {
    child_groups: Mutex<Vec<Pid>>,
}

impl State {
    /// Records a child process group to terminate at shutdown.
    #[allow(dead_code)] // used by the session runtime in a later task
    pub fn register_child_group(&self, pgid: Pid) {
        self.groups().push(pgid);
    }

    /// Takes all registered child process groups.
    pub fn take_child_groups(&self) -> Vec<Pid> {
        std::mem::take(&mut *self.groups())
    }

    fn groups(&self) -> std::sync::MutexGuard<'_, Vec<Pid>> {
        self.child_groups.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Accepts connections until `shutdown` is cancelled.
pub async fn serve(listener: UnixListener, shutdown: CancellationToken) {
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
        tokio::spawn(async move {
            handle(stream, &shutdown).await;
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

async fn handle(stream: UnixStream, shutdown: &CancellationToken) {
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
    loop {
        let msg = tokio::select! {
            () = shutdown.cancelled() => return,
            m = recv(&mut conn) => match m {
                Some(m) => m,
                None => return,
            },
        };
        let reply = match msg {
            ClientMsg::Status => DaemonMsg::DaemonStatus {
                pid: std::process::id(),
                version: PROTOCOL_VERSION,
                sessions: Vec::new(),
            },
            ClientMsg::Shutdown => {
                tracing::info!("shutdown requested");
                shutdown.cancel();
                return;
            }
            ClientMsg::Detach => return,
            ClientMsg::Hello { .. } => return, // second Hello is a protocol violation
            _ => DaemonMsg::Error {
                message: "not supported yet".into(),
            },
        };
        if !send(&mut conn, &reply).await {
            return;
        }
    }
}
