//! `baton hook <Event>`: forwards a Claude Code hook to the daemon.
//!
//! Claude injects this command's stdout into the conversation for some events
//! (and treats exit code 2 as "block"), so this command never prints, never
//! fails and never blocks for long. It is dispatched before clap parses the
//! command line, so even malformed arguments end in a silent exit 0.

use crate::client;
use baton_core::{hooks, paths};
use baton_proto::{ClientMsg, Role, SessionId};
use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

/// Largest stdin payload read; the rest is discarded.
pub const MAX_STDIN: u64 = 1024 * 1024;
/// How long to wait for stdin before forwarding the event without a payload.
const STDIN_DEADLINE: Duration = Duration::from_millis(500);
/// Budget for connecting, handshaking and sending.
const CONNECT_DEADLINE: Duration = Duration::from_millis(250);

/// Runs the hook with the arguments that follow `hook` on the command line.
/// Always returns success.
pub fn run(args: &[OsString]) -> ExitCode {
    let debug = std::env::var_os("BATON_HOOK_DEBUG").is_some_and(|v| v == "1");
    if !debug {
        // A panic message on stderr is harmless, but keep it quiet anyway.
        std::panic::set_hook(Box::new(|_| {}));
    }
    let result = std::panic::catch_unwind(|| forward(args));
    match result {
        Ok(Ok(())) => {}
        Ok(Err(why)) if debug => eprintln!("baton hook: {why}"),
        Ok(Err(_)) | Err(_) => {}
    }
    ExitCode::SUCCESS
}

/// The event name if the first argument is one of the closed event set.
fn event_arg(args: &[OsString]) -> Option<&str> {
    args.first()?.to_str().filter(|e| hooks::is_known_event(e))
}

fn forward(args: &[OsString]) -> Result<(), String> {
    let event = event_arg(args).ok_or("missing or unknown event")?;
    let session = std::env::var("BATON_SESSION").map_err(|_| "BATON_SESSION is not set")?;
    let sock = std::env::var_os("BATON_SOCK").map_or_else(paths::socket_path, PathBuf::from);
    let payload_json = read_stdin();
    let msg = ClientMsg::Hook {
        baton_session: SessionId(session),
        event: event.to_owned(),
        payload_json,
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async {
        let send = async {
            let mut conn = client::connect_to(&sock, Role::Hook)
                .await
                .map_err(|e| e.to_string())?;
            // The daemon's handshake reply was awaited above so that it
            // cannot see a closed socket before it reads the hook frame.
            conn.send(&msg).await.map_err(|e| e.to_string())
        };
        tokio::time::timeout(CONNECT_DEADLINE, send)
            .await
            .map_err(|_| "timed out talking to the daemon".to_owned())?
    })
}

/// Reads at most [`MAX_STDIN`] bytes of stdin, waiting at most
/// [`STDIN_DEADLINE`]; returns an empty string on timeout. The reader thread
/// is detached, so a stalled writer cannot keep the process alive.
fn read_stdin() -> String {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::stdin()
            .lock()
            .take(MAX_STDIN)
            .read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    rx.recv_timeout(STDIN_DEADLINE)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    #[test]
    fn only_known_events_are_accepted() {
        assert_eq!(event_arg(&args(&["Stop"])), Some("Stop"));
        assert_eq!(event_arg(&args(&["Stop", "--junk"])), Some("Stop"));
        assert_eq!(event_arg(&args(&[])), None);
        assert_eq!(event_arg(&args(&["--bogus"])), None);
        assert_eq!(event_arg(&args(&["../etc"])), None);
    }

    #[test]
    fn run_returns_success_for_garbage_arguments() {
        let bad = [OsString::from("--help"), OsString::from("x")];
        assert_eq!(run(&bad), ExitCode::SUCCESS);
    }
}
