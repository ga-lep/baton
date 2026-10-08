//! End-to-end test for the project tree, open project, session switching and
//! scrollback in the TUI.

use anyhow::Result;
use baton_testkit::{Drive, DriveOptions, bin_path};
use regex::Regex;
use std::process::{Command, Output};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(15);

/// Temp environment with a 3-repo project; stops the daemon (and so the
/// session children) on drop.
struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().canonicalize()?;
        for repo in ["alpha", "beta", "hyper"] {
            std::fs::create_dir(root.join(repo))?;
        }
        let config = format!(
            "[profiles.a]\ncommand = \"bash --norc --noprofile -s --\"\nenv = {{ WHO = \"a\" }}\n\
             [profiles.b]\ncommand = \"bash --norc --noprofile -s --\"\nenv = {{ WHO = \"b\" }}\n\
             [[projects]]\nname = \"loop\"\nprofile = \"a\"\nrepos = [\n\
             {{ path = \"{r}/alpha\" }},\n{{ path = \"{r}/beta\" }},\n\
             {{ path = \"{r}/hyper\", profile = \"b\" }},\n]\n",
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
fn open_project_switch_sessions_see_profile_env_and_scroll_back() -> Result<()> {
    let env = Env::new()?;
    let mut d = env.tui()?;
    wait(&d, "▸ loop  \\(closed\\)")?;
    d.send(b"o")?;
    wait(&d, "▾ loop")?;
    wait(&d, "3 . hyper")?;
    wait(&d, "1 . alpha")?;

    // Session 1 is shown first.
    wait(&d, "alpha · a · ")?;
    d.send(b"\r")?;
    wait(&d, "FOCUS")?;
    d.send(b"echo WHO=$WHO\r")?;
    wait(&d, "│WHO=a")?;
    d.send(b"\x1c")?;
    wait(&d, "NORMAL")?;

    d.send(b"2")?;
    wait(&d, "beta · a · ")?;
    assert!(!d.screen_text().contains("WHO=a"), "{}", d.dump());
    d.send(b"\r")?;
    wait(&d, "FOCUS")?;
    d.send(b"echo WHO=$WHO\r")?;
    wait(&d, "│WHO=a")?;
    d.send(b"\x1c")?;

    d.send(b"3")?;
    wait(&d, "hyper · b · ")?;
    d.send(b"\r")?;
    wait(&d, "FOCUS")?;
    d.send(b"echo WHO=$WHO\r")?;
    wait(&d, "│WHO=b")?;
    d.send(b"\x1c")?;
    wait(&d, "NORMAL")?;

    // Scrollback in session 1.
    d.send(b"1")?;
    wait(&d, "alpha · a · ")?;
    d.send(b"\r")?;
    wait(&d, "FOCUS")?;
    d.send(b"seq 1 500\r")?;
    wait(&d, "│500")?;
    d.send(b"\x1c")?;
    wait(&d, "NORMAL")?;
    d.send(b"\x15")?;
    wait(&d, "\\[scrollback -\\d+\\]")?;
    let scrolled = d.screen_text();
    assert!(!scrolled.contains("│500"), "{}", d.dump());
    assert!(
        Regex::new("│4[4-8][0-9]")?.is_match(&scrolled),
        "{}",
        d.dump()
    );
    d.send(b"G")?;
    wait(&d, "│500")?;
    assert!(!d.screen_text().contains("[scrollback"), "{}", d.dump());

    d.send(b"q")?;
    assert_eq!(d.wait_exit(WAIT)?, 0);
    Ok(())
}
