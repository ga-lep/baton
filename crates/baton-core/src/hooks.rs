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

#[cfg(test)]
mod tests {
    use super::*;

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
