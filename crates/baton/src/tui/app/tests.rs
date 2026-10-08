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
        model: None,
        started_at: 0,
        exit_code: None,
        usage: None,
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
    assert_eq!(app.selected, 1);
    app.on_event(key(KeyCode::Char('j')), HOST);
    assert_eq!(app.selected, 1);
    app.on_event(key(KeyCode::Up), HOST);
    assert_eq!(app.selected, 0);
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
