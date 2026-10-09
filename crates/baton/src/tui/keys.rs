//! Terminal key events to canonical [`KeySpec`]s.

use baton_core::keymap::{Key, KeySpec, Mods};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The canonical key a terminal event stands for, or `None` for events that
/// cannot be bound (modifier-only keys, SUPER/HYPER/META combinations, ...).
///
/// `G` matches with or without SHIFT, and Ctrl-\ matches both forms crossterm
/// reports for it (`Ctrl-\` and, from the legacy byte 0x1c, `Ctrl-4`).
pub fn from_event(ev: &KeyEvent) -> Option<KeySpec> {
    let m = ev.modifiers;
    if m.intersects(KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META) {
        return None;
    }
    let mods = Mods {
        ctrl: m.contains(KeyModifiers::CONTROL),
        alt: m.contains(KeyModifiers::ALT),
        shift: m.contains(KeyModifiers::SHIFT),
    };
    let key = match ev.code {
        KeyCode::Char(c) => return Some(KeySpec::from_crossterm_char(c, mods)),
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Insert => Key::Insert,
        KeyCode::F(n) => Key::F(n),
        _ => return None,
    };
    Some(KeySpec::new(key, mods))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn ev(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }

    fn spec(s: &str) -> KeySpec {
        s.parse().expect("key spec")
    }

    const NONE: KeyModifiers = KeyModifiers::NONE;

    #[test]
    fn events_match_specs() {
        let cases = [
            (ev(KeyCode::Char('n'), NONE), "n"),
            (ev(KeyCode::Char('G'), NONE), "G"),
            (ev(KeyCode::Char('G'), KeyModifiers::SHIFT), "G"),
            (ev(KeyCode::Char('?'), KeyModifiers::SHIFT), "?"),
            (ev(KeyCode::Enter, NONE), "enter"),
            (ev(KeyCode::Esc, NONE), "esc"),
            (ev(KeyCode::Tab, NONE), "tab"),
            (ev(KeyCode::BackTab, KeyModifiers::SHIFT), "shift-tab"),
            (ev(KeyCode::Tab, KeyModifiers::SHIFT), "shift-tab"),
            (ev(KeyCode::Up, NONE), "up"),
            (ev(KeyCode::PageUp, NONE), "pgup"),
            (ev(KeyCode::F(5), NONE), "f5"),
            (ev(KeyCode::Char('u'), KeyModifiers::CONTROL), "ctrl-u"),
            (ev(KeyCode::Char('n'), KeyModifiers::ALT), "alt-n"),
            (ev(KeyCode::Char('1'), KeyModifiers::ALT), "alt-1"),
            // Both crossterm forms of Ctrl-\.
            (ev(KeyCode::Char('\\'), KeyModifiers::CONTROL), "ctrl-\\"),
            (ev(KeyCode::Char('4'), KeyModifiers::CONTROL), "ctrl-\\"),
            (
                ev(
                    KeyCode::Char('g'),
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                ),
                "ctrl-g",
            ),
        ];
        for (event, want) in cases {
            assert_eq!(from_event(&event), Some(spec(want)), "{event:?} vs {want}");
        }
    }

    #[test]
    fn a_plain_4_is_not_ctrl_backslash() {
        assert_ne!(
            from_event(&ev(KeyCode::Char('4'), NONE)),
            Some(spec("ctrl-\\"))
        );
    }

    #[test]
    fn unsupported_events_have_no_spec() {
        assert_eq!(
            from_event(&ev(KeyCode::Char('x'), KeyModifiers::SUPER)),
            None
        );
        assert_eq!(from_event(&ev(KeyCode::CapsLock, NONE)), None);
        assert_eq!(from_event(&ev(KeyCode::Null, NONE)), None);
    }
}
