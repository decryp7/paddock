//! Judging a wait for a shared slot by progress, never by its length alone: a waiter behind a queue that moves, or behind a holder that
//! is running within its own bound, keeps waiting; one that sees the same picture for the whole limit has stalled. The one mechanism
//! of every wait of the push gate (heavy's slot queue and the build-folder lock). Pure: pictures and microseconds in, a verdict out, so
//! every rule is tested with a fake clock and no sleep.

use std::time::Duration;

/// One holder of a slot, as a waiter reads it from the holder's own record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    /// The holder's agent name.
    pub name: String,
    /// Its process id.
    pub pid: u32,
    /// When its record was written (seconds since the Unix epoch): a slot retaken by the same agent has a new one.
    pub epoch: Option<u64>,
}

/// What a waiter can see of the queue at one moment. Any change is progress: a slot taken or released (the holders change), or the
/// waiter's place moving. A picture that is `live` is progress by itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QueueView {
    /// The waiter's 1-based place in the ticket queue (0: no ticket).
    pub position: usize,
    /// Who holds a slot.
    pub holders: Vec<Holder>,
    /// A holder is running and is still inside the bound that holder lives under (its own cap): a step that is healthy and
    /// merely long is not a stall. Past its cap it is not live, and its own guard ends it, which changes the picture.
    pub live: bool,
}

impl QueueView {
    /// The holders in words for a message: `name (pid 12), other (pid 34)`; says so when nobody holds a slot.
    pub fn holders_text(&self) -> String {
        if self.holders.is_empty() {
            return "nobody (the load or the queue rule keeps the slot closed)".to_string();
        }
        self.holders
            .iter()
            .map(|h| format!("{} (pid {})", h.name, h.pid))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Whether a holder whose record was written at `epoch` is still inside `cap` at `now` (both seconds). An unknown start is never live.
pub fn within_cap(epoch: Option<u64>, now: u64, cap: Duration) -> bool {
    epoch.is_some_and(|e| now.saturating_sub(e) < cap.as_secs())
}

/// The stall judgement of one wait.
#[derive(Debug)]
pub struct StallClock {
    limit: u64,
    seen: Option<QueueView>,
    since: u64,
}

impl StallClock {
    /// A wait that began at `now` (microseconds) and stalls after `limit` without progress.
    pub fn new(limit: Duration, now: u64) -> StallClock {
        StallClock {
            limit: u64::try_from(limit.as_micros()).unwrap_or(u64::MAX),
            seen: None,
            since: now,
        }
    }

    /// Looks at the queue at `now`. `Some(quiet seconds)` when it showed the same picture, with no live holder, for the whole limit;
    /// any change, or a live holder, starts the count again.
    pub fn observe(&mut self, view: &QueueView, now: u64) -> Option<f64> {
        if view.live || self.seen.as_ref() != Some(view) {
            self.seen = Some(view.clone());
            self.since = now;
            return None;
        }
        let quiet = now.saturating_sub(self.since);
        (quiet >= self.limit).then(|| quiet as f64 / 1_000_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    fn view(pos: usize, holder: &str, epoch: u64, live: bool) -> QueueView {
        QueueView {
            position: pos,
            holders: vec![Holder {
                name: holder.into(),
                pid: 7,
                epoch: Some(epoch),
            }],
            live,
        }
    }

    #[test]
    fn a_picture_that_never_changes_stalls_at_the_limit_and_not_before() {
        let mut c = StallClock::new(Duration::from_secs(100), 0);
        let v = view(2, "a", 1, false);
        assert_eq!(c.observe(&v, 0), None);
        assert_eq!(c.observe(&v, 99 * S), None);
        assert_eq!(c.observe(&v, 100 * S), Some(100.0));
    }

    #[test]
    fn a_queue_that_moves_is_waited_for_far_past_the_limit() {
        let mut c = StallClock::new(Duration::from_secs(100), 0);
        for i in 0..60u64 {
            assert_eq!(
                c.observe(&view(3, "a", i, false), i * 60 * S),
                None,
                "step {i}"
            );
        }
    }

    #[test]
    fn my_place_moving_counts_as_progress() {
        let mut c = StallClock::new(Duration::from_secs(100), 0);
        assert_eq!(c.observe(&view(3, "a", 1, false), 0), None);
        assert_eq!(c.observe(&view(2, "a", 1, false), 90 * S), None);
        assert_eq!(c.observe(&view(2, "a", 1, false), 180 * S), None);
        assert_eq!(c.observe(&view(2, "a", 1, false), 190 * S), Some(100.0));
    }

    /// A holder that runs for an hour inside its own cap is progress for as long as it is live, even though nothing in the queue
    /// changes; once it is past its cap the count starts, and it stalls a whole limit later.
    #[test]
    fn a_live_holder_inside_its_cap_is_progress_and_one_past_it_is_not() {
        let mut c = StallClock::new(Duration::from_secs(100), 0);
        for t in (0..=3600).step_by(30) {
            assert_eq!(c.observe(&view(2, "a", 1, true), t * S), None, "t={t}");
        }
        // The holder's cap passed at 3600 s: not live any more, and nothing else moved.
        assert_eq!(c.observe(&view(2, "a", 1, false), 3630 * S), None);
        assert_eq!(c.observe(&view(2, "a", 1, false), 3729 * S), None);
        assert_eq!(c.observe(&view(2, "a", 1, false), 3730 * S), Some(100.0));
    }

    #[test]
    fn a_holder_is_inside_its_cap_only_with_a_known_start() {
        let cap = Duration::from_secs(3600);
        assert!(within_cap(Some(1000), 1000 + 3599, cap));
        assert!(!within_cap(Some(1000), 1000 + 3600, cap));
        assert!(!within_cap(None, 1000, cap));
    }

    #[test]
    fn the_holders_are_named_for_the_message() {
        assert_eq!(view(2, "a", 1, false).holders_text(), "a (pid 7)");
        assert!(QueueView::default().holders_text().starts_with("nobody"));
    }
}
