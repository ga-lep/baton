//! End-to-end tests for the status state machine, attention and `n` / `Alt-n`.

use anyhow::Result;
use baton_testkit::{Drive, DriveOptions, bin_path};
use regex::Regex;
use std::process::{Command, Output};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(15);

/// Temp environment with a 2-repo `fake-claude` project; stops the daemon
/// (and so the session children) on drop.
struct Env {
    dir: tempfile::TempDir,
    no_hooks: bool,
}

impl Env {
    fn new(no_hooks: bool, hook_timeout_secs: u64) -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().canonicalize()?;
        for repo in ["alpha", "beta"] {
            std::fs::create_dir(root.join(repo))?;
        }
        let config = format!(
            "hook_timeout_secs = {hook_timeout_secs}\n\
             [profiles.p]\ncommand = {fake:?}\n\
             [[projects]]\nname = \"x\"\nprofile = \"p\"\nrepos = [\n\
             {{ path = \"{r}/alpha\" }},\n{{ path = \"{r}/beta\" }},\n]\n",
            fake = bin_path("fake-claude")?.display().to_string(),
            r = root.display()
        );
        std::fs::write(dir.path().join("config.toml"), config)?;
        Ok(Self { dir, no_hooks })
    }

    fn envs(&self) -> Vec<(String, String)> {
        let p = |n: &str| self.dir.path().join(n).display().to_string();
        let mut v = vec![
            ("BATON_CONFIG".into(), p("config.toml")),
            ("BATON_STATE_DIR".into(), p("state")),
            ("BATON_RUNTIME_DIR".into(), p("run")),
            ("FAKE_CLAUDE_HOME".into(), p("fake-claude")),
        ];
        if self.no_hooks {
            v.push(("FAKE_CLAUDE_NO_HOOKS".into(), "1".into()));
        }
        v
    }

    fn baton(&self, args: &[&str]) -> Result<Output> {
        let mut c = Command::new(bin_path("baton")?);
        c.args(args).stdin(std::process::Stdio::null());
        for (k, v) in self.envs() {
            c.env(k, v);
        }
        Ok(c.output()?)
    }

    /// Types `text` into the session of repo `repo` without the TUI.
    fn send(&self, repo: &str, text: &str) -> Result<()> {
        let root = self.dir.path().canonicalize()?;
        let id = format!("x/{}/{repo}", root.display());
        let out = self.baton(&["debug", "send", &id, text])?;
        anyhow::ensure!(out.status.success(), "{out:?}");
        Ok(())
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
fn permission_your_turn_next_attention_and_viewed() -> Result<()> {
    let env = Env::new(false, 20)?;
    let mut d = env.tui()?;
    wait(&d, "▸ x  \\(closed\\)")?;
    d.send(b"o")?;
    wait(&d, "1 ○ alpha  idle")?;
    wait(&d, "2 ○ beta  idle")?;

    // alpha is on screen: a permission prompt shows up.
    d.send(b"\r")?;
    wait(&d, "FOCUS")?;
    d.send(b"perm\r")?;
    wait(&d, "1 ◐ alpha  permission")?;
    d.send(b"\x1c")?;
    wait(&d, "NORMAL")?;

    // Nothing else needs attention but alpha itself, which is excluded.
    d.send(b"n")?;
    wait(&d, "alpha · p · ◐")?;

    // beta on screen; alpha finishes its turn behind our back.
    d.send(b"2")?;
    wait(&d, "beta · p · ")?;
    env.send("alpha", "allow\\r")?;
    wait(&d, "1 ✓ alpha  your turn")?;
    assert!(d.screen_text().contains("2 ○ beta  idle"), "{}", d.dump());

    // `n` jumps to alpha; being on screen, it is viewed and goes idle.
    d.send(b"n")?;
    wait(&d, "alpha · p · ")?;
    wait(&d, "1 ○ alpha  idle")?;

    // With nothing left, the bar says so.
    d.send(b"n")?;
    wait(&d, "no session needs attention")?;

    d.send(b"q")?;
    assert_eq!(d.wait_exit(WAIT)?, 0);
    Ok(())
}

#[test]
fn alt_n_jumps_in_focus_mode_and_alt_digits_switch() -> Result<()> {
    let env = Env::new(false, 20)?;
    let mut d = env.tui()?;
    wait(&d, "▸ x  \\(closed\\)")?;
    d.send(b"o")?;
    wait(&d, "2 ○ beta  idle")?;
    d.send(b"\r")?;
    wait(&d, "FOCUS")?;

    env.send("beta", "perm\\r")?;
    wait(&d, "2 ◐ beta  permission")?;
    d.send(b"\x1bn")?;
    wait(&d, "beta · p · ◐")?;
    wait(&d, "FOCUS")?;
    d.send(b"\x1b1")?;
    wait(&d, "alpha · p · ")?;
    wait(&d, "FOCUS")?;
    d.send(b"\x1b2")?;
    wait(&d, "beta · p · ")?;
    wait(&d, "FOCUS")?;

    d.send(b"\x1c")?;
    d.send(b"q")?;
    assert_eq!(d.wait_exit(WAIT)?, 0);
    Ok(())
}

#[test]
fn a_session_without_hooks_becomes_unknown() -> Result<()> {
    let env = Env::new(true, 1)?;
    let mut d = env.tui()?;
    wait(&d, "▸ x  \\(closed\\)")?;
    d.send(b"o")?;
    wait(&d, "1 … alpha  starting")?;
    wait(&d, "1 \\? alpha  unknown")?;
    wait(&d, "2 \\? beta  unknown")?;
    d.send(b"q")?;
    assert_eq!(d.wait_exit(WAIT)?, 0);
    Ok(())
}
