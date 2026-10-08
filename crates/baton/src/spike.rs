//! M0 embedding spike: one PTY child inside a ratatui panel, in-process.
//!
//! This is the go/no-go experiment for `vt100`; see `docs/m0-spike.md`.

use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::term::encode::{encode_focus, encode_key, encode_mouse, encode_paste};
use crate::term::screen::Screen;
use crate::term::vt100_screen::Vt100Screen;
use crate::tui::render_pacer::RenderPacer;
use crate::tui::terminal_guard::TerminalGuard;

/// Width of the sidebar placeholder.
const SIDEBAR_WIDTH: u16 = 32;
/// Local scrollback kept by the spike screen.
const SCROLLBACK: usize = 5000;
/// Lines scrolled per wheel notch when the app has no mouse mode.
const WHEEL_LINES: usize = 3;
/// Command run when none is given.
const DEFAULT_COMMAND: &str = "claude";

/// Input mode of the spike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Keys go to the child.
    Focus,
    /// Keys control the spike itself.
    Normal,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Self::Focus => "FOCUS",
            Self::Normal => "NORMAL",
        }
    }
}

/// What a key press means to the spike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// Leave focus mode (the key is not forwarded).
    ToNormal,
    /// Return to focus mode.
    ToFocus,
    /// Quit the spike.
    Quit,
    /// Send the key to the child.
    Forward,
    /// Do nothing.
    Ignore,
}

/// Whether `ev` is Ctrl-\, as crossterm reports it with or without keyboard
/// enhancement (legacy terminals send 0x1c, which crossterm maps to Ctrl-4).
fn is_ctrl_backslash(ev: &KeyEvent) -> bool {
    ev.modifiers.contains(KeyModifiers::CONTROL) && matches!(ev.code, KeyCode::Char('\\' | '4'))
}

/// Classify a key press for the given mode.
pub fn classify_key(mode: Mode, ev: &KeyEvent) -> KeyAction {
    if ev.kind == KeyEventKind::Release {
        return KeyAction::Ignore;
    }
    match mode {
        Mode::Focus if is_ctrl_backslash(ev) => KeyAction::ToNormal,
        Mode::Focus => KeyAction::Forward,
        Mode::Normal => match ev.code {
            KeyCode::Enter => KeyAction::ToFocus,
            KeyCode::Char('q') if ev.modifiers.is_empty() => KeyAction::Quit,
            _ => KeyAction::Ignore,
        },
    }
}

/// Screen regions of the spike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// Sidebar placeholder.
    pub sidebar: Rect,
    /// Bordered main panel.
    pub main: Rect,
    /// Inside the main panel's border: where the child is drawn.
    pub inner: Rect,
    /// Bottom bar.
    pub bar: Rect,
}

/// Split `area` into sidebar, main panel and a one-row bottom bar.
pub fn layout(area: Rect) -> Layout {
    let bar_h = 1.min(area.height);
    let body_h = area.height - bar_h;
    let side_w = SIDEBAR_WIDTH.min(area.width);
    let sidebar = Rect::new(area.x, area.y, side_w, body_h);
    let main = Rect::new(area.x + side_w, area.y, area.width - side_w, body_h);
    let bar = Rect::new(area.x, area.y + body_h, area.width, bar_h);
    let inner = Block::default().borders(Borders::ALL).inner(main);
    Layout {
        sidebar,
        main,
        inner,
        bar,
    }
}

pub(crate) fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// Messages from the I/O threads to the main loop.
enum Msg {
    Output(Vec<u8>),
    Eof,
    Input(Event),
}

struct Spike {
    screen: Vt100Screen,
    pacer: RenderPacer,
    mode: Mode,
    view: usize,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    pty_size: (u16, u16),
    layout: Layout,
}

