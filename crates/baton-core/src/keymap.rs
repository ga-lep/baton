//! Key strings, actions and the keymap (defaults plus config overrides).
//!
//! Key strings look like `n`, `G`, `enter`, `ctrl-u`, `alt-1`, `shift-tab`.
//! Every [`KeySpec`] is stored in a canonical form so that matching a
//! terminal key event is plain equality: see [`KeySpec::new`].

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

/// A key without modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// A printable character (case significant).
    Char(char),
    /// Enter / Return.
    Enter,
    /// Escape.
    Esc,
    /// Tab.
    Tab,
    /// Shift-Tab.
    BackTab,
    /// Cursor up.
    Up,
    /// Cursor down.
    Down,
    /// Cursor left.
    Left,
    /// Cursor right.
    Right,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Home.
    Home,
    /// End.
    End,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Insert.
    Insert,
    /// Function key `F1`..`F24`.
    F(u8),
}

/// Modifier keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Mods {
    /// Control.
    pub ctrl: bool,
    /// Alt / Meta.
    pub alt: bool,
    /// Shift.
    pub shift: bool,
}

/// A key press in canonical form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeySpec {
    key: Key,
    mods: Mods,
}

/// Why a key string was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct KeyParseError(String);

impl KeySpec {
    /// Canonicalises a key and its modifiers.
    ///
    /// - `Char`: SHIFT is dropped (the character already carries it; a
    ///   lowercase letter with SHIFT becomes uppercase); with CTRL the letter
    ///   is lowercased (terminals cannot tell Ctrl-U from Ctrl-Shift-U).
    /// - `Tab` with SHIFT and `BackTab` are the same key.
    ///
    /// ```
    /// use baton_core::keymap::{Key, KeySpec, Mods};
    /// let shifted = Mods { shift: true, ..Mods::default() };
    /// assert_eq!(KeySpec::new(Key::Char('G'), shifted), "G".parse().unwrap());
    /// ```
    pub fn new(key: Key, mods: Mods) -> Self {
        match key {
            Key::Char(c) => {
                let c = if mods.ctrl {
                    c.to_ascii_lowercase()
                } else if mods.shift {
                    c.to_ascii_uppercase()
                } else {
                    c
                };
                Self {
                    key: Key::Char(c),
                    mods: Mods {
                        shift: false,
                        ..mods
                    },
                }
            }
            Key::Tab | Key::BackTab if mods.shift || key == Key::BackTab => Self {
                key: Key::BackTab,
                mods: Mods {
                    shift: false,
                    ..mods
                },
            },
            _ => Self { key, mods },
        }
    }

    /// Like [`KeySpec::new`] for a character key as crossterm reports it:
    /// legacy terminals send Ctrl-\ as byte 0x1c, which crossterm decodes as
    /// Ctrl-4, so that form is mapped back to Ctrl-\.
    pub fn from_crossterm_char(c: char, mods: Mods) -> Self {
        let c = if mods.ctrl && c == '4' { '\\' } else { c };
        Self::new(Key::Char(c), mods)
    }

    /// The key.
    pub fn key(&self) -> Key {
        self.key
    }

    /// The modifiers.
    pub fn mods(&self) -> Mods {
        self.mods
    }

    /// Whether this is a bare printable character, which in focus mode would
    /// swallow typing meant for the application.
    pub fn is_plain_printable(&self) -> bool {
        matches!(self.key, Key::Char(_)) && !self.mods.ctrl && !self.mods.alt
    }
}

