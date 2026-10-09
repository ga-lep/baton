use super::*;
use baton_proto::Status;
use crossterm::event::{MouseButton, MouseEventKind};

const HOST: Rect = Rect::new(0, 0, 120, 40);

fn info(id: &str, status: Status) -> SessionInfo {
    SessionInfo {
        id: SessionId(id.into()),
        project: "x".into(),
        repo: "/r".into(),
        profile: None,
        status,
        claude_session_id: None,
        transcript_path: None,
        model: None,
        started_at: 0,
        exit_code: None,
        usage: None,
        launch: None,
    }
}

fn sid(id: &str) -> SessionId {
    SessionId(id.into())
}

/// An app with sessions `x//a` and `x//b`, both with a snapshot.
fn app() -> App {
    let mut app = App::new(HOST);
    let now = Instant::now();
    app.on_daemon(
        DaemonMsg::SessionList(vec![
            info("x//a", Status::Running),
            info("x//b", Status::Idle),
        ]),
        now,
    );
    for id in ["x//a", "x//b"] {
        app.on_daemon(
            DaemonMsg::Snapshot {
                session: sid(id),
                rows: 37,
                cols: 86,
                bytes: b"hi".to_vec(),
            },
            now,
        );
    }
    app
}

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn sent(effects: &[Effect]) -> Vec<(SessionId, Vec<u8>)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Send(ClientMsg::Input { session, bytes }) => {
                Some((session.clone(), bytes.clone()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn output_with_a_device_attributes_query_produces_no_input_frame() {
    let mut app = app();
    for bytes in [&b"\x1b[c"[..], b"\x1b[6n", b"\x1b[>0q", b"a\x1b[cb"] {
        let fx = app.on_daemon(
            DaemonMsg::Output {
                session: sid("x//a"),
                bytes: bytes.to_vec(),
            },
            Instant::now(),
        );
        assert!(fx.is_empty(), "{fx:?}");
    }
    // Also not for the snapshot itself.
    let fx = app.on_daemon(
        DaemonMsg::Snapshot {
            session: sid("x//a"),
            rows: 5,
            cols: 5,
            bytes: b"\x1b[c".to_vec(),
        },
        Instant::now(),
    );
    assert!(fx.is_empty());
}

#[test]
fn enter_focuses_and_keys_are_encoded_to_the_selected_session() {
    let mut app = app();
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.on_event(key(KeyCode::Char('x')), HOST).is_empty());
    app.on_event(key(KeyCode::Enter), HOST);
    assert_eq!(app.mode, Mode::Focus);
    let fx = app.on_event(key(KeyCode::Char('x')), HOST);
    assert_eq!(sent(&fx), vec![(sid("x//a"), b"x".to_vec())]);
    // q is data in focus mode.
    let fx = app.on_event(key(KeyCode::Char('q')), HOST);
    assert_eq!(sent(&fx), vec![(sid("x//a"), b"q".to_vec())]);
}

#[test]
fn ctrl_backslash_in_both_forms_returns_to_normal_and_is_not_forwarded() {
    for code in ['\\', '4'] {
        let mut app = app();
        app.on_event(key(KeyCode::Enter), HOST);
        let fx = app.on_event(
            Event::Key(KeyEvent::new(KeyCode::Char(code), KeyModifiers::CONTROL)),
            HOST,
        );
        assert!(fx.is_empty());
        assert_eq!(app.mode, Mode::Normal);
    }
}

#[test]
fn q_in_normal_mode_quits_and_selection_moves_with_j_k() {
    let mut app = app();
    app.on_event(key(KeyCode::Char('j')), HOST);
    assert_eq!(
        app.selected_session().map(|s| s.id.0.as_str()),
        Some("x//b")
    );
    app.on_event(key(KeyCode::Char('j')), HOST);
    assert_eq!(
        app.selected_session().map(|s| s.id.0.as_str()),
        Some("x//b")
    );
    app.on_event(key(KeyCode::Up), HOST);
    assert_eq!(
        app.selected_session().map(|s| s.id.0.as_str()),
        Some("x//a")
    );
    assert_eq!(
        app.on_event(key(KeyCode::Char('q')), HOST),
        vec![Effect::Quit]
    );
}

#[test]
fn enter_without_sessions_stays_in_normal_mode() {
    let mut app = App::new(HOST);
    app.on_event(key(KeyCode::Enter), HOST);
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn paste_goes_through_the_encoder_in_chunks_of_at_most_max_input() {
    let mut app = app();
    // Not forwarded in normal mode.
    assert!(app.on_event(Event::Paste("hi".into()), HOST).is_empty());
    app.on_event(key(KeyCode::Enter), HOST);
    let fx = app.on_event(Event::Paste("hi".into()), HOST);
    assert_eq!(sent(&fx), vec![(sid("x//a"), b"hi".to_vec())]);

    let big = "y".repeat(MAX_INPUT * 2 + 10);
    let frames = sent(&app.on_event(Event::Paste(big.clone()), HOST));
    assert_eq!(frames.len(), 3);
    assert!(frames.iter().all(|(_, b)| b.len() <= MAX_INPUT));
    let joined: Vec<u8> = frames.into_iter().flat_map(|(_, b)| b).collect();
    assert_eq!(joined, big.as_bytes());
}

#[test]
fn bracketed_paste_markers_follow_the_app_mode() {
    let mut app = app();
    app.on_daemon(
        DaemonMsg::Output {
            session: sid("x//a"),
            bytes: b"\x1b[?2004h".to_vec(),
        },
        Instant::now(),
    );
    app.on_event(key(KeyCode::Enter), HOST);
    let fx = app.on_event(Event::Paste("p".into()), HOST);
    assert_eq!(
        sent(&fx),
        vec![(sid("x//a"), b"\x1b[200~p\x1b[201~".to_vec())]
    );
}

#[test]
fn focus_events_are_sent_only_in_focus_mode_and_when_the_app_asked() {
    let mut app = app();
    app.on_daemon(
        DaemonMsg::Output {
            session: sid("x//a"),
            bytes: b"\x1b[?1004h".to_vec(),
        },
        Instant::now(),
    );
    assert!(app.on_event(Event::FocusGained, HOST).is_empty());
    app.on_event(key(KeyCode::Enter), HOST);
    let fx = app.on_event(Event::FocusLost, HOST);
    assert_eq!(sent(&fx), vec![(sid("x//a"), b"\x1b[O".to_vec())]);
    // The other session did not enable ?1004.
    app.on_event(
        Event::Key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::CONTROL)),
        HOST,
    );
    app.on_event(key(KeyCode::Char('j')), HOST);
    app.on_event(key(KeyCode::Enter), HOST);
    assert!(app.on_event(Event::FocusLost, HOST).is_empty());
}

fn wheel(app: &App) -> Event {
    let i = app.layout.inner;
    Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: i.x + 2,
        row: i.y + 3,
        modifiers: KeyModifiers::NONE,
    })
}

