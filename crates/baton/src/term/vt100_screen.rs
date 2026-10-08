//! [`Screen`] implemented on top of `vt100`.

use super::encode::EncodeModes;
use super::screen::Screen;
use baton_core::term::{MouseMode, Scanner};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;
use tui_term::widget::{Cursor, PseudoTerminal};

/// Captures the window title, which `vt100::Screen` does not store.
#[derive(Default)]
struct TitleCallbacks {
    title: String,
}

impl vt100::Callbacks for TitleCallbacks {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = String::from_utf8_lossy(title).into_owned();
    }
}

/// `vt100`-backed screen.
pub struct Vt100Screen {
    parser: vt100::Parser<TitleCallbacks>,
    scanner: Scanner,
}

impl Vt100Screen {
    /// Create a screen of `rows` x `cols` keeping at most `scrollback_cap` history lines.
    pub fn new(rows: u16, cols: u16, scrollback_cap: usize) -> Self {
        Self {
            parser: vt100::Parser::new_with_callbacks(
                rows,
                cols,
                scrollback_cap,
                TitleCallbacks::default(),
            ),
            scanner: Scanner::new(),
        }
    }

    /// Plain-text contents of the visible screen.
    pub fn contents(&self) -> String {
        self.parser.screen().contents()
    }

    /// Whether the alternate screen is active.
    pub fn alt_screen(&self) -> bool {
        self.parser.screen().alternate_screen()
    }
}

impl Screen for Vt100Screen {
    fn process(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.parser.process(bytes);
        // The cursor is sampled once per chunk, after it was applied; a CPR
        // query in the middle of a chunk therefore sees the end-of-chunk cursor.
        let (row, col) = self.parser.screen().cursor_position();
        self.scanner
            .feed(bytes, (row.saturating_add(1), col.saturating_add(1)))
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
    }

    fn size(&self) -> (u16, u16) {
        self.parser.screen().size()
    }

    fn cursor(&self) -> (u16, u16) {
        self.parser.screen().cursor_position()
    }

    fn encode_modes(&self) -> EncodeModes {
        let screen = self.parser.screen();
        EncodeModes {
            app_cursor: screen.application_cursor(),
            app_keypad: screen.application_keypad(),
            bracketed_paste: screen.bracketed_paste(),
            term: self.scanner.modes().clone(),
        }
    }

    fn snapshot(&mut self) -> Vec<u8> {
        let offset = self.parser.screen().scrollback();
        self.parser.screen_mut().set_scrollback(0);
        let screen = self.parser.screen();
        let modes = self.scanner.modes();
        let mut out = Vec::new();
        // Enter the alternate screen first so the contents land in it.
        if screen.alternate_screen() {
            out.extend_from_slice(b"\x1b[?1049h");
        }
        out.extend(screen.state_formatted());
        // vt100 already restores mouse reporting; the scanner needs to see it
        // too, and also tracks modes vt100 does not know about.
        match modes.mouse {
            MouseMode::None => {}
            MouseMode::Press => out.extend_from_slice(b"\x1b[?1000h"),
            MouseMode::ButtonMotion => out.extend_from_slice(b"\x1b[?1002h"),
            MouseMode::AnyMotion => out.extend_from_slice(b"\x1b[?1003h"),
        }
        if modes.sgr_mouse {
            out.extend_from_slice(b"\x1b[?1006h");
        }
        if modes.focus_reporting {
            out.extend_from_slice(b"\x1b[?1004h");
        }
        if modes.modify_other_keys > 0 {
            out.extend(format!("\x1b[>4;{}m", modes.modify_other_keys).into_bytes());
        }
        if modes.kitty_flags > 0 {
            out.extend(format!("\x1b[>{}u", modes.kitty_flags).into_bytes());
        }
        // Synchronized output is deliberately not re-emitted: it would make a
        // fresh mirror hold back rendering until a matching reset.
        self.parser.screen_mut().set_scrollback(offset);
        out
    }

    fn scrollback_len(&mut self) -> usize {
        let screen = self.parser.screen_mut();
        let offset = screen.scrollback();
        // vt100 clamps the offset to the number of history lines.
        screen.set_scrollback(usize::MAX);
        let len = screen.scrollback();
        screen.set_scrollback(offset);
        len
    }

    fn scrollback_rows(&mut self, start: usize, count: usize) -> Vec<Vec<u8>> {
        let len = self.scrollback_len();
        let end = len.min(start.saturating_add(count));
        let saved = self.parser.screen().scrollback();
        let (rows, cols) = self.parser.screen().size();
        let mut out = Vec::with_capacity(end.saturating_sub(start));
        let mut pos = start;
        while pos < end {
            // With offset `len - pos` the view's first row is history line `pos`.
            self.parser.screen_mut().set_scrollback(len - pos);
            let take = (end - pos).min(usize::from(rows));
            out.extend(self.parser.screen().rows_formatted(0, cols).take(take));
            pos += take;
        }
        self.parser.screen_mut().set_scrollback(saved);
        out
    }

