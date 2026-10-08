//! End-to-end tests for desktop notifications, using the log sink so nothing
//! reaches the real desktop.

use anyhow::Result;
use baton_testkit::{Drive, DriveOptions, bin_path};
use regex::Regex;
use std::process::{Command, Output};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(15);

/// Temp environment with a one-repo `fake-claude` project; stops the daemon
/// (and so the session child) on drop.
struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new(extra_config: &str) -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().canonicalize()?;
        std::fs::create_dir(root.join("alpha"))?;
        let config = format!(
            "{extra_config}\n[profiles.p]\ncommand = {fake:?}\n\
             [[projects]]\nname = \"x\"\nprofile = \"p\"\n\
             repos = [{{ path = \"{r}/alpha\" }}]\n",
            fake = bin_path("fake-claude")?.display().to_string(),
            r = root.display()
        );
        std::fs::write(dir.path().join("config.toml"), config)?;
        Ok(Self { dir })
    }

    fn envs(&self) -> Vec<(String, String)> {
        let p = |n: &str| self.dir.path().join(n).display().to_string();
        vec![
            ("BATON_CONFIG".into(), p("config.toml")),
            ("BATON_STATE_DIR".into(), p("state")),
            ("BATON_RUNTIME_DIR".into(), p("run")),
            ("BATON_NOTIFY_SINK".into(), "log".into()),
            ("FAKE_CLAUDE_HOME".into(), p("fake-claude")),
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

    fn session_id(&self) -> Result<String> {
        Ok(format!(
            "x/{}/alpha",
            self.dir.path().canonicalize()?.display()
        ))
    }

    /// Types `text` into the session without the TUI.
    fn send(&self, text: &str) -> Result<()> {
        let out = self.baton(&["debug", "send", &self.session_id()?, text])?;
        anyhow::ensure!(out.status.success(), "{out:?}");
        Ok(())
    }

    fn log_lines(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("state/notifications.log"))
            .map(|t| t.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    fn wait_lines(&self, n: usize) -> Result<Vec<String>> {
        let end = std::time::Instant::now() + WAIT;
        loop {
            let lines = self.log_lines();
            if lines.len() >= n {
                return Ok(lines);
            }
            anyhow::ensure!(std::time::Instant::now() < end, "log lines: {lines:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
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
        .inspect_err(|_| eprintln!("{}", d.dump()))
}

#[test]
fn notifies_when_no_tui_is_attached_and_not_again_while_in_the_state() -> Result<()> {
    let env = Env::new("")?;
    let out = env.baton(&["debug", "open", "x"])?;
    assert!(out.status.success(), "{out:?}");
    env.send("perm\\r")?;
    let lines = env.wait_lines(1)?;
    assert_eq!(lines, [format!("notify {} Permission", env.session_id()?)]);

    // Still in Permission: no repeat. Leaving and re-entering notifies again.
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(env.log_lines().len(), 1);
    env.send("allow\\r")?;
    env.wait_lines(2)?; // your turn
    env.send("perm\\r")?;
    let lines = env.wait_lines(3)?;
    assert!(lines[2].ends_with(" Permission"), "{lines:?}");
    Ok(())
}

#[test]
fn stays_quiet_while_watching_and_notifies_once_focus_is_lost() -> Result<()> {
    let env = Env::new("")?;
    let mut d = env.tui()?;
    wait(&d, "▸ x  \\(closed\\)")?;
    d.send(b"o")?;
    wait(&d, "1 ○ alpha  idle")?;
    d.send(b"\r")?;
    wait(&d, "FOCUS")?;
    d.send(b"\x1c")?;
    wait(&d, "NORMAL")?;

    // Focused and on screen: the permission prompt is seen, no notification.
    d.send(b"\x1b[I")?;
    env.send("perm\\r")?;
    wait(&d, "1 ◐ alpha  permission")?;
    std::thread::sleep(Duration::from_millis(500));
    assert!(env.log_lines().is_empty(), "{:?}", env.log_lines());

    // Back to idle, then the terminal loses focus: a new prompt notifies.
    env.send("allow\\r")?;
    wait(&d, "1 ○ alpha  idle")?;
    d.send(b"\x1b[O")?;
    std::thread::sleep(Duration::from_millis(300));
    env.send("perm\\r")?;
    let lines = env.wait_lines(1)?;
    assert_eq!(lines, [format!("notify {} Permission", env.session_id()?)]);

    d.send(b"q")?;
    assert_eq!(d.wait_exit(WAIT)?, 0);
    Ok(())
}

#[test]
fn notifications_can_be_disabled_in_the_config() -> Result<()> {
    let env = Env::new("notifications = false")?;
    let out = env.baton(&["debug", "open", "x"])?;
    assert!(out.status.success(), "{out:?}");
    env.send("perm\\r")?;
    let id = env.session_id()?;
    // Wait until the status change has surely happened.
    let end = std::time::Instant::now() + WAIT;
    loop {
        let out = env.baton(&["debug", "sessions", "--json"])?;
        if String::from_utf8_lossy(&out.stdout).contains("Permission") {
            break;
        }
        anyhow::ensure!(
            std::time::Instant::now() < end,
            "never reached Permission ({id})"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(500));
    assert!(env.log_lines().is_empty());
    Ok(())
}
