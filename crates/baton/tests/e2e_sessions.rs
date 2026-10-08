//! End-to-end tests for the session runtime, driven through `baton debug`.

use anyhow::{Result, bail};
use baton_proto::{ClientMsg, DaemonMsg, PROTOCOL_VERSION, Role, SessionInfo, Status};
use baton_proto::{decode, encode};
use baton_testkit::{bin_path, wait_for};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(15);

/// Temp environment with a config; stops the daemon (and so every session
/// child) on drop so no process outlives a test.
struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    /// Two repos `a` and `b` under project `x`, using `command` as the profile.
    fn new(command: &str, nudge: bool) -> Result<Self> {
        let dir = tempfile::tempdir()?;
        for r in ["a", "b"] {
            std::fs::create_dir(dir.path().join(r))?;
        }
        let root = dir.path().canonicalize()?;
        let config = format!(
            "attach_redraw_nudge = {nudge}\n\
             [profiles.p]\ncommand = {command:?}\nenv = {{ FOO = \"foo-val\" }}\n\
             [[projects]]\nname = \"x\"\nprofile = \"p\"\n\
             repos = [{{ path = \"{r}/a\" }}, {{ path = \"{r}/b\" }}]\n",
            r = root.display()
        );
        std::fs::write(dir.path().join("config.toml"), config)?;
        Ok(Self { dir })
    }

    fn bash(nudge: bool) -> Result<Self> {
        // `-s --` makes the injected `--settings <file>` plain positional parameters.
        Self::new("bash --norc --noprofile -s --", nudge)
    }

    fn root(&self) -> Result<PathBuf> {
        Ok(self.dir.path().canonicalize()?)
    }

    fn id(&self, repo: &str) -> Result<String> {
        Ok(format!("x/{}/{repo}", self.root()?.display()))
    }

    fn run_dir(&self) -> PathBuf {
        self.dir.path().join("run")
    }

    fn sock(&self) -> PathBuf {
        self.run_dir().join("baton.sock")
    }

    fn baton(&self, args: &[&str]) -> Result<Output> {
        Ok(Command::new(bin_path("baton")?)
            .args(args)
            .env("BATON_CONFIG", self.dir.path().join("config.toml"))
            .env("BATON_STATE_DIR", self.dir.path().join("state"))
            .env("BATON_RUNTIME_DIR", self.run_dir())
            .env("FAKE_CLAUDE_HOME", self.dir.path().join("fake-claude"))
            .stdin(std::process::Stdio::null())
            .output()?)
    }

    /// Runs `baton debug ...`, failing the test on a non-zero exit.
    fn debug(&self, args: &[&str]) -> Result<String> {
        let mut full = vec!["debug"];
        full.extend_from_slice(args);
        let out = self.baton(&full)?;
        if !out.status.success() {
            bail!(
                "debug {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn screen(&self, id: &str) -> Result<String> {
        self.debug(&["screen", id])
    }

    fn wait_screen(&self, id: &str, pred: impl Fn(&str) -> bool) -> Result<String> {
        wait_for(WAIT, || self.screen(id).ok().filter(|s| pred(s)))
    }

    fn sessions(&self) -> Result<Vec<SessionInfo>> {
        Ok(serde_json::from_str(&self.debug(&["sessions", "--json"])?)?)
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = self.baton(&["daemon", "stop"]);
    }
}

fn has_line(screen: &str, line: &str) -> bool {
    screen.lines().any(|l| l.trim_end() == line)
}

#[test]
fn bash_sessions_env_reattach_and_exit() -> Result<()> {
    let env = Env::bash(false)?;
    let (a, b) = (env.id("a")?, env.id("b")?);
    env.debug(&["open", "x"])?;

    let list = env.sessions()?;
    assert_eq!(list.len(), 2);
    assert!(list.iter().all(|s| s.status == Status::Starting));
    assert!(list.iter().any(|s| s.id.0 == a) && list.iter().any(|s| s.id.0 == b));

    env.debug(&[
        "send",
        &a,
        "echo $BATON_SESSION; echo $FOO; pwd; echo $BATON_SOCK\\r",
    ])?;
    let screen = env.wait_screen(&a, |s| has_line(s, &env.sock().display().to_string()))?;
    assert!(has_line(&screen, &a), "{screen}");
    assert!(has_line(&screen, "foo-val"), "{screen}");
    assert!(
        has_line(&screen, &format!("{}/a", env.root()?.display())),
        "{screen}"
    );

    // Detach/reattach (each `debug screen` is a fresh client): identical text.
    assert_eq!(env.screen(&a)?, screen);
    // Opening again must not respawn a live session.
    env.debug(&["open", "x"])?;
    assert_eq!(env.screen(&a)?, screen);

    env.debug(&["send", &a, "exit 3\\r"])?;
    wait_for(WAIT, || {
        let list = env.sessions().ok()?;
        list.iter()
            .any(|s| s.id.0 == a && s.status == Status::Exited(3))
            .then_some(())
    })?;
    let raw = env.debug(&["sessions", "--json"])?;
    assert!(raw.contains("Exited"), "{raw}");
    let list = env.sessions()?;
    let other = list.iter().find(|s| s.id.0 == b).expect("b listed");
    assert_eq!(other.status, Status::Starting);
    let done = list.iter().find(|s| s.id.0 == a).expect("a listed");
    assert_eq!(done.exit_code, Some(3));
    Ok(())
}

#[test]
fn unknown_project_and_session_are_errors() -> Result<()> {
    let env = Env::bash(false)?;
    let out = env.baton(&["debug", "open", "nope"])?;
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown project"));
    let out = env.baton(&["debug", "send", "x//nowhere", "hi"])?;
    assert!(!out.status.success());
    Ok(())
}

#[test]
fn fake_claude_gets_da1_reply_with_no_client_attached() -> Result<()> {
    let fake = bin_path("fake-claude")?;
    let env = Env::new(&fake.display().to_string(), false)?;
    let a = env.id("a")?;
    env.debug(&["open", "x"])?;
    // Nobody attached while it started; the daemon answered the query itself.
    let screen = env.wait_screen(&a, |s| s.contains("DA1:"))?;
    assert!(screen.contains("DA1: ok"), "{screen}");
    Ok(())
}

#[test]
fn scrollback_rows_come_from_the_authoritative_screen() -> Result<()> {
    let env = Env::bash(false)?;
    let a = env.id("a")?;
    env.debug(&["open", "x"])?;
    env.debug(&["send", &a, "seq 1 100\\r"])?;
    env.wait_screen(&a, |s| has_line(s, "100"))?;
    let out = env.debug(&["scrollback", &a, "0", "50"])?;
    assert!(has_line(&out, "1"), "{out}");
    assert!(has_line(&out, "2"), "{out}");
    Ok(())
}

fn winch_session(nudge: bool) -> Result<(Env, String)> {
    let env = Env::bash(nudge)?;
    let a = env.id("a")?;
    env.debug(&["open", "x"])?;
    env.debug(&[
        "send",
        &a,
        "trap 'echo WIN$((1+1))CHED' WINCH; while :; do read -t 0.1 x; done\\r",
    ])?;
    env.wait_screen(&a, |s| s.contains("while"))?;
    Ok((env, a))
}

#[test]
fn attach_nudge_sends_sigwinch_when_enabled() -> Result<()> {
    let (env, a) = winch_session(true)?;
    env.wait_screen(&a, |s| s.contains("WIN2CHED"))?;
    Ok(())
}

#[test]
fn attach_without_nudge_sends_no_sigwinch() -> Result<()> {
    let (env, a) = winch_session(false)?;
    env.screen(&a)?;
    std::thread::sleep(Duration::from_millis(700));
    assert!(!env.screen(&a)?.contains("WIN2CHED"));
    Ok(())
}

#[test]
fn insecure_runtime_dir_is_refused_before_connecting() -> Result<()> {
    let env = Env::bash(false)?;
    std::fs::create_dir(env.run_dir())?;
    std::fs::set_permissions(env.run_dir(), std::fs::Permissions::from_mode(0o755))?;
    let listener = UnixListener::bind(env.sock())?;
    listener.set_nonblocking(true)?;
    for args in [&["daemon", "start"][..], &["daemon", "status"][..]] {
        let out = env.baton(args)?;
        assert!(!out.status.success(), "{args:?} must fail");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("refusing"), "{err}");
    }
    assert!(
        matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "client connected to a socket in an insecure directory"
    );
    Ok(())
}

