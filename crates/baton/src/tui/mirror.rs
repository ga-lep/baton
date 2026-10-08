//! A local copy of one session's screen, fed by `Snapshot` and `Output`.

use crate::term::screen::Screen;
use crate::term::vt100_screen::Vt100Screen;

/// A session's screen as the TUI sees it.
///
/// The daemon owns the authoritative screen and answers terminal queries, so
/// a mirror never produces anything for the PTY: replies are dropped here.
pub struct Mirror {
    screen: Vt100Screen,
}

impl Mirror {
    /// Builds a mirror from a daemon `Snapshot`.
    pub fn from_snapshot(rows: u16, cols: u16, bytes: &[u8]) -> Self {
        let mut mirror = Self {
            screen: Vt100Screen::new(rows, cols, 0),
        };
        mirror.feed(bytes);
        mirror
    }

    /// Feeds PTY output. Query replies are intentionally discarded.
    pub fn feed(&mut self, bytes: &[u8]) {
        drop(self.screen.process(bytes));
    }

    /// Resizes the mirror to follow the daemon's screen.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.screen.resize(rows, cols);
    }

    /// The emulated screen.
    pub fn screen(&self) -> &Vt100Screen {
        &self.screen
    }

    /// Bytes that reproduce the current screen on a fresh emulator.
    pub fn snapshot(&mut self) -> Vec<u8> {
        self.screen.snapshot()
    }

    /// Whether the application holds a synchronized-output frame open.
    pub fn sync_output(&self) -> bool {
        self.screen.encode_modes().term.sync_output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_and_output_reach_the_screen() {
        let mut m = Mirror::from_snapshot(5, 20, b"hello");
        m.feed(b"\r\nworld");
        assert_eq!(m.screen().contents(), "hello\nworld");
    }

    #[test]
    fn sync_output_follows_the_stream() {
        let mut m = Mirror::from_snapshot(5, 20, b"");
        assert!(!m.sync_output());
        m.feed(b"\x1b[?2026h");
        assert!(m.sync_output());
        m.feed(b"\x1b[?2026l");
        assert!(!m.sync_output());
    }
}
