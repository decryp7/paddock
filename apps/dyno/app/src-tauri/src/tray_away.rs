//! The delay before the tray items show "the agent is away" (a dimmed dash at the same width): the agent restarts in about
//! this long, and the bar must not blink for it. The wait runs on the injected clock (rule 60, ADR 0041), so a test moves time
//! itself.

use paddock_jobs::{CancelToken, Clock};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long the agent must stay away before the items show it.
pub const AWAY_DELAY: Duration = Duration::from_secs(3);

/// Runs a callback once the agent has been away for [`AWAY_DELAY`], unless it came back first.
pub struct AwayTimer {
    clock: Arc<dyn Clock>,
    pending: Mutex<Option<CancelToken>>,
}

impl AwayTimer {
    /// A timer on `clock`.
    pub fn new(clock: Arc<dyn Clock>) -> AwayTimer {
        AwayTimer {
            clock,
            pending: Mutex::new(None),
        }
    }

    /// The agent is away: `then` runs after [`AWAY_DELAY`] on the clock unless [`back`](Self::back) or another `lost` comes first.
    pub fn lost(&self, then: impl FnOnce() + Send + 'static) {
        let token = CancelToken::new();
        if let Some(old) = self.pending.lock().unwrap_or_else(|e| e.into_inner()).replace(token.clone()) {
            old.cancel();
        }
        let clock = Arc::clone(&self.clock);
        std::thread::spawn(move || {
            if clock.wait(AWAY_DELAY, &token).is_ok() {
                then();
            }
        });
    }

    /// The agent is back (or the app is going): a waiting `lost` does nothing.
    pub fn back(&self) {
        if let Some(old) = self.pending.lock().unwrap_or_else(|e| e.into_inner()).take() {
            old.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_jobs::FakeClock;
    use std::sync::atomic::{AtomicU32, Ordering};

    const GUARD: Duration = Duration::from_secs(10);

    fn counter() -> (Arc<AtomicU32>, impl FnOnce() + Send + 'static) {
        let n = Arc::new(AtomicU32::new(0));
        let n2 = Arc::clone(&n);
        (n, move || {
            n2.fetch_add(1, Ordering::SeqCst);
        })
    }

    /// Waits (in real time, with a guard) until the callback has run `n` times; the clock decides when it may.
    fn ran(n: &AtomicU32, want: u32) -> bool {
        let start = std::time::Instant::now();
        while n.load(Ordering::SeqCst) < want && start.elapsed() < Duration::from_secs(2) {
            std::thread::yield_now();
        }
        n.load(Ordering::SeqCst) == want
    }

    #[test]
    fn the_dash_shows_after_three_seconds_of_clock_time_and_not_before() {
        let clock = FakeClock::new();
        let timer = AwayTimer::new(Arc::new(clock.clone()));
        let (n, then) = counter();
        timer.lost(then);
        assert!(clock.wait_for_pending(1, GUARD));
        clock.advance_ms(2_999);
        assert!(!ran_within_a_moment(&n), "not yet at 2.999 s");
        clock.advance_ms(1);
        assert!(ran(&n, 1), "at 3 s");
    }

    /// A short real-time look that nothing ran (the clock has not reached the end, so nothing can).
    fn ran_within_a_moment(n: &AtomicU32) -> bool {
        for _ in 0..1_000 {
            std::thread::yield_now();
        }
        n.load(Ordering::SeqCst) > 0
    }

    #[test]
    fn coming_back_before_three_seconds_shows_nothing() {
        let clock = FakeClock::new();
        let timer = AwayTimer::new(Arc::new(clock.clone()));
        let (n, then) = counter();
        timer.lost(then);
        assert!(clock.wait_for_pending(1, GUARD));
        clock.advance_ms(2_000);
        timer.back();
        clock.advance_ms(10_000);
        assert!(!ran_within_a_moment(&n));
    }

    #[test]
    fn a_second_loss_restarts_the_wait_and_recovery_after_the_dash_works_again() {
        let clock = FakeClock::new();
        let timer = AwayTimer::new(Arc::new(clock.clone()));
        let (n1, then1) = counter();
        timer.lost(then1);
        assert!(clock.wait_for_pending(1, GUARD));
        clock.advance_ms(2_000);
        let (n2, then2) = counter();
        timer.lost(then2);
        assert!(clock.wait_for_pending(1, GUARD));
        clock.advance_ms(3_000);
        assert!(ran(&n2, 1), "the second wait ran its full three seconds");
        assert_eq!(n1.load(Ordering::SeqCst), 0, "the first was replaced");
        // The dash is up; the agent returns and goes away again: a new wait, a new dash.
        timer.back();
        let (n3, then3) = counter();
        timer.lost(then3);
        assert!(clock.wait_for_pending(1, GUARD));
        clock.advance_ms(3_000);
        assert!(ran(&n3, 1));
    }
}