fn key_name(key: Key) -> String {
    match key {
        Key::Char(' ') => "space".to_owned(),
        Key::Char(c) => c.to_string(),
        Key::Enter => "enter".to_owned(),
        Key::Esc => "esc".to_owned(),
        Key::Tab => "tab".to_owned(),
        Key::BackTab => "shift-tab".to_owned(),
        Key::Up => "up".to_owned(),
        Key::Down => "down".to_owned(),
        Key::Left => "left".to_owned(),
        Key::Right => "right".to_owned(),
        Key::PageUp => "pgup".to_owned(),
        Key::PageDown => "pgdn".to_owned(),
        Key::Home => "home".to_owned(),
        Key::End => "end".to_owned(),
        Key::Backspace => "backspace".to_owned(),
        Key::Delete => "delete".to_owned(),
        Key::Insert => "insert".to_owned(),
        Key::F(n) => format!("f{n}"),
    }
}

impl fmt::Display for KeySpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mods.ctrl {
            f.write_str("ctrl-")?;
        }
        if self.mods.alt {
            f.write_str("alt-")?;
        }
        if self.mods.shift {
            f.write_str("shift-")?;
        }
        f.write_str(&key_name(self.key))
    }
}

fn parse_named(name: &str) -> Option<Key> {
    let lower = name.to_ascii_lowercase();
    Some(match lower.as_str() {
        "enter" | "return" => Key::Enter,
        "esc" | "escape" => Key::Esc,
        "tab" => Key::Tab,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "pgup" | "pageup" => Key::PageUp,
        "pgdn" | "pgdown" | "pagedown" => Key::PageDown,
        "home" => Key::Home,
        "end" => Key::End,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "insert" | "ins" => Key::Insert,
        "space" => Key::Char(' '),
        other => {
            let n: u8 = other.strip_prefix('f')?.parse().ok()?;
            if !(1..=24).contains(&n) {
                return None;
            }
            Key::F(n)
        }
    })
}

impl FromStr for KeySpec {
    type Err = KeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = |why: &str| KeyParseError(format!("invalid key \"{s}\": {why}"));
        let mut rest = s;
        let mut mods = Mods::default();
        'prefixes: loop {
            for (prefix, flag) in [("ctrl-", 0), ("alt-", 1), ("shift-", 2)] {
                let n = prefix.len();
                if rest.len() > n
                    && rest.is_char_boundary(n)
                    && rest[..n].eq_ignore_ascii_case(prefix)
                {
                    let slot = match flag {
                        0 => &mut mods.ctrl,
                        1 => &mut mods.alt,
                        _ => &mut mods.shift,
                    };
                    if *slot {
                        return Err(bad("repeated modifier"));
                    }
                    *slot = true;
                    rest = &rest[n..];
                    continue 'prefixes;
                }
            }
            break;
        }
        let mut chars = rest.chars();
        let key = match (chars.next(), chars.next()) {
            (None, _) => return Err(bad("empty key")),
            (Some(c), None) => Key::Char(c),
            _ => parse_named(rest).ok_or_else(|| bad("unknown key name"))?,
        };
        if mods.shift && matches!(key, Key::Char(c) if !c.is_ascii_alphabetic()) {
            return Err(bad(
                "shift- only applies to letters (write the shifted character)",
            ));
        }
        Ok(Self::new(key, mods))
    }
}

/// Why a keymap section of the config was rejected. Messages name the config key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeymapError {
    /// `[keybindings.<mode>]` with a mode other than `normal` / `focus`.
    #[error("keybindings.{mode}: unknown mode (expected \"normal\" or \"focus\")")]
    UnknownMode {
        /// The mode as written.
        mode: String,
    },
    /// An action name that does not exist in that mode.
    #[error("keybindings.{mode}.{action}: unknown action")]
    UnknownAction {
        /// Mode table.
        mode: &'static str,
        /// The action as written.
        action: String,
    },
    /// An unparseable key string.
    #[error("keybindings.{mode}.{action}: {source}")]
    BadKey {
        /// Mode table.
        mode: &'static str,
        /// Action name.
        action: String,
        /// Parse failure.
        source: KeyParseError,
    },
    /// Two actions share a key in one mode.
    #[error("keybindings.{mode}: key \"{key}\" is bound to both {first} and {second}")]
    Conflict {
        /// Mode table.
        mode: &'static str,
        /// The shared key.
        key: String,
        /// First action.
        first: String,
        /// Second action.
        second: String,
    },
    /// `focus.unfocus` has no key, leaving no way out of focus mode.
    #[error(
        "keybindings.focus.unfocus: needs at least one key, otherwise there is no way out of focus mode"
    )]
    UnfocusUnbound,
    /// `focus.unfocus` is a plain printable key.
    #[error(
        "keybindings.focus.unfocus: \"{key}\" is a plain printable key and would swallow typing in focus mode; use a modified or non-character key such as ctrl-g"
    )]
    UnfocusPrintable {
        /// The offending key.
        key: String,
    },
}

