//! Outside-in smoke tests for `baton-drive` and `fake-claude`.

use std::path::Path;
use std::process::{Command, Output};

use anyhow::{Context, Result};
use baton_testkit::bin_path;

fn drive(envs: &[(&str, &Path)], steps: &[&str], cmd: &[&str]) -> Result<Output> {
    drive_with_timeout(10_000, envs, steps, cmd)
}

fn drive_with_timeout(
    timeout_ms: u64,
    envs: &[(&str, &Path)],
    steps: &[&str],
    cmd: &[&str],
) -> Result<Output> {
    let mut c = Command::new(bin_path("baton-drive")?);
    c.args(["--size", "24x80", "--timeout-ms", &timeout_ms.to_string()]);
    for s in steps {
        c.arg("--step").arg(s);
    }
    c.arg("--");
    c.args(cmd);
    for (k, v) in envs {
        c.env(k, v);
    }
    c.output().context("run baton-drive")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn drives_bash_and_resizes() -> Result<()> {
    let out = drive(
        &[],
        &[
            "send:echo hi\\r",
            "wait:(?m)^hi$",
            "resize:30x100",
            "send:tput cols\\r",
            "wait:(?m)^100$",
            "dump",
            "send:exit 3\\r",
            "expect-exit:3",
        ],
        &["bash", "--norc", "--noprofile"],
    )?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = stdout(&out);
    assert!(s.contains("--- screen 30x100 ---"), "{s}");
    Ok(())
}

#[test]
fn wait_timeout_exits_one_with_screen_on_stderr() -> Result<()> {
    let out = drive_with_timeout(
        500,
        &[],
        &["wait:never-appears-xyz"],
        &["bash", "--norc", "--noprofile"],
    )?;
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--- screen 24x80 ---"));
    Ok(())
}

#[test]
fn fake_claude_fires_session_start_and_answers_da1() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let log = dir.path().join("hooks.log");
    let settings = dir.path().join("settings.json");
    let cmd = format!("cat >> {}; echo >> {}", log.display(), log.display());
    let hooks = serde_json::json!({"hooks": {"SessionStart": [
        {"hooks": [{"type": "command", "command": cmd, "timeout": 5}]}
    ]}});
    std::fs::write(&settings, hooks.to_string())?;
    let fake = bin_path("fake-claude")?;
    let home = dir.path().join("home");
    let out = drive(
        &[("FAKE_CLAUDE_HOME", &home)],
        &[
            "wait:DA1: ok",
            "wait:> ",
            "dump",
            "send:exit 0\\r",
            "expect-exit:0",
        ],
        &[
            fake.to_str().context("utf8 path")?,
            "--settings",
            settings.to_str().context("utf8 path")?,
        ],
    )?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = stdout(&out);
    assert!(s.contains("FAKE CLAUDE session="), "{s}");
    assert!(s.contains("DA1: ok"), "{s}");
    let line = std::fs::read_to_string(&log)?;
    let v: serde_json::Value = serde_json::from_str(line.lines().next().context("empty log")?)?;
    assert_eq!(v["hook_event_name"], "SessionStart");
    assert_eq!(v["source"], "startup");
    assert!(v["session_id"].is_string());
    assert!(
        v["transcript_path"]
            .as_str()
            .context("transcript_path")?
            .ends_with(".jsonl")
    );
    Ok(())
}

#[test]
fn fake_claude_resume_rules() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let fake = bin_path("fake-claude")?;
    let fake = fake.to_str().context("utf8 path")?;
    let out = drive(
        &[("FAKE_CLAUDE_HOME", dir.path())],
        &[
            "wait:No conversation found with session ID: nope",
            "expect-exit:1",
        ],
        &[fake, "--resume", "nope"],
    )?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = drive(
        &[("FAKE_CLAUDE_HOME", dir.path())],
        &["wait:No conversation found to continue", "expect-exit:1"],
        &[fake, "--continue"],
    )?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}

#[test]
fn fake_claude_prompt_writes_deduplicable_usage() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let fake = bin_path("fake-claude")?;
    let out = drive(
        &[("FAKE_CLAUDE_HOME", dir.path())],
        &[
            "wait:> ",
            "send:prompt hi\\r",
            "wait:(?m)^done",
            "send:exit 0\\r",
            "expect-exit:0",
        ],
        &[fake.to_str().context("utf8 path")?, "--session-id", "abc"],
    )?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut found = None;
    for slug in std::fs::read_dir(dir.path().join("projects"))? {
        let p = slug?.path().join("abc.jsonl");
        if p.exists() {
            found = Some(p);
        }
    }
    let text = std::fs::read_to_string(found.context("no transcript")?)?;
    let ids: Vec<String> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["type"] == "assistant")
        .map(|v| v["message"]["id"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0], ids[1]);
    Ok(())
}
