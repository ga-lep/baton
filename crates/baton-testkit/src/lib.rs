//! Test kit for Baton: PTY driver and fake `claude` helpers.

mod drive;

pub use drive::{Drive, DriveOptions, unescape};

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

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