#[test]
fn wheel_is_forwarded_only_in_focus_mode_with_mouse_mode_on() {
    let mut app = app();
    app.on_daemon(
        DaemonMsg::Output {
            session: sid("x//a"),
            bytes: b"\x1b[?1000h\x1b[?1006h".to_vec(),
        },
        Instant::now(),
    );
    let ev = wheel(&app);
    assert!(app.on_event(ev.clone(), HOST).is_empty(), "normal mode");
    app.on_event(key(KeyCode::Enter), HOST);
    let frames = sent(&app.on_event(ev, HOST));
    assert_eq!(frames.len(), 1);
    assert!(
        frames[0].1.starts_with(b"\x1b[<64;3;4"),
        "{:?}",
        frames[0].1
    );
    // Outside the panel: nothing.
    let outside = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.on_event(outside, HOST).is_empty());
}

#[test]
fn resize_sends_the_inner_size_once() {
    let mut app = app();
    assert_eq!(app.size(), (37, 86));
    let host = Rect::new(0, 0, 100, 30);
    let fx = app.on_event(Event::Resize(100, 30), host);
    assert_eq!(
        fx,
        vec![Effect::Send(ClientMsg::Resize { rows: 27, cols: 66 })]
    );
    assert!(app.on_event(Event::Resize(100, 30), host).is_empty());
}

#[test]
fn version_mismatch_modal_y_restarts_anything_else_quits() {
    let mut app = App::new(HOST);
    app.on_mismatch(99);
    assert_eq!(app.overlay, Overlay::VersionMismatch { daemon: 99 });
    assert_eq!(
        app.on_event(key(KeyCode::Char('y')), HOST),
        vec![Effect::RestartDaemon]
    );
    for code in [
        KeyCode::Char('n'),
        KeyCode::Enter,
        KeyCode::Esc,
        KeyCode::Char('x'),
    ] {
        assert_eq!(app.on_event(key(code), HOST), vec![Effect::Quit]);
    }
}