fn send(s: &mut UnixStream, m: &ClientMsg) -> Result<()> {
    let b = encode(m)?;
    s.write_all(&u32::try_from(b.len())?.to_be_bytes())?;
    s.write_all(&b)?;
    Ok(())
}

fn recv(s: &mut UnixStream) -> Result<DaemonMsg> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len)?;
    let mut buf = vec![0u8; u32::from_be_bytes(len) as usize];
    s.read_exact(&mut buf)?;
    Ok(decode(&buf)?)
}

#[test]
fn invalid_sizes_are_rejected_without_dropping_the_connection() -> Result<()> {
    let env = Env::bash(false)?;
    assert!(env.baton(&["daemon", "start"])?.status.success());
    let mut s = UnixStream::connect(env.sock())?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    send(
        &mut s,
        &ClientMsg::Hello {
            version: PROTOCOL_VERSION,
            role: Role::Tui,
        },
    )?;
    assert!(matches!(recv(&mut s)?, DaemonMsg::Welcome { .. }));
    for bad in [
        ClientMsg::Resize { rows: 0, cols: 80 },
        ClientMsg::Resize {
            rows: 24,
            cols: 1001,
        },
        ClientMsg::Attach { rows: 0, cols: 0 },
        ClientMsg::Attach {
            rows: 1001,
            cols: 80,
        },
    ] {
        send(&mut s, &bad)?;
        assert!(
            matches!(recv(&mut s)?, DaemonMsg::Error { .. }),
            "{bad:?} accepted"
        );
    }
    // Still usable; an absurd scrollback count is clamped, not an error loop.
    send(&mut s, &ClientMsg::Status)?;
    assert!(matches!(recv(&mut s)?, DaemonMsg::DaemonStatus { .. }));
    Ok(())
}