/// An action name <-> value mapping with default keys.
trait Action: Copy + PartialEq {
    const MODE: &'static str;
    fn all() -> Vec<Self>;
    fn name(self) -> String;
    fn default_keys(self) -> Vec<&'static str>;
}

/// An action in normal mode (sidebar has focus).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalAction {
    /// Move the selection down.
    MoveDown,
    /// Move the selection up.
    MoveUp,
    /// Focus the session, or open a closed project.
    Activate,
    /// Open the project under the cursor.
    OpenProject,
    /// Select session `n` (1..=9) of the current project.
    Select(u8),
    /// Jump to the next session needing attention.
    NextAttention,
    /// Restart the selected session.
    Restart,
    /// Open the repo in the editor.
    Editor,
    /// Scroll half a page back.
    ScrollUp,
    /// Scroll half a page forward.
    ScrollDown,
    /// Scroll a page back.
    PageUp,
    /// Scroll a page forward.
    PageDown,
    /// Snap to the live bottom.
    Live,
    /// Show the help overlay.
    Help,
    /// Quit the TUI.
    Quit,
}

impl Action for NormalAction {
    const MODE: &'static str = "normal";

    fn all() -> Vec<Self> {
        use NormalAction::{
            Activate, Editor, Help, Live, MoveDown, MoveUp, NextAttention, OpenProject, PageDown,
            PageUp, Quit, Restart, ScrollDown, ScrollUp, Select,
        };
        let mut v = vec![MoveDown, MoveUp, Activate, OpenProject];
        v.extend((1..=9).map(Select));
        v.extend([
            NextAttention,
            Restart,
            Editor,
            ScrollUp,
            ScrollDown,
            PageUp,
            PageDown,
            Live,
            Help,
            Quit,
        ]);
        v
    }

    fn name(self) -> String {
        match self {
            Self::MoveDown => "move_down".to_owned(),
            Self::MoveUp => "move_up".to_owned(),
            Self::Activate => "activate".to_owned(),
            Self::OpenProject => "open_project".to_owned(),
            Self::Select(n) => format!("select_{n}"),
            Self::NextAttention => "next_attention".to_owned(),
            Self::Restart => "restart".to_owned(),
            Self::Editor => "editor".to_owned(),
            Self::ScrollUp => "scroll_up".to_owned(),
            Self::ScrollDown => "scroll_down".to_owned(),
            Self::PageUp => "page_up".to_owned(),
            Self::PageDown => "page_down".to_owned(),
            Self::Live => "live".to_owned(),
            Self::Help => "help".to_owned(),
            Self::Quit => "quit".to_owned(),
        }
    }

    fn default_keys(self) -> Vec<&'static str> {
        const DIGITS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
        match self {
            Self::MoveDown => vec!["j", "down"],
            Self::MoveUp => vec!["k", "up"],
            Self::Activate => vec!["enter", "l"],
            Self::OpenProject => vec!["o"],
            Self::Select(n) => DIGITS
                .get(usize::from(n).wrapping_sub(1))
                .map_or_else(Vec::new, |d| vec![*d]),
            Self::NextAttention => vec!["n"],
            Self::Restart => vec!["r"],
            Self::Editor => vec!["e"],
            Self::ScrollUp => vec!["ctrl-u"],
            Self::ScrollDown => vec!["ctrl-d"],
            Self::PageUp => vec!["pgup"],
            Self::PageDown => vec!["pgdn"],
            Self::Live => vec!["G"],
            Self::Help => vec!["?"],
            Self::Quit => vec!["q"],
        }
    }
}