impl Spike {
    fn write_pty(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Recompute the layout for `host` and resize the PTY and screen to match.
    fn fit(&mut self, host: Rect) -> Result<()> {
        self.layout = layout(host);
        let rows = self.layout.inner.height.max(1);
        let cols = self.layout.inner.width.max(1);
        if (rows, cols) != self.pty_size {
            self.master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })?;
            self.screen.resize(rows, cols);
            self.pty_size = (rows, cols);
            self.pacer.mark_dirty();
        }
        Ok(())
    }

    fn on_output(&mut self, bytes: &[u8], now: Instant) -> Result<()> {
        let replies = self.screen.process(bytes);
        self.write_pty(&replies)?;
        let sync = self.screen.encode_modes().term.sync_output;
        self.pacer.set_sync(sync, now);
        self.pacer.mark_dirty();
        Ok(())
    }

    /// Returns `true` when the spike should quit.
    fn on_event(&mut self, ev: Event, host: Rect) -> Result<bool> {
        match ev {
            Event::Key(key) => return self.on_key(&key),
            Event::Paste(text) if self.mode == Mode::Focus => {
                let bytes = encode_paste(&text, &self.screen.encode_modes());
                self.write_pty(&bytes)?;
            }
            Event::FocusGained | Event::FocusLost => {
                let bytes = encode_focus(ev == Event::FocusGained, &self.screen.encode_modes());
                if let Some(bytes) = bytes {
                    self.write_pty(&bytes)?;
                }
            }
            Event::Mouse(m) => self.on_mouse(&m)?,
            Event::Resize(..) => self.fit(host)?,
            _ => {}
        }
        Ok(false)
    }

    fn on_key(&mut self, key: &KeyEvent) -> Result<bool> {
        match classify_key(self.mode, key) {
            KeyAction::ToNormal => self.mode = Mode::Normal,
            KeyAction::ToFocus => self.mode = Mode::Focus,
            KeyAction::Quit => return Ok(true),
            KeyAction::Forward => {
                if self.view != 0 {
                    self.view = 0;
                    self.screen.set_view_offset(0);
                }
                if let Some(bytes) = encode_key(key, &self.screen.encode_modes()) {
                    self.write_pty(&bytes)?;
                }
            }
            KeyAction::Ignore => {}
        }
        self.pacer.mark_dirty();
        Ok(false)
    }

    fn on_mouse(&mut self, m: &MouseEvent) -> Result<()> {
        use crossterm::event::MouseEventKind::{ScrollDown, ScrollUp};
        if !contains(self.layout.inner, m.column, m.row) {
            return Ok(());
        }
        let modes = self.screen.encode_modes();
        let origin = (self.layout.inner.x, self.layout.inner.y);
        if let Some(bytes) = encode_mouse(m, origin, &modes) {
            self.write_pty(&bytes)?;
            return Ok(());
        }
        if modes.term.mouse != baton_core::term::MouseMode::None {
            return Ok(());
        }
        let max = self.screen.scrollback_len();
        match m.kind {
            ScrollUp => self.view = (self.view + WHEEL_LINES).min(max),
            ScrollDown => self.view = self.view.saturating_sub(WHEEL_LINES),
            _ => return Ok(()),
        }
        self.screen.set_view_offset(self.view);
        self.pacer.mark_dirty();
        Ok(())
    }

    fn draw(&self, terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
        let l = self.layout;
        let (rows, cols) = self.pty_size;
        let focus = self.mode == Mode::Focus;
        let hint = if focus {
            "Ctrl-\\: normal mode"
        } else {
            "Enter: focus, q: quit"
        };
        let bar = format!(" {} | {rows}x{cols} | {hint}", self.mode.label());
        terminal.draw(|f| {
            f.render_widget(
                Block::default().borders(Borders::ALL).title("Baton"),
                l.sidebar,
            );
            f.render_widget(Block::default().borders(Borders::ALL), l.main);
            self.screen.render(l.inner, f.buffer_mut(), focus);
            let style = Style::default().add_modifier(Modifier::REVERSED);
            f.render_widget(Paragraph::new(bar).style(style), l.bar);
        })?;
        Ok(())
    }
}

fn spawn_threads(mut reader: Box<dyn Read + Send>) -> Receiver<Msg> {
    let (tx, rx) = mpsc::sync_channel(64);
    let out_tx = tx.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        // EOF or EIO both mean the child is gone.
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || out_tx.send(Msg::Output(buf[..n].to_vec())).is_err() {
                break;
            }
        }
        let _ = out_tx.send(Msg::Eof);
    });
    std::thread::spawn(move || {
        while let Ok(ev) = event::read() {
            if tx.send(Msg::Input(ev)).is_err() {
                break;
            }
        }
    });
    rx
}

