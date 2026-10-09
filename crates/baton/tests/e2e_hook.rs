//! End-to-end tests for `baton hook` and the injected hooks.json.

use anyhow::{Result, bail};
use baton_proto::SessionInfo;
use baton_testkit::{bin_path, wait_for};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(15);

/// Temp environment; stops the daemon (and so every session child) on drop.
struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new(command: &str) -> Result<Self> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join("a"))?;
        let root = dir.path().canonicalize()?;
        let config = format!(
            "[profiles.p]\ncommand = {command:?}\n\
             [[projects]]\nname = \"x\"\nprofile = \"p\"\n\
             repos = [{{ path = \"{r}/a\" }}]\n",
            r = root.display()
        );
        std::fs::write(dir.path().join("config.toml"), config)?;
        Ok(Self { dir })
    }

    fn cmd(&self, args: &[&str]) -> Result<Command> {
        let mut c = Command::new(bin_path("baton")?);
        c.args(args)
            .env("BATON_CONFIG", self.dir.path().join("config.toml"))
            .env("BATON_STATE_DIR", self.dir.path().join("state"))
            .env("BATON_NO_UPDATE_CHECK", "1")
            .env("BATON_RUNTIME_DIR", self.dir.path().join("run"))
            .env("BATON_NOTIFY_SINK", "off")
            .env("FAKE_CLAUDE_HOME", self.dir.path().join("fake-claude"))
            .env_remove("BATON_SESSION")
            .env_remove("BATON_SOCK")
            .env_remove("BATON_HOOK_DEBUG");
        Ok(c)
    }

    fn baton(&self, args: &[&str]) -> Result<Output> {
        Ok(self.cmd(args)?.stdin(Stdio::null()).output()?)
    }

    fn debug(&self, args: &[&str]) -> Result<String> {
        let mut full = vec!["debug"];
        full.extend_from_slice(args);
        let out = self.baton(&full)?;
        if !out.status.success() {
            bail!("debug {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn id(&self) -> Result<String> {
        Ok(format!("x/{}/a", self.dir.path().canonicalize()?.display()))
    }

    fn sessions(&self) -> Result<Vec<SessionInfo>> {
        Ok(serde_json::from_str(&self.debug(&["sessions", "--json"])?)?)
    }

    fn fake_home(&self) -> PathBuf {
        self.dir.path().join("fake-claude")
    }

    /// Runs `baton <args>` with `stdin` and extra env; returns it with its wall time.
    fn run_hook(
        &self,
        args: &[&str],
        stdin: &[u8],
        env: &[(&str, &str)],
    ) -> Result<(Output, Duration)> {
        let mut c = self.cmd(args)?;
        c.envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let start = Instant::now();
        let mut child = c.spawn()?;
        if let Some(mut s) = child.stdin.take() {
            let _ = s.write_all(stdin); // the hook may exit before reading it all
        }
        let out = child.wait_with_output()?;
        Ok((out, start.elapsed()))
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = self.baton(&["daemon", "stop"]);
    }
}

fn assert_silent_success(out: &Output, took: Duration, what: &str) {
    assert_eq!(out.status.code(), Some(0), "{what}: {:?}", out.status);
    assert!(out.stdout.is_empty(), "{what}: stdout {:?}", out.stdout);
    assert!(out.stderr.is_empty(), "{what}: stderr {:?}", out.stderr);
    assert!(took < Duration::from_secs(2), "{what}: took {took:?}");
}

/// Description, args, stdin and extra env of one failure scenario.
type Case<'a> = (&'a str, Vec<&'a str>, &'a [u8], Vec<(&'a str, &'a str)>);

#[test]
fn hook_is_silent_and_exits_zero_in_every_failure_case() -> Result<()> {
    let env = Env::new("true")?;
    let file = env.dir.path().join("not-a-socket");
    std::fs::write(&file, "x")?;
    let file = file.to_str().unwrap_or_default().to_owned();
    let big = vec![b'x'; 3 * 1024 * 1024];
    let cases: Vec<Case> = vec![
        (
            "daemon down",
            vec!["hook", "Stop"],
            b"{\"hook_event_name\":\"Stop\"}",
            vec![("BATON_SESSION", "x")],
        ),
        (
            "nonexistent sock",
            vec!["hook", "Stop"],
            b"{}",
            vec![("BATON_SESSION", "x"), ("BATON_SOCK", "/nonexistent/sock")],
        ),
        (
            "sock is a file",
            vec!["hook", "Stop"],
            b"{}",
            vec![("BATON_SESSION", "x"), ("BATON_SOCK", &file)],
        ),
        ("no BATON_SESSION", vec!["hook", "Stop"], b"{}", vec![]),
        (
            "garbage stdin",
            vec!["hook", "Stop"],
            b"garbage",
            vec![("BATON_SESSION", "x")],
        ),
        (
            "empty stdin",
            vec!["hook", "Stop"],
            b"",
            vec![("BATON_SESSION", "x")],
        ),
        (
            "huge stdin",
            vec!["hook", "Stop"],
            &big,
            vec![("BATON_SESSION", "x")],
        ),
        ("no event", vec!["hook"], b"{}", vec![]),
        ("unknown flag", vec!["hook", "--bogus"], b"{}", vec![]),
        (
            "unknown event",
            vec!["hook", "../../etc/passwd"],
            b"{}",
            vec![("BATON_SESSION", "x")],
        ),
        ("extra args", vec!["hook", "Stop", "a", "-z"], b"{}", vec![]),
    ];
    for (what, args, stdin, envs) in cases {
        let (out, took) = env.run_hook(&args, stdin, &envs)?;
        assert_silent_success(&out, took, what);
    }
    Ok(())
}