fn attached_client(env: &Env) -> Result<UnixStream> {
    let mut s = UnixStream::connect(env.sock())?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    send(
        &mut s,
        &ClientMsg::Hello {
            version: PROTOCOL_VERSION,
            role: Role::Tui,
        },
    )?;
    assert!(matches!(recv(&mut s)?, DaemonMsg::Welcome { .. }));
    send(&mut s, &ClientMsg::Attach { rows: 24, cols: 80 })?;
    assert!(matches!(recv(&mut s)?, DaemonMsg::SessionList(_)));
    Ok(s)
}

#[test]
fn second_attach_takes_over_and_notifies_the_first() -> Result<()> {
    let env = Env::bash(false)?;
    assert!(env.baton(&["daemon", "start"])?.status.success());
    let mut first = attached_client(&env)?;
    let _second = attached_client(&env)?;
    assert_eq!(
        recv(&mut first)?,
        DaemonMsg::Error {
            message: "replaced by another client".into()
        }
    );
    assert!(recv(&mut first).is_err(), "first connection must close");
    Ok(())
}

#[test]
fn oversized_input_is_rejected_and_the_connection_survives() -> Result<()> {
    let env = Env::bash(false)?;
    let a = env.id("a")?;
    env.debug(&["open", "x"])?;
    let mut s = UnixStream::connect(env.sock())?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    send(
        &mut s,
        &ClientMsg::Hello {
            version: PROTOCOL_VERSION,
            role: Role::Tui,
        },
    )?;
    assert!(matches!(recv(&mut s)?, DaemonMsg::Welcome { .. }));
    let session = baton_proto::SessionId(a);
    send(
        &mut s,
        &ClientMsg::Input {
            session: session.clone(),
            bytes: vec![b'x'; baton_proto::MAX_INPUT + 1],
        },
    )?;
    assert!(matches!(recv(&mut s)?, DaemonMsg::Error { .. }));
    // Exactly the cap is accepted (no reply), and the connection still works.
    send(
        &mut s,
        &ClientMsg::Input {
            session,
            bytes: vec![b' '; baton_proto::MAX_INPUT],
        },
    )?;
    send(&mut s, &ClientMsg::Status)?;
    assert!(matches!(recv(&mut s)?, DaemonMsg::DaemonStatus { .. }));
    Ok(())
}

#[test]
fn a_client_that_cannot_keep_up_is_told_and_disconnected() -> Result<()> {
    let env = Env::bash(false)?;
    let a = env.id("a")?;
    env.debug(&["open", "x"])?;
    let mut s = attached_client(&env)?;
    s.set_read_timeout(Some(Duration::from_secs(15)))?;
    // Flood output while this client does not read at all.
    env.debug(&["send", &a, "yes 0123456789abcdef | head -c 200000000\\r"])?;
    std::thread::sleep(Duration::from_secs(3));
    let mut told = false;
    for _ in 0..200_000 {
        match recv(&mut s) {
            Ok(DaemonMsg::Error { message }) => {
                assert!(message.contains("too slow"), "{message}");
                told = true;
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(told, "never received the slow-client error");
    assert!(recv(&mut s).is_err(), "connection must close after it");
    Ok(())
}