#[test]
fn disconnected_modal_r_reconnects_q_quits_and_other_keys_are_swallowed() {
    let mut app = app();
    app.on_event(key(KeyCode::Enter), HOST);
    app.on_disconnected();
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.on_event(key(KeyCode::Char('x')), HOST).is_empty());
    assert!(app.on_event(key(KeyCode::Enter), HOST).is_empty());
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(
        app.on_event(key(KeyCode::Char('r')), HOST),
        vec![Effect::Reconnect]
    );
    assert_eq!(
        app.on_event(key(KeyCode::Char('q')), HOST),
        vec![Effect::Quit]
    );
    app.on_connected();
    assert_eq!(app.overlay, Overlay::None);
    assert!(app.sessions.is_empty());
}

#[test]
fn status_changes_and_session_list_updates_keep_the_selection() {
    let mut app = app();
    app.on_event(key(KeyCode::Char('j')), HOST);
    app.on_daemon(
        DaemonMsg::StatusChanged {
            session: sid("x//b"),
            status: Status::Exited(3),
        },
        Instant::now(),
    );
    assert_eq!(app.sessions[1].exit_code, Some(3));
    app.on_daemon(
        DaemonMsg::SessionList(vec![
            info("x//c", Status::Starting),
            info("x//a", Status::Running),
            info("x//b", Status::Exited(3)),
        ]),
        Instant::now(),
    );
    assert_eq!(
        app.selected_session().map(|s| s.id.0.as_str()),
        Some("x//b")
    );
}

// ---- Task 12: project tree, open project, switching, scrollback ----

use crate::tui::scrollback::PAGE;
use crate::tui::sidebar::Cursor;

/// `app()` plus configured projects `x` (open) and `y` (closed).
fn tree_app() -> App {
    let mut app = app();
    app.set_projects(vec!["x".into(), "y".into()]);
    app
}

fn ch(c: char) -> Event {
    key(KeyCode::Char(c))
}