    fn set_view_offset(&mut self, n: usize) {
        self.parser.screen_mut().set_scrollback(n);
    }

    fn title(&self) -> String {
        self.parser.callbacks().title.clone()
    }

    fn render(&self, area: Rect, buf: &mut Buffer, show_cursor: bool) {
        let cursor = Cursor::default().visibility(show_cursor);
        PseudoTerminal::new(self.parser.screen())
            .cursor(cursor)
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    fn assert_same(a: &mut Vt100Screen, b: &mut Vt100Screen) {
        assert_eq!(a.contents(), b.contents());
        assert_eq!(a.cursor(), b.cursor());
        assert_eq!(a.encode_modes(), b.encode_modes());
        assert_eq!(a.alt_screen(), b.alt_screen());
    }

    fn roundtrip(stream: &[u8]) {
        let mut a = Vt100Screen::new(10, 40, 100);
        a.process(stream);
        let snap = a.snapshot();
        let mut b = Vt100Screen::new(10, 40, 100);
        b.process(&snap);
        assert_same(&mut a, &mut b);
    }

    #[test]
    fn snapshot_colored() {
        roundtrip(b"\x1b[31mred\x1b[0m plain\r\n\x1b[1;44mbold on blue\x1b[0m\r\nx");
    }

    #[test]
    fn snapshot_alt_screen() {
        roundtrip(b"main\r\n\x1b[?1049h\x1b[2J\x1b[3;4Halt text\x1b[32m!");
    }

    #[test]
    fn snapshot_mouse_and_modes() {
        let mut a = Vt100Screen::new(10, 40, 100);
        a.process(b"\x1b[?1004h\x1b[?2004h\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[?1h\x1b=hi");
        let m = a.encode_modes();
        assert!(m.term.focus_reporting && m.term.sgr_mouse && m.bracketed_paste);
        assert!(m.app_cursor && m.app_keypad);
        roundtrip(b"\x1b[?1004h\x1b[?2004h\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[?1h\x1b=hi");
    }

    #[test]
    fn snapshot_modify_other_keys_and_kitty() {
        let mut a = Vt100Screen::new(10, 40, 100);
        a.process(b"\x1b[>4;2m\x1b[>5u\x1b[?1049h\x1b[?1003h");
        assert_eq!(a.encode_modes().term.modify_other_keys, 2);
        roundtrip(b"\x1b[>4;2m\x1b[>5u\x1b[?1049h\x1b[?1003h\x1b[1;1Hq");
    }

    #[test]
    fn scrollback_trimmed_and_ordered() {
        let mut s = Vt100Screen::new(3, 10, 5);
        for i in 0..20 {
            s.process(format!("line{i:02}\r\n").as_bytes());
        }
        assert_eq!(s.scrollback_len(), 5);
        let rows = s.scrollback_rows(0, 10);
        assert_eq!(rows.len(), 5);
        let text: Vec<String> = rows
            .iter()
            .map(|r| String::from_utf8_lossy(r).into_owned())
            .collect();
        for (i, t) in text.iter().enumerate() {
            assert!(t.contains(&format!("line{:02}", 13 + i)), "{i}: {t:?}");
        }
        let mid = s.scrollback_rows(2, 2);
        assert_eq!(mid.len(), 2);
        assert!(String::from_utf8_lossy(&mid[0]).contains("line15"));
        s.set_view_offset(2);
        let _ = s.scrollback_rows(0, 5);
        let _ = s.snapshot();
        s.set_view_offset(0);
    }

    #[test]
    fn dsr_uses_screen_cursor() {
        let mut s = Vt100Screen::new(10, 40, 0);
        assert_eq!(s.process(b"ab\r\ncd\x1b[6n"), b"\x1b[2;3R");
        assert_eq!(s.process(b"\x1b[c"), b"\x1b[?62;22c");
    }

    #[test]
    fn resize_size_title() {
        let mut s = Vt100Screen::new(10, 40, 0);
        s.resize(5, 20);
        assert_eq!(s.size(), (5, 20));
        s.process(b"\x1b]0;hello\x07");
        assert_eq!(s.title(), "hello");
    }

    #[test]
    fn render_red_line() {
        let mut s = Vt100Screen::new(3, 10, 0);
        s.process(b"\x1b[31mhello\x1b[0m");
        let mut term = Terminal::new(TestBackend::new(10, 3)).unwrap();
        term.draw(|f| s.render(f.area(), f.buffer_mut(), false))
            .unwrap();
        let buf = term.backend().buffer();
        let line: String = (0..5).map(|x| buf[(x, 0)].symbol()).collect();
        assert_eq!(line, "hello");
        assert_eq!(buf[(0, 0)].fg, Color::Indexed(1));
    }
}
