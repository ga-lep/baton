//! `baton doctor`: checks the environment and that every profile's `claude`
//! really fires the hooks Baton injects (so `unknown` statuses can be
//! diagnosed).
//!
//! The hook probe launches the profile command in a PTY with a private
//! `--settings` file whose `SessionStart` hook writes its stdin to a file. It
//! never sends any input, so a real `claude` spends no tokens.

use crate::client::{self, ClientError};
use crate::daemon::spawn::build_command;
use crate::update::{self, Reason};
use baton_core::config::{Config, SessionSpec};
use baton_core::hooks::shell_quote;
use baton_core::paths;
use baton_core::update::Outcome;
use baton_proto::{PROTOCOL_VERSION, Role, SessionId};
use portable_pty::{Child, MasterPty, PtySize, native_pty_system};
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::fmt;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Default time the probe waits for `SessionStart`.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
/// Time a process group gets between SIGTERM and SIGKILL.
const TERM_GRACE: Duration = Duration::from_secs(1);
/// Time `<cmd> --version` may take.
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
/// Most bytes captured from `<cmd> --version`.
const MAX_VERSION_OUTPUT: u64 = 4096;
/// Most characters of a printed value.
const MAX_PRINT_CHARS: usize = 200;
/// Most bytes read from `probe.json`.
const MAX_PROBE_JSON: u64 = 1024 * 1024;
/// Time to wait for the daemon handshake.
const DAEMON_TIMEOUT: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(50);
const HINT: &str = "trust dialog pending or hooks disabled (disableAllHooks / --bare)?";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Pass,
    Warn,
    Fail,
}

/// One printed result line.
struct Line {
    level: Level,
    text: String,
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tag = match self.level {
            Level::Pass => "PASS",
            Level::Warn => "WARN",
            Level::Fail => "FAIL",
        };
        write!(f, "{tag} {}", self.text)
    }
}

/// Prints check results and remembers whether any failed.
#[derive(Default)]
struct Report {
    failed: bool,
}

impl Report {
    fn emit(&mut self, level: Level, text: String) {
        self.failed |= level == Level::Fail;
        println!("{}", Line { level, text });
    }
}

