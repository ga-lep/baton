//! TUI state and its reducer: events and daemon frames in, effects out.
//!
//! Nothing here touches the terminal or the socket, so every behavior is
//! unit-testable.

use std::collections::HashMap;
use std::time::Instant;

use baton_proto::{ClientMsg, DaemonMsg, MAX_INPUT, SessionId, SessionInfo};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};
use ratatui::layout::Rect;

use super::mirror::Mirror;
use super::render_pacer::RenderPacer;
use crate::spike::{KeyAction, Layout, Mode, classify_key, contains, layout};
use crate::term::encode::{encode_focus, encode_key, encode_mouse, encode_paste};
use crate::term::screen::Screen;

/// A modal that takes over the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// No modal.
    None,
    /// The daemon speaks another protocol version.
    VersionMismatch { daemon: u32 },
    /// The daemon connection is gone.
    Disconnected,
}

/// Something the event loop must do on behalf of the reducer.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Send a frame to the daemon.
    Send(ClientMsg),
    /// Detach and exit.
    Quit,
    /// Connect (starting the daemon if needed) and attach again.
    Reconnect,
    /// Stop the running daemon, start a new one and attach.
    RestartDaemon,
}

/// All TUI state.
pub struct App {
    /// Sessions as last listed by the daemon.
    pub sessions: Vec<SessionInfo>,
    mirrors: HashMap<SessionId, Mirror>,
    /// Index into `sessions`.
    pub selected: usize,
    /// Input mode.
    pub mode: Mode,
    /// Active modal.
    pub overlay: Overlay,
    /// Current screen layout.
    pub layout: Layout,
    /// Render scheduling.
    pub pacer: RenderPacer,
    /// Last error reported by the daemon.
    pub notice: Option<String>,
    size: (u16, u16),
}

fn inner_size(l: &Layout) -> (u16, u16) {
    (l.inner.height.max(1), l.inner.width.max(1))
}

/// `Input` frames for `bytes`, split to the daemon's per-frame limit.
fn input(session: &SessionId, bytes: &[u8]) -> Vec<Effect> {
    bytes
        .chunks(MAX_INPUT)
        .map(|c| {
            Effect::Send(ClientMsg::Input {
                session: session.clone(),
                bytes: c.to_vec(),
            })
        })
        .collect()
}

impl App {
    /// A new app laid out for `host`.
    pub fn new(host: Rect) -> Self {
        let layout = layout(host);
        let mut pacer = RenderPacer::new();
        pacer.mark_dirty();
        Self {
            sessions: Vec::new(),
            mirrors: HashMap::new(),
            selected: 0,
            mode: Mode::Normal,
            overlay: Overlay::None,
            size: inner_size(&layout),
            layout,
            pacer,
            notice: None,
        }
    }

    /// `(rows, cols)` of the main panel's inner area.
    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    /// The selected session, if any.
    pub fn selected_session(&self) -> Option<&SessionInfo> {
        self.sessions.get(self.selected)
    }

    /// The mirror of the selected session, if one exists.
    pub fn selected_mirror(&self) -> Option<&Mirror> {
        self.mirrors.get(&self.selected_session()?.id)
    }

    /// A fresh attach is about to stream everything again.
    pub fn on_connected(&mut self) {
        self.sessions.clear();
        self.mirrors.clear();
        self.selected = 0;
        self.mode = Mode::Normal;
        self.overlay = Overlay::None;
        self.notice = None;
        self.pacer.mark_dirty();
    }

    /// The daemon connection is gone.
    pub fn on_disconnected(&mut self) {
        self.mode = Mode::Normal;
        self.overlay = Overlay::Disconnected;
        self.pacer.mark_dirty();
    }

    /// The daemon speaks protocol version `daemon`.
    pub fn on_mismatch(&mut self, daemon: u32) {
        self.mode = Mode::Normal;
        self.overlay = Overlay::VersionMismatch { daemon };
        self.pacer.mark_dirty();
    }

    /// Applies a daemon frame. Mirrors never answer terminal queries, so
    /// `Output` never yields an `Input`.
    pub fn on_daemon(&mut self, msg: DaemonMsg, now: Instant) -> Vec<Effect> {
        match msg {
            DaemonMsg::SessionList(list) => self.set_sessions(list),
            DaemonMsg::Snapshot {
                session,
                rows,
                cols,
                bytes,
            } => {
                self.mirrors
                    .insert(session.clone(), Mirror::from_snapshot(rows, cols, &bytes));
                self.touched(&session, now);
            }
            DaemonMsg::Output { session, bytes } => {
                if let Some(m) = self.mirrors.get_mut(&session) {
                    m.feed(&bytes);
                }
                self.touched(&session, now);
            }
            DaemonMsg::StatusChanged { session, status } => {
                if let Some(s) = self.sessions.iter_mut().find(|s| s.id == session) {
                    s.status = status;
                    if let baton_proto::Status::Exited(code) = status {
                        s.exit_code = Some(code);
                    }
                }
                self.pacer.mark_dirty();
            }
            DaemonMsg::Error { message } => {
                self.notice = Some(message);
                self.pacer.mark_dirty();
            }
            _ => {}
        }
        Vec::new()
    }

