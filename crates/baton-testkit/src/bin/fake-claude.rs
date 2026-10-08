//! `fake-claude`: a deterministic stand-in for `claude` that speaks the hook
//! contract Baton relies on.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::sys::termios::{self, SetArg};
use serde_json::{Value, json};

#[derive(Default)]
struct Args {
    settings: Option<PathBuf>,
    resume: Option<String>,
    continue_: bool,
    session_id: Option<String>,
    model: Option<String>,
}

fn parse_args(argv: &[String]) -> Args {
    let mut a = Args::default();
    let mut it = argv.iter().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--settings" => a.settings = it.next().map(PathBuf::from),
            "--resume" => a.resume = it.next().cloned(),
            "--continue" => a.continue_ = true,
            "--session-id" => a.session_id = it.next().cloned(),
            "--model" => a.model = it.next().cloned(),
            _ => {}
        }
    }
    a
}

fn home_dir() -> PathBuf {
    if let Some(h) = std::env::var_os("FAKE_CLAUDE_HOME") {
        return h.into();
    }
    if let Some(h) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return h.into();
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join(".fake-claude")
}

fn slug(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn find_transcript(projects: &Path, id: &str) -> Option<PathBuf> {
    fs::read_dir(projects)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path().join(format!("{id}.jsonl")))
        .find(|p| p.exists())
}

fn latest_transcript(dir: &Path) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .max_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok())
}

fn log_launch(argv: &[String], cwd: &Path) -> Result<()> {
    let Some(path) = std::env::var_os("FAKE_CLAUDE_LOG") else {
        return Ok(());
    };
    let env = |k: &str| std::env::var(k).ok();
    let line = json!({
        "argv": argv,
        "cwd": cwd,
        "env": {
            "BATON_SESSION": env("BATON_SESSION"),
            "BATON_SOCK": env("BATON_SOCK"),
            "CLAUDE_CONFIG_DIR": env("CLAUDE_CONFIG_DIR"),
        },
    });
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{line}")?;
    Ok(())
}

/// Send DA1 and wait up to 2 s for the `c`-terminated reply.
fn probe_da1() -> bool {
    let stdin = std::io::stdin();
    let Ok(orig) = termios::tcgetattr(&stdin) else {
        return false;
    };
    let mut raw = orig.clone();
    termios::cfmakeraw(&mut raw);
    if termios::tcsetattr(&stdin, SetArg::TCSANOW, &raw).is_err() {
        return false;
    }
    let mut out = std::io::stdout();
    let _ = out.write_all(b"\x1b[c");
    let _ = out.flush();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut ok = false;
    'outer: while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let ms = u16::try_from(left.as_millis()).unwrap_or(u16::MAX);
        let mut fds = [PollFd::new(stdin.as_fd(), PollFlags::POLLIN)];
        match poll(&mut fds, PollTimeout::from(ms)) {
            Ok(n) if n > 0 => {}
            _ => break,
        }
        let mut b = [0u8; 1];
        // Read the fd directly: a buffered reader would hide queued bytes from poll.
        match nix::unistd::read(&stdin, &mut b) {
            Ok(1) if b[0] == b'c' => {
                ok = true;
                break 'outer;
            }
            Ok(1) => {}
            _ => break,
        }
    }
    let _ = termios::tcsetattr(&stdin, SetArg::TCSANOW, &orig);
    ok
}

struct Session {
    id: String,
    model: String,
    cwd: PathBuf,
    transcript: PathBuf,
    hooks: Value,
    hooks_enabled: bool,
}

