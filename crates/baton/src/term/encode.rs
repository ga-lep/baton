//! Converts crossterm input events into the bytes a terminal application expects.

use baton_core::term::{MouseMode, TermModes};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseEvent, MouseEventKind,
};

/// Terminal modes of the target application that affect input encoding.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EncodeModes {
    /// DECCKM: cursor keys send `SS3` instead of `CSI`.
    pub app_cursor: bool,
    /// DECKPAM: keypad keys send `SS3` sequences.
    pub app_keypad: bool,
    /// `?2004`: pasted text is bracketed.
    pub bracketed_paste: bool,
    /// Modes tracked from the application's output stream.
    pub term: TermModes,
}

const ESC: u8 = 0x1b;
const PASTE_END: &str = "\x1b[201~";

/// xterm modifier parameter (`1 + shift/alt/ctrl bits`), or `None` when unmodified.
fn mod_param(m: KeyModifiers) -> Option<u8> {
    let mut bits = 0;
    if m.contains(KeyModifiers::SHIFT) {
        bits |= 1;
    }
    if m.contains(KeyModifiers::ALT) {
        bits |= 2;
    }
    if m.contains(KeyModifiers::CONTROL) {
        bits |= 4;
    }
    (bits != 0).then_some(bits + 1)
}

/// `ESC [ 27 ; mod ; code ~` (xterm modifyOtherKeys format).
fn other_keys(param: u8, code: u32) -> Vec<u8> {
    format!("\x1b[27;{param};{code}~").into_bytes()
}

/// Prefix `bytes` with ESC when Alt is held.
fn meta(alt: bool, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 1);
    if alt {
        out.push(ESC);
    }
    out.extend_from_slice(bytes);
    out
}

/// `CSI`/`SS3` sequence ending in a letter: `ESC [ A`, `ESC O A` or `ESC [ 1 ; m A`.
fn letter_key(final_byte: char, mods: Option<u8>, ss3: bool) -> Vec<u8> {
    match mods {
        Some(m) => format!("\x1b[1;{m}{final_byte}"),
        None if ss3 => format!("\x1bO{final_byte}"),
        None => format!("\x1b[{final_byte}"),
    }
    .into_bytes()
}

/// `CSI n ~` or `CSI n ; m ~`.
fn tilde_key(n: u8, mods: Option<u8>) -> Vec<u8> {
    match mods {
        Some(m) => format!("\x1b[{n};{m}~"),
        None => format!("\x1b[{n}~"),
    }
    .into_bytes()
}

fn encode_enter(mods: KeyModifiers, modes: &EncodeModes) -> Vec<u8> {
    let t = &modes.term;
    let Some(p) = mod_param(mods) else {
        return b"\r".to_vec();
    };
    if mods.contains(KeyModifiers::SHIFT) {
        // modifyOtherKeys is honoured before kitty: Claude negotiates kitty
        // but Baton never answers its query, so the xterm form is the safe one.
        if t.modify_other_keys >= 1 {
            other_keys(p, 13)
        } else if t.kitty_flags & 1 != 0 {
            format!("\x1b[13;{p}u").into_bytes()
        } else {
            // Meta-enter, which Claude treats as a newline.
            meta(true, b"\r")
        }
    } else if mods.contains(KeyModifiers::CONTROL) && t.modify_other_keys == 2 {
        other_keys(p, 13)
    } else {
        meta(mods.contains(KeyModifiers::ALT), b"\r")
    }
}

fn encode_char(ch: char, mods: KeyModifiers, modes: &EncodeModes) -> Vec<u8> {
    let alt = mods.contains(KeyModifiers::ALT);
    if mods.contains(KeyModifiers::CONTROL) {
        if ch.is_ascii_alphabetic() {
            return meta(alt, &[ch.to_ascii_lowercase() as u8 - b'a' + 1]);
        }
        let legacy = match ch {
            ' ' | '@' => Some(0x00),
            '[' => Some(0x1b),
            '\\' => Some(0x1c),
            ']' => Some(0x1d),
            '^' => Some(0x1e),
            '_' => Some(0x1f),
            '?' => Some(0x7f),
            _ => None,
        };
        if modes.term.modify_other_keys == 2
            && !ch.is_ascii_alphanumeric()
            && let Some(p) = mod_param(mods)
        {
            return other_keys(p, u32::from(ch));
        }
        if let Some(b) = legacy {
            return meta(alt, &[b]);
        }
    }
    let mut buf = [0u8; 4];
    meta(alt, ch.encode_utf8(&mut buf).as_bytes())
}

fn encode_keypad(code: KeyCode) -> Option<Vec<u8>> {
    let b = match code {
        KeyCode::Char(c @ '0'..='9') => b'p' + (c as u8 - b'0'),
        KeyCode::Char('*') => b'j',
        KeyCode::Char('+') => b'k',
        KeyCode::Char(',') => b'l',
        KeyCode::Char('-') => b'm',
        KeyCode::Char('.') => b'n',
        KeyCode::Char('/') => b'o',
        KeyCode::Char('=') => b'X',
        KeyCode::Enter => b'M',
        _ => return None,
    };
    Some(vec![ESC, b'O', b])
}

