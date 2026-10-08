//! Incremental scanner over PTY output bytes.

/// Mouse reporting mode requested by the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseMode {
    #[default]
    None,
    /// `?1000`: button press/release.
    Press,
    /// `?1002`: plus drag motion.
    ButtonMotion,
    /// `?1003`: all motion.
    AnyMotion,
}

/// Terminal modes that `vt100` does not expose.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TermModes {
    pub modify_other_keys: u8,
    pub kitty_flags: u8,
    pub sync_output: bool,
    pub focus_reporting: bool,
    pub mouse: MouseMode,
    pub sgr_mouse: bool,
    pub alt_screen: bool,
}

const MAX_CSI_LEN: usize = 128;
const MAX_KITTY_STACK: usize = 64;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    Esc,
    Csi,
    /// Inside OSC (`bel_ends` true) or DCS/SOS/PM/APC.
    Str {
        bel_ends: bool,
    },
    /// Saw ESC inside a string; `\` terminates it.
    StrEsc {
        bel_ends: bool,
    },
}

/// Incremental escape-sequence scanner.
///
/// Feed it PTY output in arbitrary chunks; sequences may be split anywhere.
#[derive(Debug, Default)]
pub struct Scanner {
    modes: TermModes,
    state: State,
    /// Bytes of the CSI in progress (after `ESC [`).
    csi: Vec<u8>,
    /// Set when the CSI in progress grew too long and must be ignored.
    csi_overflow: bool,
    kitty_stack: Vec<u8>,
}

