//! End-to-end tests for persistence, the launch ladder and restart (`fake-claude`).

use anyhow::{Context, Result, bail, ensure};
use baton_testkit::{Drive, DriveOptions, bin_path, wait_for};
use regex::Regex;
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(20);

/// Temp environment with a 1-repo `fake-claude` project; stops the daemon
/// (and so the session child) on drop.
struct Env {
    dir: tempfile::TempDir,
    root: PathBuf,
}

impl Env {
    fn new() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().canonicalize()?;
        std::fs::create_dir(root.join("r"))?;
        let config = format!(
            "[profiles.p]\ncommand = {fake:?}\n\
             [[projects]]\nname = \"x\"\nprofile = \"p\"\nrepos = [{{ path = \"{r}/r\" }}]\n",
            fake = bin_path("fake-claude")?.display().to_string(),
            r = root.display()
        );
        std::fs::write(root.join("config.toml"), config)?;
        Ok(Self { dir, root })
    }

    fn p(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn sid(&self) -> String {
        format!("x/{}/r", self.root.display())
    }

    fn baton(&self, args: &[&str]) -> Result<Output> {
        let mut c = Command::new(bin_path("baton")?);
        c.args(args).stdin(std::process::Stdio::null());
        c.env("BATON_CONFIG", self.p("config.toml"))
            .env("BATON_STATE_DIR", self.p("state"))
            .env("BATON_NO_UPDATE_CHECK", "1")
            .env("BATON_RUNTIME_DIR", self.p("run"))
            .env("BATON_NOTIFY_SINK", "off")
            .env("FAKE_CLAUDE_HOME", self.p("fake-claude"))
            .env("FAKE_CLAUDE_LOG", self.p("launch.log"));
        Ok(c.output()?)
    }

    fn baton_env(&self, args: &[&str], extra: &[(&str, &str)]) -> Result<Output> {
        let mut c = Command::new(bin_path("baton")?);
        c.args(args).stdin(std::process::Stdio::null());
        c.env("BATON_CONFIG", self.p("config.toml"))
            .env("BATON_STATE_DIR", self.p("state"))
            .env("BATON_NO_UPDATE_CHECK", "1")
            .env("BATON_RUNTIME_DIR", self.p("run"))
            .env("BATON_NOTIFY_SINK", "off")
            .env("FAKE_CLAUDE_HOME", self.p("fake-claude"))
            .env("FAKE_CLAUDE_LOG", self.p("launch.log"));
        for (k, v) in extra {
            c.env(k, v);
        }
        Ok(c.output()?)
    }

    fn envs(&self) -> Vec<(String, String)> {
        let p = |n: &str| self.p(n).display().to_string();
        vec![
            ("BATON_CONFIG".into(), p("config.toml")),
            ("BATON_STATE_DIR".into(), p("state")),
            ("BATON_NO_UPDATE_CHECK".into(), "1".into()),
            ("BATON_RUNTIME_DIR".into(), p("run")),
            ("BATON_NOTIFY_SINK".into(), "off".into()),
            ("FAKE_CLAUDE_HOME".into(), p("fake-claude")),
            ("FAKE_CLAUDE_LOG".into(), p("launch.log")),
        ]
    }

    fn ok(&self, args: &[&str]) -> Result<Output> {
        let out = self.baton(args)?;
        ensure!(out.status.success(), "{args:?}: {out:?}");
        Ok(out)
    }

    /// The session as listed by `debug sessions --json`.
    fn session(&self) -> Result<Option<Value>> {
        let out = self.ok(&["debug", "sessions", "--json"])?;
        let list: Vec<Value> = serde_json::from_slice(&out.stdout)?;
        Ok(list.into_iter().next())
    }

    fn wait_session(&self, what: &str, pred: impl Fn(&Value) -> bool) -> Result<Value> {
        wait_for(WAIT, || self.session().ok().flatten().filter(|s| pred(s)))
            .with_context(|| format!("waiting for {what}"))
    }

    /// Waits for the session to be idle and returns its claude session id.
    fn wait_idle(&self) -> Result<(String, Value)> {
        let s = self.wait_session("an idle session", |s| {
            s["status"] == "Idle" && s["claude_session_id"].is_string()
        })?;
        let id = s["claude_session_id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        Ok((id, s))
    }

    /// The `argv` of every launch so far, minus the program.
    fn launches(&self) -> Vec<Vec<String>> {
        let text = std::fs::read_to_string(self.p("launch.log")).unwrap_or_default();
        text.lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .map(|v| {
                v["argv"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|a| a.as_str().map(str::to_owned))
                    .collect()
            })
            .collect()
    }

    fn wait_launches(&self, n: usize) -> Result<Vec<Vec<String>>> {
        wait_for(WAIT, || Some(self.launches()).filter(|l| l.len() >= n))
            .with_context(|| format!("waiting for {n} launches, have {:?}", self.launches()))
    }

    /// Waits until `state.json` records `id` for the session.
    fn wait_persisted(&self, id: &str) -> Result<()> {
        let path = self.p("state/state.json");
        wait_for(WAIT, || {
            let v: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
            (v["sessions"][self.sid()]["claude_session_id"] == id).then_some(())
        })
        .context("waiting for state.json")
    }

    fn stop(&self) -> Result<()> {
        self.ok(&["daemon", "stop"]).map(drop)
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = self.baton(&["daemon", "stop"]);
        let _ = &self.dir;
    }
}

fn has_pair(argv: &[String], flag: &str, value: &str) -> bool {
    argv.windows(2).any(|w| w[0] == flag && w[1] == value)
}

#[test]
fn reopen_after_daemon_stop_resumes_the_same_conversation() -> Result<()> {
    let env = Env::new()?;
    env.ok(&["debug", "open", "x"])?;
    let (id, first) = env.wait_idle()?;
    // No history yet: --continue fails early, then a fresh id is assigned.
    let launches = env.wait_launches(2)?;
    assert!(
        launches[0].contains(&"--continue".to_owned()),
        "{launches:?}"
    );
    assert!(has_pair(&launches[1], "--session-id", &id), "{launches:?}");
    assert_eq!(first["launch"], "fresh");

    env.ok(&["debug", "send", &env.sid(), "prompt hi\\r"])?;
    env.wait_session("a finished turn", |s| s["status"] == "YourTurn")?;
    env.wait_persisted(&id)?;
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(env.p("state/state.json"))?
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    env.stop()?;

    env.ok(&["debug", "open", "x"])?;
    let launches = env.wait_launches(3)?;
    assert!(has_pair(&launches[2], "--resume", &id), "{launches:?}");
    let (again, s) = env.wait_idle()?;
    assert_eq!(again, id);
    assert_eq!(s["launch"], "resume");
    assert_eq!(env.launches().len(), 3, "no further attempts");
    Ok(())
}

#[test]
fn a_deleted_transcript_walks_resume_continue_fresh() -> Result<()> {
    let env = Env::new()?;
    env.ok(&["debug", "open", "x"])?;
    let (id, _) = env.wait_idle()?;
    env.wait_persisted(&id)?;
    env.stop()?;
    std::fs::remove_dir_all(env.p("fake-claude/projects"))?;

    env.ok(&["debug", "open", "x"])?;
    let launches = env.wait_launches(5)?;
    assert!(has_pair(&launches[2], "--resume", &id), "{launches:?}");
    assert!(
        launches[3].contains(&"--continue".to_owned()),
        "{launches:?}"
    );
    let fresh = launches[4]
        .windows(2)
        .find(|w| w[0] == "--session-id")
        .map(|w| w[1].clone())
        .context("no --session-id on the last rung")?;
    assert_ne!(fresh, id);
    let (now, s) = env.wait_idle()?;
    assert_eq!(now, fresh);
    assert_eq!(s["launch"], "fresh");
    assert_eq!(env.launches().len(), 5);
    Ok(())
}

#[test]
fn debug_restart_relaunches_with_resume() -> Result<()> {
    let env = Env::new()?;
    env.ok(&["debug", "open", "x"])?;
    let (id, _) = env.wait_idle()?;
    let before = env.wait_launches(2)?.len();
    env.ok(&["debug", "restart", &env.sid()])?;
    let launches = env.wait_launches(before + 1)?;
    let last = launches.last().context("no launch")?;
    assert!(has_pair(last, "--resume", &id), "{launches:?}");
    let (again, s) = env.wait_idle()?;
    assert_eq!(again, id);
    assert_eq!(s["launch"], "resume");
    // Restarting an unknown session is an error, not a crash.
    let out = env.baton(&["debug", "restart", "nope//x"])?;
    assert!(!out.status.success());
    Ok(())
}

#[test]
fn a_corrupt_state_file_is_set_aside_and_the_daemon_starts_empty() -> Result<()> {
    let env = Env::new()?;
    let state = env.p("state");
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(&state)?;
    }
    std::fs::write(state.join("state.json"), b"{ this is not json")?;
    env.ok(&["debug", "open", "x"])?;
    env.wait_idle()?;
    let aside = std::fs::read_dir(&state)?.filter_map(Result::ok).any(|e| {
        e.file_name()
            .to_string_lossy()
            .starts_with("state.json.bad-")
    });
    if !aside {
        bail!("no state.json.bad-* in {state:?}");
    }
    Ok(())
}

