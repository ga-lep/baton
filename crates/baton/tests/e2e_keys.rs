//! End-to-end tests for remappable keybindings, the help overlay and the `e`
//! editor. The editor is always a stand-in command, never a real GUI editor.

use anyhow::Result;
use baton_testkit::{Drive, DriveOptions, bin_path};
use regex::Regex;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(15);

/// Temp environment with a 2-repo `fake-claude` project; stops the daemon
/// (and so the session children) on drop.
struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new(extra_config: &str) -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().canonicalize()?;
        for repo in ["alpha", "beta"] {
            std::fs::create_dir(root.join(repo))?;
        }
        let config = format!(
            "{extra_config}\n[profiles.p]\ncommand = {fake:?}\n\
             [[projects]]\nname = \"x\"\nprofile = \"p\"\nrepos = [\n\
             {{ path = \"{r}/alpha\" }},\n{{ path = \"{r}/beta\" }},\n]\n",
            fake = bin_path("fake-claude")?.display().to_string(),
            r = root.display()
        );
        std::fs::write(dir.path().join("config.toml"), config)?;
        Ok(Self { dir })
    }

    fn root(&self) -> Result<std::path::PathBuf> {
        Ok(self.dir.path().canonicalize()?)
    }

    fn envs(&self) -> Vec<(String, String)> {
        let p = |n: &str| self.dir.path().join(n).display().to_string();
        vec![
            ("BATON_CONFIG".into(), p("config.toml")),
            ("BATON_STATE_DIR".into(), p("state")),
            ("BATON_RUNTIME_DIR".into(), p("run")),
            ("BATON_NOTIFY_SINK".into(), "off".into()),
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

    /// Types `text` into the session of repo `repo` without the TUI.
    fn send(&self, repo: &str, text: &str) -> Result<()> {
        let id = format!("x/{}/{repo}", self.root()?.display());
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

fn open_both(d: &Drive) -> Result<()> {
    wait(d, "▸ x  \\(closed\\)")?;
    d.send(b"o")?;
    wait(d, "1 ○ alpha  idle")?;
    wait(d, "2 ○ beta  idle")
}

#[test]
fn remapped_keys_work_help_shows_them_and_e_runs_the_editor() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let edited = dir.path().canonicalize()?.join("edited");
    // A stand-in editor: it only records the path it is given.
    let extra = format!(
        "editor = \"sh -c 'echo {{path}} > {}'\"\n\
         [keybindings.normal]\nnext_attention = \"x\"\n\
         [keybindings.focus]\nunfocus = \"ctrl-g\"\n",
        edited.display()
    );
    let env = Env::new(&extra)?;
    let mut d = env.tui()?;
    open_both(&d)?;

    // Help lists the live bindings; Esc closes it.
    d.send(b"?")?;
    wait(&d, "next_attention +x")?;
    wait(&d, "unfocus +ctrl-g")?;
    d.send(b"\x1b")?;
    let end = Instant::now() + WAIT;
    while d.screen_text().contains("Key bindings") {
        anyhow::ensure!(Instant::now() < end, "help never closed\n{}", d.dump());
        std::thread::sleep(Duration::from_millis(20));
    }

    // beta needs permission; alpha is shown.
    env.send("beta", "perm\\r")?;
    wait(&d, "2 ◐ beta  permission")?;
    wait(&d, "alpha · p · ")?;
    d.send(b"n")?;
    std::thread::sleep(Duration::from_millis(400));
    assert!(d.screen_text().contains("alpha · p · "), "{}", d.dump());
    assert!(!d.screen_text().contains("beta · p · "), "{}", d.dump());
    d.send(b"x")?;
    wait(&d, "beta · p · ◐")?;

    // Ctrl-g leaves focus mode.
    d.send(b"1")?;
    wait(&d, "alpha · p · ")?;
    d.send(b"\r")?;
    wait(&d, "FOCUS")?;
    wait(&d, "Ctrl-g back")?;
    d.send(b"\x07")?;
    wait(&d, "NORMAL")?;

    // `e` runs the editor with the shown repo.
    let want = format!("{}\n", env.root()?.join("alpha").display());
    d.send(b"e")?;
    let end = Instant::now() + WAIT;
    while std::fs::read_to_string(&edited).ok().as_deref() != Some(want.as_str()) {
        anyhow::ensure!(Instant::now() < end, "editor never ran\n{}", d.dump());
        std::thread::sleep(Duration::from_millis(20));
    }

    d.send(b"q")?;
    assert_eq!(d.wait_exit(WAIT)?, 0);
    Ok(())
}

#[test]
fn a_failing_editor_shows_in_the_bar_for_five_seconds() -> Result<()> {
    let env = Env::new("editor = \"definitely-not-an-editor-xyz {path}\"\n")?;
    let mut d = env.tui()?;
    open_both(&d)?;
    d.send(b"e")?;
    wait(&d, "cannot run definitely-not-an-editor-xyz")?;
    let shown = Instant::now();
    let end = shown + Duration::from_secs(15);
    while d
        .screen_text()
        .contains("cannot run definitely-not-an-editor-xyz")
    {
        anyhow::ensure!(Instant::now() < end, "message never expired\n{}", d.dump());
        std::thread::sleep(Duration::from_millis(50));
    }
    let shown_for = shown.elapsed();
    assert!(shown_for >= Duration::from_secs(3), "{shown_for:?}");
    assert!(shown_for <= Duration::from_secs(8), "{shown_for:?}");
    d.send(b"q")?;
    assert_eq!(d.wait_exit(WAIT)?, 0);
    Ok(())
}

#[test]
fn invalid_keybindings_fail_config_check_naming_the_key() -> Result<()> {
    for (extra, needle) in [
        (
            "[keybindings.normal]\nteleport = \"x\"\n",
            "keybindings.normal.teleport",
        ),
        (
            "[keybindings.focus]\nunfocus = \"x\"\n",
            "keybindings.focus.unfocus",
        ),
        (
            "[keybindings.normal]\nquit = \"ctrl-nope\"\n",
            "keybindings.normal.quit",
        ),
        ("[keybindings.normal]\nrestart = \"n\"\n", "bound to both"),
    ] {
        let env = Env::new(extra)?;
        let out = env.baton(&["config", "check"])?;
        assert_eq!(out.status.code(), Some(1), "{extra}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(needle), "{extra}: {err}");
    }
    Ok(())
}
