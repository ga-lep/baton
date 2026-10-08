//! Scrollback view: history fetched from the daemon, shown through a local
//! emulator so the visible rows keep their colors and attributes.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::mirror::Mirror;
use crate::term::screen::Screen;
use crate::term::vt100_screen::Vt100Screen;

/// Rows requested per `GetScrollback` page (the daemon's per-request cap).
pub const PAGE: u32 = 10_000;

/// A history fetch in progress.
#[derive(Debug)]
pub struct Loading {
    /// History rows received so far, oldest first.
    pub rows: Vec<Vec<u8>>,
    /// Lines to scroll back once the history is complete.
    pub want: usize,
}

/// Per-session scrollback state; absent means the live view.
pub enum Scroll {
    /// Waiting for history pages.
    Loading(Loading),
    /// Showing history.
    View(Box<View>),
}

/// A frozen copy of history plus the live screen, scrolled back `offset` lines.
pub struct View {
    screen: Vt100Screen,
    history: usize,
    offset: usize,
}

impl View {
    /// Builds a view from `history` (oldest first) followed by `live`'s
    /// current screen, scrolled back `offset` lines (clamped to the history).
    pub fn build(history: &[Vec<u8>], live: &mut Mirror, offset: usize) -> Self {
        let (rows, cols) = live.screen().size();
        let mut screen = Vt100Screen::new(rows, cols, history.len() + usize::from(rows) + 1);
        for row in history {
            drop(screen.process(row));
            drop(screen.process(b"\r\n"));
        }
        // Push the last history row off the visible area into scrollback.
        for _ in 1..rows {
            drop(screen.process(b"\r\n"));
        }
        drop(screen.process(b"\x1b[H\x1b[J"));
        drop(screen.process(&live.snapshot()));
        let mut view = Self {
            screen,
            history: history.len(),
            offset: 0,
        };
        view.scroll(isize::try_from(offset).unwrap_or(isize::MAX));
        view
    }

    /// Scrolls `lines` towards history (negative: towards live); returns the
    /// new offset, clamped to `0..=history`.
    pub fn scroll(&mut self, lines: isize) -> usize {
        self.offset = self.offset.saturating_add_signed(lines).min(self.history);
        self.screen.set_view_offset(self.offset);
        self.offset
    }

    /// Lines scrolled back from live.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Draws the scrolled screen.
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        self.screen.render(area, buf, false);
    }

    /// Plain text of the visible rows.
    #[cfg(test)]
    pub fn contents(&self) -> String {
        self.screen.contents()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(n: usize) -> Vec<Vec<u8>> {
        (0..n).map(|i| format!("h{i}").into_bytes()).collect()
    }

    #[test]
    fn offset_zero_shows_the_live_screen() {
        let mut live = Mirror::from_snapshot(5, 20, b"live-1\r\nlive-2");
        let v = View::build(&lines(50), &mut live, 0);
        assert_eq!(v.contents(), "live-1\nlive-2");
    }

    #[test]
    fn scrolling_back_reveals_history_oldest_first() {
        let mut live = Mirror::from_snapshot(5, 20, b"live");
        let mut v = View::build(&lines(50), &mut live, 5);
        assert_eq!(v.contents(), "h45\nh46\nh47\nh48\nh49");
        assert_eq!(v.scroll(2), 7);
        assert_eq!(v.contents(), "h43\nh44\nh45\nh46\nh47");
        assert_eq!(v.scroll(-100), 0);
        assert_eq!(v.contents(), "live");
        assert_eq!(v.scroll(1000), 50);
        assert_eq!(v.contents(), "h0\nh1\nh2\nh3\nh4");
    }
}
