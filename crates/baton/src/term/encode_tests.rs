use super::encode::*;
use baton_core::term::{MouseMode, TermModes};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};

const NONE: KeyModifiers = KeyModifiers::NONE;
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;
const ALT: KeyModifiers = KeyModifiers::ALT;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;

fn plain() -> EncodeModes {
    EncodeModes::default()
}
fn cursor() -> EncodeModes {
    EncodeModes {
        app_cursor: true,
        ..plain()
    }
}
fn mok(level: u8) -> EncodeModes {
    EncodeModes {
        term: TermModes {
            modify_other_keys: level,
            ..TermModes::default()
        },
        ..plain()
    }
}
fn kitty(flags: u8) -> EncodeModes {
    EncodeModes {
        term: TermModes {
            kitty_flags: flags,
            ..TermModes::default()
        },
        ..plain()
    }
}
fn c(ch: char) -> KeyCode {
    KeyCode::Char(ch)
}

#[test]
fn key_table() {
    let rows: Vec<(KeyCode, KeyModifiers, EncodeModes, &[u8])> = vec![
        // printable
        (c('a'), NONE, plain(), b"a"),
        (c('Z'), SHIFT, plain(), b"Z"),
        (c(' '), NONE, plain(), b" "),
        (c('~'), NONE, plain(), b"~"),
        (c('é'), NONE, plain(), "é".as_bytes()),
        (c('中'), NONE, plain(), "中".as_bytes()),
        (c('😀'), NONE, plain(), "😀".as_bytes()),
        // simple keys
        (KeyCode::Enter, NONE, plain(), b"\r"),
        (KeyCode::Tab, NONE, plain(), b"\t"),
        (KeyCode::Backspace, NONE, plain(), b"\x7f"),
        (KeyCode::Esc, NONE, plain(), b"\x1b"),
        (KeyCode::BackTab, SHIFT, plain(), b"\x1b[Z"),
        (KeyCode::Tab, SHIFT, plain(), b"\x1b[Z"),
        (KeyCode::Null, NONE, plain(), b"\x00"),
        // ctrl letters
        (c('a'), CTRL, plain(), b"\x01"),
        (c('b'), CTRL, plain(), b"\x02"),
        (c('c'), CTRL, plain(), b"\x03"),
        (c('d'), CTRL, plain(), b"\x04"),
        (c('l'), CTRL, plain(), b"\x0c"),
        (c('m'), CTRL, plain(), b"\x0d"),
        (c('z'), CTRL, plain(), b"\x1a"),
        (c('A'), CTRL | SHIFT, plain(), b"\x01"),
        // ctrl punctuation
        (c('@'), CTRL, plain(), b"\x00"),
        (c(' '), CTRL, plain(), b"\x00"),
        (c('['), CTRL, plain(), b"\x1b"),
        (c('\\'), CTRL, plain(), b"\x1c"),
        (c(']'), CTRL, plain(), b"\x1d"),
        (c('^'), CTRL, plain(), b"\x1e"),
        (c('_'), CTRL, plain(), b"\x1f"),
        (c('?'), CTRL, plain(), b"\x7f"),
        (c('['), CTRL, mok(1), b"\x1b"),
        // ctrl punctuation under modifyOtherKeys 2
        (c('['), CTRL, mok(2), b"\x1b[27;5;91~"),
        (c(']'), CTRL, mok(2), b"\x1b[27;5;93~"),
        (c(' '), CTRL, mok(2), b"\x1b[27;5;32~"),
        (c('a'), CTRL, mok(2), b"\x01"),
        // alt
        (c('x'), ALT, plain(), b"\x1bx"),
        (c('é'), ALT, plain(), "\u{1b}é".as_bytes()),
        (c('a'), ALT | CTRL, plain(), b"\x1b\x01"),
        (KeyCode::Enter, ALT, plain(), b"\x1b\r"),
        (KeyCode::Backspace, ALT, plain(), b"\x1b\x7f"),
        (KeyCode::Esc, ALT, plain(), b"\x1b\x1b"),
        (KeyCode::Tab, ALT, plain(), b"\x1b\t"),
        // shift-enter
        (KeyCode::Enter, SHIFT, plain(), b"\x1b\r"),
        (KeyCode::Enter, SHIFT, mok(1), b"\x1b[27;2;13~"),
        (KeyCode::Enter, SHIFT, mok(2), b"\x1b[27;2;13~"),
        (KeyCode::Enter, SHIFT, kitty(1), b"\x1b[13;2u"),
        (KeyCode::Enter, SHIFT, kitty(5), b"\x1b[13;2u"),
        (KeyCode::Enter, SHIFT, kitty(2), b"\x1b\r"),
        (KeyCode::Enter, CTRL, plain(), b"\r"),
        (KeyCode::Enter, CTRL, mok(1), b"\r"),
        (KeyCode::Enter, CTRL, mok(2), b"\x1b[27;5;13~"),
        // arrows
        (KeyCode::Up, NONE, plain(), b"\x1b[A"),
        (KeyCode::Down, NONE, plain(), b"\x1b[B"),
        (KeyCode::Right, NONE, plain(), b"\x1b[C"),
        (KeyCode::Left, NONE, plain(), b"\x1b[D"),
        (KeyCode::Up, NONE, cursor(), b"\x1bOA"),
        (KeyCode::Down, NONE, cursor(), b"\x1bOB"),
        (KeyCode::Right, NONE, cursor(), b"\x1bOC"),
        (KeyCode::Left, NONE, cursor(), b"\x1bOD"),
        (KeyCode::Right, CTRL, plain(), b"\x1b[1;5C"),
        (KeyCode::Left, CTRL, cursor(), b"\x1b[1;5D"),
        (KeyCode::Up, SHIFT, plain(), b"\x1b[1;2A"),
        (KeyCode::Down, ALT, plain(), b"\x1b[1;3B"),
        (KeyCode::Up, CTRL | SHIFT | ALT, plain(), b"\x1b[1;8A"),
        // home/end/nav
        (KeyCode::Home, NONE, plain(), b"\x1b[H"),
        (KeyCode::End, NONE, plain(), b"\x1b[F"),
        (KeyCode::Home, NONE, cursor(), b"\x1bOH"),
        (KeyCode::End, NONE, cursor(), b"\x1bOF"),
        (KeyCode::Home, CTRL, plain(), b"\x1b[1;5H"),
        (KeyCode::End, SHIFT, plain(), b"\x1b[1;2F"),
        (KeyCode::Insert, NONE, plain(), b"\x1b[2~"),
        (KeyCode::Delete, NONE, plain(), b"\x1b[3~"),
        (KeyCode::PageUp, NONE, plain(), b"\x1b[5~"),
        (KeyCode::PageDown, NONE, plain(), b"\x1b[6~"),
        (KeyCode::Delete, CTRL, plain(), b"\x1b[3;5~"),
        (KeyCode::PageUp, SHIFT, plain(), b"\x1b[5;2~"),
        (KeyCode::PageDown, ALT, plain(), b"\x1b[6;3~"),
        (KeyCode::Insert, CTRL | SHIFT, plain(), b"\x1b[2;6~"),
        // function keys
        (KeyCode::F(1), NONE, plain(), b"\x1bOP"),
        (KeyCode::F(2), NONE, plain(), b"\x1bOQ"),
        (KeyCode::F(3), NONE, plain(), b"\x1bOR"),
        (KeyCode::F(4), NONE, plain(), b"\x1bOS"),
        (KeyCode::F(5), NONE, plain(), b"\x1b[15~"),
        (KeyCode::F(6), NONE, plain(), b"\x1b[17~"),
        (KeyCode::F(7), NONE, plain(), b"\x1b[18~"),
        (KeyCode::F(8), NONE, plain(), b"\x1b[19~"),
        (KeyCode::F(9), NONE, plain(), b"\x1b[20~"),
        (KeyCode::F(10), NONE, plain(), b"\x1b[21~"),
        (KeyCode::F(11), NONE, plain(), b"\x1b[23~"),
        (KeyCode::F(12), NONE, plain(), b"\x1b[24~"),
        (KeyCode::F(1), CTRL, plain(), b"\x1b[1;5P"),
        (KeyCode::F(5), SHIFT, plain(), b"\x1b[15;2~"),
    ];
    assert!(rows.len() >= 60);
    for (code, mods, modes, want) in rows {
        let ev = KeyEvent::new(code, mods);
        assert_eq!(
            encode_key(&ev, &modes).as_deref(),
            Some(want),
            "{code:?} + {mods:?} with {modes:?}"
        );
    }
}

