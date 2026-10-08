//! When a status change deserves a desktop notification, and what it says.

use crate::attention::needs_attention;
use baton_proto::{SessionId, Status};

/// What the attached client shows (mirrors `ClientMsg::ClientView`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientView {
    /// The session on screen, if any.
    pub on_screen: Option<SessionId>,
    /// Whether the host terminal has focus.
    pub terminal_focused: bool,
}

/// Longest text (in characters) handed to a notification backend.
pub const MAX_TEXT_CHARS: usize = 80;

/// Whether the move `prev` -> `new` of `session` should raise a notification.
///
/// It must enter an attention state (a different one than before), and the
/// user must not already be looking: no client attached (`view` is `None`),
/// the terminal unfocused, or another session on screen.
pub fn should_notify(
    prev: Status,
    new: Status,
    view: Option<&ClientView>,
    session: &SessionId,
    enabled: bool,
) -> bool {
    if !enabled || new == prev || !needs_attention(new) {
        return false;
    }
    match view {
        None => true,
        Some(v) => !v.terminal_focused || v.on_screen.as_ref() != Some(session),
    }
}

/// Makes `text` safe for a notification backend: control characters are
/// dropped and the result is capped at [`MAX_TEXT_CHARS`] characters
/// (ending in `…` when cut).
pub fn sanitize(text: &str) -> String {
    let mut chars = text.chars().filter(|c| !c.is_control());
    let mut out: String = chars.by_ref().take(MAX_TEXT_CHARS).collect();
    if chars.next().is_some() {
        out.pop();
        out.push('…');
    }
    out
}

/// The notification summary for a session in an attention `status`, or
/// `None` if the status needs no attention.
pub fn summary(repo: &str, status: Status) -> Option<String> {
    let what = match status {
        Status::Permission => "needs permission",
        Status::YourTurn => "your turn",
        _ => return None,
    };
    Some(format!("{}: {what}", sanitize(repo)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use Status::*;

    fn id(s: &str) -> SessionId {
        SessionId(s.into())
    }

    fn view(on: Option<&str>, focused: bool) -> ClientView {
        ClientView {
            on_screen: on.map(id),
            terminal_focused: focused,
        }
    }

    #[test]
    fn full_table() {
        let me = id("me");
        // (attached, focused, on_screen) x entering vs remaining x enabled.
        for attached in [false, true] {
            for focused in [false, true] {
                for on_screen in [false, true] {
                    for (prev, new, entering) in [
                        (Running, Permission, true),
                        (Running, YourTurn, true),
                        (Permission, YourTurn, true),
                        (Permission, Permission, false),
                        (YourTurn, YourTurn, false),
                        (Running, Idle, false),
                        (Running, Exited(1), false),
                        (YourTurn, Idle, false),
                    ] {
                        for enabled in [false, true] {
                            let v =
                                view(if on_screen { Some("me") } else { Some("other") }, focused);
                            let v = attached.then_some(&v);
                            let user_looking = attached && focused && on_screen;
                            let want = enabled && entering && !user_looking;
                            assert_eq!(
                                should_notify(prev, new, v, &me, enabled),
                                want,
                                "attached={attached} focused={focused} on_screen={on_screen} \
                                 {prev:?}->{new:?} enabled={enabled}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn attached_with_nothing_on_screen_notifies() {
        let v = view(None, true);
        assert!(should_notify(
            Running,
            Permission,
            Some(&v),
            &id("me"),
            true
        ));
    }

    #[test]
    fn sanitize_strips_controls_and_caps() {
        assert_eq!(sanitize("a\x1b[31mb\u{7}\nc\u{85}d"), "a[31mbcd");
        assert_eq!(sanitize("plain repo"), "plain repo");
        let long = "x".repeat(500);
        let s = sanitize(&long);
        assert_eq!(s.chars().count(), MAX_TEXT_CHARS);
        assert!(s.ends_with('…'));
        assert_eq!(
            sanitize(&"y".repeat(MAX_TEXT_CHARS)).chars().count(),
            MAX_TEXT_CHARS
        );
        assert!(!sanitize(&"y".repeat(MAX_TEXT_CHARS)).ends_with('…'));
    }

    #[test]
    fn summaries() {
        assert_eq!(
            summary("api", Permission).as_deref(),
            Some("api: needs permission")
        );
        assert_eq!(summary("api", YourTurn).as_deref(), Some("api: your turn"));
        assert_eq!(summary("api", Idle), None);
        assert_eq!(
            summary("a\x07pi", Permission).as_deref(),
            Some("api: needs permission")
        );
    }
}