fn ctrl(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

fn cursor_is(app: &App, c: Cursor) {
    assert_eq!(app.cursor(), Some(&c));
}

fn shown(app: &App) -> Option<&str> {
    app.selected_session().map(|s| s.id.0.as_str())
}

#[test]
fn cursor_starts_on_the_first_session_and_walks_projects_and_sessions() {
    let mut app = tree_app();
    cursor_is(&app, Cursor::Session(sid("x//a")));
    app.on_event(ch('j'), HOST);
    cursor_is(&app, Cursor::Session(sid("x//b")));
    app.on_event(key(KeyCode::Down), HOST);
    cursor_is(&app, Cursor::Project("y".into()));
    // The main panel keeps the last session while the cursor is on a project.
    assert_eq!(shown(&app), Some("x//b"));
    app.on_event(ch('j'), HOST);
    cursor_is(&app, Cursor::Project("y".into()));
    app.on_event(ch('k'), HOST);
    app.on_event(key(KeyCode::Up), HOST);
    app.on_event(ch('k'), HOST);
    cursor_is(&app, Cursor::Project("x".into()));
    assert_eq!(shown(&app), Some("x//a"));
}

#[test]
fn enter_and_l_focus_sessions_and_open_closed_projects() {
    for open in [KeyCode::Enter, KeyCode::Char('l')] {
        let mut app = tree_app();
        app.on_event(key(open), HOST);
        assert_eq!(app.mode, Mode::Focus);
    }
    let mut app = tree_app();
    app.on_event(ch('j'), HOST);
    app.on_event(ch('j'), HOST);
    let fx = app.on_event(key(KeyCode::Enter), HOST);
    assert_eq!(
        fx,
        vec![Effect::Send(ClientMsg::OpenProject { name: "y".into() })]
    );
    assert_eq!(app.mode, Mode::Normal);
    // On an open project, Enter steps into its first session.
    let mut app = tree_app();
    app.on_event(ch('k'), HOST);
    cursor_is(&app, Cursor::Project("x".into()));
    assert!(app.on_event(key(KeyCode::Enter), HOST).is_empty());
    cursor_is(&app, Cursor::Session(sid("x//a")));
}

#[test]
fn o_opens_the_project_under_the_cursor() {
    let mut app = tree_app();
    let open = |n: &str| vec![Effect::Send(ClientMsg::OpenProject { name: n.into() })];
    assert_eq!(app.on_event(ch('o'), HOST), open("x"));
    app.on_event(ch('j'), HOST);
    app.on_event(ch('j'), HOST);
    assert_eq!(app.on_event(ch('o'), HOST), open("y"));
}

#[test]
fn digits_select_the_nth_session_of_the_current_project() {
    let mut app = tree_app();
    app.on_event(ch('2'), HOST);
    assert_eq!(shown(&app), Some("x//b"));
    cursor_is(&app, Cursor::Session(sid("x//b")));
    app.on_event(ch('1'), HOST);
    assert_eq!(shown(&app), Some("x//a"));
    // No third session: nothing changes.
    app.on_event(ch('3'), HOST);
    assert_eq!(shown(&app), Some("x//a"));
    // Digits are data in focus mode.
    app.on_event(key(KeyCode::Enter), HOST);
    let fx = app.on_event(ch('2'), HOST);
    assert_eq!(sent(&fx), vec![(sid("x//a"), b"2".to_vec())]);
}

#[test]
fn a_session_list_for_a_new_project_keeps_the_cursor() {
    let mut app = tree_app();
    app.on_event(ch('j'), HOST);
    app.on_event(ch('j'), HOST);
    let mut list = app.sessions.clone();
    let mut y = info("y//c", Status::Starting);
    y.project = "y".into();
    list.push(y);
    app.on_daemon(DaemonMsg::SessionList(list), Instant::now());
    cursor_is(&app, Cursor::Project("y".into()));
    assert_eq!(shown(&app), Some("x//b"));
}

fn history(n: usize) -> Vec<Vec<u8>> {
    (0..n).map(|i| format!("line{i}").into_bytes()).collect()
}

fn deliver(app: &mut App, start: u32, rows: Vec<Vec<u8>>) -> Vec<Effect> {
    app.on_daemon(
        DaemonMsg::Scrollback {
            session: sid("x//a"),
            start,
            rows,
        },
        Instant::now(),
    )
}

fn get_scrollback(start: u32) -> Effect {
    Effect::Send(ClientMsg::GetScrollback {
        session: sid("x//a"),
        start,
        count: PAGE,
    })
}

#[test]
fn ctrl_u_fetches_history_then_scrolls_by_half_and_full_pages() {
    let mut app = tree_app();
    assert_eq!(app.size().0, 37);
    assert_eq!(app.on_event(ctrl('u'), HOST), vec![get_scrollback(0)]);
    assert_eq!(app.scroll_offset(), 0);
    assert!(deliver(&mut app, 0, history(100)).is_empty());
    assert_eq!(app.scroll_offset(), 18);
    app.on_event(key(KeyCode::PageUp), HOST);
    assert_eq!(app.scroll_offset(), 55);
    app.on_event(ctrl('d'), HOST);
    assert_eq!(app.scroll_offset(), 37);
    app.on_event(key(KeyCode::PageDown), HOST);
    assert_eq!(app.scroll_offset(), 0);
    // Further down is a no-op, not a request.
    assert!(app.on_event(key(KeyCode::PageDown), HOST).is_empty());
}

#[test]
fn scrolling_is_clamped_to_history_and_g_returns_to_live() {
    let mut app = tree_app();
    app.on_event(ctrl('u'), HOST);
    deliver(&mut app, 0, history(20));
    assert_eq!(app.scroll_offset(), 18);
    app.on_event(key(KeyCode::PageUp), HOST);
    assert_eq!(app.scroll_offset(), 20);
    app.on_event(ch('G'), HOST);
    assert_eq!(app.scroll_offset(), 0);
    // The next scroll reuses nothing stale: it asks again.
    assert_eq!(app.on_event(ctrl('u'), HOST), vec![get_scrollback(0)]);
}

#[test]
fn full_history_pages_are_followed_until_a_short_page() {
    let mut app = tree_app();
    app.on_event(ctrl('u'), HOST);
    let page = vec![b"l".to_vec(); PAGE as usize];
    assert_eq!(deliver(&mut app, 0, page), vec![get_scrollback(PAGE)]);
    assert_eq!(app.scroll_offset(), 0);
    assert!(deliver(&mut app, PAGE, history(5)).is_empty());
    assert_eq!(app.scroll_offset(), 18);
}

#[test]
fn empty_history_and_stale_replies_do_not_scroll() {
    let mut app = tree_app();
    // Reply nobody asked for.
    assert!(deliver(&mut app, 0, history(10)).is_empty());
    assert_eq!(app.scroll_offset(), 0);
    app.on_event(ctrl('u'), HOST);
    deliver(&mut app, 0, Vec::new());
    assert_eq!(app.scroll_offset(), 0);
}

#[test]
fn the_alt_screen_refuses_to_scroll_and_shows_a_hint() {
    let mut app = tree_app();
    app.on_daemon(
        DaemonMsg::Output {
            session: sid("x//a"),
            bytes: b"\x1b[?1049h".to_vec(),
        },
        Instant::now(),
    );
    assert!(app.on_event(ctrl('u'), HOST).is_empty());
    assert_eq!(
        app.hint,
        Some("app manages its own scrolling (mouse wheel)")
    );
    app.on_event(ch('x'), HOST);
    assert_eq!(app.hint, None);
}

#[test]
fn typing_in_focus_mode_and_resizing_leave_scrollback() {
    let mut app = tree_app();
    app.on_event(ctrl('u'), HOST);
    deliver(&mut app, 0, history(100));
    app.on_event(key(KeyCode::Enter), HOST);
    app.on_event(ch('x'), HOST);
    assert_eq!(app.scroll_offset(), 0);

    app.on_event(ctrl('\\'), HOST);
    app.on_event(ctrl('u'), HOST);
    deliver(&mut app, 0, history(100));
    assert_eq!(app.scroll_offset(), 18);
    app.on_event(Event::Resize(100, 30), Rect::new(0, 0, 100, 30));
    assert_eq!(app.scroll_offset(), 0);
}

#[test]
fn scroll_keys_are_data_in_focus_mode() {
    let mut app = tree_app();
    app.on_event(key(KeyCode::Enter), HOST);
    let fx = app.on_event(ctrl('u'), HOST);
    assert_eq!(sent(&fx), vec![(sid("x//a"), vec![0x15])]);
}

fn alt(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT))
}

