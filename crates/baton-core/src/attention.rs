//! Which sessions need the user, and which one to jump to next.

use baton_proto::Status;

/// Whether a session in `status` is waiting for the user.
pub fn needs_attention(status: Status) -> bool {
    matches!(status, Status::Permission | Status::YourTurn)
}

/// The index of the next session needing attention after `current`, in
/// sidebar `order`, wrapping around and never returning `current` itself.
/// `current` is `None` when nothing is selected.
pub fn next_after(order: &[Status], current: Option<usize>) -> Option<usize> {
    let len = order.len();
    let start = current.map_or(0, |c| c.saturating_add(1));
    (0..len)
        .map(|i| start.wrapping_add(i) % len)
        .find(|&idx| Some(idx) != current && needs_attention(order[idx]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use Status::*;

    #[test]
    fn only_permission_and_your_turn_need_attention() {
        for (s, want) in [
            (Starting, false),
            (Running, false),
            (Permission, true),
            (YourTurn, true),
            (Idle, false),
            (Exited(0), false),
            (Exited(1), false),
            (Unknown, false),
        ] {
            assert_eq!(needs_attention(s), want, "{s:?}");
        }
    }

    #[test]
    fn picks_the_next_one_after_current() {
        let order = [Idle, YourTurn, Running, Permission, YourTurn];
        assert_eq!(next_after(&order, Some(0)), Some(1));
        assert_eq!(next_after(&order, Some(1)), Some(3));
        assert_eq!(next_after(&order, Some(2)), Some(3));
        assert_eq!(next_after(&order, Some(3)), Some(4));
    }

    #[test]
    fn wraps_around() {
        let order = [Permission, Idle, YourTurn];
        assert_eq!(next_after(&order, Some(2)), Some(0));
        assert_eq!(next_after(&order, Some(0)), Some(2));
    }

    #[test]
    fn excludes_the_current_session() {
        assert_eq!(next_after(&[Idle, YourTurn, Idle], Some(1)), None);
        assert_eq!(next_after(&[YourTurn], Some(0)), None);
    }

    #[test]
    fn nothing_selected_starts_from_the_top() {
        assert_eq!(next_after(&[Idle, YourTurn, Permission], None), Some(1));
    }

    #[test]
    fn none_when_nothing_needs_attention_or_empty() {
        assert_eq!(next_after(&[Idle, Running, Exited(0)], Some(0)), None);
        assert_eq!(next_after(&[], None), None);
        assert_eq!(next_after(&[YourTurn], Some(7)), Some(0), "stale index");
    }
}
