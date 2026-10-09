//! Daemon client: connect, handshake and `ensure_daemon()` auto-spawn.

use baton_core::paths;
use baton_proto::{
    ClientMsg, DaemonMsg, PROTOCOL_VERSION, ProtoError, Role, decode, encode, framed,
};
use futures_util::{SinkExt, StreamExt};
use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
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
    #[error("refusing to talk to a socket served by uid {server_uid} (expected {expected_uid})")]
    UntrustedServer { server_uid: u32, expected_uid: u32 },
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
/// The runtime directory must be private (see [`paths::ensure_runtime_dir`])
/// before anything is connected to, and the server must run as our user.
///
/// # Errors
/// [`ClientError::NotRunning`] if nothing is listening,
/// [`ClientError::UntrustedServer`] if another user serves the socket, or any
/// directory-check or handshake failure.
pub async fn connect(role: Role) -> Result<Conn, ClientError> {
    connect_to(&paths::socket_path(), role).await
}

/// Like [`connect`], for the socket at `path` (the runtime directory is still
/// checked, and the server must still run as our user).
///
/// # Errors
/// As for [`connect`].
pub async fn connect_to(path: &Path, role: Role) -> Result<Conn, ClientError> {
    paths::ensure_runtime_dir()?;
    connect_at(path, role, nix::unistd::getuid().as_raw()).await
}

/// Connects to the socket at `path`, requiring the server's uid to be
/// `expected_uid` (checked before any byte is sent).
async fn connect_at(path: &Path, role: Role, expected_uid: u32) -> Result<Conn, ClientError> {
    let stream = UnixStream::connect(path)
        .await
        .map_err(|e| match e.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => ClientError::NotRunning,
            _ => ClientError::Io(e),
        })?;
    let server_uid = stream.peer_cred()?.uid();
    if server_uid != expected_uid {
        return Err(ClientError::UntrustedServer {
            server_uid,
            expected_uid,
        });
    }
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

/// How long a SIGTERMed daemon gets to exit before `restart_daemon` gives up.
const RESTART_STOP_TIMEOUT: Duration = Duration::from_secs(8);

/// Whether two `/proc/<pid>/exe` style paths name the same binary, ignoring
/// the kernel's ` (deleted)` suffix (a rebuilt binary replaces the file).
fn exe_matches(a: &Path, b: &Path) -> bool {
    fn clean(p: &Path) -> &str {
        let s = p.to_str().unwrap_or_default();
        s.strip_suffix(" (deleted)").unwrap_or(s)
    }
    let (a, b) = (clean(a), clean(b));
    !a.is_empty() && a == b
}

/// Whether `pid` is a live process running the same executable as this one.
/// Guards `restart_daemon` against signalling an arbitrary process.
fn is_our_daemon(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    let (Ok(theirs), Ok(ours)) = (
        std::fs::read_link(format!("/proc/{pid}/exe")),
        std::env::current_exe(),
    ) else {
        return false;
    };
    exe_matches(&theirs, &ours)
}

/// Stops the running daemon (found through the socket's peer credentials, so
/// it works across protocol versions) and starts a fresh one.
///
/// # Errors
/// If the socket is served by another user or by a process that is not this
/// binary, the old daemon does not exit, or
/// the new one cannot be started.
pub async fn restart_daemon(role: Role) -> Result<Conn, ClientError> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    paths::ensure_runtime_dir()?;
    let sock = paths::socket_path();
    if let Ok(stream) = UnixStream::connect(&sock).await {
        let cred = stream.peer_cred()?;
        let expected_uid = nix::unistd::getuid().as_raw();
        if cred.uid() != expected_uid {
            return Err(ClientError::UntrustedServer {
                server_uid: cred.uid(),
                expected_uid,
            });
        }
        drop(stream);
        let raw = cred
            .pid()
            .ok_or_else(|| io::Error::other("cannot identify the daemon process"))?;
        if !is_our_daemon(raw) {
            return Err(ClientError::Io(io::Error::other(format!(
                "pid {raw} serving the socket is not a baton binary; not signalling it"
            ))));
        }
        let pid = Pid::from_raw(raw);
        // SIGTERM makes the daemon terminate its sessions and exit.
        let _ = kill(pid, Signal::SIGTERM);
        let deadline = Instant::now() + RESTART_STOP_TIMEOUT;
        while kill(pid, None).is_ok() {
            if Instant::now() >= deadline {
                return Err(ClientError::Io(io::Error::other(
                    "old daemon did not stop in time",
                )));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    ensure_daemon(role).await
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixListener;

    fn serve_one(listener: UnixListener) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            // Accept and hold the connection open, never answering.
            if let Ok((s, _)) = listener.accept().await {
                tokio::time::sleep(Duration::from_millis(500)).await;
                drop(s);
            }
        })
    }

    #[test]
    fn exe_match_ignores_deleted_suffix_and_rejects_other_binaries() {
        let p = Path::new;
        assert!(exe_matches(p("/a/baton"), p("/a/baton")));
        assert!(exe_matches(p("/a/baton (deleted)"), p("/a/baton")));
        assert!(exe_matches(p("/a/baton"), p("/a/baton (deleted)")));
        assert!(!exe_matches(p("/usr/bin/sleep"), p("/a/baton")));
    }

    #[test]
    fn daemon_pid_must_be_positive_and_run_our_binary() {
        assert!(!is_our_daemon(0));
        assert!(!is_our_daemon(-1));
        assert!(!is_our_daemon(i32::MAX));
        let me = i32::try_from(std::process::id()).expect("pid fits");
        assert!(is_our_daemon(me));
        // init is not us.
        assert!(!is_our_daemon(1));
    }

    #[tokio::test]
    async fn refuses_a_server_running_as_another_uid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let server = serve_one(UnixListener::bind(&path).unwrap());
        let ours = nix::unistd::getuid().as_raw();
        // Pretend we expect a different user: the real server (us) is untrusted.
        let r = connect_at(&path, Role::Ctl, ours.wrapping_add(1)).await;
        assert!(
            matches!(r, Err(ClientError::UntrustedServer { server_uid, .. }) if server_uid == ours),
            "{:?}",
            r.err()
        );
        server.abort();
    }

    #[tokio::test]
    async fn same_uid_server_gets_past_the_peer_check() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let _server = serve_one(UnixListener::bind(&path).unwrap());
        let ours = nix::unistd::getuid().as_raw();
        // The fake server never answers Hello, so the handshake fails later
        // (EOF or reset), but never with a uid error.
        let r = connect_at(&path, Role::Ctl, ours).await;
        assert!(
            matches!(r, Err(ClientError::Closed | ClientError::Io(_))),
            "{:?}",
            r.err()
        );
    }
}