/// Run the spike with `cmd` (an argument vector; `claude` when empty).
///
/// # Errors
/// Fails if the terminal, PTY or child cannot be set up, or on I/O errors.
pub fn run(cmd: &[String]) -> Result<()> {
    let default = [DEFAULT_COMMAND.to_owned()];
    let argv = if cmd.is_empty() { &default[..] } else { cmd };
    let (program, args) = argv.split_first().context("empty command")?;

    let guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let host = Rect::from((Default::default(), terminal.size()?));
    let l = layout(host);
    let (rows, cols) = (l.inner.height.max(1), l.inner.width.max(1));

    let pair = native_pty_system().openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut builder = CommandBuilder::new(program);
    builder.args(args);
    // portable-pty would default to $HOME.
    builder.cwd(std::env::current_dir().context("current dir")?);
    builder.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(builder)?;
    drop(pair.slave);
    let rx = spawn_threads(pair.master.try_clone_reader()?);
    let mut spike = Spike {
        screen: Vt100Screen::new(rows, cols, SCROLLBACK),
        pacer: RenderPacer::new(),
        mode: Mode::Focus,
        view: 0,
        writer: pair.master.take_writer()?,
        master: pair.master,
        pty_size: (rows, cols),
        layout: l,
    };
    spike.pacer.mark_dirty();

    let result = event_loop(&mut spike, &rx, &mut terminal);
    let _ = child.kill();
    let _ = child.wait();
    drop(terminal);
    drop(guard);
    result
}

fn event_loop(
    spike: &mut Spike,
    rx: &Receiver<Msg>,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> Result<()> {
    loop {
        let now = Instant::now();
        let msg = match spike.pacer.next_deadline(now) {
            Some(wait) => rx.recv_timeout(wait),
            None => rx.recv_timeout(Duration::from_secs(3600)),
        };
        match msg {
            Ok(Msg::Output(bytes)) => spike.on_output(&bytes, Instant::now())?,
            Ok(Msg::Eof) => return Ok(()),
            Ok(Msg::Input(ev)) => {
                let host = Rect::from((Default::default(), terminal.size()?));
                if spike.on_event(ev, host)? {
                    return Ok(());
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => bail!("event channel closed"),
        }
        let now = Instant::now();
        if spike.pacer.should_render(now) {
            spike.draw(terminal)?;
            spike.pacer.rendered(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn ctrl_backslash_both_encodings_switch_to_normal() {
        for code in ['\\', '4'] {
            let ev = key(KeyCode::Char(code), KeyModifiers::CONTROL);
            assert_eq!(classify_key(Mode::Focus, &ev), KeyAction::ToNormal);
        }
        let plain = key(KeyCode::Char('4'), KeyModifiers::NONE);
        assert_eq!(classify_key(Mode::Focus, &plain), KeyAction::Forward);
    }

    #[test]
    fn normal_mode_keys() {
        let q = key(KeyCode::Char('q'), KeyModifiers::NONE);
        let enter = key(KeyCode::Enter, KeyModifiers::NONE);
        let x = key(KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(classify_key(Mode::Normal, &q), KeyAction::Quit);
        assert_eq!(classify_key(Mode::Normal, &enter), KeyAction::ToFocus);
        assert_eq!(classify_key(Mode::Normal, &x), KeyAction::Ignore);
        assert_eq!(classify_key(Mode::Focus, &q), KeyAction::Forward);
    }

    #[test]
    fn layout_40x120_gives_37x86_inner() {
        let l = layout(Rect::new(0, 0, 120, 40));
        assert_eq!((l.inner.height, l.inner.width), (37, 86));
        assert_eq!(l.sidebar.width, 32);
        assert_eq!(l.bar.height, 1);
    }

    #[test]
    fn layout_survives_tiny_hosts() {
        for (w, h) in [(0, 0), (1, 1), (10, 2), (40, 1)] {
            let _ = layout(Rect::new(0, 0, w, h));
        }
    }
}