#[test]
fn hook_with_unreachable_daemon_is_fast() -> Result<()> {
    let env = Env::new("true")?;
    let envs = [("BATON_SESSION", "x"), ("BATON_SOCK", "/nonexistent/sock")];
    let (out, took) = env.run_hook(&["hook", "Stop"], b"{}", &envs)?;
    assert_silent_success(&out, took, "unreachable");
    assert!(took < Duration::from_millis(900), "took {took:?}");
    Ok(())
}

#[test]
fn hook_with_stalled_stdin_gives_up() -> Result<()> {
    let env = Env::new("true")?;
    let mut c = env.cmd(&["hook", "Stop"])?;
    c.env("BATON_SESSION", "x")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let start = Instant::now();
    let mut child = c.spawn()?;
    let _keep_open = child.stdin.take(); // never written, never closed
    let out = child.wait_with_output()?;
    assert_silent_success(&out, start.elapsed(), "stalled stdin");
    assert!(start.elapsed() >= Duration::from_millis(400));
    Ok(())
}

#[test]
fn hook_debug_flag_enables_stderr() -> Result<()> {
    let env = Env::new("true")?;
    let envs = [
        ("BATON_SESSION", "x"),
        ("BATON_SOCK", "/nonexistent/sock"),
        ("BATON_HOOK_DEBUG", "1"),
    ];
    let (out, _) = env.run_hook(&["hook", "Stop"], b"{}", &envs)?;
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.is_empty());
    Ok(())
}

#[test]
fn fake_claude_session_start_reaches_the_daemon() -> Result<()> {
    let fake = bin_path("fake-claude")?;
    let env = Env::new(&fake.display().to_string())?;
    let id = env.id()?;
    env.debug(&["open", "x"])?;
    let info = wait_for(WAIT, || {
        env.sessions()
            .ok()?
            .into_iter()
            .find(|s| s.claude_session_id.is_some())
    })?;
    let claude_id = info.claude_session_id.clone().unwrap_or_default();
    let screen = env.debug(&["screen", &id])?;
    assert!(
        screen.contains(&format!("FAKE CLAUDE session={claude_id} ")),
        "{screen}"
    );
    let transcript = PathBuf::from(info.transcript_path.clone().unwrap_or_default());
    assert!(transcript.starts_with(env.fake_home()), "{transcript:?}");
    assert!(transcript.ends_with(format!("{claude_id}.jsonl")));
    assert_eq!(info.model.as_deref(), Some("claude-opus-5-5"));
    Ok(())
}

#[test]
fn hooks_json_is_written_privately_at_daemon_start() -> Result<()> {
    let env = Env::new("true")?;
    env.debug(&["open", "x"])?;
    let path = env.dir.path().join("run/hooks.json");
    let text = std::fs::read_to_string(&path)?;
    let v: serde_json::Value = serde_json::from_str(&text)?;
    let hooks = v["hooks"].as_object().map(|m| m.len());
    assert_eq!(hooks, Some(9));
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&path)?.permissions().mode() & 0o077, 0);
    Ok(())
}

#[test]
fn hook_for_an_unknown_session_is_dropped_and_daemon_survives() -> Result<()> {
    let fake = bin_path("fake-claude")?;
    let env = Env::new(&fake.display().to_string())?;
    env.debug(&["open", "x"])?;
    let sock = env.dir.path().join("run/baton.sock");
    let sock = sock.to_str().unwrap_or_default();
    let envs = [("BATON_SESSION", "nope//nowhere"), ("BATON_SOCK", sock)];
    let (out, took) = env.run_hook(
        &["hook", "SessionStart"],
        br#"{"session_id":"zzz","transcript_path":"/t"}"#,
        &envs,
    )?;
    assert_silent_success(&out, took, "unknown session");
    // The daemon still answers and nothing leaked into a real session.
    let list = env.sessions()?;
    assert_eq!(list.len(), 1);
    assert_ne!(list[0].claude_session_id.as_deref(), Some("zzz"));
    Ok(())
}

#[test]
fn invalid_session_start_fields_are_not_stored() -> Result<()> {
    let fake = bin_path("fake-claude")?;
    let env = Env::new(&fake.display().to_string())?;
    let id = env.id()?;
    env.debug(&["open", "x"])?;
    wait_for(WAIT, || {
        env.sessions().ok()?.pop().filter(|s| s.model.is_some())
    })?;
    let sock = env.dir.path().join("run/baton.sock");
    let sock = sock.to_str().unwrap_or_default();
    let envs = [("BATON_SESSION", id.as_str()), ("BATON_SOCK", sock)];
    let long = "m".repeat(129);
    let payload = format!(
        r#"{{"session_id":"a\u001b[2Jb","transcript_path":"relative.jsonl","model":"{long}"}}"#
    );
    let (out, took) = env.run_hook(&["hook", "SessionStart"], payload.as_bytes(), &envs)?;
    assert_silent_success(&out, took, "bad SessionStart");
    wait_for(WAIT, || {
        let s = env.sessions().ok()?.pop()?;
        (s.claude_session_id.is_none()).then_some(s)
    })
    .map(|s| {
        assert_eq!(s.transcript_path, None);
        assert_eq!(s.model, None);
    })
}
