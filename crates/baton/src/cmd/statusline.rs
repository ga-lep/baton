//! `baton statusline`: the status line of Claude sessions started by Baton.
//!
//! Claude Code passes it JSON that includes the subscription quota
//! (`rate_limits`). It relays that JSON to the daemon, then runs the user's own
//! `statusline` command from the config with the same JSON on stdin and prints
//! its output. Without such a command it prints nothing. It never fails.

use super::hook;
use baton_core::config::Config;
use baton_core::paths;
use baton_proto::{ClientMsg, SessionId};
use std::io::Write;
use std::process::{Child, Command, ExitCode, Stdio};

/// Runs the status line: relay, then the user's command.
pub fn run() -> ExitCode {
    let payload = hook::read_stdin();
    // Started before the relay so the two overlap.
    let child = user_command().and_then(|cmd| spawn(&cmd, &payload));
    if let Ok(session) = std::env::var("BATON_SESSION") {
        let _ = hook::send(ClientMsg::StatusLine {
            baton_session: SessionId(session),
            payload_json: payload,
        });
    }
    if let Some(out) = child.and_then(|c| c.wait_with_output().ok()) {
        let _ = std::io::stdout().write_all(&out.stdout);
    }
    ExitCode::SUCCESS
}

/// The `statusline` command from the config; `None` if unset or the config
/// cannot be loaded.
fn user_command() -> Option<String> {
    let path = paths::config_file().ok()?;
    Config::load(&path, &|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .ok()?
        .statusline
}

/// Starts `cmd` through `sh -c` with `payload` on its stdin.
fn spawn(cmd: &str, payload: &str) -> Option<Child> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let payload = payload.to_owned();
        // A command that does not read its stdin must not block us.
        std::thread::spawn(move || {
            let _ = stdin.write_all(payload.as_bytes());
        });
    }
    Some(child)
}
