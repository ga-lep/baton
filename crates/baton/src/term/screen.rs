//! Terminal emulator abstraction.
//!
//! Callers (daemon, TUI mirror, spike) only see [`Screen`], so the emulator
//! behind it can be swapped without touching them.

use super::encode::EncodeModes;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// An emulated terminal screen fed with PTY output.
pub trait Screen: Send {
    /// Feed PTY output; returns the replies to any answerable terminal queries.
    fn process(&mut self, bytes: &[u8]) -> Vec<u8>;
    /// Resize the screen.
    fn resize(&mut self, rows: u16, cols: u16);
    /// `(rows, cols)`.
    fn size(&self) -> (u16, u16);
    /// 0-based `(row, col)` of the cursor.
    fn cursor(&self) -> (u16, u16);
    /// Modes of the application that affect input encoding.
    fn encode_modes(&self) -> EncodeModes;
    /// Bytes that make a fresh screen reproduce this one, including modes.
    ///
    /// Takes `&mut self` because the emulator's scrollback view is temporarily
    /// reset; the view offset is restored before returning.
    fn snapshot(&mut self) -> Vec<u8>;
    /// Number of lines currently held in scrollback.
    fn scrollback_len(&mut self) -> usize;
    /// Up to `count` formatted scrollback rows starting at `start`, oldest first.
    fn scrollback_rows(&mut self, start: usize, count: usize) -> Vec<Vec<u8>>;
    /// Scroll the view `n` lines back into history (`0` is the live screen).
    fn set_view_offset(&mut self, n: usize);
    /// Window title set by the application.
    fn title(&self) -> String;
    /// Draw the (possibly scrolled) screen into `buf`.
    fn render(&self, area: Rect, buf: &mut Buffer, show_cursor: bool);
}
