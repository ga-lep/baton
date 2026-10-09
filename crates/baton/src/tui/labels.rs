//! Status badges and labels (spec section 4).

use baton_proto::Status;

/// Status badge.
pub fn badge(status: Status) -> &'static str {
    match status {
        Status::Starting => "…",
        Status::Running => "●",
        Status::Permission => "◐",
        Status::YourTurn => "✓",
        Status::Idle => "○",
        Status::Exited(_) => "✗",
        Status::Unknown => "?",
        Status::Closed => "◌",
    }
}

/// Status text.
pub fn status_label(status: Status) -> String {
    match status {
        Status::Starting => "starting".into(),
        Status::Running => "running".into(),
        Status::Permission => "permission".into(),
        Status::YourTurn => "your turn".into(),
        Status::Idle => "idle".into(),
        Status::Exited(code) => format!("exited {code}"),
        Status::Unknown => "unknown".into(),
        Status::Closed => "closed".into(),
    }
}