fn views(effects: &[Effect]) -> Vec<(Option<SessionId>, bool)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Send(ClientMsg::ClientView {
                on_screen,
                terminal_focused,
            }) => Some((on_screen.clone(), *terminal_focused)),
            _ => None,
        })
        .collect()
}

/// `app()` with `x//a` running, `x//b` idle and a third session `x//c`.
fn three(a: Status, b: Status, c: Status) -> App {
    let mut app = App::new(HOST);
    app.on_daemon(
        DaemonMsg::SessionList(vec![info("x//a", a), info("x//b", b), info("x//c", c)]),
        Instant::now(),
    );
    app
}

#[test]
fn n_jumps_to_the_next_attention_session_after_the_current_one() {
    let mut app = three(Status::Idle, Status::Permission, Status::YourTurn);
    assert_eq!(shown(&app), Some("x//a"));
    app.on_event(ch('n'), HOST);
    assert_eq!(shown(&app), Some("x//b"));
    cursor_is(&app, Cursor::Session(sid("x//b")));
    app.on_event(ch('n'), HOST);
    assert_eq!(shown(&app), Some("x//c"));
    app.on_event(ch('n'), HOST);
    assert_eq!(shown(&app), Some("x//b"), "wraps, skipping idle a");
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn n_with_nothing_to_attend_to_shows_a_hint_and_stays_put() {
    let mut app = three(Status::Idle, Status::Running, Status::Exited(0));
    app.on_event(ch('n'), HOST);
    assert_eq!(shown(&app), Some("x//a"));
    assert_eq!(app.hint, Some(NO_ATTENTION_HINT));
    assert_eq!(NO_ATTENTION_HINT, "no session needs attention");
    // The only attention session being the current one counts as none.
    let mut app = three(Status::YourTurn, Status::Idle, Status::Idle);
    app.on_event(ch('n'), HOST);
    assert_eq!(app.hint, Some(NO_ATTENTION_HINT));
    // The hint goes away with the next key.
    app.on_event(ch('j'), HOST);
    assert_eq!(app.hint, None);
}

#[test]
fn n_is_data_in_focus_mode_but_alt_n_jumps_and_stays_focused() {
    let mut app = three(Status::Idle, Status::Idle, Status::Permission);
    app.on_event(key(KeyCode::Enter), HOST);
    assert_eq!(app.mode, Mode::Focus);
    let fx = app.on_event(ch('n'), HOST);
    assert_eq!(sent(&fx), vec![(sid("x//a"), b"n".to_vec())]);
    let fx = app.on_event(alt('n'), HOST);
    assert_eq!(app.mode, Mode::Focus);
    assert_eq!(shown(&app), Some("x//c"));
    assert!(sent(&fx).is_empty(), "Alt-n is not forwarded: {fx:?}");
}

#[test]
fn alt_digits_switch_sessions_in_focus_mode() {
    let mut app = three(Status::Idle, Status::Idle, Status::Idle);
    app.on_event(key(KeyCode::Enter), HOST);
    let fx = app.on_event(alt('3'), HOST);
    assert_eq!(shown(&app), Some("x//c"));
    assert_eq!(app.mode, Mode::Focus);
    assert!(sent(&fx).is_empty());
    app.on_event(alt('1'), HOST);
    assert_eq!(shown(&app), Some("x//a"));
    app.on_event(alt('9'), HOST);
    assert_eq!(shown(&app), Some("x//a"), "no 9th session: unchanged");
}

#[test]
fn client_view_is_sent_when_the_selection_or_terminal_focus_changes() {
    let mut app = App::new(HOST);
    let fx = app.on_daemon(
        DaemonMsg::SessionList(vec![info("x//a", Status::Idle), info("x//b", Status::Idle)]),
        Instant::now(),
    );
    assert_eq!(views(&fx), vec![(Some(sid("x//a")), true)]);
    // Nothing changed: nothing sent.
    let fx = app.on_daemon(
        DaemonMsg::StatusChanged {
            session: sid("x//b"),
            status: Status::Running,
        },
        Instant::now(),
    );
    assert!(views(&fx).is_empty());
    let fx = app.on_event(ch('j'), HOST);
    assert_eq!(views(&fx), vec![(Some(sid("x//b")), true)]);
    let fx = app.on_event(Event::FocusLost, HOST);
    assert_eq!(views(&fx), vec![(Some(sid("x//b")), false)]);
    let fx = app.on_event(Event::FocusGained, HOST);
    assert_eq!(views(&fx), vec![(Some(sid("x//b")), true)]);
    // Moving back up shows the first session again.
    let fx = app.on_event(ch('k'), HOST);
    assert_eq!(views(&fx), vec![(Some(sid("x//a")), true)]);
}

#[test]
fn client_view_is_resent_after_reconnecting() {
    let mut app = app();
    app.on_connected();
    let fx = app.on_daemon(
        DaemonMsg::SessionList(vec![info("x//a", Status::Idle)]),
        Instant::now(),
    );
    assert_eq!(views(&fx), vec![(Some(sid("x//a")), true)]);
}

fn restarts(effects: &[Effect]) -> Vec<SessionId> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Send(ClientMsg::Restart { session }) => Some(session.clone()),
            _ => None,
        })
        .collect()
}