/// Runs `baton doctor`; exits 1 if any check failed.
pub fn run(no_probe: bool) -> ExitCode {
    let mut report = Report::default();
    let config = check_config(&mut report);
    let dirs_ok = check_dirs(&mut report);
    check_daemon(&mut report, dirs_ok);
    if let Some(config) = config {
        let probe_timeout = std::env::var("BATON_DOCTOR_PROBE_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .map_or(PROBE_TIMEOUT, Duration::from_secs);
        for spec in first_session_per_profile(&config) {
            if check_command(&mut report, spec) && !no_probe {
                check_probe(&mut report, spec, probe_timeout);
            }
        }
    }
    check_notify(&mut report);
    check_version(&mut report);
    if report.failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// The first configured session of each profile, in config order.
fn first_session_per_profile(config: &Config) -> Vec<&SessionSpec> {
    let mut seen: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for spec in config.projects.iter().flat_map(|p| &p.sessions) {
        if !seen.contains(&spec.profile.as_str()) {
            seen.push(&spec.profile);
            out.push(spec);
        }
    }
    out
}

fn check_config(report: &mut Report) -> Option<Config> {
    let loaded = paths::config_file()
        .map_err(|e| e.to_string())
        .and_then(|path| {
            Config::load(&path, &|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
                .map(|c| (path, c))
                .map_err(|e| e.to_string())
        });
    match loaded {
        Ok((path, config)) => {
            report.emit(Level::Pass, format!("config: {} parses", path.display()));
            Some(config)
        }
        Err(e) => {
            report.emit(Level::Fail, format!("config: {}", sanitize(&e)));
            None
        }
    }
}

fn check_dirs(report: &mut Report) -> bool {
    let mut ok = true;
    match paths::ensure_runtime_dir() {
        Ok(dir) => report.emit(
            Level::Pass,
            format!("dirs: runtime dir {} is private (0700)", dir.display()),
        ),
        Err(e) => {
            ok = false;
            report.emit(Level::Fail, format!("dirs: runtime dir: {e}"));
        }
    }
    match paths::ensure_state_dir() {
        Ok(dir) if nix::unistd::access(&dir, nix::unistd::AccessFlags::W_OK).is_ok() => {
            report.emit(
                Level::Pass,
                format!("dirs: state dir {} is writable", dir.display()),
            );
        }
        Ok(dir) => {
            report.emit(
                Level::Fail,
                format!("dirs: state dir {} is not writable", dir.display()),
            );
        }
        Err(e) => report.emit(Level::Fail, format!("dirs: state dir: {e}")),
    }
    ok
}

fn check_daemon(report: &mut Report, dirs_ok: bool) {
    if !dirs_ok {
        report.emit(
            Level::Warn,
            "daemon: skipped (runtime dir unusable)".to_owned(),
        );
        return;
    }
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(ClientError::Io)
        .and_then(|rt| {
            rt.block_on(async {
                tokio::time::timeout(DAEMON_TIMEOUT, client::connect(Role::Ctl))
                    .await
                    .unwrap_or(Err(ClientError::Closed))
            })
        });
    match result {
        Ok(conn) => report.emit(
            Level::Pass,
            format!(
                "daemon: running pid={} protocol={PROTOCOL_VERSION}",
                conn.pid
            ),
        ),
        Err(ClientError::NotRunning) => {
            report.emit(Level::Warn, "daemon: not running".to_owned());
        }
        Err(e) => report.emit(Level::Fail, format!("daemon: {e}")),
    }
}

/// Looks `program` up on `path_env` (or uses it directly if it has a `/`).
fn resolve_program(program: &str, path_env: Option<&OsStr>) -> Option<PathBuf> {
    let is_exec = |p: &Path| {
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if program.contains('/') {
        let p = std::path::absolute(program).ok()?;
        return is_exec(&p).then_some(p);
    }
    std::env::split_paths(path_env?)
        .map(|d| d.join(program))
        .find(|p| is_exec(p))
}

/// Resolves the profile command and captures its version; false if unusable.
fn check_command(report: &mut Report, spec: &SessionSpec) -> bool {
    let profile = &spec.profile;
    let Some(program) = spec.argv.first() else {
        report.emit(Level::Fail, format!("command: empty (profile {profile})"));
        return false;
    };
    let Some(path) = resolve_program(program, std::env::var_os("PATH").as_deref()) else {
        report.emit(
            Level::Fail,
            format!(
                "command: {} not found on PATH (profile {profile})",
                sanitize(program)
            ),
        );
        return false;
    };
    report.emit(
        Level::Pass,
        format!(
            "command: {} (profile {profile})",
            sanitize(&path.to_string_lossy())
        ),
    );
    match capture_version(spec, &path) {
        Ok(line) => report.emit(Level::Pass, format!("version: {line} (profile {profile})")),
        Err(e) => report.emit(Level::Warn, format!("version: {e} (profile {profile})")),
    }
    true
}

/// Removes control characters and caps the length.
fn sanitize(s: &str) -> String {
    s.chars()
        .filter(|c| !baton_core::update::is_unsafe_char(*c))
        .take(MAX_PRINT_CHARS)
        .collect()
}

/// Terminates a whole process group: SIGTERM, a grace period (ended early
/// once `exited` returns true), then SIGKILL.
fn terminate_group(pgid: nix::unistd::Pid, exited: &mut dyn FnMut() -> bool) {
    use nix::sys::signal::{Signal, killpg};
    // ESRCH just means the group is already gone.
    let _ = killpg(pgid, Signal::SIGTERM);
    let deadline = Instant::now() + TERM_GRACE;
    while Instant::now() < deadline && !exited() {
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = killpg(pgid, Signal::SIGKILL);
}

/// Kills and reaps a `std` child's process group when dropped.
struct StdGroupGuard {
    pgid: nix::unistd::Pid,
    child: std::process::Child,
}

impl Drop for StdGroupGuard {
    fn drop(&mut self) {
        let child = &mut self.child;
        terminate_group(self.pgid, &mut || matches!(child.try_wait(), Ok(Some(_))));
        let _ = child.wait();
    }
}

/// Runs `<profile argv> --version` with a timeout and bounded output.
fn capture_version(spec: &SessionSpec, program: &Path) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(spec.argv.iter().skip(1))
        .arg("--version")
        .envs(&spec.env)
        .current_dir(&spec.repo)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = cmd.spawn().map_err(|e| format!("cannot run: {e}"))?;
    let stdout = child.stdout.take();
    let pid = i32::try_from(child.id()).map_err(|e| e.to_string())?;
    let _guard = StdGroupGuard {
        pgid: nix::unistd::Pid::from_raw(pid),
        child,
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(out) = stdout {
            let _ = out.take(MAX_VERSION_OUTPUT).read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    let buf = rx
        .recv_timeout(VERSION_TIMEOUT)
        .map_err(|_| format!("--version timed out after {}s", VERSION_TIMEOUT.as_secs()))?;
    let text = String::from_utf8_lossy(&buf);
    text.lines()
        .map(|l| sanitize(l).trim().to_owned())
        .find(|l| !l.is_empty())
        .ok_or_else(|| "--version printed nothing".to_owned())
}

/// The probe's `--settings` JSON: one `SessionStart` hook that stores its
/// stdin in `probe_json`.
fn probe_settings(probe_json: &Path) -> String {
    let inner = format!("cat > {}", shell_quote(&probe_json.to_string_lossy()));
    let command = format!("sh -c {}", shell_quote(&inner));
    json!({"hooks": {"SessionStart": [{"hooks": [{
        "type": "command",
        "command": command,
        "timeout": 5,
    }]}]}})
    .to_string()
}

/// Reads at most [`MAX_PROBE_JSON`] bytes of `probe.json`; `Some` once it
/// holds a complete `SessionStart` payload with a session id.
fn read_probe(path: &Path) -> Option<String> {
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_PROBE_JSON)
        .read_to_string(&mut text)
        .ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    if v.get("hook_event_name").and_then(Value::as_str) != Some("SessionStart") {
        return None;
    }
    v.get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Kills and reaps the probe's process group when dropped, also on panic.
struct ProbeGuard {
    pgid: nix::unistd::Pid,
    child: Box<dyn Child + Send + Sync>,
    /// Kept open so the PTY stays alive; dropped after the group is gone.
    master: Box<dyn MasterPty + Send>,
}

impl Drop for ProbeGuard {
    fn drop(&mut self) {
        let child = &mut self.child;
        terminate_group(self.pgid, &mut || matches!(child.try_wait(), Ok(Some(_))));
        let _ = child.wait();
    }
}

/// Launches the profile in a PTY and waits for its `SessionStart` hook.
fn check_probe(report: &mut Report, spec: &SessionSpec, timeout: Duration) {
    let profile = &spec.profile;
    match run_probe(spec, timeout) {
        Ok(()) => report.emit(
            Level::Pass,
            format!("hooks: SessionStart received (profile {profile})"),
        ),
        Err(why) => report.emit(Level::Fail, format!("hooks: {why} (profile {profile})")),
    }
}

fn run_probe(spec: &SessionSpec, timeout: Duration) -> Result<(), String> {
    // Declared first so it is removed last, after the guard has reaped the group.
    let tmp = tempfile::Builder::new()
        .prefix("baton-doctor-")
        .tempdir()
        .map_err(|e| format!("cannot create temp dir: {e}"))?;
    let hooks = tmp.path().join("hooks.json");
    let probe_json = tmp.path().join("probe.json");
    std::fs::write(&hooks, probe_settings(&probe_json))
        .map_err(|e| format!("cannot write probe settings: {e}"))?;

    let cmd = build_command(
        spec,
        &SessionId("doctor-probe".to_owned()),
        &tmp.path().join("no.sock"),
        &hooks,
        &[],
    )
    .map_err(|e| format!("cannot build command: {e:#}"))?;
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("cannot open pty: {e:#}"))?;
    // portable-pty calls setsid() in the child: the pid is its group id. The
    // writer half is deliberately never taken (its drop would write to the
    // terminal): nothing is ever sent to the probe.
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("cannot start command: {e:#}"))?;
    drop(pair.slave);
    let pid = child.process_id().ok_or("child has no pid")?;
    let pgid = nix::unistd::Pid::from_raw(i32::try_from(pid).map_err(|e| e.to_string())?);
    let mut guard = ProbeGuard {
        pgid,
        child,
        master: pair.master,
    };
    // Drain the child's terminal output so it can never block on a full PTY.
    if let Ok(mut reader) = guard.master.try_clone_reader() {
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while reader.read(&mut buf).is_ok_and(|n| n > 0) {}
        });
    }

    let deadline = Instant::now() + timeout;
    loop {
        if read_probe(&probe_json).is_some() {
            return Ok(());
        }
        if let Ok(Some(status)) = guard.child.try_wait() {
            // The hook may have landed just before the exit.
            return if read_probe(&probe_json).is_some() {
                Ok(())
            } else {
                Err(format!("command exited ({status}) before SessionStart"))
            };
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "no SessionStart within {}s: {HINT}",
                timeout.as_secs()
            ));
        }
        std::thread::sleep(POLL);
    }
}