/// An action in focus mode (session pane has focus).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusAction {
    /// Back to normal mode.
    Unfocus,
    /// Jump to the next session needing attention.
    NextAttention,
    /// Switch to session `n` (1..=9) of the current project.
    Session(u8),
}

impl Action for FocusAction {
    const MODE: &'static str = "focus";

    fn all() -> Vec<Self> {
        let mut v = vec![Self::Unfocus, Self::NextAttention];
        v.extend((1..=9).map(Self::Session));
        v
    }

    fn name(self) -> String {
        match self {
            Self::Unfocus => "unfocus".to_owned(),
            Self::NextAttention => "next_attention".to_owned(),
            Self::Session(n) => format!("session_{n}"),
        }
    }

    fn default_keys(self) -> Vec<&'static str> {
        const ALT_DIGITS: [&str; 9] = [
            "alt-1", "alt-2", "alt-3", "alt-4", "alt-5", "alt-6", "alt-7", "alt-8", "alt-9",
        ];
        match self {
            Self::Unfocus => vec!["ctrl-\\"],
            Self::NextAttention => vec!["alt-n"],
            Self::Session(n) => ALT_DIGITS
                .get(usize::from(n).wrapping_sub(1))
                .map_or_else(Vec::new, |d| vec![*d]),
        }
    }
}

/// Raw override table: `mode -> action name -> key strings`.
pub type Overrides = BTreeMap<String, BTreeMap<String, Vec<String>>>;

type Table<A> = Vec<(A, Vec<KeySpec>)>;

