//! Terminal UI: the ratatui client of the daemon.

pub mod app;
pub mod event_loop;
pub mod labels;
pub mod mirror;
pub mod render_pacer;
pub mod scrollback;
pub mod sidebar;
pub mod terminal_guard;
pub mod ui;

/// Runs the TUI until the user quits.
///
/// # Errors
/// If the daemon cannot be reached or started, or on terminal I/O errors.
pub fn run() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(event_loop::run())
}
