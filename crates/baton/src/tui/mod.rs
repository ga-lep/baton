//! Terminal UI: the ratatui client of the daemon.

pub mod app;
pub mod editor;
pub mod event_loop;
pub mod git;
pub mod help;
pub mod info_panel;
pub mod keys;
pub mod labels;
pub mod mirror;
// Not wired into the UI until Task 2 of project-summary-pane; remove then.
#[allow(dead_code)]
pub mod project_summary;
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
