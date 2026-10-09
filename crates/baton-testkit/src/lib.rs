//! Test kit for Baton: PTY driver and fake `claude` helpers.

mod drive;
mod release_server;

pub use drive::{Drive, DriveOptions, unescape};
pub use release_server::{ReleaseServer, Reply};

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// Proxy variables `ureq` honours; a spawned baton must never inherit them,
/// or update-check tests would talk to the user's proxy, not the fake server.
pub const PROXY_VARS: &[&str] = &[
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
];

/// Variables that locate state or switch behaviour; tests must set the ones
/// they need explicitly rather than inherit them.
pub const CONTROLLED_VARS: &[&str] = &[
    "BATON_CONFIG",
    "BATON_STATE_DIR",
    "BATON_RUNTIME_DIR",
    "BATON_NOTIFY_SINK",
    "BATON_UPDATE_URL",
    "BATON_NO_UPDATE_CHECK",
    "BATON_SESSION",
    "BATON_SOCK",
    "BATON_HOOK_DEBUG",
    "BATON_DOCTOR_PROBE_TIMEOUT_SECS",
    "XDG_CONFIG_HOME",
    "XDG_STATE_HOME",
    "XDG_RUNTIME_DIR",
];

/// Removes the inherited proxy, `BATON_*` and `XDG_*` variables from `cmd`.
/// Call it before applying the test's own `.env(..)` settings.
pub fn scrub_env(cmd: &mut Command) -> &mut Command {
    for k in PROXY_VARS.iter().chain(CONTROLLED_VARS) {
        cmd.env_remove(k);
    }
    cmd
}

/// Package that owns each workspace binary.
fn package_of(name: &str) -> &'static str {
    match name {
        "baton" => "baton",
        _ => "baton-testkit",
    }
}

/// Locate a workspace binary next to the running test executable, building
/// it with `cargo build --bins -p <pkg>` (once per process) if missing.
///
/// # Errors
/// Fails if the target directory cannot be derived or the build fails.
pub fn bin_path(name: &str) -> Result<PathBuf> {
    static BUILD: Mutex<()> = Mutex::new(());
    let exe = std::env::current_exe().context("current_exe")?;
    // target/<profile>/deps/<test> or target/<profile>/<bin>
    let mut dir = exe.parent().context("exe has no parent")?.to_path_buf();
    if dir.file_name().is_some_and(|n| n == "deps") {
        dir.pop();
    }
    let path = dir.join(name);
    let _guard = BUILD.lock().unwrap_or_else(|e| e.into_inner());
    if !path.exists() {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut cmd = Command::new(cargo);
        cmd.args(["build", "--bins", "-p", package_of(name)]);
        if dir.file_name().is_some_and(|n| n == "release") {
            cmd.arg("--release");
        }
        let status = cmd.status().context("spawn cargo build")?;
        if !status.success() || !path.exists() {
            bail!("could not build binary {name} at {}", path.display());
        }
    }
    Ok(path)
}

/// Poll `f` every 20 ms until it returns `Some`, or fail after `timeout`.
///
/// # Errors
/// Returns an error when the timeout elapses first.
pub fn wait_for<T>(timeout: Duration, mut f: impl FnMut() -> Option<T>) -> Result<T> {
    let start = Instant::now();
    loop {
        if let Some(v) = f() {
            return Ok(v);
        }
        if start.elapsed() >= timeout {
            bail!("timed out after {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn scrub_env_removes_proxy_and_controlled_vars() {
        let mut cmd = Command::new("true");
        scrub_env(&mut cmd);
        let removed: Vec<&OsStr> = cmd
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k)
            .collect();
        for k in PROXY_VARS.iter().chain(CONTROLLED_VARS) {
            assert!(removed.contains(&OsStr::new(k)), "{k} not scrubbed");
        }
    }
}
