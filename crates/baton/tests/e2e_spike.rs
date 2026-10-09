//! End-to-end tests for `baton spike`, driven through `baton-drive`.

use std::path::Path;
use std::process::{Command, Output};

use anyhow::{Context, Result};
use baton_testkit::bin_path;

fn drive(envs: &[(&str, &Path)], steps: &[&str], cmd: &[&str]) -> Result<Output> {
    let mut c = Command::new(bin_path("baton-drive")?);
    c.args(["--size", "40x120", "--timeout-ms", "15000"]);
    for s in steps {
        c.arg("--step").arg(s);
    }
    c.arg("--").arg(bin_path("baton")?).args(cmd);
    c.env("BATON_NO_UPDATE_CHECK", "1")
        .env_remove("BATON_UPDATE_URL");
    for (k, v) in envs {
        c.env(k, v);
    }
    c.output().context("run baton-drive")
}

fn check(out: &Output) -> String {
    assert!(
        out.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn spike_embeds_bash_resizes_and_quits() -> Result<()> {
    let out = drive(
        &[],
        &[
            "wait:FOCUS",
            "wait:37x86",
            "send:tput cols\\r",
            "wait:│86 ",
            "send:printf '\\e[31mRED\\e[0m\\n'\\r",
            "wait:│RED",
            "resize:30x100",
            "wait:27x66",
            "send:tput cols\\r",
            "wait:│66 ",
            "dump",
            "send:\\x1c",
            "wait:NORMAL",
            "send:q",
            "expect-exit:0",
        ],
        &["spike", "--", "bash", "--norc", "--noprofile"],
    )?;
    let s = check(&out);
    assert!(s.contains("27x66"), "{s}");
    Ok(())
}

#[test]
fn spike_answers_queries_for_fake_claude() -> Result<()> {
    let fake = bin_path("fake-claude")?;
    let home = tempfile::tempdir()?;
    let out = drive(
        &[("FAKE_CLAUDE_HOME", home.path())],
        &[
            "wait:DA1: ok",
            "send:\\x1c",
            "wait:NORMAL",
            "send:q",
            "expect-exit:0",
        ],
        &["spike", "--", fake.to_str().context("utf8 path")?],
    )?;
    check(&out);
    Ok(())
}