fn check_notify(report: &mut Report) {
    let has_tool = resolve_program("notify-send", std::env::var_os("PATH").as_deref()).is_some();
    let has_bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some_and(|v| !v.is_empty())
        || std::env::var_os("XDG_RUNTIME_DIR").is_some_and(|d| Path::new(&d).join("bus").exists());
    match (has_tool, has_bus) {
        (true, true) => report.emit(
            Level::Pass,
            "notify: notify-send and the D-Bus session bus are available".to_owned(),
        ),
        (false, _) => report.emit(
            Level::Warn,
            "notify: notify-send not found on PATH".to_owned(),
        ),
        (true, false) => report.emit(Level::Warn, "notify: no D-Bus session bus found".to_owned()),
    }
}

/// Reports the running version against the latest release. Never `FAIL`:
/// an offline machine or a private repo must not change doctor's exit code.
fn check_version(report: &mut Report) {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    if !baton_core::update::enabled(crate::cmd::version::config_flag(), &|k| {
        std::env::var(k).ok()
    }) {
        report.emit(
            Level::Pass,
            format!("version: {VERSION} (update check disabled)"),
        );
        return;
    }
    match update::check(false, update::CLI_TIMEOUT) {
        Ok(checked) => match checked.outcome {
            Outcome::UpToDate => {
                report.emit(Level::Pass, format!("version: {VERSION} (latest)"));
            }
            Outcome::Newer { latest } => report.emit(
                Level::Warn,
                format!(
                    "version: {}",
                    update::describe_newer(
                        &sanitize(&latest),
                        VERSION,
                        checked.html_url.as_deref().map(sanitize).as_deref()
                    )
                ),
            ),
            Outcome::Unknown(why) => report.emit(
                Level::Warn,
                format!(
                    "version: {}",
                    update::describe_failure(&sanitize(&why), Reason::Parens)
                ),
            ),
        },
        Err(why) => report.emit(
            Level::Warn,
            format!(
                "version: {}",
                update::describe_failure(&sanitize(&why), Reason::Parens)
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_control_characters_and_caps_length() {
        assert_eq!(sanitize("2.1\x1b[31m.295\r\u{7}"), "2.1[31m.295");
        assert_eq!(sanitize("a\u{85}b\u{202e}c\u{200b}d\u{feff}"), "abcd");
        assert_eq!(sanitize(&"a".repeat(1000)).len(), MAX_PRINT_CHARS);
    }

    #[test]
    fn probe_settings_quotes_the_path_for_both_shell_layers() {
        let json = probe_settings(Path::new("/tmp/it's $x/probe.json"));
        let v: Value = serde_json::from_str(&json).expect("json");
        let cmd = v["hooks"]["SessionStart"][0]["hooks"][0]["command"]
            .as_str()
            .expect("command");
        let words = shell_words::split(cmd).expect("shell words");
        assert_eq!(words[..2], ["sh", "-c"]);
        let inner = shell_words::split(&words[2]).expect("inner words");
        assert_eq!(inner, ["cat", ">", "/tmp/it's $x/probe.json"]);
    }

    #[test]
    fn read_probe_needs_a_complete_session_start() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("probe.json");
        assert!(read_probe(&p).is_none());
        std::fs::write(&p, "{\"hook_event_name\":\"SessionS").expect("write");
        assert!(read_probe(&p).is_none());
        std::fs::write(&p, r#"{"hook_event_name":"Stop","session_id":"a"}"#).expect("write");
        assert!(read_probe(&p).is_none());
        std::fs::write(&p, r#"{"hook_event_name":"SessionStart","session_id":"a"}"#)
            .expect("write");
        assert_eq!(read_probe(&p).as_deref(), Some("a"));
    }

    #[test]
    fn read_probe_caps_the_bytes_read() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("probe.json");
        let pad = " ".repeat(usize::try_from(MAX_PROBE_JSON).expect("fits"));
        std::fs::write(
            &p,
            format!(r#"{pad}{{"hook_event_name":"SessionStart","session_id":"a"}}"#),
        )
        .expect("write");
        assert!(read_probe(&p).is_none());
    }

    #[test]
    fn resolve_program_searches_path_and_requires_exec_bit() {
        let dir = tempfile::tempdir().expect("tmp");
        let exe = dir.path().join("tool");
        std::fs::write(&exe, "#!/bin/sh\n").expect("write");
        let path = std::env::join_paths([dir.path()]).expect("join");
        assert_eq!(resolve_program("tool", Some(&path)), None);
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        assert_eq!(resolve_program("tool", Some(&path)), Some(exe.clone()));
        assert_eq!(resolve_program("tool", None), None);
        assert_eq!(
            resolve_program(&exe.to_string_lossy(), None),
            Some(exe.clone())
        );
    }

    #[test]
    fn version_capture_returns_the_first_line() {
        let spec = SessionSpec {
            project: "x".into(),
            repo: PathBuf::from("/"),
            profile: "p".into(),
            argv: vec!["sh".into(), "-c".into(), "echo \"$0 v1\"; echo two".into()],
            args: vec![],
            env: Default::default(),
        };
        let sh = resolve_program("sh", std::env::var_os("PATH").as_deref()).expect("sh");
        // `--version` becomes $0 for the script.
        assert_eq!(
            capture_version(&spec, &sh).expect("version"),
            "--version v1"
        );
    }
}
