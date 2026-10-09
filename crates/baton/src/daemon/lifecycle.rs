//! Single-instance lock, socket binding and child-process cleanup.

use nix::fcntl::{Flock, FlockArg};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::fs::OpenOptions;
use std::io;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::Path;
use std::time::Duration;
use tokio::net::UnixListener;

/// Held for the daemon's lifetime; dropping it releases the lock.
pub type DaemonLock = Flock<std::fs::File>;

/// Takes the exclusive `flock` on `<run_dir>/baton.lock`.
///
/// Returns `Ok(None)` when another daemon holds it.
///
/// # Errors
/// On I/O errors other than the lock being contended.
pub fn acquire_lock(run_dir: &Path) -> io::Result<Option<DaemonLock>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode_0600()
        .open(run_dir.join("baton.lock"))?;
    match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
        Ok(lock) => Ok(Some(lock)),
        Err((_, nix::errno::Errno::EWOULDBLOCK)) => Ok(None),
        Err((_, e)) => Err(io::Error::from(e)),
    }
}

/// Like [`acquire_lock`], but keeps trying for up to `wait` while the lock is
/// held: a daemon that is shutting down releases it just after removing its
/// socket, so a start racing a stop must not mistake it for a live daemon.
///
/// # Errors
/// On I/O errors other than the lock being contended.
pub fn acquire_lock_within(run_dir: &Path, wait: Duration) -> io::Result<Option<DaemonLock>> {
    let deadline = std::time::Instant::now() + wait;
    loop {
        if let Some(lock) = acquire_lock(run_dir)? {
            return Ok(Some(lock));
        }
        if std::time::Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

trait ModeExt {
    fn mode_0600(&mut self) -> &mut Self;
}

impl ModeExt for OpenOptions {
    fn mode_0600(&mut self) -> &mut Self {
        std::os::unix::fs::OpenOptionsExt::mode(self, 0o600)
    }
}

/// Binds the socket at `path` with mode 0600, replacing a stale socket file.
///
/// Callers must hold the daemon lock, which is what makes removing the old
/// file safe. Anything at `path` that is not a socket is left alone and the
/// bind fails.
///
/// # Errors
/// On bind or permission errors.
pub fn bind_socket(path: &Path) -> io::Result<UnixListener> {
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && meta.file_type().is_socket()
    {
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Removes the socket file, ignoring a missing file.
pub fn remove_socket(path: &Path) {
    if let Err(e) = std::fs::remove_file(path)
        && e.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!("removing socket: {e}");
    }
}

fn group_alive(pgid: Pid) -> bool {
    !matches!(killpg(pgid, None), Err(nix::errno::Errno::ESRCH))
}

/// Sends SIGHUP to each process group, then SIGKILL to survivors after `grace`.
pub async fn terminate_groups(pgids: &[Pid], grace: Duration) {
    for &g in pgids {
        let _ = killpg(g, Signal::SIGHUP);
    }
    let deadline = tokio::time::Instant::now() + grace;
    while tokio::time::Instant::now() < deadline && pgids.iter().any(|&g| group_alive(g)) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    for &g in pgids {
        if group_alive(g) {
            let _ = killpg(g, Signal::SIGKILL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::Command;

    #[test]
    fn second_lock_is_refused_until_first_dropped() {
        let t = tempfile::tempdir().unwrap();
        let first = acquire_lock(t.path()).unwrap();
        assert!(first.is_some());
        assert!(acquire_lock(t.path()).unwrap().is_none());
        drop(first);
        assert!(acquire_lock(t.path()).unwrap().is_some());
    }

    #[test]
    fn waiting_for_the_lock_outlasts_a_daemon_that_is_exiting() {
        let t = tempfile::tempdir().unwrap();
        let first = acquire_lock(t.path()).unwrap().expect("first lock");
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(first);
        });
        let started = std::time::Instant::now();
        assert!(
            acquire_lock_within(t.path(), Duration::from_secs(2))
                .unwrap()
                .is_some()
        );
        assert!(started.elapsed() >= Duration::from_millis(100));
        releaser.join().unwrap();
    }

    #[test]
    fn waiting_for_a_lock_that_stays_held_gives_up() {
        let t = tempfile::tempdir().unwrap();
        let _held = acquire_lock(t.path()).unwrap().expect("first lock");
        let started = std::time::Instant::now();
        assert!(
            acquire_lock_within(t.path(), Duration::from_millis(150))
                .unwrap()
                .is_none()
        );
        assert!(started.elapsed() >= Duration::from_millis(150));
    }

    #[tokio::test]
    async fn bind_replaces_stale_socket_and_sets_0600() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("s.sock");
        drop(std::os::unix::net::UnixListener::bind(&p).unwrap());
        let _l = bind_socket(&p).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[tokio::test]
    async fn bind_refuses_to_replace_regular_file() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("s.sock");
        std::fs::write(&p, b"x").unwrap();
        assert!(bind_socket(&p).is_err());
        assert!(p.exists());
    }

    #[tokio::test]
    async fn terminate_kills_group_even_if_hup_is_ignored() {
        let mut child = Command::new("sh")
            .args(["-c", "trap '' HUP; sleep 30"])
            .process_group(0)
            .spawn()
            .unwrap();
        let pgid = Pid::from_raw(i32::try_from(child.id()).unwrap());
        tokio::time::sleep(Duration::from_millis(200)).await; // let the trap install
        terminate_groups(&[pgid], Duration::from_millis(200)).await;
        assert_eq!(child.wait().unwrap().signal(), Some(9));
    }
}
