//! End-to-end tests for the daemon lifecycle (all state in temp dirs).

use anyhow::Result;
use baton_proto::{ClientMsg, DaemonMsg, PROTOCOL_VERSION, Role, decode, encode};
use baton_testkit::{bin_path, wait_for};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::Duration;

/// Temp environment; stops the daemon on drop so no process outlives a test.
struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Result<Self> {
        Ok(Self {
            dir: tempfile::tempdir()?,
        })
    }

    fn run_dir(&self) -> PathBuf {
        self.dir.path().join("run")
    }

    fn sock(&self) -> PathBuf {
        self.run_dir().join("baton.sock")
    }

    fn baton(&self, args: &[&str]) -> Result<Output> {
        Ok(Command::new(bin_path("baton")?)
            .args(args)
            .env("BATON_CONFIG", self.dir.path().join("config.toml"))
            .env("BATON_STATE_DIR", self.dir.path().join("state"))
            .env("BATON_RUNTIME_DIR", self.run_dir())
            .env("BATON_NOTIFY_SINK", "off")
            .env("BATON_NO_UPDATE_CHECK", "1")
            .env_remove("BATON_UPDATE_URL")
            .stdin(std::process::Stdio::null())
            .output()?)
    }

    fn connect(&self) -> Result<UnixStream> {
        let s = UnixStream::connect(self.sock())?;
        s.set_read_timeout(Some(Duration::from_secs(5)))?;
        Ok(s)
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = self.baton(&["daemon", "stop"]);
    }
}

fn send(s: &mut UnixStream, m: &ClientMsg) -> Result<()> {
    let b = encode(m)?;
    s.write_all(&u32::try_from(b.len())?.to_be_bytes())?;
    s.write_all(&b)?;
    Ok(())
}

/// Reads one frame; `None` on EOF.
fn recv(s: &mut UnixStream) -> Result<Option<DaemonMsg>> {
    let mut len = [0u8; 4];
    if s.read_exact(&mut len).is_err() {
        return Ok(None);
    }
    let mut buf = vec![0u8; u32::from_be_bytes(len) as usize];
    s.read_exact(&mut buf)?;
    Ok(Some(decode(&buf)?))
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn start_status_start_stop_status() -> Result<()> {
    let env = Env::new()?;
    let out = env.baton(&["daemon", "start"])?;
    assert!(out.status.success());
    let started = text(&out);
    assert!(started.starts_with("daemon started pid="), "{started}");
    let pid = started.trim().trim_start_matches("daemon started pid=");

    let out = env.baton(&["daemon", "status"])?;
    assert!(out.status.success());
    assert_eq!(
        text(&out),
        format!("running pid={pid} protocol={PROTOCOL_VERSION} sessions=0\n")
    );

    let mode =
        |p: PathBuf| -> Result<u32> { Ok(std::fs::metadata(p)?.permissions().mode() & 0o777) };
    assert_eq!(mode(env.sock())?, 0o600);
    assert_eq!(mode(env.run_dir())?, 0o700);

    let out = env.baton(&["daemon", "start"])?;
    assert!(out.status.success());
    assert_eq!(text(&out), format!("daemon already running pid={pid}\n"));

    let out = env.baton(&["daemon", "stop"])?;
    assert!(out.status.success());
    assert!(!env.sock().exists());

    let out = env.baton(&["daemon", "status"])?;
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out), "not running\n");
    assert!(env.dir.path().join("state/daemon.log").exists());
    Ok(())
}

#[test]
fn handshake_rules_and_robustness() -> Result<()> {
    let env = Env::new()?;
    assert!(env.baton(&["daemon", "start"])?.status.success());

    // Version mismatch gets a reply, then the connection closes.
    let mut s = env.connect()?;
    send(
        &mut s,
        &ClientMsg::Hello {
            version: PROTOCOL_VERSION + 1,
            role: Role::Ctl,
        },
    )?;
    assert_eq!(
        recv(&mut s)?,
        Some(DaemonMsg::VersionMismatch {
            daemon_version: PROTOCOL_VERSION
        })
    );
    assert_eq!(recv(&mut s)?, None);

    // A non-Hello first frame closes the connection.
    let mut s = env.connect()?;
    send(&mut s, &ClientMsg::Status)?;
    assert_eq!(recv(&mut s)?, None);

    // Garbage and mid-frame disconnects do not hurt the daemon.
    let mut s = env.connect()?;
    s.write_all(&[0, 0, 0, 3, 0xff, 0xff, 0xff])?;
    assert_eq!(recv(&mut s)?, None);
    let mut s = env.connect()?;
    s.write_all(&[0, 0, 0, 50, 1, 2])?;
    drop(s);
    let mut s = env.connect()?;
    s.write_all(&[0xff, 0xff, 0xff, 0xff])?; // oversize header
    assert_eq!(recv(&mut s)?, None);

    let out = env.baton(&["daemon", "status"])?;
    assert!(out.status.success(), "{}", text(&out));
    Ok(())
}

#[test]
fn stale_socket_is_replaced_and_stop_waits() -> Result<()> {
    let env = Env::new()?;
    std::fs::create_dir_all(env.run_dir())?;
    std::fs::set_permissions(env.run_dir(), std::fs::Permissions::from_mode(0o700))?;
    drop(std::os::unix::net::UnixListener::bind(env.sock())?); // leaves a stale file
    assert!(env.sock().exists());
    assert!(env.baton(&["daemon", "start"])?.status.success());
    assert!(env.baton(&["daemon", "status"])?.status.success());
    assert!(env.baton(&["daemon", "stop"])?.status.success());
    wait_for(Duration::from_secs(1), || {
        (!env.sock().exists()).then_some(())
    })?;
    Ok(())
}
