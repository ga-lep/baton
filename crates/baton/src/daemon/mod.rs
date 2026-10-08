//! The background daemon: single instance, unix socket, handshake, clean shutdown.

pub mod lifecycle;
pub mod notifier;
pub mod registry;
pub mod server;
pub mod session;
pub mod spawn;

use anyhow::{Context, Result};
use baton_core::paths;
use std::os::unix::fs::DirBuilderExt;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Grace period between SIGHUP and SIGKILL for child process groups.
const KILL_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

/// Outcome of [`run_foreground`].
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The daemon ran and shut down cleanly.
    Finished,
    /// Another daemon already holds the lock.
    AlreadyRunning,
}

/// Runs the daemon attached to the current process until shutdown.
///
/// # Errors
/// If the runtime/state dirs, lock, logging or socket cannot be set up.
pub fn run_foreground() -> Result<Outcome> {
    // Private-dir check comes first: nothing is bound in an untrusted directory.
    let run_dir = paths::ensure_runtime_dir().context("runtime dir")?;
    let Some(lock) = lifecycle::acquire_lock(&run_dir).context("daemon lock")? else {
        return Ok(Outcome::AlreadyRunning);
    };
    let state_dir = paths::state_dir().context("state dir")?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&state_dir)
        .context("creating state dir")?;
    let _log_guard = crate::logging::init(&state_dir)?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?;
    let result = rt.block_on(serve_until_shutdown(&run_dir));
    if let Err(e) = &result {
        tracing::error!("daemon failed: {e:#}");
    }
    drop(lock); // held for the whole lifetime, released only after cleanup
    result.map(|()| Outcome::Finished)
}

async fn serve_until_shutdown(run_dir: &std::path::Path) -> Result<()> {
    use tokio::signal::unix::{SignalKind, signal};
    let sock = run_dir.join("baton.sock");
    write_hooks_json(run_dir).context("writing hooks.json")?;
    let listener = lifecycle::bind_socket(&sock).context("binding socket")?;
    tracing::info!(pid = std::process::id(), "daemon listening");

    let state = Arc::new(server::State::new());
    let shutdown = CancellationToken::new();
    let mut term = signal(SignalKind::terminate()).context("SIGTERM handler")?;
    let mut int = signal(SignalKind::interrupt()).context("SIGINT handler")?;
    let serving = server::serve(listener, shutdown.clone(), Arc::clone(&state));
    tokio::pin!(serving);
    tokio::select! {
        () = &mut serving => {}
        _ = term.recv() => tracing::info!("SIGTERM received"),
        _ = int.recv() => tracing::info!("SIGINT received"),
    }
    shutdown.cancel();
    let groups = state.groups.take();
    lifecycle::terminate_groups(&groups, KILL_GRACE).await;
    lifecycle::remove_socket(&sock);
    tracing::info!("daemon stopped");
    Ok(())
}

/// Writes `<run_dir>/hooks.json` for the current executable, atomically
/// (private temp file, then rename) inside the private runtime directory.
fn write_hooks_json(run_dir: &std::path::Path) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let exe = std::env::current_exe().context("locating the baton executable")?;
    let json = baton_core::hooks::settings_json(&exe);
    let tmp = run_dir.join(format!("hooks.json.tmp.{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(json.as_bytes())?;
    f.sync_all()?;
    drop(f);
    if let Err(e) = std::fs::rename(&tmp, run_dir.join("hooks.json")) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}
