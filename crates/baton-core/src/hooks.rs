//! Claude Code hook plumbing: the injected settings JSON and payload parsing.

use baton_proto::{Quota, QuotaWindow};
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

/// The `--settings` JSON registering `<exe> hook <Event>` for every event and
/// `<exe> statusline` as the status line (which relays the quota to the daemon
/// and runs the user's own status line command, if configured).
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
    json!({
        "hooks": hooks,
        "statusLine": {"type": "command", "command": format!("{exe} statusline"), "padding": 0},
    })
    .to_string()
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

/// The subscription quota in a status line payload (`rate_limits`), or `None`
/// when it has no usable window. Percentages must lie in 0..=100 and reset
/// times be positive; anything else drops that window.
pub fn parse_quota(json: &str) -> Option<Quota> {
    let v: Value = serde_json::from_str(json).ok()?;
    let limits = v.get("rate_limits")?;
    let window = |name: &str| {
        let w = limits.get(name)?;
        let used_pct = w.get("used_percentage")?.as_f64()?;
        let resets_at = w.get("resets_at")?.as_u64().filter(|t| *t > 0)?;
        (0.0..=100.0).contains(&used_pct).then_some(QuotaWindow {
            used_pct: used_pct as f32,
            resets_at,
        })
    };
    let quota = Quota {
        five_hour: window("five_hour"),
        seven_day: window("seven_day"),
    };
    (quota.five_hour.is_some() || quota.seven_day.is_some()).then_some(quota)
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
    fn quota_is_read_from_rate_limits() {
        let json = r#"{"model":{"id":"m"},"rate_limits":{
            "five_hour":{"used_percentage":23.5,"resets_at":1738425600},
            "seven_day":{"used_percentage":41,"resets_at":1738857600}}}"#;
        assert_eq!(
            parse_quota(json),
            Some(Quota {
                five_hour: Some(QuotaWindow {
                    used_pct: 23.5,
                    resets_at: 1_738_425_600
                }),
                seven_day: Some(QuotaWindow {
                    used_pct: 41.0,
                    resets_at: 1_738_857_600
                }),
            })
        );
    }

    #[test]
    fn quota_windows_are_independent_and_validated() {
        let only_week = r#"{"rate_limits":{"seven_day":{"used_percentage":5,"resets_at":9}}}"#;
        let q = parse_quota(only_week).expect("quota");
        assert_eq!(q.five_hour, None);
        assert!(q.seven_day.is_some());
        for bad in [
            "",
            "not json",
            "{}",
            r#"{"rate_limits":{}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":101,"resets_at":9}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":-1,"resets_at":9}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":"5","resets_at":9}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":5,"resets_at":0}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":5}}}"#,
        ] {
            assert_eq!(parse_quota(bad), None, "{bad}");
        }
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
