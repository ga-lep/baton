//! Daemon client: connect, handshake and `ensure_daemon()` auto-spawn.

use baton_core::paths;
use baton_proto::{
    ClientMsg, DaemonMsg, PROTOCOL_VERSION, ProtoError, Role, decode, encode, framed,
};
use futures_util::{SinkExt, StreamExt};
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tokio::net::UnixStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

/// How long to wait for a freshly spawned daemon to accept connections.
pub const START_TIMEOUT: Duration = Duration::from_secs(5);

/// Errors talking to the daemon.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("daemon is not running")]
    NotRunning,
    #[error(
        "protocol version mismatch (daemon speaks {daemon_version}, client {PROTOCOL_VERSION})"
    )]
    VersionMismatch { daemon_version: u32 },
    #[error("daemon closed the connection")]
    Closed,
    #[error("unexpected reply from daemon: {0:?}")]
    Unexpected(Box<DaemonMsg>),
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
    #[error("protocol: {0}")]
    Proto(#[from] ProtoError),
}

/// An established, handshaken connection.
pub struct Conn {
    framed: Framed<UnixStream, LengthDelimitedCodec>,
    /// Daemon pid from `Welcome`.
    pub pid: u32,
}

impl Conn {
    /// Sends one message.
    ///
    /// # Errors
    /// On encode or write failure.
    pub async fn send(&mut self, msg: &ClientMsg) -> Result<(), ClientError> {
        self.framed.send(encode(msg)?).await?;
        Ok(())
    }

    /// Receives one message.
    ///
    /// # Errors
    /// On EOF, read failure or an undecodable frame.
    pub async fn recv(&mut self) -> Result<DaemonMsg, ClientError> {
        match self.framed.next().await {
            Some(frame) => Ok(decode(&frame?)?),
            None => Err(ClientError::Closed),
        }
    }
}

/// Connects to the daemon socket and performs the `Hello` handshake.
///
/// # Errors
/// [`ClientError::NotRunning`] if nothing is listening, or any handshake failure.
pub async fn connect(role: Role) -> Result<Conn, ClientError> {
    let stream = UnixStream::connect(paths::socket_path())
        .await
        .map_err(|e| match e.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => ClientError::NotRunning,
            _ => ClientError::Io(e),
        })?;
    let mut conn = Conn {
        framed: framed(stream),
        pid: 0,
    };
    conn.send(&ClientMsg::Hello {
        version: PROTOCOL_VERSION,
        role,
    })
    .await?;
    match conn.recv().await? {
        DaemonMsg::Welcome { pid, .. } => {
            conn.pid = pid;
            Ok(conn)
        }
        DaemonMsg::VersionMismatch { daemon_version } => {
            Err(ClientError::VersionMismatch { daemon_version })
        }
        other => Err(ClientError::Unexpected(Box::new(other))),
    }
}

/// Runs `<this exe> daemon start` with null stdio; the daemon detaches itself.
///
/// # Errors
/// If the executable cannot be found or spawned.
pub fn spawn_daemon_start() -> io::Result<()> {
    Command::new(std::env::current_exe()?)
        .args(["daemon", "start"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    Ok(())
}

/// Detached re-exec of `baton daemon start --foreground` in its own session.
///
/// # Errors
/// If the executable cannot be found or spawned.
pub fn spawn_detached_foreground() -> io::Result<()> {
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.args(["daemon", "start", "--foreground"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: the closure runs between fork and exec and only calls the
    // async-signal-safe `setsid`; it allocates nothing and touches no locks.
    unsafe {
        cmd.pre_exec(|| nix::unistd::setsid().map(drop).map_err(io::Error::from));
    }
    // The child is intentionally not waited on: it is a long-lived daemon.
    cmd.spawn().map(drop)
}

/// Retries `connect` with backoff until `timeout` elapses.
///
/// # Errors
/// The last connect error once the timeout is reached.
pub async fn connect_with_retry(role: Role, timeout: Duration) -> Result<Conn, ClientError> {
    let deadline = Instant::now() + timeout;
    let mut delay = Duration::from_millis(10);
    loop {
        match connect(role).await {
            Ok(c) => return Ok(c),
            Err(ClientError::NotRunning | ClientError::Closed) if Instant::now() < deadline => {
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_millis(200));
            }
            Err(e) => return Err(e),
        }
    }
}

/// Connects, or spawns `baton daemon start` and retries for up to 5 s.
///
/// # Errors
/// If the daemon cannot be spawned or never accepts the connection.
#[allow(dead_code)] // used by the TUI and hook clients in later tasks
pub async fn ensure_daemon(role: Role) -> Result<Conn, ClientError> {
    match connect(role).await {
        Err(ClientError::NotRunning) => {
            tokio::task::spawn_blocking(spawn_daemon_start)
                .await
                .map_err(|e| ClientError::Io(io::Error::other(e)))??;
            connect_with_retry(role, START_TIMEOUT).await
        }
        other => other,
    }
}
