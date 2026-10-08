//! The per-session status state machine (spec section 6, Findings 3-5).
//!
//! [`next`] is pure: the daemon only feeds it events and acts on the result.

use baton_proto::Status;

/// An event that can move a session between statuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input<'a> {
    /// The child process was (re)launched.
    Spawn,
    /// A hook fired: its event name and, for `Notification`, its subtype.
    Hook {
        event: &'a str,
        notification_type: Option<&'a str>,
    },
    /// The user is looking at the session (or marked it viewed).
    Viewed,
    /// The child exited with this code.
    Exited(i32),
    /// `hook_timeout_secs` elapsed since spawn, the process still alive.
    HookTimeout,
}

/// Side information about a transition.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Effects {
    /// The status differs from the previous one.
    pub changed: bool,
    /// The event or notification subtype is not one Baton knows; callers log it.
    pub unrecognized: bool,
}

/// The status after `input`, and what happened.
pub fn next(current: Status, input: Input<'_>) -> (Status, Effects) {
    let mut unrecognized = false;
    let to = match (current, input) {
        (_, Input::Spawn) => Status::Starting,
        // Terminal until the next spawn; the first exit code wins.
        (Status::Exited(_), _) => current,
        (_, Input::Exited(code)) => Status::Exited(code),
        (Status::Starting, Input::HookTimeout) => Status::Unknown,
        (_, Input::HookTimeout) => current,
        (Status::YourTurn, Input::Viewed) => Status::Idle,
        (_, Input::Viewed) => current,
        (
            _,
            Input::Hook {
                event,
                notification_type,
            },
        ) => hook(current, event, notification_type, &mut unrecognized),
    };
    (
        to,
        Effects {
            changed: to != current,
            unrecognized,
        },
    )
}

