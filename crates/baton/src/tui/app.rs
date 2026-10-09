//! TUI state and its reducer: events and daemon frames in, effects out.
//!
//! Nothing here touches the terminal or the socket, so every behavior is
//! unit-testable.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use baton_core::attention;
use baton_core::keymap::{FocusAction, Keymap, NormalAction};
use baton_proto::{ClientMsg, DaemonMsg, MAX_INPUT, SessionId, SessionInfo, Status};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, MouseEvent};
use ratatui::layout::Rect;

use super::keys;
use super::mirror::Mirror;
use super::render_pacer::RenderPacer;
use super::scrollback::{Loading, PAGE, Scroll, View};
use super::sidebar::{self, Cursor, Row};
use crate::spike::{Layout, Mode, contains, layout};
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
    /// The key binding overview (`?`).
    Help,
    /// The config file could not be loaded (message in [`App::config_error`]).
    ConfigError,
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
    /// Open `path` with the editor command `template`.
    OpenEditor {
        /// The `editor` config value (`{path}` is the placeholder).
        template: String,
        /// The repo directory.
        path: String,
    },
}

/// How long an editor spawn error stays in the bottom bar.
pub const FLASH_FOR: Duration = Duration::from_secs(5);

/// Hint shown when scrolling is refused on the alternate screen.
pub const ALT_SCREEN_HINT: &str = "app manages its own scrolling (mouse wheel)";

/// Info-panel notice left once the config error popup is dismissed.
pub const CONFIG_ERROR_NOTICE: &str = "config file is invalid; run `baton config check`";

/// Hint shown when `n` finds no session to jump to.
pub const NO_ATTENTION_HINT: &str = "no session needs attention";

/// A scroll request from the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scrolling {
    HalfUp,
    HalfDown,
    PageUp,
    PageDown,
    Live,
}

/// All TUI state.
pub struct App {
    /// Sessions as last listed by the daemon.
    pub sessions: Vec<SessionInfo>,
    mirrors: HashMap<SessionId, Mirror>,
    /// Configured project names, in config order.
    projects: Vec<String>,
    /// Sidebar cursor; `None` until there is a row to put it on.
    cursor: Option<Cursor>,
    /// Whether the user moved the cursor (until then it follows the data).
    moved: bool,
    /// The session shown in the main panel.
    current: Option<SessionId>,
    scroll: HashMap<SessionId, Scroll>,
    /// One-line hint for the bottom bar, cleared by the next key.
    pub hint: Option<&'static str>,
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
    /// The session a pending `Restart <repo>? [y/N]` prompt is about.
    confirm: Option<SessionId>,
    /// Why the config file could not be loaded, for the `ConfigError` overlay.
    config_error: Option<String>,
    size: (u16, u16),
    /// Whether the host terminal has focus (assumed until told otherwise).
    terminal_focused: bool,
    /// The last `ClientView` sent, to send only changes.
    sent_view: Option<(Option<SessionId>, bool)>,
    keymap: Keymap,
    /// The `editor` command template.
    editor: String,
    /// A message for the bottom bar and the instant it expires.
    flash: Option<(String, Instant)>,
}