impl Scanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Current tracked modes.
    pub fn modes(&self) -> &TermModes {
        &self.modes
    }

    /// Consume `bytes`, updating modes, and return the concatenated replies to
    /// any answerable queries. `cursor` is the 1-based `(row, col)` used for
    /// `CSI 6n`.
    pub fn feed(&mut self, bytes: &[u8], cursor: (u16, u16)) -> Vec<u8> {
        let mut replies = Vec::new();
        for &b in bytes {
            self.step(b, cursor, &mut replies);
        }
        replies
    }

    fn step(&mut self, b: u8, cursor: (u16, u16), replies: &mut Vec<u8>) {
        match self.state {
            State::Ground => {
                if b == 0x1b {
                    self.state = State::Esc;
                }
            }
            State::Esc => match b {
                b'[' => {
                    self.csi.clear();
                    self.csi_overflow = false;
                    self.state = State::Csi;
                }
                b']' => self.state = State::Str { bel_ends: true },
                b'P' | b'X' | b'^' | b'_' => self.state = State::Str { bel_ends: false },
                0x1b => {}
                _ => self.state = State::Ground,
            },
            State::Csi => match b {
                0x1b => self.state = State::Esc,
                0x18 | 0x1a => self.state = State::Ground,
                0x40..=0x7e => {
                    self.state = State::Ground;
                    if !self.csi_overflow {
                        let csi = std::mem::take(&mut self.csi);
                        self.dispatch_csi(&csi, b, cursor, replies);
                        self.csi = csi;
                    }
                }
                _ => {
                    if self.csi.len() < MAX_CSI_LEN {
                        self.csi.push(b);
                    } else {
                        self.csi_overflow = true;
                    }
                }
            },
            State::Str { bel_ends } => match b {
                0x07 if bel_ends => self.state = State::Ground,
                0x1b => self.state = State::StrEsc { bel_ends },
                0x18 | 0x1a => self.state = State::Ground,
                _ => {}
            },
            State::StrEsc { bel_ends } => match b {
                b'\\' => self.state = State::Ground,
                0x1b => {}
                // Anything else stays inside the string: its contents are never
                // interpreted, so embedded escapes cannot toggle modes.
                _ => self.state = State::Str { bel_ends },
            },
        }
    }

    fn dispatch_csi(&mut self, raw: &[u8], final_byte: u8, cursor: (u16, u16), out: &mut Vec<u8>) {
        let private = raw.first().copied().filter(|b| (0x3c..=0x3f).contains(b));
        let rest = if private.is_some() { &raw[1..] } else { raw };
        let split = rest
            .iter()
            .position(|b| (0x20..=0x2f).contains(b))
            .unwrap_or(rest.len());
        let (params, intermediates) = rest.split_at(split);

        if let Some(reply) =
            super::responder::reply_for_csi(private, params, intermediates, final_byte, cursor)
        {
            out.extend(reply);
            return;
        }
        if !intermediates.is_empty() {
            return;
        }
        let nums = parse_params(params);
        match (private, final_byte) {
            (Some(b'?'), b'h') => nums.iter().for_each(|&n| self.set_private_mode(n, true)),
            (Some(b'?'), b'l') => nums.iter().for_each(|&n| self.set_private_mode(n, false)),
            (Some(b'>'), b'u') => self.kitty_push(nums.first().copied().flatten().unwrap_or(0)),
            (Some(b'<'), b'u') => self.kitty_pop(nums.first().copied().flatten().unwrap_or(1)),
            (Some(b'='), b'u') => self.kitty_set(&nums),
            (Some(b'>'), b'm') if nums.first().copied().flatten() == Some(4) => {
                let level = nums.get(1).copied().flatten().unwrap_or(0);
                self.modes.modify_other_keys = u8::try_from(level).unwrap_or(0);
            }
            _ => {}
        }
    }

    fn set_private_mode(&mut self, mode: Option<u32>, on: bool) {
        let m = &mut self.modes;
        match mode {
            Some(1000) => set_mouse(m, MouseMode::Press, on),
            Some(1002) => set_mouse(m, MouseMode::ButtonMotion, on),
            Some(1003) => set_mouse(m, MouseMode::AnyMotion, on),
            Some(1004) => m.focus_reporting = on,
            Some(1006) => m.sgr_mouse = on,
            Some(47 | 1047 | 1049) => m.alt_screen = on,
            Some(2026) => m.sync_output = on,
            _ => {}
        }
    }

    fn kitty_push(&mut self, flags: u32) {
        if self.kitty_stack.len() >= MAX_KITTY_STACK {
            self.kitty_stack.remove(0);
        }
        self.kitty_stack.push(u8::try_from(flags).unwrap_or(0));
        self.sync_kitty();
    }

    fn kitty_pop(&mut self, n: u32) {
        let n = usize::try_from(n).unwrap_or(usize::MAX);
        let keep = self.kitty_stack.len().saturating_sub(n);
        self.kitty_stack.truncate(keep);
        self.sync_kitty();
    }

    /// `CSI = flags ; mode u`: 1 = set (default), 2 = OR, 3 = clear bits.
    fn kitty_set(&mut self, nums: &[Option<u32>]) {
        let flags = u8::try_from(nums.first().copied().flatten().unwrap_or(0)).unwrap_or(0);
        let mode = nums.get(1).copied().flatten().unwrap_or(1);
        if self.kitty_stack.is_empty() {
            self.kitty_stack.push(0);
        }
        if let Some(top) = self.kitty_stack.last_mut() {
            match mode {
                1 => *top = flags,
                2 => *top |= flags,
                3 => *top &= !flags,
                _ => {}
            }
        }
        self.sync_kitty();
    }

    fn sync_kitty(&mut self) {
        self.modes.kitty_flags = self.kitty_stack.last().copied().unwrap_or(0);
    }
}

fn set_mouse(m: &mut TermModes, which: MouseMode, on: bool) {
    if on {
        m.mouse = which;
    } else if m.mouse == which {
        m.mouse = MouseMode::None;
    }
}