fn hook(current: Status, event: &str, kind: Option<&str>, unrecognized: &mut bool) -> Status {
    match event {
        "SessionStart" if matches!(current, Status::Starting | Status::Unknown) => Status::Idle,
        // `/clear`, resume and compaction mid-conversation keep the status.
        "SessionStart" => current,
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" => Status::Running,
        "PermissionRequest" => Status::Permission,
        "Stop" | "StopFailure" => Status::YourTurn,
        // The process exit decides, not the end of the conversation.
        "SessionEnd" => current,
        "Notification" => match kind {
            Some("permission_prompt" | "elicitation_dialog" | "elicitation_url_dialog") => {
                Status::Permission
            }
            Some("idle_prompt") => current,
            _ => {
                *unrecognized = true;
                current
            }
        },
        _ => {
            *unrecognized = true;
            current
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Status::*;

    fn hook(event: &str) -> Input<'_> {
        Input::Hook {
            event,
            notification_type: None,
        }
    }

    fn notify(kind: &str) -> Input<'_> {
        Input::Hook {
            event: "Notification",
            notification_type: Some(kind),
        }
    }

    const ALL: [Status; 8] = [
        Starting,
        Running,
        Permission,
        YourTurn,
        Idle,
        Exited(0),
        Exited(2),
        Unknown,
    ];

    fn st(current: Status, input: Input<'_>) -> Status {
        next(current, input).0
    }

    #[test]
    fn spawn_gives_starting() {
        assert_eq!(st(Exited(1), Input::Spawn), Starting, "relaunch");
        assert_eq!(st(Starting, Input::Spawn), Starting);
    }

    #[test]
    fn session_start_gives_idle_from_starting_or_unknown_only() {
        assert_eq!(st(Starting, hook("SessionStart")), Idle);
        assert_eq!(st(Unknown, hook("SessionStart")), Idle);
        // /clear or compaction mid-conversation changes nothing.
        for s in [Running, Permission, YourTurn, Idle, Exited(0)] {
            assert_eq!(st(s, hook("SessionStart")), s, "{s:?}");
        }
    }

    #[test]
    fn activity_hooks_give_running() {
        for e in ["UserPromptSubmit", "PreToolUse", "PostToolUse"] {
            for s in [Starting, Running, Permission, YourTurn, Idle, Unknown] {
                assert_eq!(st(s, hook(e)), Running, "{e} from {s:?}");
            }
        }
    }

    #[test]
    fn permission_signals_give_permission() {
        for s in [Starting, Running, Idle, YourTurn, Unknown] {
            assert_eq!(st(s, hook("PermissionRequest")), Permission);
            for kind in [
                "permission_prompt",
                "elicitation_dialog",
                "elicitation_url_dialog",
            ] {
                assert_eq!(st(s, notify(kind)), Permission, "{kind} from {s:?}");
            }
        }
    }

    #[test]
    fn stop_and_stop_failure_give_your_turn() {
        for e in ["Stop", "StopFailure"] {
            for s in [Starting, Running, Permission, Idle, Unknown] {
                assert_eq!(st(s, hook(e)), YourTurn, "{e} from {s:?}");
            }
        }
    }

    #[test]
    fn idle_prompt_changes_nothing() {
        for s in ALL {
            let (to, fx) = next(s, notify("idle_prompt"));
            assert_eq!(to, s);
            assert_eq!(fx, Effects::default(), "recognized, so not logged");
        }
    }

    #[test]
    fn unknown_events_and_subtypes_change_nothing_and_are_flagged() {
        for s in ALL.into_iter().filter(|s| !matches!(s, Exited(_))) {
            for input in [
                hook("PreCompact"),
                hook(""),
                notify("auth_success"),
                notify("agent_needs_input"),
                notify("quota_auto_resume_5h"),
                notify(""),
                Input::Hook {
                    event: "Notification",
                    notification_type: None,
                },
            ] {
                let (to, fx) = next(s, input);
                assert_eq!(to, s, "{input:?} from {s:?}");
                assert!(fx.unrecognized && !fx.changed, "{input:?}");
            }
        }
    }

    #[test]
    fn session_end_changes_nothing() {
        for s in ALL {
            let (to, fx) = next(s, hook("SessionEnd"));
            assert_eq!(to, s);
            assert_eq!(fx, Effects::default());
        }
    }

    #[test]
    fn viewed_only_clears_your_turn() {
        assert_eq!(st(YourTurn, Input::Viewed), Idle);
        for s in ALL.into_iter().filter(|s| *s != YourTurn) {
            assert_eq!(st(s, Input::Viewed), s, "{s:?}");
        }
    }

    #[test]
    fn child_exit_gives_exited_from_any_live_status() {
        for s in ALL.into_iter().filter(|s| !matches!(s, Exited(_))) {
            assert_eq!(st(s, Input::Exited(3)), Exited(3), "{s:?}");
        }
    }

    #[test]
    fn timeout_only_applies_while_starting() {
        assert_eq!(st(Starting, Input::HookTimeout), Unknown);
        for s in ALL.into_iter().filter(|s| *s != Starting) {
            assert_eq!(st(s, Input::HookTimeout), s, "{s:?}");
        }
    }

    #[test]
    fn exited_is_terminal_until_relaunch() {
        let inputs = [
            hook("SessionStart"),
            hook("UserPromptSubmit"),
            hook("PreToolUse"),
            hook("PostToolUse"),
            hook("PermissionRequest"),
            hook("Stop"),
            hook("StopFailure"),
            hook("SessionEnd"),
            notify("permission_prompt"),
            notify("idle_prompt"),
            Input::Viewed,
            Input::HookTimeout,
        ];
        for input in inputs {
            assert_eq!(st(Exited(5), input), Exited(5), "{input:?}");
        }
        assert_eq!(
            st(Exited(5), Input::Exited(9)),
            Exited(5),
            "first code wins"
        );
        assert_eq!(st(Exited(5), Input::Spawn), Starting);
    }

    #[test]
    fn unknown_recovers_on_any_hook() {
        assert_eq!(st(Unknown, hook("Stop")), YourTurn);
        assert_eq!(st(Unknown, hook("PreToolUse")), Running);
        assert_eq!(st(Unknown, hook("SessionStart")), Idle);
    }

    #[test]
    fn changed_flag_tracks_the_transition() {
        assert!(next(Idle, hook("PreToolUse")).1.changed);
        assert!(!next(Running, hook("PreToolUse")).1.changed);
        assert!(next(Running, Input::Exited(0)).1.changed);
    }

    #[test]
    fn a_normal_turn_walks_the_expected_path() {
        let mut s = st(Exited(0), Input::Spawn);
        for (input, want) in [
            (hook("SessionStart"), Idle),
            (hook("UserPromptSubmit"), Running),
            (hook("PreToolUse"), Running),
            (hook("PermissionRequest"), Permission),
            (notify("permission_prompt"), Permission),
            (hook("PostToolUse"), Running),
            (hook("Stop"), YourTurn),
            (notify("idle_prompt"), YourTurn),
            (Input::Viewed, Idle),
        ] {
            s = st(s, input);
            assert_eq!(s, want, "{input:?}");
        }
    }
}