fn is_closed(s: &SessionInfo) -> bool {
    s.status == Status::Closed
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
            projects: Vec::new(),
            cursor: None,
            moved: false,
            current: None,
            scroll: HashMap::new(),
            hint: None,
            mode: Mode::Normal,
            overlay: Overlay::None,
            size: inner_size(&layout),
            layout,
            pacer,
            notice: None,
            confirm: None,
            config_error: None,
            terminal_focused: true,
            sent_view: None,
            keymap: Keymap::default(),
            editor: baton_core::config::Config::default().editor,
            flash: None,
        }
    }

    /// Replaces the key bindings (read from the config on attach).
    pub fn set_keymap(&mut self, keymap: Keymap) {
        self.keymap = keymap;
        self.pacer.mark_dirty();
    }

    /// Sets the `editor` command template.
    pub fn set_editor(&mut self, template: String) {
        self.editor = template;
    }

    /// The active key bindings.
    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// Shows an editor spawn error in the bottom bar for [`FLASH_FOR`].
    pub fn on_editor_error(&mut self, message: String, now: Instant) {
        self.flash = Some((message, now + FLASH_FOR));
        self.pacer.mark_dirty();
    }

    /// The message to show in the bottom bar, if any.
    pub fn flash(&self) -> Option<&str> {
        self.flash.as_ref().map(|(m, _)| m.as_str())
    }

    /// Drops an expired bottom-bar message; `true` if one was dropped.
    pub fn expire(&mut self, now: Instant) -> bool {
        if self.flash.as_ref().is_some_and(|(_, until)| now >= *until) {
            self.flash = None;
            self.pacer.mark_dirty();
            return true;
        }
        false
    }

    /// How long until the loop must wake up (a render or a message expiring).
    pub fn next_deadline(&self, now: Instant) -> Option<Duration> {
        let flash = self
            .flash
            .as_ref()
            .map(|(_, until)| until.saturating_duration_since(now));
        match (self.pacer.next_deadline(now), flash) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// `(rows, cols)` of the main panel's inner area.
    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    /// The session shown in the main panel, if any.
    pub fn selected_session(&self) -> Option<&SessionInfo> {
        let id = self.current.as_ref()?;
        self.sessions.iter().find(|s| &s.id == id)
    }

    /// Configured projects merged with the sessions, as sidebar rows.
    pub fn rows(&self) -> Vec<Row<'_>> {
        sidebar::rows(&self.projects, &self.sessions)
    }

    /// The sidebar cursor.
    #[cfg(test)]
    pub fn cursor(&self) -> Option<&Cursor> {
        self.cursor.as_ref()
    }

    /// Index of the cursor among `rows()`.
    pub fn cursor_index(&self) -> Option<usize> {
        let cursor = self.cursor.as_ref()?;
        self.rows().iter().position(|r| &r.cursor() == cursor)
    }

    /// Lines the selected session is scrolled back (0 is live).
    pub fn scroll_offset(&self) -> usize {
        self.scroll_view().map_or(0, View::offset)
    }

    /// The scrolled view of the selected session, if it is scrolled back.
    pub fn scroll_view(&self) -> Option<&View> {
        match self.scroll.get(&self.selected_session()?.id)? {
            Scroll::View(v) => Some(v),
            Scroll::Loading(_) => None,
        }
    }

    /// Sets the configured project names (read from the config on attach).
    pub fn set_projects(&mut self, projects: Vec<String>) {
        self.projects = projects;
        self.settle();
        self.pacer.mark_dirty();
    }

    /// The mirror of the selected session, if one exists.
    pub fn selected_mirror(&self) -> Option<&Mirror> {
        self.mirrors.get(&self.selected_session()?.id)
    }

    /// A fresh attach is about to stream everything again.
    pub fn on_connected(&mut self) {
        self.sessions.clear();
        self.mirrors.clear();
        self.scroll.clear();
        self.cursor = None;
        self.moved = false;
        self.current = None;
        self.hint = None;
        self.mode = Mode::Normal;
        self.overlay = Overlay::None;
        self.notice = None;
        self.confirm = None;
        self.config_error = None;
        self.sent_view = None;
        self.pacer.mark_dirty();
    }

    /// The question of the pending restart confirmation, if there is one.
    pub fn confirm_prompt(&self) -> Option<String> {
        let id = self.confirm.as_ref()?;
        let s = self.sessions.iter().find(|s| &s.id == id)?;
        Some(format!("Restart {}? [y/N]", sidebar::repo_name(s)))
    }

    /// The config file could not be loaded: pops up `message` until a key is pressed.
    pub fn on_config_error(&mut self, message: String) {
        self.config_error = Some(message);
        self.overlay = Overlay::ConfigError;
        self.notice = Some(CONFIG_ERROR_NOTICE.to_owned());
        self.pacer.mark_dirty();
    }

    /// Why the config file could not be loaded, if it could not.
    pub fn config_error(&self) -> Option<&str> {
        self.config_error.as_deref()
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
        let mut effects = self.apply_daemon(msg, now);
        self.sync_view(&mut effects);
        effects
    }

    /// Appends a `ClientView` if the shown session or terminal focus changed
    /// since the last one sent.
    fn sync_view(&mut self, effects: &mut Vec<Effect>) {
        if matches!(
            self.overlay,
            Overlay::VersionMismatch { .. } | Overlay::Disconnected
        ) {
            return; // no usable daemon connection
        }
        let view = (
            self.selected_session().map(|s| s.id.clone()),
            self.terminal_focused,
        );
        if self.sent_view.as_ref() == Some(&view) {
            return;
        }
        effects.push(Effect::Send(ClientMsg::ClientView {
            on_screen: view.0.clone(),
            terminal_focused: view.1,
        }));
        self.sent_view = Some(view);
    }

    fn apply_daemon(&mut self, msg: DaemonMsg, now: Instant) -> Vec<Effect> {
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
                self.scroll.remove(&session);
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
                    if let Status::Exited(code) = status {
                        s.exit_code = Some(code);
                    }
                }
                self.pacer.mark_dirty();
            }
            DaemonMsg::UsageUpdated { session, usage } => {
                if let Some(s) = self.sessions.iter_mut().find(|s| s.id == session) {
                    s.usage = usage;
                }
                self.pacer.mark_dirty();
            }
            DaemonMsg::QuotaUpdated { session, quota } => {
                if let Some(s) = self.sessions.iter_mut().find(|s| s.id == session) {
                    s.quota = Some(quota);
                }
                self.pacer.mark_dirty();
            }
            DaemonMsg::Scrollback {
                session,
                start,
                rows,
            } => return self.on_history(&session, start, rows),
            DaemonMsg::Error { message } => {
                self.notice = Some(message);
                self.pacer.mark_dirty();
            }
            _ => {}
        }
        Vec::new()
    }

    fn set_sessions(&mut self, list: Vec<SessionInfo>) {
        self.mirrors
            .retain(|id, _| list.iter().any(|s| &s.id == id));
        self.scroll.retain(|id, _| list.iter().any(|s| &s.id == id));
        self.sessions = list;
        self.settle();
        self.pacer.mark_dirty();
    }

    /// Repairs the cursor and the shown session after the data changed.
    fn settle(&mut self) {
        let rows = self.rows();
        let resolved = self
            .cursor
            .as_ref()
            .and_then(|c| rows.iter().find(|r| &r.cursor() == c));
        // Until the user moves, the cursor follows the data: first session.
        let target = match resolved {
            Some(_) if self.moved => resolved,
            _ => rows
                .iter()
                .find(|r| matches!(r, Row::Session { .. }))
                .or(rows.first()),
        };
        let cursor = target.map(Row::cursor);
        let from_cursor = match target {
            Some(Row::Session { info, .. }) => Some(info.id.clone()),
            _ => None,
        };
        let keep = self
            .current
            .clone()
            .filter(|id| self.sessions.iter().any(|s| &s.id == id));
        let first = self.sessions.first().map(|s| s.id.clone());
        self.cursor = cursor;
        self.current = from_cursor.or(keep).or(first);
    }

    /// Moves the cursor to `row`; a session row also becomes the shown session.
    fn go_to(&mut self, cursor: Cursor) {
        if let Cursor::Session(id) = &cursor {
            self.current = Some(id.clone());
        }
        self.cursor = Some(cursor);
        self.moved = true;
    }

    /// The project the cursor is in.
    fn cursor_project(&self) -> Option<String> {
        match self.cursor.as_ref()? {
            Cursor::Project(name) => Some(name.clone()),
            Cursor::Session(id) => self
                .sessions
                .iter()
                .find(|s| &s.id == id)
                .map(|s| s.project.clone()),
        }
    }

    fn open_project(&self, name: String) -> Vec<Effect> {
        vec![Effect::Send(ClientMsg::OpenProject { name })]
    }

    /// Runs a normal-mode action.
    fn run_normal(&mut self, action: NormalAction) -> Vec<Effect> {
        match action {
            NormalAction::MoveDown => self.step(1),
            NormalAction::MoveUp => self.step(-1),
            NormalAction::Activate => self.activate(),
            NormalAction::OpenProject => self
                .cursor_project()
                .map_or_else(Vec::new, |n| self.open_project(n)),
            NormalAction::Select(n) => {
                self.pick(usize::from(n));
                Vec::new()
            }
            NormalAction::NextAttention => {
                self.next_attention();
                Vec::new()
            }
            NormalAction::Restart => self.ask_restart(),
            NormalAction::Editor => self.open_editor(),
            NormalAction::ScrollUp => self.scroll_by(Scrolling::HalfUp),
            NormalAction::ScrollDown => self.scroll_by(Scrolling::HalfDown),
            NormalAction::PageUp => self.scroll_by(Scrolling::PageUp),
            NormalAction::PageDown => self.scroll_by(Scrolling::PageDown),
            NormalAction::Live => self.scroll_by(Scrolling::Live),
            NormalAction::Help => {
                self.overlay = Overlay::Help;
                Vec::new()
            }
            NormalAction::Quit => vec![Effect::Quit],
        }
    }

    /// Runs a focus-mode action.
    fn run_focus(&mut self, action: FocusAction) {
        match action {
            FocusAction::Unfocus => self.mode = Mode::Normal,
            FocusAction::NextAttention => self.next_attention(),
            FocusAction::Session(n) => self.pick(usize::from(n)),
        }
    }

    /// `e`: open the shown session's repo in the editor.
    fn open_editor(&self) -> Vec<Effect> {
        match self.selected_session() {
            Some(s) => vec![Effect::OpenEditor {
                template: self.editor.clone(),
                path: s.repo.clone(),
            }],
            None => Vec::new(),
        }
    }

    /// Keys while the help overlay is up: `?`, its remapped keys and Esc close
    /// it; everything else is swallowed.
    fn on_help_key(&mut self, key: &KeyEvent) {
        let spec = keys::from_event(key);
        let closes = key.code == KeyCode::Esc
            || key.code == KeyCode::Char('?')
            || spec.is_some_and(|s| self.keymap.normal_action(&s) == Some(NormalAction::Help));
        if closes {
            self.overlay = Overlay::None;
        }
    }

    /// `r`: restart the shown session, after asking while it is working or
    /// waiting for a permission.
    fn ask_restart(&mut self) -> Vec<Effect> {
        let Some(s) = self.selected_session() else {
            return Vec::new();
        };
        let (id, status) = (s.id.clone(), s.status);
        if status == Status::Closed {
            // Nothing runs yet: opening the project is what brings it back.
            let project = s.project.clone();
            return self.open_project(project);
        }
        if matches!(status, Status::Running | Status::Permission) {
            self.confirm = Some(id);
            return Vec::new();
        }
        vec![Effect::Send(ClientMsg::Restart { session: id })]
    }

    /// `Enter` / `l`: focus a session, open a closed project, or step into an
    /// open one.
    fn activate(&mut self) -> Vec<Effect> {
        let rows = self.rows();
        let first_of = |name: &str| {
            rows.iter().find_map(|r| match r {
                Row::Session { info, .. }
                    if info.project == name && info.status != Status::Closed =>
                {
                    Some(info.id.clone())
                }
                _ => None,
            })
        };
        match self.cursor.clone() {
            Some(Cursor::Session(_)) if self.selected_session().is_some_and(is_closed) => self
                .cursor_project()
                .map_or_else(Vec::new, |n| self.open_project(n)),
            Some(Cursor::Session(_)) => {
                self.mode = Mode::Focus;
                Vec::new()
            }
            Some(Cursor::Project(name)) => match first_of(&name) {
                Some(id) => {
                    self.go_to(Cursor::Session(id));
                    Vec::new()
                }
                None => self.open_project(name),
            },
            None => Vec::new(),
        }
    }

    /// Jumps to the next session needing attention, or says there is none.
    fn next_attention(&mut self) {
        let sessions: Vec<(SessionId, Status)> = self
            .rows()
            .iter()
            .filter_map(|r| match r {
                Row::Session { info, .. } => Some((info.id.clone(), info.status)),
                Row::Project { .. } => None,
            })
            .collect();
        let order: Vec<Status> = sessions.iter().map(|(_, s)| *s).collect();
        let current = self
            .current
            .as_ref()
            .and_then(|c| sessions.iter().position(|(id, _)| id == c));
        match attention::next_after(&order, current) {
            Some(i) => self.go_to(Cursor::Session(sessions[i].0.clone())),
            None => self.hint = Some(NO_ATTENTION_HINT),
        }
    }

    /// `1`..`9`: session `n` of the cursor's project.
    fn pick(&mut self, n: usize) {
        let Some(project) = self.cursor_project() else {
            return;
        };
        let id = self
            .sessions
            .iter()
            .filter(|s| s.project == project)
            .nth(n - 1)
            .map(|s| s.id.clone());
        if let Some(id) = id {
            self.go_to(Cursor::Session(id));
        }
    }

    fn step(&mut self, delta: isize) -> Vec<Effect> {
        let rows = self.rows();
        let last = rows.len().saturating_sub(1);
        let from = self.cursor_index().unwrap_or(0);
        let to = from.saturating_add_signed(delta).min(last);
        let next = rows.get(to).map(Row::cursor);
        if let Some(c) = next {
            self.go_to(c);
        }
        Vec::new()
    }

    /// Scroll keys for the shown session.
    fn scroll_by(&mut self, how: Scrolling) -> Vec<Effect> {
        let Some(id) = self.selected_session().map(|s| s.id.clone()) else {
            return Vec::new();
        };
        let rows = usize::from(self.size.0);
        let half = (rows / 2).max(1);
        let up = matches!(how, Scrolling::HalfUp | Scrolling::PageUp);
        let amount = match how {
            Scrolling::HalfUp | Scrolling::HalfDown => half,
            _ => rows,
        };
        if how == Scrolling::Live {
            self.scroll.remove(&id);
            return Vec::new();
        }
        match self.scroll.get_mut(&id) {
            Some(Scroll::View(view)) => {
                let delta = isize::try_from(amount).unwrap_or(isize::MAX);
                if view.scroll(if up { delta } else { -delta }) == 0 {
                    self.scroll.remove(&id);
                }
                Vec::new()
            }
            Some(Scroll::Loading(l)) => {
                if up {
                    l.want += amount;
                }
                Vec::new()
            }
            None if up => {
                if self
                    .mirrors
                    .get(&id)
                    .is_some_and(|m| m.screen().alt_screen())
                {
                    self.hint = Some(ALT_SCREEN_HINT);
                    return Vec::new();
                }
                self.scroll.insert(
                    id.clone(),
                    Scroll::Loading(Loading {
                        rows: Vec::new(),
                        want: amount,
                    }),
                );
                vec![Self::history_page(id, 0)]
            }
            None => Vec::new(),
        }
    }

    fn history_page(session: SessionId, start: u32) -> Effect {
        Effect::Send(ClientMsg::GetScrollback {
            session,
            start,
            count: PAGE,
        })
    }

    /// A page of history arrived.
    fn on_history(&mut self, session: &SessionId, start: u32, rows: Vec<Vec<u8>>) -> Vec<Effect> {
        let Some(Scroll::Loading(loading)) = self.scroll.get_mut(session) else {
            return Vec::new();
        };
        if loading.rows.len() != start as usize {
            return Vec::new();
        }
        let full = rows.len() >= PAGE as usize;
        loading.rows.extend(rows);
        if full {
            let next = u32::try_from(loading.rows.len()).unwrap_or(u32::MAX);
            return vec![Self::history_page(session.clone(), next)];
        }
        let Some(Scroll::Loading(done)) = self.scroll.remove(session) else {
            return Vec::new();
        };
        if let Some(mirror) = self.mirrors.get_mut(session)
            && !done.rows.is_empty()
        {
            let view = View::build(&done.rows, mirror, done.want);
            self.scroll
                .insert(session.clone(), Scroll::View(Box::new(view)));
        }
        self.pacer.mark_dirty();
        Vec::new()
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
            Event::FocusGained => self.terminal_focused = true,
            Event::FocusLost => self.terminal_focused = false,
            _ => {}
        }
        let mut effects = self.apply_event(ev, host);
        self.sync_view(&mut effects);
        effects
    }

    fn apply_event(&mut self, ev: Event, host: Rect) -> Vec<Effect> {
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
        self.hint = None;
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
            Overlay::Help => {
                self.on_help_key(key);
                return Vec::new();
            }
            Overlay::ConfigError => {
                self.overlay = Overlay::None;
                return match key.code {
                    KeyCode::Char('q') => vec![Effect::Quit],
                    _ => Vec::new(),
                };
            }
            Overlay::None => {}
        }
        // The answer to a pending question is consumed by the question.
        if let Some(id) = self.confirm.take() {
            return match key.code {
                KeyCode::Char('y' | 'Y') => vec![Effect::Send(ClientMsg::Restart { session: id })],
                _ => Vec::new(),
            };
        }
        let spec = keys::from_event(key);
        match self.mode {
            Mode::Normal => spec
                .and_then(|s| self.keymap.normal_action(&s))
                .map_or_else(Vec::new, |a| self.run_normal(a)),
            Mode::Focus => {
                if let Some(action) = spec.and_then(|s| self.keymap.focus_action(&s)) {
                    self.run_focus(action);
                    return Vec::new();
                }
                // Typing returns to the live view, as in a terminal.
                if let Some(id) = self.selected_session().map(|s| s.id.clone()) {
                    self.scroll.remove(&id);
                }
                let modes = self.selected_modes();
                self.to_session(|_| encode_key(key, &modes).unwrap_or_default())
            }
        }
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
        // Cached history was laid out for the old size.
        self.scroll.clear();
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
