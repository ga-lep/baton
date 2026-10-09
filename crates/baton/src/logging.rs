//! Daemon logging to `<state>/daemon.log` via `tracing-appender`.

use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;

/// Installs a global subscriber writing to `<state_dir>/daemon.log`.
///
/// The returned guard flushes the log on drop; keep it alive until exit.
///
/// # Errors
/// If a global subscriber is already installed.
pub fn init(state_dir: &Path) -> anyhow::Result<WorkerGuard> {
    let appender = tracing_appender::rolling::never(state_dir, "daemon.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .try_init()
        .map_err(|e| anyhow::anyhow!("installing log subscriber: {e}"))?;
    Ok(guard)
}