fn build_table<A: Action>(
    over: Option<&BTreeMap<String, Vec<String>>>,
) -> Result<Table<A>, KeymapError> {
    let actions = A::all();
    let mut table: Table<A> = Vec::new();
    if let Some(over) = over {
        for name in over.keys() {
            if !actions.iter().any(|a| a.name() == *name) {
                return Err(KeymapError::UnknownAction {
                    mode: A::MODE,
                    action: name.clone(),
                });
            }
        }
    }
    for a in actions {
        let name = a.name();
        let keys = match over.and_then(|o| o.get(&name)) {
            Some(strings) => strings
                .iter()
                .map(|s| {
                    s.parse::<KeySpec>().map_err(|source| KeymapError::BadKey {
                        mode: A::MODE,
                        action: name.clone(),
                        source,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            // Defaults are validated by a unit test; a bad one is just skipped.
            None => a
                .default_keys()
                .into_iter()
                .filter_map(|s| s.parse().ok())
                .collect(),
        };
        table.push((a, keys));
    }
    let mut seen: Vec<(KeySpec, A)> = Vec::new();
    for (a, keys) in &table {
        for k in keys {
            match seen.iter().find(|(sk, _)| sk == k) {
                Some((_, prev)) if prev != a => {
                    return Err(KeymapError::Conflict {
                        mode: A::MODE,
                        key: k.to_string(),
                        first: prev.name(),
                        second: a.name(),
                    });
                }
                Some(_) => {}
                None => seen.push((*k, *a)),
            }
        }
    }
    Ok(table)
}

fn lookup<A: Copy>(table: &[(A, Vec<KeySpec>)], k: &KeySpec) -> Option<A> {
    table
        .iter()
        .find(|(_, keys)| keys.contains(k))
        .map(|(a, _)| *a)
}

fn describe<A: Action>(table: &[(A, Vec<KeySpec>)]) -> Vec<(String, String)> {
    table
        .iter()
        .map(|(a, keys)| {
            let keys = if keys.is_empty() {
                "(unbound)".to_owned()
            } else {
                keys.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            (a.name(), keys)
        })
        .collect()
}

/// The active key bindings of both modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    normal: Table<NormalAction>,
    focus: Table<FocusAction>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::from_overrides(&Overrides::new()).unwrap_or(Self {
            // Unreachable: the defaults are checked by a unit test.
            normal: Vec::new(),
            focus: Vec::new(),
        })
    }
}

impl Keymap {
    /// Defaults with `overrides` applied per action, then validated.
    ///
    /// # Errors
    /// On an unknown mode or action, an unparseable key, two actions on one
    /// key in a mode, or an `unfocus` that is unbound or a plain printable key.
    pub fn from_overrides(overrides: &Overrides) -> Result<Self, KeymapError> {
        if let Some(mode) = overrides
            .keys()
            .find(|m| !matches!(m.as_str(), "normal" | "focus"))
        {
            return Err(KeymapError::UnknownMode { mode: mode.clone() });
        }
        let normal = build_table::<NormalAction>(overrides.get("normal"))?;
        let focus = build_table::<FocusAction>(overrides.get("focus"))?;
        let unfocus = focus
            .iter()
            .find(|(a, _)| *a == FocusAction::Unfocus)
            .map(|(_, keys)| keys.as_slice())
            .unwrap_or_default();
        if unfocus.is_empty() {
            return Err(KeymapError::UnfocusUnbound);
        }
        if let Some(k) = unfocus.iter().find(|k| k.is_plain_printable()) {
            return Err(KeymapError::UnfocusPrintable { key: k.to_string() });
        }
        Ok(Self { normal, focus })
    }

    /// The normal-mode action bound to `k`.
    pub fn normal_action(&self, k: &KeySpec) -> Option<NormalAction> {
        lookup(&self.normal, k)
    }

    /// The focus-mode action bound to `k`.
    pub fn focus_action(&self, k: &KeySpec) -> Option<FocusAction> {
        lookup(&self.focus, k)
    }

    /// Keys bound to a normal-mode action.
    pub fn normal_keys(&self, action: NormalAction) -> &[KeySpec] {
        self.normal
            .iter()
            .find(|(a, _)| *a == action)
            .map_or(&[], |(_, k)| k.as_slice())
    }

    /// Keys bound to a focus-mode action.
    pub fn focus_keys(&self, action: FocusAction) -> &[KeySpec] {
        self.focus
            .iter()
            .find(|(a, _)| *a == action)
            .map_or(&[], |(_, k)| k.as_slice())
    }

    /// `(action name, keys)` for every normal-mode action, for the help overlay.
    pub fn describe_normal(&self) -> Vec<(String, String)> {
        describe(&self.normal)
    }

    /// `(action name, keys)` for every focus-mode action, for the help overlay.
    pub fn describe_focus(&self) -> Vec<(String, String)> {
        describe(&self.focus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn spec(s: &str) -> KeySpec {
        s.parse().unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    fn plain(key: Key) -> KeySpec {
        KeySpec::new(key, Mods::default())
    }

    fn mods(ctrl: bool, alt: bool, shift: bool) -> Mods {
        Mods { ctrl, alt, shift }
    }

    #[test]
    fn parser_table() {
        let cases: Vec<(&str, KeySpec)> = vec![
            ("n", plain(Key::Char('n'))),
            ("G", plain(Key::Char('G'))),
            ("?", plain(Key::Char('?'))),
            ("-", plain(Key::Char('-'))),
            ("enter", plain(Key::Enter)),
            ("Enter", plain(Key::Enter)),
            ("ESC", plain(Key::Esc)),
            ("tab", plain(Key::Tab)),
            ("up", plain(Key::Up)),
            ("PgUp", plain(Key::PageUp)),
            ("pagedown", plain(Key::PageDown)),
            ("f5", plain(Key::F(5))),
            ("space", plain(Key::Char(' '))),
            (
                "ctrl-u",
                KeySpec::new(Key::Char('u'), mods(true, false, false)),
            ),
            (
                "Ctrl-U",
                KeySpec::new(Key::Char('u'), mods(true, false, false)),
            ),
            (
                "alt-n",
                KeySpec::new(Key::Char('n'), mods(false, true, false)),
            ),
            (
                "alt-1",
                KeySpec::new(Key::Char('1'), mods(false, true, false)),
            ),
            (
                "ctrl-\\",
                KeySpec::new(Key::Char('\\'), mods(true, false, false)),
            ),
            (
                "ctrl--",
                KeySpec::new(Key::Char('-'), mods(true, false, false)),
            ),
            ("shift-tab", plain(Key::BackTab)),
            ("shift-g", plain(Key::Char('G'))),
            (
                "ctrl-alt-x",
                KeySpec::new(Key::Char('x'), mods(true, true, false)),
            ),
            ("shift-up", KeySpec::new(Key::Up, mods(false, false, true))),
        ];
        for (text, want) in cases {
            assert_eq!(spec(text), want, "{text}");
        }
    }

    #[test]
    fn parser_rejects_garbage() {
        for bad in [
            "",
            "ctrl-",
            "nn",
            "ctrl-nope",
            "f0",
            "f99",
            "shift-1",
            "ctrl-ctrl",
            "meta-x",
            "alt-",
        ] {
            assert!(bad.parse::<KeySpec>().is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn display_round_trips() {
        for text in [
            "n",
            "G",
            "?",
            "enter",
            "esc",
            "tab",
            "up",
            "pgup",
            "f5",
            "ctrl-u",
            "alt-n",
            "alt-1",
            "ctrl-\\",
            "shift-tab",
            "space",
            "ctrl--",
            "shift-up",
            "ctrl-alt-x",
        ] {
            let s = spec(text);
            assert_eq!(s.to_string(), text);
            assert_eq!(spec(&s.to_string()), s);
        }
    }

    #[test]
    fn matching_normalises_terminal_variants() {
        // `G` with or without SHIFT.
        assert_eq!(
            KeySpec::new(Key::Char('G'), mods(false, false, true)),
            spec("G")
        );
        assert_eq!(KeySpec::new(Key::Char('G'), Mods::default()), spec("G"));
        // `?` is usually reported with SHIFT.
        assert_eq!(
            KeySpec::new(Key::Char('?'), mods(false, false, true)),
            spec("?")
        );
        // Ctrl-Shift-U is Ctrl-U to a terminal.
        assert_eq!(
            KeySpec::new(Key::Char('U'), mods(true, false, false)),
            spec("ctrl-u")
        );
        // Both crossterm forms of Ctrl-\.
        assert_eq!(
            KeySpec::from_crossterm_char('\\', mods(true, false, false)),
            spec("ctrl-\\")
        );
        assert_eq!(
            KeySpec::from_crossterm_char('4', mods(true, false, false)),
            spec("ctrl-\\")
        );
        assert_ne!(
            KeySpec::from_crossterm_char('4', Mods::default()),
            spec("ctrl-\\")
        );
        // Shift-Tab arrives as Tab+SHIFT or BackTab.
        assert_eq!(
            KeySpec::new(Key::Tab, mods(false, false, true)),
            spec("shift-tab")
        );
        assert_eq!(
            KeySpec::new(Key::BackTab, mods(false, false, true)),
            spec("shift-tab")
        );
    }

    fn overrides(
        entries: &[(&str, &str, &[&str])],
    ) -> BTreeMap<String, BTreeMap<String, Vec<String>>> {
        let mut m: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
        for (mode, action, keys) in entries {
            m.entry((*mode).to_owned()).or_default().insert(
                (*action).to_owned(),
                keys.iter().map(|k| (*k).to_owned()).collect(),
            );
        }
        m
    }

    fn err(entries: &[(&str, &str, &[&str])]) -> String {
        Keymap::from_overrides(&overrides(entries))
            .expect_err("should be rejected")
            .to_string()
    }

    #[test]
    fn defaults_match_the_spec() {
        let k = Keymap::default();
        let n = |s: &str| k.normal_action(&spec(s));
        assert_eq!(n("j"), Some(NormalAction::MoveDown));
        assert_eq!(n("down"), Some(NormalAction::MoveDown));
        assert_eq!(n("k"), Some(NormalAction::MoveUp));
        assert_eq!(n("enter"), Some(NormalAction::Activate));
        assert_eq!(n("l"), Some(NormalAction::Activate));
        assert_eq!(n("o"), Some(NormalAction::OpenProject));
        assert_eq!(n("3"), Some(NormalAction::Select(3)));
        assert_eq!(n("n"), Some(NormalAction::NextAttention));
        assert_eq!(n("r"), Some(NormalAction::Restart));
        assert_eq!(n("e"), Some(NormalAction::Editor));
        assert_eq!(n("ctrl-u"), Some(NormalAction::ScrollUp));
        assert_eq!(n("ctrl-d"), Some(NormalAction::ScrollDown));
        assert_eq!(n("pgup"), Some(NormalAction::PageUp));
        assert_eq!(n("pgdn"), Some(NormalAction::PageDown));
        assert_eq!(n("G"), Some(NormalAction::Live));
        assert_eq!(n("?"), Some(NormalAction::Help));
        assert_eq!(n("q"), Some(NormalAction::Quit));
        assert_eq!(n("x"), None);
        let f = |s: &str| k.focus_action(&spec(s));
        assert_eq!(f("ctrl-\\"), Some(FocusAction::Unfocus));
        assert_eq!(f("alt-n"), Some(FocusAction::NextAttention));
        assert_eq!(f("alt-7"), Some(FocusAction::Session(7)));
        assert_eq!(f("q"), None);
    }

    #[test]
    fn overrides_replace_per_action_and_accept_lists() {
        let k = Keymap::from_overrides(&overrides(&[
            ("normal", "next_attention", &["x"]),
            ("normal", "quit", &["q", "ctrl-c"]),
            ("focus", "unfocus", &["ctrl-g"]),
        ]))
        .expect("valid");
        assert_eq!(
            k.normal_action(&spec("x")),
            Some(NormalAction::NextAttention)
        );
        assert_eq!(k.normal_action(&spec("n")), None);
        assert_eq!(k.normal_action(&spec("ctrl-c")), Some(NormalAction::Quit));
        assert_eq!(k.normal_action(&spec("q")), Some(NormalAction::Quit));
        assert_eq!(k.focus_action(&spec("ctrl-g")), Some(FocusAction::Unfocus));
        assert_eq!(k.focus_action(&spec("ctrl-\\")), None);
        // Untouched actions keep their defaults.
        assert_eq!(k.normal_action(&spec("r")), Some(NormalAction::Restart));
    }

    #[test]
    fn a_swap_is_not_a_conflict() {
        let k = Keymap::from_overrides(&overrides(&[
            ("normal", "restart", &["n"]),
            ("normal", "next_attention", &["r"]),
        ]))
        .expect("valid swap");
        assert_eq!(k.normal_action(&spec("n")), Some(NormalAction::Restart));
    }

    #[test]
    fn rejects_unknown_actions_and_modes() {
        let e = err(&[("normal", "teleport", &["x"])]);
        assert!(e.contains("keybindings.normal.teleport"), "{e}");
        assert!(e.contains("unknown action"), "{e}");
        // A focus action in the normal table (and vice versa) is unknown there.
        let e = err(&[("normal", "unfocus", &["x"])]);
        assert!(e.contains("keybindings.normal.unfocus"), "{e}");
        let e = err(&[("focus", "quit", &["x"])]);
        assert!(e.contains("keybindings.focus.quit"), "{e}");
        let e = err(&[("select_1", "quit", &["x"])]);
        assert!(e.contains("keybindings.select_1"), "{e}");
        assert!(e.contains("unknown mode"), "{e}");
        let e = err(&[("normal", "select_10", &["x"])]);
        assert!(e.contains("keybindings.normal.select_10"), "{e}");
    }

    #[test]
    fn rejects_bad_key_specs_naming_the_config_key() {
        let e = err(&[("normal", "quit", &["q", "ctrl-nope"])]);
        assert!(e.contains("keybindings.normal.quit"), "{e}");
        assert!(e.contains("ctrl-nope"), "{e}");
    }

    #[test]
    fn rejects_conflicts_within_a_mode_only() {
        let e = err(&[("normal", "restart", &["n"])]);
        assert!(e.contains("\"n\""), "{e}");
        assert!(e.contains("restart") && e.contains("next_attention"), "{e}");
        // The same key in the other mode is fine.
        Keymap::from_overrides(&overrides(&[("focus", "next_attention", &["q"])]))
            .expect("different modes do not conflict");
        // Within one action a repeated key is harmless.
        Keymap::from_overrides(&overrides(&[("normal", "quit", &["q", "q"])]))
            .expect("same action");
    }

    #[test]
    fn unfocus_cannot_be_lost_or_swallow_typing() {
        let e = err(&[("focus", "unfocus", &["x"])]);
        assert!(e.contains("keybindings.focus.unfocus"), "{e}");
        assert!(e.contains("typing"), "{e}");
        let e = err(&[("focus", "unfocus", &["G"])]);
        assert!(e.contains("typing"), "{e}");
        let e = err(&[("focus", "unfocus", &[])]);
        assert!(e.contains("keybindings.focus.unfocus"), "{e}");
        assert!(e.contains("way out"), "{e}");
        // Another focus action taking unfocus's key leaves it unbound only via a conflict.
        let e = err(&[("focus", "next_attention", &["ctrl-\\"])]);
        assert!(e.contains("conflict") || e.contains("both"), "{e}");
        // Non-printable and modified keys are fine.
        for ok in ["esc", "ctrl-g", "alt-q", "f12", "tab"] {
            Keymap::from_overrides(&overrides(&[("focus", "unfocus", &[ok])])).expect(ok);
        }
    }

    #[test]
    fn default_keys_all_parse_and_default_equals_empty_overrides() {
        for a in NormalAction::all() {
            for k in a.default_keys() {
                assert!(k.parse::<KeySpec>().is_ok(), "{k}");
            }
            assert!(!Keymap::default().normal_keys(a).is_empty(), "{a:?}");
        }
        for a in FocusAction::all() {
            assert!(!Keymap::default().focus_keys(a).is_empty(), "{a:?}");
        }
    }

    #[test]
    fn describe_lists_live_bindings() {
        let k = Keymap::from_overrides(&overrides(&[("normal", "next_attention", &["x"])]))
            .expect("valid");
        let normal = k.describe_normal();
        assert!(
            normal.contains(&("next_attention".to_owned(), "x".to_owned())),
            "{normal:?}"
        );
        assert!(
            normal.contains(&("move_down".to_owned(), "j, down".to_owned())),
            "{normal:?}"
        );
        assert!(
            normal.contains(&("select_9".to_owned(), "9".to_owned())),
            "{normal:?}"
        );
        let focus = k.describe_focus();
        assert!(
            focus.contains(&("unfocus".to_owned(), "ctrl-\\".to_owned())),
            "{focus:?}"
        );
        assert!(
            focus.contains(&("session_1".to_owned(), "alt-1".to_owned())),
            "{focus:?}"
        );
    }
}
