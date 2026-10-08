//! Decides when to draw: on change, at most 60 fps, and not while the
//! application holds a synchronized-output frame open (for at most 50 ms).

use std::time::{Duration, Instant};

/// Minimum time between two renders (60 fps).
pub const MIN_INTERVAL: Duration = Duration::from_micros(16_667);
/// Longest a render is deferred while synchronized output is on.
pub const SYNC_TIMEOUT: Duration = Duration::from_millis(50);

/// Render scheduling state.
#[derive(Debug, Default)]
pub struct RenderPacer {
    dirty: bool,
    last_render: Option<Instant>,
    sync_since: Option<Instant>,
}

impl RenderPacer {
    /// A new pacer with nothing to draw.
    pub fn new() -> Self {
        Self::default()
    }

    /// Something changed; a render is needed.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Record the application's synchronized-output state.
    pub fn set_sync(&mut self, on: bool, now: Instant) {
        if !on {
            self.sync_since = None;
        } else if self.sync_since.is_none() {
            self.sync_since = Some(now);
        }
    }

    /// The earliest instant a pending render may happen, if one is pending.
    fn ready_at(&self, now: Instant) -> Option<Instant> {
        if !self.dirty {
            return None;
        }
        let by_rate = self.last_render.map(|t| t + MIN_INTERVAL);
        let by_sync = self.sync_since.map(|t| t + SYNC_TIMEOUT);
        Some(match (by_rate, by_sync) {
            (Some(a), Some(b)) => a.max(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => now,
        })
    }

    /// Whether to render right now.
    pub fn should_render(&self, now: Instant) -> bool {
        self.ready_at(now).is_some_and(|t| now >= t)
    }

    /// How long until a pending render is allowed; `None` when nothing is pending.
    pub fn next_deadline(&self, now: Instant) -> Option<Duration> {
        self.ready_at(now).map(|t| t.saturating_duration_since(now))
    }

    /// A render happened at `now`.
    pub fn rendered(&mut self, now: Instant) {
        self.dirty = false;
        self.last_render = Some(now);
        if self.sync_since.is_some() {
            self.sync_since = Some(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn idle_pacer_never_renders() {
        let p = RenderPacer::new();
        let now = Instant::now();
        assert!(!p.should_render(now));
        assert_eq!(p.next_deadline(now), None);
    }

    #[test]
    fn dirty_renders_immediately_then_rate_limits() {
        let t0 = Instant::now();
        let mut p = RenderPacer::new();
        p.mark_dirty();
        assert!(p.should_render(t0));
        p.rendered(t0);
        p.mark_dirty();
        assert!(!p.should_render(t0 + ms(5)));
        assert!(p.should_render(t0 + ms(17)));
        let wait = p.next_deadline(t0 + ms(5)).expect("pending");
        assert!(wait <= ms(12) && wait > ms(10));
    }

    #[test]
    fn sync_output_defers_up_to_50ms() {
        let t0 = Instant::now();
        let mut p = RenderPacer::new();
        p.set_sync(true, t0);
        p.mark_dirty();
        assert!(!p.should_render(t0 + ms(30)));
        assert!(p.should_render(t0 + ms(50)));
        p.rendered(t0 + ms(50));
        p.mark_dirty();
        // Still inside the frame: the deferral window restarts.
        assert!(!p.should_render(t0 + ms(70)));
        // Frame ends: render without waiting for the timeout.
        p.set_sync(false, t0 + ms(75));
        assert!(p.should_render(t0 + ms(75)));
    }
}