#[test]
fn key_ignored_events() {
    let rel = KeyEvent::new_with_kind(c('a'), NONE, KeyEventKind::Release);
    assert_eq!(encode_key(&rel, &plain()), None);
    let rep = KeyEvent::new_with_kind(c('a'), NONE, KeyEventKind::Repeat);
    assert_eq!(encode_key(&rep, &plain()).as_deref(), Some(&b"a"[..]));
    assert_eq!(
        encode_key(&KeyEvent::new(KeyCode::F(13), NONE), &plain()),
        None
    );
    assert_eq!(
        encode_key(&KeyEvent::new(KeyCode::CapsLock, NONE), &plain()),
        None
    );
}

#[test]
fn keypad_application_mode() {
    let kp = |code| {
        KeyEvent::new_with_kind_and_state(code, NONE, KeyEventKind::Press, KeyEventState::KEYPAD)
    };
    let app = EncodeModes {
        app_keypad: true,
        ..plain()
    };
    assert_eq!(
        encode_key(&kp(c('0')), &app).as_deref(),
        Some(&b"\x1bOp"[..])
    );
    assert_eq!(
        encode_key(&kp(c('9')), &app).as_deref(),
        Some(&b"\x1bOy"[..])
    );
    assert_eq!(
        encode_key(&kp(KeyCode::Enter), &app).as_deref(),
        Some(&b"\x1bOM"[..])
    );
    assert_eq!(
        encode_key(&kp(c('0')), &plain()).as_deref(),
        Some(&b"0"[..])
    );
    // Non-keypad digit is unaffected.
    let d = KeyEvent::new(c('0'), NONE);
    assert_eq!(encode_key(&d, &app).as_deref(), Some(&b"0"[..]));
}

