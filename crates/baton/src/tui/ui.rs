//! Drawing: sidebar, info panel, main panel, bottom bar and modals.

use baton_proto::{SessionInfo, Status};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout as Split, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use super::app::{App, Overlay};
use crate::spike::Mode;
use crate::term::screen::Screen;

/// Bottom bar text in normal mode (spec section 4).
pub const NORMAL_BAR: &str =
    " NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  ? help  q quit";
/// Bottom bar text in focus mode (spec section 4).
pub const FOCUS_BAR: &str = " FOCUS │ Ctrl-\\ back  Alt-n next-attention  Alt-1..9 session";
/// Shown when the daemon connection is lost.
pub const DISCONNECTED_TEXT: &str = "daemon disconnected — press r to reconnect, q to quit";
/// Height of the info panel under the session list.
const INFO_HEIGHT: u16 = 8;

/// Status badge from spec section 4.
fn badge(status: Status) -> &'static str {
    match status {
        Status::Starting => "…",
        Status::Running => "●",
        Status::Permission => "◐",
        Status::YourTurn => "✓",
        Status::Idle => "○",
        Status::Exited(_) => "✗",
        Status::Unknown => "?",
    }
}

fn status_label(status: Status) -> String {
    match status {
        Status::Starting => "starting".into(),
        Status::Running => "running".into(),
        Status::Permission => "permission".into(),
        Status::YourTurn => "your turn".into(),
        Status::Idle => "idle".into(),
        Status::Exited(code) => format!("exited {code}"),
        Status::Unknown => "unknown".into(),
    }
}

fn short_name(s: &SessionInfo) -> String {
    let base = s.repo.rsplit('/').next().unwrap_or(&s.repo);
    format!("{}/{base}", s.project)
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

    let items: Vec<ListItem> = app
        .sessions
        .iter()
        .map(|s| {
            ListItem::new(format!(
                "{} {}  {}",
                badge(s.status),
                short_name(s),
                status_label(s.status)
            ))
        })
        .collect();
    let mut state = ListState::default().with_selected((!items.is_empty()).then_some(app.selected));
    f.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Sessions"))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        list_area,
        &mut state,
    );
    f.render_widget(
        Paragraph::new(info_text(app))
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
    let title = app.selected_session().map_or_else(
        || "Baton".to_owned(),
        |s| format!("{} · {}", short_name(s), status_label(s.status)),
    );
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border)
            .title(title),
        l.main,
    );
    match app.selected_mirror() {
        Some(m) => m.screen().render(l.inner, f.buffer_mut(), focus),
        None => f.render_widget(
            Paragraph::new("No sessions. Start a project with `baton debug open <name>`."),
            l.inner,
        ),
    }

    let bar = if focus { FOCUS_BAR } else { NORMAL_BAR };
    f.render_widget(
        Paragraph::new(bar).style(Style::default().add_modifier(Modifier::REVERSED)),
        l.bar,
    );

    match app.overlay {
        Overlay::None => {}
        Overlay::VersionMismatch { daemon } => modal(f, &mismatch_text(daemon)),
        Overlay::Disconnected => modal(f, DISCONNECTED_TEXT),
    }
}

fn info_text(app: &App) -> String {
    let Some(s) = app.selected_session() else {
        return "no session".into();
    };
    let mut text = format!("{}\nstatus  {}", s.repo, status_label(s.status));
    if let Some(msg) = &app.notice {
        text.push_str("\n! ");
        text.push_str(msg);
    }
    text
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
    use baton_proto::DaemonMsg;
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
            model: None,
            started_at: 0,
            exit_code: None,
            usage: None,
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
        let out = render(&app_with_session());
        assert!(out.contains("● x/a  running"), "{out}");
        assert!(out.contains("│mirrored text"), "{out}");
        assert!(out.contains(NORMAL_BAR.trim()), "{out}");
    }

    #[test]
    fn focus_mode_shows_focus_bar() {
        let mut app = app_with_session();
        app.mode = Mode::Focus;
        let out = render(&app);
        assert!(out.contains(FOCUS_BAR.trim()), "{out}");
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
