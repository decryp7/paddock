//! Time budgets that count only the time this process could run.
//!
//! A wait for the browser is bounded, but a bound read off the wall clock also counts time in which nothing ran: the
//! machine or the virtual machine was stopped (sleep, a paused container, a host so overloaded that the VM got no CPU
//! for seconds). The browser stopped with us, so it could not answer, and when everything runs again the wait has
//! "timed out" although the browser never had the time. Two live-browser tests failed together that way (Debrief's
//! renderer tests, both with a `Page.enable` timed out at the same moment) after one 10 s stall of the test container.
//!
//! [`Budget`] waits in short slices and looks at how late each one ended: a slice that ended much later than asked was
//! a stall, and counts as the slice it asked for, not as the time that passed. Ordinary slowness (a busy but running
//! machine, a wake-up up to [`STALL`] late) still counts in full, so a browser that does not answer fails within its
//! bound plus the stalls, and only a gap in which no thread of ours ran for seconds is discounted.

use std::time::{Duration, Instant};

/// The longest single wait; a stall is seen at the end of the slice it fell in.
pub const SLICE: Duration = Duration::from_millis(100);
/// A slice that ends this much later than it asked for was a stall of the process, not time the browser had. Long
/// enough that a merely busy machine (threads waking a few hundred milliseconds late) is not taken for one.
pub const STALL: Duration = Duration::from_secs(2);

/// What is left of a bounded wait (see the module documentation).
#[derive(Debug, Clone)]
pub struct Budget {
    left: Duration,
    last: Instant,
    asked: Duration,
}

impl Budget {
    /// A budget of `total` starting now.
    pub fn new(total: Duration) -> Budget {
        Budget::starting(total, Instant::now())
    }

    /// A budget of `total` starting at `now` (tests pass their own instants).
    pub fn starting(total: Duration, now: Instant) -> Budget {
        Budget {
            left: total,
            last: now,
            asked: Duration::ZERO,
        }
    }

    /// The next wait (at most [`SLICE`]), or `None` once the budget is spent. Call it again after each wait.
    pub fn next_wait(&mut self) -> Option<Duration> {
        self.next_wait_at(Instant::now())
    }

    /// [`Budget::next_wait`] with the time of the call given.
    pub fn next_wait_at(&mut self, now: Instant) -> Option<Duration> {
        let passed = now.saturating_duration_since(self.last);
        let counted = if passed > self.asked + STALL { self.asked } else { passed };
        self.left = self.left.saturating_sub(counted);
        self.last = now;
        if self.left.is_zero() {
            return None;
        }
        self.asked = self.left.min(SLICE);
        Some(self.asked)
    }

    /// The budget left (after the last [`Budget::next_wait`]).
    pub fn left(&self) -> Duration {
        self.left
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs waits of what the budget asks plus `late` each, from `t0`; returns how many waits fit.
    fn waits(total: Duration, late: impl Fn(usize) -> Duration) -> usize {
        let t0 = Instant::now();
        let mut b = Budget::starting(total, t0);
        let mut now = t0;
        let mut n = 0;
        while let Some(w) = b.next_wait_at(now) {
            now += w + late(n);
            n += 1;
            assert!(n < 10_000, "the budget never ends");
        }
        n
    }

    #[test]
    fn waits_on_time_spend_the_whole_budget_in_slices() {
        assert_eq!(waits(Duration::from_secs(1), |_| Duration::ZERO), 10);
        assert_eq!(
            waits(Duration::from_millis(250), |_| Duration::ZERO),
            3,
            "the last slice is what is left"
        );
    }

    #[test]
    fn a_stall_counts_as_the_slice_it_fell_in_not_as_the_time_that_passed() {
        // A 10 s stall (the CI container got no CPU) in the third slice of a 1 s budget: still ten slices of waiting.
        let n = waits(Duration::from_secs(1), |i| {
            if i == 2 {
                Duration::from_secs(10)
            } else {
                Duration::ZERO
            }
        });
        assert_eq!(n, 10);
    }

    #[test]
    fn ordinary_slowness_counts_in_full() {
        // Every slice ends 400 ms late (a busy machine that still runs): 500 ms each, so a 1 s budget is two waits.
        assert_eq!(waits(Duration::from_secs(1), |_| Duration::from_millis(400)), 2);
        // Up to the stall bound, lateness is counted: one wait 1.9 s late spends a 1 s budget.
        assert_eq!(waits(Duration::from_secs(1), |_| Duration::from_millis(1900)), 1);
    }

    #[test]
    fn a_stall_before_the_first_wait_is_not_counted_and_an_empty_budget_waits_not_at_all() {
        let t0 = Instant::now();
        let mut b = Budget::starting(Duration::from_secs(1), t0);
        assert_eq!(b.next_wait_at(t0 + Duration::from_secs(30)), Some(SLICE));
        assert_eq!(b.left(), Duration::from_secs(1));
        assert_eq!(Budget::starting(Duration::ZERO, t0).next_wait_at(t0), None);
    }

    #[test]
    fn a_clock_that_goes_backwards_counts_nothing() {
        let t0 = Instant::now() + Duration::from_secs(5);
        let mut b = Budget::starting(Duration::from_secs(1), t0);
        assert!(b.next_wait_at(t0 - Duration::from_secs(1)).is_some());
        assert_eq!(b.left(), Duration::from_secs(1));
    }
}