#[test]
fn paste_table() {
    let bracketed = EncodeModes {
        bracketed_paste: true,
        ..plain()
    };
    assert_eq!(encode_paste("hi", &bracketed), b"\x1b[200~hi\x1b[201~");
    assert_eq!(encode_paste("a\nb", &bracketed), b"\x1b[200~a\nb\x1b[201~");
    assert_eq!(encode_paste("", &bracketed), b"\x1b[200~\x1b[201~");
    assert_eq!(
        encode_paste("x\x1b[201~y", &bracketed),
        b"\x1b[200~xy\x1b[201~"
    );
    // Stripping must not let a forged terminator reassemble.
    assert_eq!(
        encode_paste("\x1b[2\x1b[201~01~", &bracketed),
        b"\x1b[200~\x1b[201~"
    );
    assert_eq!(encode_paste("a\nb\r\nc", &plain()), b"a\rb\rc");
    assert_eq!(encode_paste("é", &plain()), "é".as_bytes());
}

fn mouse(kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers,
    }
}

fn mouse_modes(sgr: bool, mode: MouseMode) -> EncodeModes {
    EncodeModes {
        term: TermModes {
            mouse: mode,
            sgr_mouse: sgr,
            ..TermModes::default()
        },
        ..plain()
    }
}

#[test]
fn mouse_wheel() {
    let on = mouse_modes(true, MouseMode::Press);
    let up = mouse(MouseEventKind::ScrollUp, 10, 5, NONE);
    let down = mouse(MouseEventKind::ScrollDown, 10, 5, NONE);
    assert_eq!(
        encode_mouse(&up, (0, 0), &on).as_deref(),
        Some(&b"\x1b[<64;11;6M"[..])
    );
    assert_eq!(
        encode_mouse(&down, (0, 0), &on).as_deref(),
        Some(&b"\x1b[<65;11;6M"[..])
    );
    // Relative to the panel origin.
    assert_eq!(
        encode_mouse(&up, (4, 2), &on).as_deref(),
        Some(&b"\x1b[<64;7;4M"[..])
    );
    // Modifiers add to the button code.
    let ctrl_up = mouse(MouseEventKind::ScrollUp, 0, 0, CTRL);
    assert_eq!(
        encode_mouse(&ctrl_up, (0, 0), &on).as_deref(),
        Some(&b"\x1b[<80;1;1M"[..])
    );
    // Any non-None mouse mode works.
    let any = mouse_modes(true, MouseMode::AnyMotion);
    assert!(encode_mouse(&up, (0, 0), &any).is_some());
}

#[test]
fn mouse_ignored() {
    let up = mouse(MouseEventKind::ScrollUp, 10, 5, NONE);
    assert_eq!(
        encode_mouse(&up, (0, 0), &mouse_modes(true, MouseMode::None)),
        None
    );
    assert_eq!(
        encode_mouse(&up, (0, 0), &mouse_modes(false, MouseMode::Press)),
        None
    );
    assert_eq!(encode_mouse(&up, (0, 0), &plain()), None);
    // Outside the panel (left of / above the origin).
    let on = mouse_modes(true, MouseMode::Press);
    assert_eq!(encode_mouse(&up, (11, 0), &on), None);
    assert_eq!(encode_mouse(&up, (0, 6), &on), None);
    // Non-wheel events are not forwarded.
    let click = mouse(MouseEventKind::Down(MouseButton::Left), 1, 1, NONE);
    assert_eq!(encode_mouse(&click, (0, 0), &on), None);
    let mv = mouse(MouseEventKind::Moved, 1, 1, NONE);
    assert_eq!(encode_mouse(&mv, (0, 0), &on), None);
}

#[test]
fn focus() {
    let on = EncodeModes {
        term: TermModes {
            focus_reporting: true,
            ..TermModes::default()
        },
        ..plain()
    };
    assert_eq!(encode_focus(true, &on).as_deref(), Some(&b"\x1b[I"[..]));
    assert_eq!(encode_focus(false, &on).as_deref(), Some(&b"\x1b[O"[..]));
    assert_eq!(encode_focus(true, &plain()), None);
    assert_eq!(encode_focus(false, &plain()), None);
}
