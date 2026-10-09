//! Drawing: sidebar, info panel, main panel, bottom bar and modals.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout as Split, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use baton_core::keymap::{FocusAction, KeySpec, Keymap, NormalAction};

use super::app::{App, Overlay};
use super::info_panel;
use super::labels::{badge, status_label};
use super::sidebar::repo_name;
use crate::spike::Mode;
use crate::term::screen::Screen;

/// A key as the bottom bar shows it: `Ctrl-u`, `Alt-n`, `⏎`.
fn label(k: &KeySpec) -> String {
    let text = k.to_string();
    let mut out = String::new();
    let mut rest = text.as_str();
    for (prefix, shown) in [("ctrl-", "Ctrl-"), ("alt-", "Alt-"), ("shift-", "Shift-")] {
        if let Some(r) = rest.strip_prefix(prefix) {
            out.push_str(shown);
            rest = r;
        }
    }
    out.push_str(if rest == "enter" { "⏎" } else { rest });
    out
}

/// Bottom bar text in normal mode (spec section 4), from the live keymap.
pub fn normal_bar(keymap: &Keymap) -> String {
    let parts = [
        (NormalAction::Activate, "focus"),
        (NormalAction::NextAttention, "next-attention"),
        (NormalAction::Restart, "restart"),
        (NormalAction::Editor, "editor"),
        (NormalAction::OpenProject, "open project"),
        (NormalAction::Help, "help"),
        (NormalAction::Quit, "quit"),
    ];
    let items: Vec<String> = parts
        .iter()
        .filter_map(|(a, what)| {
            let first = keymap.normal_keys(*a).first()?;
            Some(format!("{} {what}", label(first)))
        })
        .collect();
    format!(" NORMAL │ {}", items.join("  "))
}

/// Bottom bar text in focus mode (spec section 4), from the live keymap.
pub fn focus_bar(keymap: &Keymap) -> String {
    let mut items = Vec::new();
    if let Some(k) = keymap.focus_keys(FocusAction::Unfocus).first() {
        items.push(format!("{} back", label(k)));
    }
    if let Some(k) = keymap.focus_keys(FocusAction::NextAttention).first() {
        items.push(format!("{} next-attention", label(k)));
    }
    let first = keymap
        .focus_keys(FocusAction::Session(1))
        .first()
        .map(label);
    let ninth = keymap
        .focus_keys(FocusAction::Session(9))
        .first()
        .map(label);
    if let Some(first) = first {
        let range = match ninth {
            Some(n) if n.strip_suffix('9') == first.strip_suffix('1') && n.ends_with('9') => {
                format!("{first}..9")
            }
            _ => first,
        };
        items.push(format!("{range} session"));
    }
    format!(" FOCUS │ {}", items.join("  "))
}
/// Shown when the daemon connection is lost.
pub const DISCONNECTED_TEXT: &str = "daemon disconnected — press r to reconnect, q to quit";
/// Height of the info panel under the session list: 11 rows, a notice and the border.
const INFO_HEIGHT: u16 = 14;

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn info_text(app: &App, width: u16) -> Vec<Line<'static>> {
    let Some(s) = app.selected_session() else {
        return vec![Line::from("no session")];
    };
    let home = std::env::var("HOME").ok();
    info_panel::lines(
        s,
        unix_now(),
        home.as_deref(),
        usize::from(width),
        app.notice.as_deref(),
    )
}

/// Text of the version-mismatch modal.
pub fn mismatch_text(daemon: u32) -> String {
    format!(
        "Daemon protocol v{} ≠ v{daemon}. Restart daemon (sessions will be resumed)? [y/N]",
        baton_proto::PROTOCOL_VERSION
    )
}