/// Encode a key event for the application, or `None` if it sends nothing.
///
/// Releases and keys without a terminal encoding return `None`.
pub fn encode_key(ev: &KeyEvent, modes: &EncodeModes) -> Option<Vec<u8>> {
    if ev.kind == KeyEventKind::Release {
        return None;
    }
    let mods = ev.modifiers;
    let alt = mods.contains(KeyModifiers::ALT);
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    let shift = mods.contains(KeyModifiers::SHIFT);
    let param = mod_param(mods);
    if modes.app_keypad
        && ev.state.contains(KeyEventState::KEYPAD)
        && param.is_none()
        && let Some(bytes) = encode_keypad(ev.code)
    {
        return Some(bytes);
    }
    Some(match ev.code {
        KeyCode::Char(ch) => encode_char(ch, mods, modes),
        KeyCode::Enter => encode_enter(mods, modes),
        KeyCode::Tab if shift => meta(alt, b"\x1b[Z"),
        KeyCode::BackTab => meta(alt, b"\x1b[Z"),
        KeyCode::Tab => match param {
            Some(p) if ctrl && modes.term.modify_other_keys == 2 => other_keys(p, 9),
            _ => meta(alt, b"\t"),
        },
        KeyCode::Backspace => meta(alt, if ctrl { b"\x08" } else { b"\x7f" }),
        KeyCode::Esc => meta(alt, b"\x1b"),
        KeyCode::Null => vec![0],
        KeyCode::Up => letter_key('A', param, modes.app_cursor),
        KeyCode::Down => letter_key('B', param, modes.app_cursor),
        KeyCode::Right => letter_key('C', param, modes.app_cursor),
        KeyCode::Left => letter_key('D', param, modes.app_cursor),
        KeyCode::Home => letter_key('H', param, modes.app_cursor),
        KeyCode::End => letter_key('F', param, modes.app_cursor),
        KeyCode::Insert => tilde_key(2, param),
        KeyCode::Delete => tilde_key(3, param),
        KeyCode::PageUp => tilde_key(5, param),
        KeyCode::PageDown => tilde_key(6, param),
        KeyCode::F(n @ 1..=4) => letter_key(char::from(b'P' + n - 1), param, true),
        KeyCode::F(n @ 5..=12) => {
            const CODES: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
            tilde_key(CODES[usize::from(n - 5)], param)
        }
        _ => return None,
    })
}

/// Encode pasted text. Bracketed when the application asked for it (with any
/// embedded end marker removed), otherwise newlines become carriage returns.
pub fn encode_paste(text: &str, modes: &EncodeModes) -> Vec<u8> {
    if modes.bracketed_paste {
        let mut clean = text.to_owned();
        // Repeat so removal cannot splice a new marker together.
        while clean.contains(PASTE_END) {
            clean = clean.replace(PASTE_END, "");
        }
        let mut out = b"\x1b[200~".to_vec();
        out.extend_from_slice(clean.as_bytes());
        out.extend_from_slice(PASTE_END.as_bytes());
        out
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

/// Encode a mouse wheel event as SGR, relative to the panel `origin`
/// (0-based `(column, row)` of its top-left cell).
///
/// Returns `None` unless the application enabled mouse reporting and SGR
/// encoding, the event is a wheel event, and it lies inside the panel.
pub fn encode_mouse(ev: &MouseEvent, origin: (u16, u16), modes: &EncodeModes) -> Option<Vec<u8>> {
    let t = &modes.term;
    if t.mouse == MouseMode::None || !t.sgr_mouse {
        return None;
    }
    let mut button: u16 = match ev.kind {
        MouseEventKind::ScrollUp => 64,
        MouseEventKind::ScrollDown => 65,
        _ => return None,
    };
    let x = ev.column.checked_sub(origin.0)?.checked_add(1)?;
    let y = ev.row.checked_sub(origin.1)?.checked_add(1)?;
    if ev.modifiers.contains(KeyModifiers::SHIFT) {
        button += 4;
    }
    if ev.modifiers.contains(KeyModifiers::ALT) {
        button += 8;
    }
    if ev.modifiers.contains(KeyModifiers::CONTROL) {
        button += 16;
    }
    Some(format!("\x1b[<{button};{x};{y}M").into_bytes())
}

/// Encode a focus change, only when the application enabled focus reporting.
pub fn encode_focus(focused: bool, modes: &EncodeModes) -> Option<Vec<u8>> {
    modes.term.focus_reporting.then(|| {
        if focused {
            b"\x1b[I".to_vec()
        } else {
            b"\x1b[O".to_vec()
        }
    })
}
