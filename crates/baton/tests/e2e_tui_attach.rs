//! End-to-end tests for the TUI client (`baton` with no subcommand).

use anyhow::{Context, Result};
use baton_proto::{ClientMsg, DaemonMsg, decode, encode};
use baton_testkit::{Drive, DriveOptions, bin_path};
use regex::Regex;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(15);

/// Temp environment with one bash repo; stops the daemon (and so the session
/// child) on drop so no process outlives a test.
struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join("a"))?;
        let root = dir.path().canonicalize()?;
        let config = format!(
            "[profiles.p]\ncommand = \"bash --norc --noprofile -s --\"\n\
             [[projects]]\nname = \"x\"\nprofile = \"p\"\n\
             repos = [{{ path = \"{}/a\" }}]\n",
            root.display()
        );
        std::fs::write(dir.path().join("config.toml"), config)?;
        Ok(Self { dir })
    }

    fn run_dir(&self) -> PathBuf {
        self.dir.path().join("run")
    }

    fn envs(&self) -> Vec<(String, String)> {
        let p = |n: &str| self.dir.path().join(n).display().to_string();
        vec![
            ("BATON_CONFIG".into(), p("config.toml")),
            ("BATON_STATE_DIR".into(), p("state")),
            ("BATON_RUNTIME_DIR".into(), p("run")),
            ("BATON_NOTIFY_SINK".into(), "off".into()),
        ]
    }

    fn baton(&self, args: &[&str]) -> Result<Output> {
        let mut c = Command::new(bin_path("baton")?);
        c.args(args).stdin(std::process::Stdio::null());
        for (k, v) in self.envs() {
            c.env(k, v);
        }
        Ok(c.output()?)
    }

    fn tui(&self) -> Result<Drive> {
        let argv = vec![bin_path("baton")?.display().to_string()];
        Drive::spawn(
            &argv,
            &DriveOptions {
                rows: 40,
                cols: 120,
                env: self.envs(),
                cwd: None,
            },
        )
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = self.baton(&["daemon", "stop"]);
    }
}

fn wait(d: &Drive, pattern: &str) -> Result<()> {
    d.wait_for(&Regex::new(pattern)?, WAIT)
}

#[test]
fn attach_focus_quit_and_reattach_keep_the_session() -> Result<()> {
    let env = Env::new()?;
    assert!(env.baton(&["debug", "open", "x"])?.status.success());

    let mut d = env.tui()?;
    wait(&d, "NORMAL")?;
    d.send(b"\r")?;
    wait(&d, "FOCUS")?;
    d.send(b"echo persisted-$((40+2))\r")?;
    wait(&d, "│persisted-42")?;
    d.send(b"\x1c")?;
    wait(&d, "NORMAL")?;
    d.send(b"q")?;
    assert_eq!(d.wait_exit(WAIT)?, 0);

    let out = env.baton(&["daemon", "status"])?;
    let status = String::from_utf8_lossy(&out.stdout);
    assert!(status.contains("sessions=1"), "{status}");

    let mut d = env.tui()?;
    wait(&d, "│persisted-42")?;
    d.send(b"q")?;
    assert_eq!(d.wait_exit(WAIT)?, 0);
    Ok(())
}

#[test]
fn losing_the_daemon_shows_a_banner_and_r_reconnects() -> Result<()> {
    let env = Env::new()?;
    assert!(env.baton(&["debug", "open", "x"])?.status.success());
    let mut d = env.tui()?;
    wait(&d, "NORMAL")?;
    assert!(env.baton(&["daemon", "stop"])?.status.success());
    wait(&d, "daemon disconnected — press r to reconnect, q to quit")?;
    d.send(b"r")?;
    wait(&d, "No sessions")?;
    assert!(
        !d.screen_text().contains("daemon disconnected"),
        "{}",
        d.dump()
    );
    d.send(b"q")?;
    assert_eq!(d.wait_exit(WAIT)?, 0);
    Ok(())
}

#[test]
fn version_mismatch_shows_the_modal_and_n_quits() -> Result<()> {
    let env = Env::new()?;
    std::fs::create_dir(env.run_dir())?;
    std::fs::set_permissions(env.run_dir(), std::fs::Permissions::from_mode(0o700))?;
    let listener = UnixListener::bind(env.run_dir().join("baton.sock"))?;
    std::thread::spawn(move || {
        while let Ok((mut s, _)) = listener.accept() {
            let mut len = [0u8; 4];
            let mut hello = Vec::new();
            if s.read_exact(&mut len).is_ok() {
                hello.resize(u32::from_be_bytes(len) as usize, 0);
                let _ = s.read_exact(&mut hello);
            }
            assert!(matches!(
                decode::<ClientMsg>(&hello),
                Ok(ClientMsg::Hello { .. })
            ));
            if let Ok(b) = encode(&DaemonMsg::VersionMismatch { daemon_version: 99 }) {
                let _ = s.write_all(&u32::try_from(b.len()).unwrap_or(0).to_be_bytes());
                let _ = s.write_all(&b);
            }
        }
    });
    let mut d = env.tui()?;
    wait(
        &d,
        &format!(
            "Daemon protocol v{} ≠ v99. Restart daemon \\(sessions will be resumed\\)\\? \\[y/N\\]",
            baton_proto::PROTOCOL_VERSION
        ),
    )
    .context("modal")?;
    d.send(b"n")?;
    assert_eq!(d.wait_exit(WAIT)?, 0);
    Ok(())
}
