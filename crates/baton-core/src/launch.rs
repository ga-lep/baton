//! The launch ladder: how a session's `claude` is started and what to try
//! next when an attempt dies early (pure logic, no processes).

use std::time::Duration;

/// An attempt that exits non-zero sooner than this, before any
/// `SessionStart`, is an early failure.
pub const EARLY_FAILURE_WINDOW: Duration = Duration::from_secs(10);

/// Whether `s` is a canonical hyphenated UUID (8-4-4-4-12 hex digits).
///
/// Claude session ids are UUIDs. Ids read back from `state.json` or a hook
/// payload end up as `claude` arguments, so anything else is refused.
pub fn is_uuid(s: &str) -> bool {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let mut parts = s.split('-');
    GROUPS.iter().all(|&len| {
        parts
            .next()
            .is_some_and(|p| p.len() == len && p.bytes().all(|b| b.is_ascii_hexdigit()))
    }) && parts.next().is_none()
}

/// One way of starting `claude`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rung {
    /// `--resume <id>`: reopen a known conversation.
    Resume(String),
    /// `--continue`: the latest conversation in the directory.
    Continue,
    /// `--session-id <id>`: a new conversation whose id is chosen up front.
    Fresh(String),
}

/// The first rung: resume `claude_session_id` if it is a valid UUID, else
/// continue.
pub fn first_rung(claude_session_id: Option<&str>) -> Rung {
    match claude_session_id.filter(|id| is_uuid(id)) {
        Some(id) => Rung::Resume(id.to_owned()),
        None => Rung::Continue,
    }
}

impl Rung {
    /// The rung to try after this one failed early; `fresh_id` supplies the
    /// new UUID for the last resort. `None` when nothing is left.
    #[must_use]
    pub fn next(&self, fresh_id: impl FnOnce() -> String) -> Option<Rung> {
        match self {
            Rung::Resume(_) => Some(Rung::Continue),
            Rung::Continue => Some(Rung::Fresh(fresh_id())),
            Rung::Fresh(_) => None,
        }
    }

    /// The `claude` arguments of this rung.
    pub fn args(&self) -> Vec<String> {
        match self {
            Rung::Resume(id) => vec!["--resume".into(), id.clone()],
            Rung::Continue => vec!["--continue".into()],
            Rung::Fresh(id) => vec!["--session-id".into(), id.clone()],
        }
    }

    /// `resume`, `continue` or `fresh`, as shown in the info panel.
    pub fn label(&self) -> &'static str {
        match self {
            Rung::Resume(_) => "resume",
            Rung::Continue => "continue",
            Rung::Fresh(_) => "fresh",
        }
    }

    /// The conversation id this rung fixes before `SessionStart`, if any.
    pub fn known_session_id(&self) -> Option<&str> {
        match self {
            Rung::Resume(id) | Rung::Fresh(id) => Some(id),
            Rung::Continue => None,
        }
    }
}

/// Whether an exit is a failed launch attempt: a non-zero code within
/// [`EARLY_FAILURE_WINDOW`] and before any `SessionStart`. A slow trust
/// dialog (no hook yet, but still running) never qualifies.
pub fn is_early_failure(code: i32, elapsed: Duration, saw_session_start: bool) -> bool {
    code != 0 && elapsed < EARLY_FAILURE_WINDOW && !saw_session_start
}

/// The conversation id to record when a child is launched with `rung`.
///
/// Only a rung that carries an id replaces the remembered one; `--continue`
/// keeps `previous` until a `SessionStart` reports the real id, so a transient
/// failure of `--resume` cannot erase it.
pub fn id_after_launch(previous: Option<&str>, rung: &Rung) -> Option<String> {
    rung.known_session_id().or(previous).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const ID: &str = "123e4567-e89b-42d3-a456-426614174000";

    #[test]
    fn uuid_check_accepts_only_canonical_uuids() {
        assert!(is_uuid(ID));
        assert!(is_uuid("123E4567-E89B-42D3-A456-426614174000"));
        for bad in [
            "",
            "123e4567",
            "123e4567-e89b-42d3-a456-42661417400",
            "123e4567-e89b-42d3-a456-4266141740000",
            "123e4567-e89b-42d3-a456-42661417400g",
            "--settings",
            "-123e4567-e89b-42d3-a456-42661417400",
            "123e4567-e89b-42d3-a456-426614174000\n",
            "123e4567e89b42d3a456426614174000",
            "$(touch x)-e89b-42d3-a456-426614174000",
        ] {
            assert!(!is_uuid(bad), "{bad:?}");
        }
    }

    #[test]
    fn first_rung_resumes_a_known_id_else_continues() {
        assert_eq!(first_rung(Some(ID)), Rung::Resume(ID.into()));
        assert_eq!(first_rung(None), Rung::Continue);
        // Anything that is not a UUID is never passed to claude.
        assert_eq!(first_rung(Some("--dangerous")), Rung::Continue);
        assert_eq!(first_rung(Some("")), Rung::Continue);
    }

    #[test]
    fn ladder_goes_resume_continue_fresh_then_stops() {
        let fresh = || "00000000-0000-4000-8000-000000000001".to_owned();
        let r = Rung::Resume(ID.into());
        let c = r.next(fresh).unwrap();
        assert_eq!(c, Rung::Continue);
        let f = c.next(fresh).unwrap();
        assert_eq!(f, Rung::Fresh(fresh()));
        assert_eq!(f.next(fresh), None);
    }

    #[test]
    fn rung_args_and_labels() {
        let r = Rung::Resume(ID.into());
        assert_eq!(r.args(), ["--resume", ID]);
        assert_eq!(r.label(), "resume");
        assert_eq!(Rung::Continue.args(), ["--continue"]);
        assert_eq!(Rung::Continue.label(), "continue");
        let f = Rung::Fresh(ID.into());
        assert_eq!(f.args(), ["--session-id", ID]);
        assert_eq!(f.label(), "fresh");
        assert_eq!(f.known_session_id(), Some(ID));
        assert_eq!(r.known_session_id(), Some(ID));
        assert_eq!(Rung::Continue.known_session_id(), None);
    }

    #[test]
    fn early_failure_table() {
        let s = Duration::from_secs;
        // (exit code, elapsed, saw SessionStart) -> early failure
        let table = [
            (1, s(2), false, true),
            (127, s(0), false, true),
            (1, s(9), false, true),
            (1, s(10), false, false), // at the limit: not early
            (1, s(30), false, false), // a slow trust dialog is not a failure
            (0, s(2), false, false),  // clean exit
            (1, s(2), true, false),   // it started: not a launch failure
        ];
        for (code, elapsed, saw, want) in table {
            assert_eq!(
                is_early_failure(code, elapsed, saw),
                want,
                "{code} {elapsed:?} {saw}"
            );
        }
    }

    #[test]
    fn only_a_rung_with_an_id_replaces_the_remembered_id() {
        let (a, b) = ("a-id", "b-id");
        let table = [
            (Some(a), Rung::Resume(b.into()), Some(b)),
            (None, Rung::Resume(b.into()), Some(b)),
            (Some(a), Rung::Fresh(b.into()), Some(b)),
            (Some(a), Rung::Continue, Some(a)),
            (None, Rung::Continue, None),
        ];
        for (prev, rung, want) in table {
            assert_eq!(
                id_after_launch(prev, &rung).as_deref(),
                want,
                "{prev:?} {rung:?}"
            );
        }
    }
}