fn app_with(a: Status, b: Status) -> App {
    let mut app = App::new(HOST);
    app.on_daemon(
        DaemonMsg::SessionList(vec![info("x//a", a), info("x//b", b)]),
        Instant::now(),
    );
    app
}

#[test]
fn r_restarts_a_quiet_session_without_asking() {
    for status in [
        Status::Idle,
        Status::YourTurn,
        Status::Starting,
        Status::Unknown,
        Status::Exited(1),
    ] {
        let mut app = app_with(status, Status::Idle);
        let effects = app.on_event(key(KeyCode::Char('r')), HOST);
        assert_eq!(restarts(&effects), [sid("x//a")], "{status:?}");
        assert_eq!(app.confirm_prompt(), None, "{status:?}");
    }
}

#[test]
fn r_asks_before_restarting_a_running_or_permission_session() {
    for status in [Status::Running, Status::Permission] {
        let mut app = app_with(status, Status::Idle);
        let effects = app.on_event(key(KeyCode::Char('r')), HOST);
        assert!(restarts(&effects).is_empty(), "{status:?}");
        assert_eq!(app.confirm_prompt().as_deref(), Some("Restart r? [y/N]"));
        let effects = app.on_event(key(KeyCode::Char('y')), HOST);
        assert_eq!(restarts(&effects), [sid("x//a")], "{status:?}");
        assert_eq!(app.confirm_prompt(), None);
    }
}