#[test]
fn a_failure_after_session_start_is_reported_not_retried() -> Result<()> {
    let env = Env::new()?;
    env.ok(&["debug", "open", "x"])?;
    env.wait_idle()?;
    let launches = env.launches().len();
    env.ok(&["debug", "send", &env.sid(), "exit 3\\r"])?;
    let s = env.wait_session("the exit", |s| s["exit_code"] == 3)?;
    assert_eq!(s["status"]["Exited"], 3);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(env.launches().len(), launches, "no relaunch");
    // `r` on an exited session works too.
    env.ok(&["debug", "restart", &env.sid()])?;
    env.wait_launches(launches + 1)?;
    env.wait_idle()?;
    Ok(())
}

#[test]
fn a_foreign_session_id_in_state_json_never_reaches_the_command_line() -> Result<()> {
    let env = Env::new()?;
    let state = env.p("state");
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(&state)?;
    }
    let evil = "--dangerously-skip-permissions";
    let json = serde_json::json!({
        "version": 1,
        "sessions": {
            env.sid(): {
                "project": "x",
                "repo": env.p("r").display().to_string(),
                "profile": "p",
                "claude_session_id": evil,
                "transcript_path": null,
                "last_status": "Idle",
                "updated_at": 1,
            }
        }
    });
    std::fs::write(state.join("state.json"), json.to_string())?;
    env.ok(&["debug", "open", "x"])?;
    env.wait_idle()?;
    for argv in env.launches() {
        assert!(!argv.iter().any(|a| a.contains("dangerously")), "{argv:?}");
        assert!(!argv.contains(&"--resume".to_owned()), "{argv:?}");
    }
    Ok(())
}