/// Split `a;b;c` into numbers; empty or non-numeric fields become `None`.
fn parse_params(raw: &[u8]) -> Vec<Option<u32>> {
    raw.split(|&b| b == b';')
        .map(|f| {
            std::str::from_utf8(f)
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE_START: &[u8] = b"\x1b[<u\x1b[>5u\x1b[?u\x1b[>4;2m\x1b[>0q\x1b]7501;?\x1b\\\x1b[c\x1b[?1004h\x1b[?2004h\x1b[?1049h\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b]0;title\x07\x1b[?2026h";

    fn run(chunks: &[&[u8]]) -> (TermModes, Vec<u8>) {
        let mut s = Scanner::new();
        let mut out = Vec::new();
        for c in chunks {
            out.extend(s.feed(c, (3, 7)));
        }
        (s.modes().clone(), out)
    }

    #[test]
    fn claude_startup_sets_modes() {
        let (m, r) = run(&[CLAUDE_START]);
        assert_eq!(m.kitty_flags, 5);
        assert_eq!(m.modify_other_keys, 2);
        assert!(m.sync_output && m.focus_reporting && m.alt_screen && m.sgr_mouse);
        assert_eq!(m.mouse, MouseMode::ButtonMotion);
        assert_eq!(r, b"\x1b[?62;22c");
    }

    #[test]
    fn reset_sequence_clears_modes() {
        let mut s = Scanner::new();
        s.feed(CLAUDE_START, (1, 1));
        s.feed(b"\x1b[<u\x1b[>4m\x1b[?2026l", (1, 1));
        assert_eq!(s.modes().kitty_flags, 0);
        assert_eq!(s.modes().modify_other_keys, 0);
        assert!(!s.modes().sync_output);
    }

    #[test]
    fn fresh_reset_example() {
        let (m, _) = run(&[b"\x1b[<u\x1b[>4m\x1b[?2026l"]);
        assert_eq!(
            (m.kitty_flags, m.modify_other_keys, m.sync_output),
            (0, 0, false)
        );
    }

    #[test]
    fn kitty_stack_push_pop() {
        let (m, _) = run(&[b"\x1b[>1u\x1b[>5u\x1b[<u"]);
        assert_eq!(m.kitty_flags, 1);
        let (m, _) = run(&[b"\x1b[>1u\x1b[>5u\x1b[<2u"]);
        assert_eq!(m.kitty_flags, 0);
    }

    #[test]
    fn mouse_modes_toggle() {
        let (m, _) = run(&[b"\x1b[?1000h\x1b[?1003h\x1b[?1006h"]);
        assert_eq!(m.mouse, MouseMode::AnyMotion);
        let (m, _) = run(&[b"\x1b[?1000h\x1b[?1003h\x1b[?1003l\x1b[?1006h\x1b[?1006l"]);
        assert_eq!(m.mouse, MouseMode::None);
        assert!(!m.sgr_mouse);
    }

    #[test]
    fn multi_param_private_modes() {
        let (m, _) = run(&[b"\x1b[?1049;1004h\x1b[?1049l"]);
        assert!(m.focus_reporting && !m.alt_screen);
    }

    #[test]
    fn replies() {
        assert_eq!(run(&[b"\x1b[c"]).1, b"\x1b[?62;22c");
        assert_eq!(run(&[b"\x1b[0c"]).1, b"\x1b[?62;22c");
        assert_eq!(run(&[b"\x1b[5n"]).1, b"\x1b[0n");
        assert_eq!(run(&[b"\x1b[6n"]).1, b"\x1b[3;7R");
        assert_eq!(run(&[b"a\x1b[cb\x1b[c"]).1.len(), 2 * 9);
    }

    #[test]
    fn unanswered_queries() {
        for q in [
            &b"\x1b[?u"[..],
            b"\x1b[>0q",
            b"\x1b]7501;?\x1b\\",
            b"\x1b]7501;?\x07",
            b"\x1b[>c",
            b"\x1b[?6n",
        ] {
            assert!(run(&[q]).1.is_empty(), "{q:?}");
        }
    }

    #[test]
    fn string_contents_not_misparsed() {
        let (m, r) = run(&[
            b"\x1b]0;\x1b[?1049h\x1b[c\x07",
            b"\x1bP1$r\x1b[?2026h\x1b[c\x1b\\",
            b"\x1b_x\x1b[?1004h\x1b\\\x1bXjunk\x1b[c\x1b\\",
        ]);
        assert_eq!(m, TermModes::default());
        assert!(r.is_empty());
    }

    #[test]
    fn state_recovers_after_string() {
        let (m, r) = run(&[b"\x1b]0;t\x1b\\\x1b[?2026h\x1b[c"]);
        assert!(m.sync_output);
        assert_eq!(r, b"\x1b[?62;22c");
    }

    #[test]
    fn every_split_matches_single_feed() {
        let mut inputs: Vec<&[u8]> = vec![CLAUDE_START, b"\x1b[6n\x1b[5n\x1b[0c\x1b[>1u\x1b[<u"];
        inputs.push(b"\x1b]0;a\x1b[c\x07\x1bP\x1b[?1004h\x1b\\\x1b[?1049h\x1b[c");
        for input in inputs {
            let whole = run(&[input]);
            for i in 0..=input.len() {
                let split = run(&[&input[..i], &input[i..]]);
                assert_eq!(split, whole, "split at {i} of {input:?}");
            }
        }
    }
}
