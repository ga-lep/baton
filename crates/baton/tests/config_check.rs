//! End-to-end tests for `baton config check`.

use anyhow::Result;
use baton_testkit::bin_path;
use std::process::{Command, Output};

fn check(config: &str) -> Result<Output> {
    let d = tempfile::tempdir()?;
    let file = d.path().join("config.toml");
    std::fs::write(&file, config)?;
    Ok(Command::new(bin_path("baton")?)
        .args(["config", "check"])
        .env("BATON_CONFIG", &file)
        .env("BATON_STATE_DIR", d.path().join("state"))
        .env("BATON_RUNTIME_DIR", d.path().join("run"))
        .output()?)
}

const GOOD: &str = "[profiles.p]\ncommand=\"bash --norc\"\nenv={FOO=\"bar\"}\n[[projects]]\nname=\"x\"\nprofile=\"p\"\nrepos=[{path=\"/tmp\"}]\n";

#[test]
fn prints_one_line_per_session() -> Result<()> {
    let out = check(GOOD)?;
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "x  /tmp  profile=p  cmd=[\"bash\",\"--norc\"]  env=FOO\n"
    );
    Ok(())
}

#[test]
fn invalid_config_exits_1_with_error() -> Result<()> {
    let out = check(&GOOD.replace("profile=\"p\"", "profile=\"nope\""))?;
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown profile \"nope\""), "{err}");
    Ok(())
}

#[test]
fn missing_config_is_ok_and_empty() -> Result<()> {
    let d = tempfile::tempdir()?;
    let out = Command::new(bin_path("baton")?)
        .args(["config", "check"])
        .env("BATON_CONFIG", d.path().join("none.toml"))
        .output()?;
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    Ok(())
}