/// Draws the whole UI.
pub fn draw(f: &mut Frame, app: &App) {
    let l = app.layout;
    let focus = app.mode == Mode::Focus;
    let [list_area, info_area] = Split::vertical([
        Constraint::Min(3),
        Constraint::Length(INFO_HEIGHT.min(l.sidebar.height.saturating_sub(3))),
    ])
    .areas(l.sidebar);

    let items: Vec<ListItem> = app.rows().iter().map(|r| r.item()).collect();
    let mut state = ListState::default().with_selected(app.cursor_index());
    f.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Projects"))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        list_area,
        &mut state,
    );
    f.render_widget(
        Paragraph::new(info_text(app, info_area.width.saturating_sub(2)))
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("Session")),
        info_area,
    );

    let border = if focus {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let mut title = app.selected_session().map_or_else(
        || "Baton".to_owned(),
        |s| {
            format!(
                "{} · {} · {} {}",
                repo_name(s),
                s.profile.as_deref().unwrap_or("default"),
                badge(s.status),
                status_label(s.status)
            )
        },
    );
    if app.scroll_offset() > 0 {
        title.push_str(&format!(" [scrollback -{}]", app.scroll_offset()));
    }
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border)
            .title(title),
        l.main,
    );
    match (app.scroll_view(), app.selected_mirror()) {
        (Some(v), _) => v.render(l.inner, f.buffer_mut()),
        (None, Some(m)) => m.screen().render(l.inner, f.buffer_mut(), focus),
        (None, None) => f.render_widget(
            Paragraph::new("No sessions. Start a project with `baton debug open <name>`."),
            l.inner,
        ),
    }

    let mode = if focus { "FOCUS" } else { "NORMAL" };
    let bar = match app.hint {
        _ if app.confirm_prompt().is_some() => {
            format!(" NORMAL │ {}", app.confirm_prompt().unwrap_or_default())
        }
        _ if app.flash().is_some() => format!(" {mode} │ {}", app.flash().unwrap_or_default()),
        Some(hint) => format!(" {mode} │ {hint}"),
        None if focus => focus_bar(app.keymap()),
        None => normal_bar(app.keymap()),
    };
    f.render_widget(
        Paragraph::new(bar).style(Style::default().add_modifier(Modifier::REVERSED)),
        l.bar,
    );

    match app.overlay {
        Overlay::None => {}
        Overlay::VersionMismatch { daemon } => modal(f, &mismatch_text(daemon)),
        Overlay::Disconnected => modal(f, DISCONNECTED_TEXT),
        Overlay::Help => super::help::draw(f, app.keymap()),
    }
}

