//! End-to-end tests for `baton doctor`.

use anyhow::Result;
use baton_testkit::bin_path;
use std::process::{Command, Output, Stdio};

/// Temp environment with one `fake-claude` profile `p`.
struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new(profile_env: &str) -> Result<Self> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join("a"))?;
        std::fs::create_dir(dir.path().join("tmp"))?;
        let root = dir.path().canonicalize()?;
        let config = format!(
            "[profiles.p]\ncommand = {cmd:?}\n[profiles.p.env]\n{profile_env}\n\
             [[projects]]\nname = \"x\"\nprofile = \"p\"\n\
             repos = [{{ path = \"{r}/a\" }}]\n",
            cmd = bin_path("fake-claude")?.display().to_string(),
            r = root.display()
        );
        std::fs::write(dir.path().join("config.toml"), config)?;
        Ok(Self { dir })
    }

    fn doctor(&self, args: &[&str]) -> Result<Output> {
        Ok(Command::new(bin_path("baton")?)
            .arg("doctor")
            .args(args)
            .env("BATON_CONFIG", self.dir.path().join("config.toml"))
            .env("BATON_STATE_DIR", self.dir.path().join("state"))
            .env("BATON_RUNTIME_DIR", self.dir.path().join("run"))
            .env("BATON_NOTIFY_SINK", "off")
            .env("BATON_DOCTOR_PROBE_TIMEOUT_SECS", "4")
            .env("FAKE_CLAUDE_HOME", self.dir.path().join("fake-claude"))
            .env("TMPDIR", self.dir.path().join("tmp"))
            .stdin(Stdio::null())
            .output()?)
    }

    /// The probe's private temp dir must be gone afterwards.
    fn assert_tmp_clean(&self) -> Result<()> {
        let left: Vec<_> = std::fs::read_dir(self.dir.path().join("tmp"))?.collect();
        assert!(left.is_empty(), "probe temp dir left behind: {left:?}");
        Ok(())
    }
}

#[test]
fn healthy_fake_claude_profile_passes_every_check() -> Result<()> {
    let env = Env::new("")?;
    let out = env.doctor(&[])?;
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("PASS hooks: SessionStart received (profile p)"),
        "{text}"
    );
    assert!(text.contains("PASS config"), "{text}");
    assert!(text.contains("PASS dirs"), "{text}");
    assert!(text.contains("WARN daemon"), "{text}");
    assert!(!text.contains("FAIL"), "{text}");
    env.assert_tmp_clean()
}

#[test]
fn hooks_that_never_fire_fail_the_probe_with_a_hint() -> Result<()> {
    let env = Env::new("FAKE_CLAUDE_NO_HOOKS = \"1\"")?;
    let out = env.doctor(&[])?;
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("FAIL hooks"), "{text}");
    assert!(
        text.contains("trust dialog pending or hooks disabled"),
        "{text}"
    );
    env.assert_tmp_clean()
}

#[test]
fn no_probe_skips_the_hook_check() -> Result<()> {
    let env = Env::new("FAKE_CLAUDE_NO_HOOKS = \"1\"")?;
    let out = env.doctor(&["--no-probe"])?;
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(!text.contains("hooks"), "{text}");
    Ok(())
}

#[test]
fn invalid_config_fails() -> Result<()> {
    let env = Env::new("")?;
    std::fs::write(env.dir.path().join("config.toml"), "not [valid")?;
    let out = env.doctor(&[])?;
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("FAIL config"), "{text}");
    Ok(())
}