/// Runs a session to idle, waits for it to be persisted, stops the daemon.
fn persist_and_stop(env: &Env) -> Result<String> {
    env.ok(&["debug", "open", "x"])?;
    let (id, _) = env.wait_idle()?;
    env.wait_persisted(&id)?;
    env.stop()?;
    Ok(id)
}

#[test]
fn a_persisted_session_is_listed_as_closed_after_a_daemon_restart() -> Result<()> {
    let env = Env::new()?;
    let id = persist_and_stop(&env)?;
    // Any command starts a fresh daemon; nothing is opened.
    let s = env
        .session()?
        .context("the remembered session is not listed")?;
    assert_eq!(s["status"], "Closed", "{s}");
    assert_eq!(s["claude_session_id"], id.as_str());
    assert!(s["transcript_path"].is_string(), "{s}");
    assert!(s["launch"].is_null(), "{s}");
    assert_eq!(s["project"], "x");
    assert_eq!(env.launches().len(), 2, "listing must not launch anything");

    // Opening the project replaces it with a live session resuming the id.
    env.ok(&["debug", "open", "x"])?;
    let launches = env.wait_launches(3)?;
    assert!(has_pair(&launches[2], "--resume", &id), "{launches:?}");
    let (again, live) = env.wait_idle()?;
    assert_eq!(again, id);
    assert_eq!(live["launch"], "resume");
    let out = env.ok(&["debug", "sessions", "--json"])?;
    let list: Vec<Value> = serde_json::from_slice(&out.stdout)?;
    assert_eq!(list.len(), 1, "no duplicate closed entry: {list:?}");
    Ok(())
}