    fn set_sessions(&mut self, list: Vec<SessionInfo>) {
        let keep = self.selected_session().map(|s| s.id.clone());
        self.mirrors
            .retain(|id, _| list.iter().any(|s| &s.id == id));
        self.sessions = list;
        self.selected = keep
            .and_then(|id| self.sessions.iter().position(|s| s.id == id))
            .unwrap_or(0)
            .min(self.sessions.len().saturating_sub(1));
        self.pacer.mark_dirty();
    }

    /// A mirror changed: redraw if it is the visible one.
    fn touched(&mut self, session: &SessionId, now: Instant) {
        if self.selected_session().is_some_and(|s| &s.id == session) {
            let sync = self.mirrors.get(session).is_some_and(Mirror::sync_output);
            self.pacer.set_sync(sync, now);
            self.pacer.mark_dirty();
        }
    }

    /// Applies a terminal event for a host terminal of size `host`.
    pub fn on_event(&mut self, ev: Event, host: Rect) -> Vec<Effect> {
        match ev {
            Event::Key(key) => self.on_key(&key),
            Event::Resize(..) => self.fit(host),
            // Everything else only reaches the application in focus mode.
            _ if self.mode != Mode::Focus || self.overlay != Overlay::None => Vec::new(),
            Event::Paste(text) => self.to_session(|app| encode_paste(&text, &app.selected_modes())),
            Event::FocusGained | Event::FocusLost => {
                let gained = ev == Event::FocusGained;
                let modes = self.selected_modes();
                self.to_session(|_| encode_focus(gained, &modes).unwrap_or_default())
            }
            Event::Mouse(m) => self.on_mouse(&m),
        }
    }

    fn selected_modes(&self) -> crate::term::encode::EncodeModes {
        self.selected_mirror()
            .map(|m| m.screen().encode_modes())
            .unwrap_or_default()
    }

    /// `Input` frames for the selected session with bytes made by `f`.
    fn to_session(&self, f: impl FnOnce(&Self) -> Vec<u8>) -> Vec<Effect> {
        match self.selected_session() {
            Some(s) => input(&s.id, &f(self)),
            None => Vec::new(),
        }
    }

    fn on_mouse(&self, m: &MouseEvent) -> Vec<Effect> {
        if !contains(self.layout.inner, m.column, m.row) {
            return Vec::new();
        }
        let origin = (self.layout.inner.x, self.layout.inner.y);
        let modes = self.selected_modes();
        self.to_session(|_| encode_mouse(m, origin, &modes).unwrap_or_default())
    }

    fn on_key(&mut self, key: &KeyEvent) -> Vec<Effect> {
        if key.kind == KeyEventKind::Release {
            return Vec::new();
        }
        self.pacer.mark_dirty();
        match self.overlay {
            Overlay::VersionMismatch { .. } => {
                return vec![match key.code {
                    KeyCode::Char('y' | 'Y') => Effect::RestartDaemon,
                    _ => Effect::Quit,
                }];
            }
            Overlay::Disconnected => {
                return match key.code {
                    KeyCode::Char('r') => vec![Effect::Reconnect],
                    KeyCode::Char('q') => vec![Effect::Quit],
                    _ => Vec::new(),
                };
            }
            Overlay::None => {}
        }
        if self.mode == Mode::Normal && key.modifiers == KeyModifiers::NONE {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => return self.select(1),
                KeyCode::Char('k') | KeyCode::Up => return self.select(-1),
                _ => {}
            }
        }
        match classify_key(self.mode, key) {
            KeyAction::ToNormal => {
                self.mode = Mode::Normal;
                Vec::new()
            }
            KeyAction::ToFocus => {
                if self.selected_session().is_some() {
                    self.mode = Mode::Focus;
                }
                Vec::new()
            }
            KeyAction::Quit => vec![Effect::Quit],
            KeyAction::Forward => {
                let modes = self.selected_modes();
                self.to_session(|_| encode_key(key, &modes).unwrap_or_default())
            }
            KeyAction::Ignore => Vec::new(),
        }
    }

    fn select(&mut self, delta: isize) -> Vec<Effect> {
        let last = self.sessions.len().saturating_sub(1);
        self.selected = self.selected.saturating_add_signed(delta).min(last);
        Vec::new()
    }

    /// Re-lays out for `host`; tells the daemon if the panel size changed.
    pub fn fit(&mut self, host: Rect) -> Vec<Effect> {
        self.layout = layout(host);
        self.pacer.mark_dirty();
        let size = inner_size(&self.layout);
        if size == self.size {
            return Vec::new();
        }
        self.size = size;
        for m in self.mirrors.values_mut() {
            m.resize(size.0, size.1);
        }
        vec![Effect::Send(ClientMsg::Resize {
            rows: size.0,
            cols: size.1,
        })]
    }
}

#[cfg(test)]
mod tests;