fn modal(f: &mut Frame, text: &str) {
    let area = f.area();
    let width = u16::try_from(text.chars().count() + 4)
        .unwrap_or(u16::MAX)
        .min(area.width);
    let height = 3.min(area.height);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(text.to_owned())
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL)),
        rect,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use baton_proto::{DaemonMsg, SessionInfo, Status};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::time::Instant;

    fn render(app: &App) -> String {
        let mut t = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
        t.draw(|f| draw(f, app)).expect("draw");
        let buf = t.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn app_with_session() -> App {
        let mut app = App::new(Rect::new(0, 0, 120, 40));
        let id = baton_proto::SessionId("x//tmp/a".into());
        let info = SessionInfo {
            id: id.clone(),
            project: "x".into(),
            repo: "/tmp/a".into(),
            profile: None,
            status: Status::Running,
            claude_session_id: None,
            transcript_path: None,
            model: None,
            started_at: 0,
            exit_code: None,
            usage: None,
            launch: None,
        };
        let now = Instant::now();
        app.on_daemon(DaemonMsg::SessionList(vec![info]), now);
        app.on_daemon(
            DaemonMsg::Snapshot {
                session: id,
                rows: 37,
                cols: 86,
                bytes: b"mirrored text".to_vec(),
            },
            now,
        );
        app
    }

    #[test]
    fn normal_mode_shows_session_mirror_and_bar() {
        let app = app_with_session();
        let out = render(&app);
        assert!(out.contains("1 ● a  running"), "{out}");
        assert!(out.contains("│mirrored text"), "{out}");
        assert!(out.contains(normal_bar(app.keymap()).trim()), "{out}");
    }

    #[test]
    fn info_panel_shows_the_launch_rung_and_the_bar_the_restart_question() {
        let mut app = app_with_session();
        app.sessions[0].launch = Some("resume".into());
        let out = render(&app);
        assert!(out.contains("launch   resume"), "{out}");
        assert!(!out.contains("Restart a?"), "{out}");
        app.on_event(
            crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('r'),
                crossterm::event::KeyModifiers::NONE,
            )),
            Rect::new(0, 0, 120, 40),
        );
        let out = render(&app);
        assert!(out.contains("Restart a? [y/N]"), "{out}");
        assert!(!out.contains(normal_bar(app.keymap()).trim()), "{out}");
    }

    #[test]
    fn sidebar_shows_open_and_closed_projects_and_dims_closed_ones() {
        let mut app = app_with_session();
        app.set_projects(vec!["x".into(), "y".into()]);
        let out = render(&app);
        assert!(out.contains("▾ x"), "{out}");
        assert!(out.contains("▸ y  (closed)"), "{out}");
        let mut t = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
        t.draw(|f| draw(f, &app)).expect("draw");
        let buf = t.backend().buffer().clone();
        let cell_with = |sym: &str| {
            (0..buf.area.height)
                .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
                .find(|&(x, y)| buf[(x, y)].symbol() == sym)
                .map(|p| buf[p].modifier)
        };
        assert!(cell_with("▸").is_some_and(|m| m.contains(Modifier::DIM)));
        assert!(cell_with("▾").is_some_and(|m| !m.contains(Modifier::DIM)));
    }

    #[test]
    fn main_title_and_info_panel_describe_the_session() {
        let mut app = app_with_session();
        app.sessions[0].profile = Some("p".into());
        let out = render(&app);
        assert!(out.contains("a · p · ● running"), "{out}");
        for field in ["/tmp/a", "profile  p", "status   running", "uptime   "] {
            assert!(out.contains(field), "{field}: {out}");
        }
    }

    #[test]
    fn scrolled_back_title_shows_the_indicator_and_history() {
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
        let mut app = app_with_session();
        let host = Rect::new(0, 0, 120, 40);
        app.on_event(
            Event::Key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)),
            host,
        );
        let history = (0..100).map(|i| format!("old{i}").into_bytes()).collect();
        app.on_daemon(
            DaemonMsg::Scrollback {
                session: baton_proto::SessionId("x//tmp/a".into()),
                start: 0,
                rows: history,
            },
            Instant::now(),
        );
        let out = render(&app);
        assert!(out.contains("[scrollback -18]"), "{out}");
        assert!(out.contains("│old82") && out.contains("│old99"), "{out}");
    }

    #[test]
    fn focus_mode_shows_focus_bar() {
        let mut app = app_with_session();
        app.mode = Mode::Focus;
        let out = render(&app);
        assert!(out.contains(focus_bar(app.keymap()).trim()), "{out}");
    }

    #[test]
    fn default_bars_match_the_spec_text() {
        let k = Keymap::default();
        assert_eq!(
            normal_bar(&k),
            " NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  ? help  q quit"
        );
        assert_eq!(
            focus_bar(&k),
            " FOCUS │ Ctrl-\\ back  Alt-n next-attention  Alt-1..9 session"
        );
    }

    fn press(app: &mut App, c: char) {
        app.on_event(
            crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(c),
                crossterm::event::KeyModifiers::NONE,
            )),
            Rect::new(0, 0, 120, 40),
        );
    }

    #[test]
    fn bars_and_help_follow_the_remapped_keys() {
        let mut o = baton_core::keymap::Overrides::new();
        o.entry("normal".into())
            .or_default()
            .insert("next_attention".into(), vec!["x".into()]);
        o.entry("focus".into())
            .or_default()
            .insert("unfocus".into(), vec!["ctrl-g".into()]);
        let mut app = app_with_session();
        app.set_keymap(Keymap::from_overrides(&o).expect("valid"));
        let out = render(&app);
        assert!(out.contains("x next-attention"), "{out}");
        press(&mut app, '?');
        let out = render(&app);
        assert!(out.contains("next_attention  x"), "{out}");
        assert!(out.contains("unfocus         ctrl-g"), "{out}");
        assert!(out.contains("Key bindings"), "{out}");
        press(&mut app, '?');
        assert!(!render(&app).contains("Key bindings"));
        app.mode = Mode::Focus;
        assert!(render(&app).contains("Ctrl-g back"));
    }

    #[test]
    fn editor_errors_replace_the_hint_in_the_bar() {
        let mut app = app_with_session();
        app.on_editor_error("cannot run nope: not found".into(), Instant::now());
        let out = render(&app);
        assert!(out.contains("NORMAL │ cannot run nope: not found"), "{out}");
    }

    #[test]
    fn modals_show_their_text() {
        let mut app = app_with_session();
        app.on_mismatch(7);
        assert!(render(&app).contains(&mismatch_text(7)));
        app.on_disconnected();
        assert!(render(&app).contains(DISCONNECTED_TEXT));
    }

    #[test]
    fn tiny_hosts_do_not_panic() {
        for (w, h) in [(0, 0), (1, 1), (10, 3), (33, 5)] {
            let mut app = App::new(Rect::new(0, 0, w, h));
            app.on_mismatch(2);
            let mut t = Terminal::new(TestBackend::new(w, h)).expect("terminal");
            t.draw(|f| draw(f, &app)).expect("draw");
        }
    }
}
