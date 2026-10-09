//! Claude Code hook plumbing: the injected settings JSON and payload parsing.

use serde_json::{Value, json};
use std::path::Path;

/// The closed set of hook events Baton registers (Finding 2).
pub const HOOK_EVENTS: [&str; 9] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "Notification",
    "Stop",
    "StopFailure",
    "SessionEnd",
];

/// Whether `name` is one of [`HOOK_EVENTS`].
pub fn is_known_event(name: &str) -> bool {
    HOOK_EVENTS.contains(&name)
}

/// Quotes `s` as a single `sh` word.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The `--settings` JSON registering `<exe> hook <Event>` for every event.
///
/// The exe path is shell-quoted because Claude runs hook commands through a
/// shell; a non-UTF-8 path is converted lossily.
pub fn settings_json(exe: &Path) -> String {
    let exe = shell_quote(&exe.to_string_lossy());
    let hooks: serde_json::Map<String, Value> = HOOK_EVENTS
        .iter()
        .map(|event| {
            let entry = json!([{"hooks": [{
                "type": "command",
                "command": format!("{exe} hook {event}"),
                "timeout": 5,
            }]}]);
            ((*event).to_owned(), entry)
        })
        .collect();
    json!({ "hooks": hooks }).to_string()
}

/// The fields of a hook's stdin JSON that Baton uses; everything is optional
/// and unknown fields are ignored.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct HookPayload {
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub source: Option<String>,
    pub notification_type: Option<String>,
    pub tool_name: Option<String>,
}

impl HookPayload {
    /// Parses leniently: malformed JSON or wrong field types give `None`
    /// fields rather than an error.
    pub fn parse(json: &str) -> Self {
        let v: Value = serde_json::from_str(json).unwrap_or(Value::Null);
        let field = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        Self {
            session_id: field("session_id"),
            transcript_path: field("transcript_path"),
            cwd: field("cwd"),
            model: field("model"),
            source: field("source"),
            notification_type: field("notification_type"),
            tool_name: field("tool_name"),
        }
    }
}

/// Longest accepted session id or model name.
pub const MAX_ID_LEN: usize = 128;
/// Longest accepted transcript path.
pub const MAX_PATH_LEN: usize = 4096;

fn clean(s: Option<String>, max: usize) -> Option<String> {
    s.filter(|v| !v.is_empty() && v.len() <= max && !v.chars().any(char::is_control))
}

impl HookPayload {
    /// Drops the fields the daemon stores from `SessionStart` unless they are
    /// sane: ids and model at most [`MAX_ID_LEN`] bytes, the transcript path
    /// at most [`MAX_PATH_LEN`] bytes and absolute, none empty or containing
    /// control characters. The payload is client-supplied, so it is untrusted.
    #[must_use]
    pub fn validated(self) -> Self {
        let transcript_path =
            clean(self.transcript_path, MAX_PATH_LEN).filter(|p| Path::new(p).is_absolute());
        Self {
            session_id: clean(self.session_id, MAX_ID_LEN),
            model: clean(self.model, MAX_ID_LEN),
            transcript_path,
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(id: &str, path: &str, model: &str) -> HookPayload {
        HookPayload {
            session_id: Some(id.into()),
            transcript_path: Some(path.into()),
            model: Some(model.into()),
            ..HookPayload::default()
        }
    }

    #[test]
    fn sane_session_start_fields_pass() {
        let p = payload(
            "0b1c-uuid",
            "/home/u/.claude/projects/x/s.jsonl",
            "claude-opus-5-5",
        );
        assert_eq!(p.clone().validated(), p);
        assert_eq!(HookPayload::default().validated(), HookPayload::default());
    }

    #[test]
    fn overlong_fields_are_dropped_at_the_cap() {
        let ok_id = "a".repeat(MAX_ID_LEN);
        let ok_path = format!("/{}", "p".repeat(MAX_PATH_LEN - 1));
        let p = payload(&ok_id, &ok_path, &ok_id).validated();
        assert!(p.session_id.is_some() && p.model.is_some() && p.transcript_path.is_some());
        let long_id = "a".repeat(MAX_ID_LEN + 1);
        let long_path = format!("/{}", "p".repeat(MAX_PATH_LEN));
        let p = payload(&long_id, &long_path, &long_id).validated();
        assert_eq!(p.session_id, None);
        assert_eq!(p.model, None);
        assert_eq!(p.transcript_path, None);
    }

    #[test]
    fn control_characters_are_rejected() {
        for bad in ["a\nb", "a\x1b[2Jb", "\0", "a\u{85}b", "tab\there", "\x7f"] {
            let p = payload(bad, &format!("/t/{bad}"), bad).validated();
            assert_eq!(p.session_id, None, "{bad:?}");
            assert_eq!(p.model, None, "{bad:?}");
            assert_eq!(p.transcript_path, None, "{bad:?}");
        }
    }

    #[test]
    fn empty_and_relative_values_are_rejected() {
        let p = payload("", "", "").validated();
        assert_eq!(p, HookPayload::default());
        for rel in ["t.jsonl", "./t.jsonl", "../x", "projects/x.jsonl"] {
            assert_eq!(payload("i", rel, "m").validated().transcript_path, None);
        }
    }

    #[test]
    fn other_fields_survive_validation() {
        let mut p = payload("\n", "rel", "\n");
        p.cwd = Some("/c".into());
        p.notification_type = Some("idle_prompt".into());
        let v = p.validated();
        assert_eq!(v.cwd.as_deref(), Some("/c"));
        assert_eq!(v.notification_type.as_deref(), Some("idle_prompt"));
    }

    #[test]
    fn settings_match_golden_file() {
        let got = settings_json(Path::new("/opt/it's/baton"));
        let want = include_str!("../tests/golden/hooks.json");
        assert_eq!(got.trim(), want.trim());
    }

    #[test]
    fn quoting_survives_sh() {
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn event_set_is_closed() {
        for e in HOOK_EVENTS {
            assert!(is_known_event(e));
        }
        for e in ["", "stop", "Stop ", "../x", "PreCompact", "Stop; rm"] {
            assert!(!is_known_event(e), "{e:?}");
        }
    }

    #[test]
    fn payload_is_tolerant() {
        let p = HookPayload::parse(
            r#"{"session_id":"s","transcript_path":"/t","model":"m","extra":{"a":1},"cwd":7}"#,
        );
        assert_eq!(p.session_id.as_deref(), Some("s"));
        assert_eq!(p.transcript_path.as_deref(), Some("/t"));
        assert_eq!(p.model.as_deref(), Some("m"));
        assert_eq!(p.cwd, None, "wrong-typed field is dropped");
        assert_eq!(HookPayload::parse("garbage"), HookPayload::default());
        assert_eq!(HookPayload::parse(""), HookPayload::default());
    }
}
