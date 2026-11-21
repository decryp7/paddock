//! Judging a queue wait by progress, not by its length: a step that waits behind a moving queue keeps waiting; one whose queue made
//! no progress for the whole limit has stalled. Pure: tokens and microseconds in, a verdict out, so every rule is tested with a
//! fake clock and no sleep.

use std::time::Duration;

/// What a waiter can see of the queue at one moment. Any change is progress: a slot taken or released (the holders change), or this
/// waiter's place moving (up when others ahead were served, down when a priority run went first: either way the queue moves).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QueueView {
    /// This waiter's 1-based place in the ticket queue (0: no ticket, a high-priority run).
    pub position: usize,
    /// Who holds a slot: agent name, pid and the record's epoch (a slot retaken by the same agent has a new epoch).
    pub holders: Vec<(String, u32, Option<u64>)>,
}

impl QueueView {
    /// The holders in words for a message: `name (pid 12), other (pid 34)`; says so when nobody holds a slot.
    pub fn holders_text(&self) -> String {
        if self.holders.is_empty() {
            return "nobody (the load or the queue rule keeps the slot closed)".to_string();
        }
        self.holders
            .iter()
            .map(|(n, p, _)| format!("{n} (pid {p})"))
            .collect::<Vec<_>>()
            .join(", ")
    }
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

    /// Looks at the queue at `now`. `Some(quiet seconds)` when it showed the same picture for the whole limit; any change starts the
    /// count again.
    pub fn observe(&mut self, view: &QueueView, now: u64) -> Option<f64> {
        if self.seen.as_ref() != Some(view) {
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

    fn view(pos: usize, holder: &str, epoch: u64) -> QueueView {
        QueueView {
            position: pos,
            holders: vec![(holder.into(), 7, Some(epoch))],
        }
    }

    const S: u64 = 1_000_000;

    #[test]
    fn a_picture_that_never_changes_stalls_at_the_limit_and_not_before() {
        let mut c = StallClock::new(Duration::from_secs(100), 0);
        let v = view(2, "a", 1);
        assert_eq!(c.observe(&v, 0), None);
        assert_eq!(c.observe(&v, 99 * S), None);
        assert_eq!(c.observe(&v, 100 * S), Some(100.0));
    }

    #[test]
    fn a_queue_that_moves_is_waited_for_far_past_the_limit() {
        let mut c = StallClock::new(Duration::from_secs(100), 0);
        // a new holder every 60 s for an hour: never quiet for 100 s
        for i in 0..60u64 {
            assert_eq!(c.observe(&view(3, "a", i), i * 60 * S), None, "step {i}");
        }
    }

    #[test]
    fn my_place_moving_counts_as_progress() {
        let mut c = StallClock::new(Duration::from_secs(100), 0);
        assert_eq!(c.observe(&view(3, "a", 1), 0), None);
        assert_eq!(c.observe(&view(2, "a", 1), 90 * S), None);
        assert_eq!(c.observe(&view(2, "a", 1), 180 * S), None);
        assert_eq!(c.observe(&view(2, "a", 1), 190 * S), Some(100.0));
    }

    #[test]
    fn the_holders_are_named_for_the_message() {
        assert_eq!(view(2, "a", 1).holders_text(), "a (pid 7)");
        assert!(QueueView::default().holders_text().starts_with("nobody"));
    }
}