impl Session {
    fn fire(&self, event: &str, extra: Value) {
        if !self.hooks_enabled {
            return;
        }
        let mut input = json!({
            "session_id": self.id,
            "transcript_path": self.transcript,
            "cwd": self.cwd,
            "hook_event_name": event,
            "permission_mode": "default",
            "scratchpad_dir": home_dir().join("scratch"),
        });
        if let (Some(base), Some(add)) = (input.as_object_mut(), extra.as_object()) {
            base.extend(add.clone());
        }
        let matcher_subject = extra
            .get("tool_name")
            .or_else(|| extra.get("notification_type"))
            .and_then(Value::as_str);
        let Some(groups) = self
            .hooks
            .get("hooks")
            .and_then(|h| h.get(event))
            .and_then(Value::as_array)
        else {
            return;
        };
        for group in groups {
            let matcher = group.get("matcher").and_then(Value::as_str).unwrap_or("");
            if !(matcher.is_empty() || matcher == "*" || Some(matcher) == matcher_subject) {
                continue;
            }
            let cmds = group
                .get("hooks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten();
            for cmd in cmds.filter_map(|h| h.get("command").and_then(Value::as_str)) {
                run_hook(cmd, &input.to_string());
            }
        }
    }

    fn append(&self, entry: &Value) -> Result<()> {
        let mut f = OpenOptions::new().append(true).open(&self.transcript)?;
        writeln!(f, "{entry}")?;
        Ok(())
    }

    fn assistant_turn(&self) -> Result<()> {
        let msg_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
        for text in ["thinking", "done"] {
            self.append(&json!({
                "type": "assistant",
                "sessionId": self.id,
                "isSidechain": false,
                "message": {
                    "id": msg_id,
                    "model": self.model,
                    "role": "assistant",
                    "content": [{"type": "text", "text": text}],
                    "usage": {
                        "input_tokens": 100,
                        "output_tokens": 50,
                        "cache_read_input_tokens": 1000,
                        "cache_creation_input_tokens": 10,
                    },
                },
            }))?;
        }
        Ok(())
    }
}

fn run_hook(cmd: &str, input: &str) {
    // The hook command is data from the settings file, executed by `sh -c`
    // exactly as Claude Code does; the argv itself is a fixed vector.
    let Ok(mut child) = Command::new("sh")
        .args(["-c", cmd])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    if let Some(mut stdin) = child.stdin.take() {
        // A hook that does not read its stdin closes the pipe early.
        let _ = stdin.write_all(input.as_bytes());
    }
    let _ = child.wait();
}

fn prompt() {
    let mut out = std::io::stdout();
    let _ = out.write_all(b"> ");
    let _ = out.flush();
}

fn run() -> Result<u8> {
    let argv: Vec<String> = std::env::args().collect();
    let args = parse_args(&argv);
    let cwd = std::env::current_dir().context("cwd")?;
    log_launch(&argv, &cwd)?;
    let projects = home_dir().join("projects");
    let project_dir = projects.join(slug(&cwd));

    let (id, source, existing) = if let Some(id) = &args.resume {
        match find_transcript(&projects, id) {
            Some(p) => (id.clone(), "resume", Some(p)),
            None => {
                println!("No conversation found with session ID: {id}");
                return Ok(1);
            }
        }
    } else if args.continue_ {
        match latest_transcript(&project_dir) {
            Some(p) => {
                let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
                (stem.to_string(), "resume", Some(p))
            }
            None => {
                println!("No conversation found to continue");
                return Ok(1);
            }
        }
    } else {
        let id = args
            .session_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        (id, "startup", None)
    };

    let transcript = existing.unwrap_or_else(|| project_dir.join(format!("{id}.jsonl")));
    fs::create_dir_all(transcript.parent().context("transcript dir")?)?;
    if !transcript.exists() {
        fs::write(
            &transcript,
            format!(
                "{}\n",
                json!({"type": "permission-mode", "permissionMode": "default"})
            ),
        )?;
    }
    let hooks = match &args.settings {
        Some(p) => serde_json::from_str(&fs::read_to_string(p)?).unwrap_or(Value::Null),
        None => Value::Null,
    };
    let model = args
        .model
        .clone()
        .unwrap_or_else(|| "claude-opus-5-5".into());
    let session = Session {
        id,
        model,
        cwd,
        transcript,
        hooks,
        hooks_enabled: std::env::var_os("FAKE_CLAUDE_NO_HOOKS").is_none_or(|v| v != "1"),
    };

    let da1 = if probe_da1() { "ok" } else { "timeout" };
    println!("DA1: {da1}");
    println!("FAKE CLAUDE session={} model={}", session.id, session.model);
    session.fire(
        "SessionStart",
        json!({"source": source, "model": session.model}),
    );
    prompt();

    let mut line = String::new();
    let stdin = std::io::stdin();
    loop {
        line.clear();
        if stdin.read_line(&mut line)? == 0 {
            session.fire("SessionEnd", json!({"reason": "other"}));
            return Ok(0);
        }
        let cmdline = line.trim();
        let (cmd, rest) = cmdline.split_once(' ').unwrap_or((cmdline, ""));
        match cmd {
            "prompt" => {
                session.fire("UserPromptSubmit", json!({"prompt": rest}));
                let tool = json!({"tool_name": "Bash", "tool_input": {"command": "true"}});
                session.fire("PreToolUse", tool.clone());
                session.fire("PostToolUse", tool);
                session.assistant_turn()?;
                println!("done");
                session.fire("Stop", json!({"stop_hook_active": false, "last_assistant_message": "done"}));
            }
            "perm" => {
                let tool = json!({"tool_name": "Bash", "tool_input": {"command": "true"}});
                session.fire("PreToolUse", tool.clone());
                session.fire("PermissionRequest", tool);
                session.fire("Notification", json!({"notification_type": "permission_prompt", "message": "Claude needs your permission"}));
                println!("permission needed");
            }
            "allow" => {
                session.fire("PostToolUse", json!({"tool_name": "Bash", "tool_input": {"command": "true"}}));
                session.fire("Stop", json!({"stop_hook_active": false, "last_assistant_message": "done"}));
            }
            "idle" => session.fire("Notification", json!({"notification_type": "idle_prompt", "message": "Claude is waiting for your input"})),
            "color" => println!("\x1b[31mred text\x1b[0m"),
            "exit" => {
                session.fire("SessionEnd", json!({"reason": "prompt_input_exit"}));
                return Ok(rest.trim().parse().unwrap_or(0));
            }
            _ => {}
        }
        prompt();
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("fake-claude: {e:#}");
            ExitCode::from(70)
        }
    }
}