#[test]
fn anything_but_y_cancels_the_restart_prompt() {
    for answer in [
        KeyCode::Char('n'),
        KeyCode::Char('N'),
        KeyCode::Esc,
        KeyCode::Enter,
        KeyCode::Char('r'),
    ] {
        let mut app = app_with(Status::Running, Status::Idle);
        app.on_event(key(KeyCode::Char('r')), HOST);
        let effects = app.on_event(key(answer), HOST);
        assert!(restarts(&effects).is_empty(), "{answer:?}");
        assert_eq!(app.confirm_prompt(), None, "{answer:?}");
        // The answer was consumed: it did not also move or focus anything.
        assert_eq!(app.mode, Mode::Normal);
    }
}

#[test]
fn r_targets_the_session_under_the_cursor_and_does_nothing_on_a_project_row() {
    let mut app = app_with(Status::Idle, Status::Idle);
    app.on_event(key(KeyCode::Char('j')), HOST);
    let effects = app.on_event(key(KeyCode::Char('r')), HOST);
    assert_eq!(restarts(&effects), [sid("x//b")]);

    let mut app = App::new(HOST);
    app.set_projects(vec!["x".into()]);
    assert!(
        restarts(&app.on_event(key(KeyCode::Char('r')), HOST)).is_empty(),
        "no session to restart"
    );
}

#[test]
fn r_is_data_in_focus_mode() {
    let mut app = app_with(Status::Idle, Status::Idle);
    app.mode = Mode::Focus;
    let effects = app.on_event(key(KeyCode::Char('r')), HOST);
    assert!(restarts(&effects).is_empty());
    assert_eq!(sent(&effects), [(sid("x//a"), b"r".to_vec())]);
}

fn closed_app() -> App {
    let mut app = App::new(HOST);
    app.set_projects(vec!["x".into()]);
    app.on_daemon(
        DaemonMsg::SessionList(vec![info("x//a", Status::Closed)]),
        Instant::now(),
    );
    app
}

fn open_x() -> Vec<Effect> {
    vec![Effect::Send(ClientMsg::OpenProject { name: "x".into() })]
}

#[test]
fn enter_or_r_on_a_remembered_session_opens_its_project_instead() {
    for k in [KeyCode::Enter, KeyCode::Char('l'), KeyCode::Char('r')] {
        let mut app = closed_app();
        cursor_is(&app, Cursor::Session(sid("x//a")));
        assert_eq!(app.on_event(key(k), HOST), open_x(), "{k:?}");
        assert_eq!(app.mode, Mode::Normal, "{k:?}");
    }
}

#[test]
fn enter_on_a_project_with_only_remembered_sessions_opens_it() {
    let mut app = closed_app();
    app.on_event(ch('k'), HOST);
    cursor_is(&app, Cursor::Project("x".into()));
    assert_eq!(app.on_event(key(KeyCode::Enter), HOST), open_x());
}

#[test]
fn opening_replaces_remembered_sessions_with_live_ones() {
    let mut app = closed_app();
    app.on_daemon(
        DaemonMsg::SessionList(vec![info("x//a", Status::Starting)]),
        Instant::now(),
    );
    assert_eq!(
        app.selected_session().map(|s| s.status),
        Some(Status::Starting)
    );
    app.on_event(key(KeyCode::Enter), HOST);
    assert_eq!(app.mode, Mode::Focus);
}

#[test]
fn usage_updates_replace_the_sessions_usage_and_none_means_n_a() {
    let mut app = app();
    let usage = baton_proto::Usage {
        input: 7,
        ..Default::default()
    };
    app.on_daemon(
        DaemonMsg::UsageUpdated {
            session: sid("x//b"),
            usage: Some(usage.clone()),
        },
        Instant::now(),
    );
    let by_id = |app: &App, id: &str| {
        app.sessions
            .iter()
            .find(|s| s.id == sid(id))
            .and_then(|s| s.usage.clone())
    };
    assert_eq!(by_id(&app, "x//b"), Some(usage));
    assert_eq!(by_id(&app, "x//a"), None);
    app.on_daemon(
        DaemonMsg::UsageUpdated {
            session: sid("x//b"),
            usage: None,
        },
        Instant::now(),
    );
    assert_eq!(by_id(&app, "x//b"), None);
}
