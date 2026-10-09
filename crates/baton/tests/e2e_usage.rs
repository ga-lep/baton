//! End-to-end tests for the transcript tailer, usage, cost and the info panel
//! (`fake-claude`).

use anyhow::{Context, Result, ensure};
use baton_testkit::{Drive, DriveOptions, bin_path, wait_for};
use regex::Regex;
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(20);

/// Temp environment with a 1-repo `fake-claude` project whose profile sets
/// `CLAUDE_CONFIG_DIR`; stops the daemon (and so the session child) on drop.
struct Env {
    dir: tempfile::TempDir,
    root: PathBuf,
    /// Where `fake-claude` writes transcripts (`FAKE_CLAUDE_HOME`).
    fake_home: PathBuf,
}

impl Env {
    /// `inside`: the transcript lands under the profile's `CLAUDE_CONFIG_DIR`
    /// (allowed) or in an unrelated directory (must be refused).
    fn new(inside: bool) -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().canonicalize()?;
        std::fs::create_dir(root.join("r"))?;
        let fake_home = if inside {
            root.join("claude")
        } else {
            root.join("elsewhere")
        };
        let config = format!(
            "[profiles.p]\ncommand = {fake:?}\nenv = {{ CLAUDE_CONFIG_DIR = \"{c}/claude\" }}\n\
             [[projects]]\nname = \"x\"\nprofile = \"p\"\nrepos = [{{ path = \"{c}/r\" }}]\n\
             [pricing]\n\
             default = {{ input = 3.0, output = 15.0, cache_read = 0.3, cache_write = 3.75 }}\n",
            fake = bin_path("fake-claude")?.display().to_string(),
            c = root.display()
        );
        std::fs::write(root.join("config.toml"), config)?;
        Ok(Self {
            dir,
            root,
            fake_home,
        })
    }

    fn p(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn sid(&self) -> String {
        format!("x/{}/r", self.root.display())
    }

    fn envs(&self) -> Vec<(String, String)> {
        let p = |n: &str| self.p(n).display().to_string();
        vec![
            ("BATON_CONFIG".into(), p("config.toml")),
            ("BATON_STATE_DIR".into(), p("state")),
            ("BATON_NO_UPDATE_CHECK".into(), "1".into()),
            ("BATON_RUNTIME_DIR".into(), p("run")),
            ("BATON_NOTIFY_SINK".into(), "off".into()),
            (
                "FAKE_CLAUDE_HOME".into(),
                self.fake_home.display().to_string(),
            ),
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

    fn ok(&self, args: &[&str]) -> Result<Output> {
        let out = self.baton(args)?;
        ensure!(out.status.success(), "{args:?}: {out:?}");
        Ok(out)
    }

    fn session(&self) -> Result<Option<Value>> {
        let out = self.ok(&["debug", "sessions", "--json"])?;
        let list: Vec<Value> = serde_json::from_slice(&out.stdout)?;
        Ok(list.into_iter().next())
    }

    fn wait_session(&self, what: &str, pred: impl Fn(&Value) -> bool) -> Result<Value> {
        wait_for(WAIT, || self.session().ok().flatten().filter(|s| pred(s)))
            .with_context(|| format!("waiting for {what}"))
    }

    /// Opens the project, runs two prompts and waits for the second turn.
    fn two_prompts(&self) -> Result<()> {
        self.ok(&["debug", "open", "x"])?;
        self.wait_session("an idle session", |s| s["status"] == "Idle")?;
        for _ in 0..2 {
            self.ok(&["debug", "send", &self.sid(), "prompt hi\\r"])?;
            // Running, then back to a finished turn.
            self.wait_session("a finished turn", |s| s["status"] == "YourTurn")?;
        }
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
        let _ = &self.dir;
    }
}

#[test]
fn usage_counts_each_message_once_and_matches_the_formulas() -> Result<()> {
    let env = Env::new(true)?;
    env.two_prompts()?;
    let s = env.wait_session("both turns counted", |s| s["usage"]["input"] == 200)?;
    let u = &s["usage"];
    // Each prompt writes two entries with one message id: counted once.
    assert_eq!(u["input"], 200);
    assert_eq!(u["output"], 100);
    assert_eq!(u["cache_read"], 2000);
    assert_eq!(u["cache_write"], 20);
    let pct = u["context_pct"].as_f64().context("context_pct")?;
    assert!(
        (pct - (100.0 + 1000.0 + 10.0) / 200_000.0 * 100.0).abs() < 1e-4,
        "{pct}"
    );
    let cost = u["cost_usd"].as_f64().context("cost_usd")?;
    let expect = (200.0 * 3.0 + 100.0 * 15.0 + 2000.0 * 0.3 + 20.0 * 3.75) / 1e6;
    assert!((cost - expect).abs() < 1e-9, "{cost} vs {expect}");
    assert_eq!(u["model"], "claude-opus-5-5");
    Ok(())
}

#[test]
fn a_transcript_outside_the_config_dir_is_never_read() -> Result<()> {
    let env = Env::new(false)?;
    env.two_prompts()?;
    // The transcript exists and has usage, but is not under CLAUDE_CONFIG_DIR.
    let written = std::fs::read_dir(env.fake_home.join("projects"))?
        .filter_map(Result::ok)
        .flat_map(|d| std::fs::read_dir(d.path()).into_iter().flatten().flatten())
        .map(|f| std::fs::read_to_string(f.path()).unwrap_or_default())
        .any(|t| t.contains("\"input_tokens\":100"));
    ensure!(
        written,
        "fake-claude wrote no transcript under the fake home"
    );
    // Several poll intervals later it is still `n/a`.
    std::thread::sleep(Duration::from_millis(3500));
    let s = env.session()?.context("no session")?;
    assert!(s["usage"].is_null(), "{s}");
    assert_eq!(s["status"], "YourTurn", "the tailer never affects status");
    Ok(())
}

#[test]
fn the_info_panel_shows_context_tokens_and_estimated_cost() -> Result<()> {
    let env = Env::new(true)?;
    env.two_prompts()?;
    env.wait_session("both turns counted", |s| s["usage"]["input"] == 200)?;
    let d = env.tui()?;
    let wait = |pat: &str| d.wait_for(&Regex::new(pat).expect("regex"), WAIT);
    wait(r"context\s+░*█*░*\s+\d+%")?;
    wait(r"tokens\s+200 in / 100 out")?;
    wait(r"cache\s+2\.0k read / 20 write")?;
    wait(r"cost\s+~\$0\.0028 \(est\.\)")?;
    wait(r"model\s+claude-opus-5-5")?;
    Ok(())
}