#[test]
fn remembered_sessions_of_projects_gone_from_the_config_are_not_listed() -> Result<()> {
    let env = Env::new()?;
    persist_and_stop(&env)?;
    let config = std::fs::read_to_string(env.p("config.toml"))?;
    std::fs::write(
        env.p("config.toml"),
        config.replace("name = \"x\"", "name = \"renamed\""),
    )?;
    assert!(env.session()?.is_none());
    Ok(())
}

#[test]
fn the_tui_shows_a_remembered_session_under_its_closed_project() -> Result<()> {
    let env = Env::new()?;
    persist_and_stop(&env)?;
    let argv = vec![bin_path("baton")?.display().to_string()];
    let mut d = Drive::spawn(
        &argv,
        &DriveOptions {
            rows: 30,
            cols: 100,
            env: env.envs(),
            cwd: None,
        },
    )?;
    d.wait_for(&Regex::new("▸ x  \\(closed\\)")?, WAIT)?;
    d.wait_for(&Regex::new("1 . r  closed")?, WAIT)?;
    assert_eq!(env.launches().len(), 2, "nothing launched yet");
    // Opening the project makes it live.
    d.send(b"o")?;
    d.wait_for(&Regex::new("▾ x")?, WAIT)?;
    d.wait_for(&Regex::new("1 . r  idle")?, WAIT)?;
    let launches = env.wait_launches(3)?;
    assert!(launches[2].contains(&"--resume".to_owned()), "{launches:?}");
    d.send(b"q")?;
    assert_eq!(d.wait_exit(WAIT)?, 0);
    Ok(())
}

#[test]
fn a_failed_resume_keeps_the_stored_id_until_session_start_replaces_it() -> Result<()> {
    let env = Env::new()?;
    let id = persist_and_stop(&env)?;
    // The conversation file now has another name: --resume <id> fails early,
    // --continue finds it, and with hooks off no SessionStart ever reports
    // an id.
    let dir = std::fs::read_dir(env.p("fake-claude/projects"))?
        .next()
        .context("no project dir")??
        .path();
    std::fs::rename(dir.join(format!("{id}.jsonl")), dir.join("other.jsonl"))?;
    env.baton_env(&["debug", "open", "x"], &[("FAKE_CLAUDE_NO_HOOKS", "1")])?;
    let launches = env.wait_launches(4)?;
    assert!(has_pair(&launches[2], "--resume", &id), "{launches:?}");
    assert!(
        launches[3].contains(&"--continue".to_owned()),
        "{launches:?}"
    );
    let s = env.wait_session("the continue rung", |s| s["launch"] == "continue")?;
    assert_eq!(s["claude_session_id"], id.as_str(), "{s}");
    let path = env.p("state/state.json");
    wait_for(WAIT, || {
        let v: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
        let e = &v["sessions"][env.sid()];
        (e["claude_session_id"] == id.as_str()).then_some(())
    })
    .context("state.json lost the stored id")?;
    Ok(())
}
