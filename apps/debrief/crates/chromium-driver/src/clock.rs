//! Time budgets that do not count a stall of this process.
//!
//! A wait for the browser is bounded, but a bound read off the wall clock also counts time in which nothing ran: the
//! process or its virtual machine was stopped (a paused container, a host so overloaded that the VM got no CPU for
//! seconds; `Instant` already leaves out a sleeping computer on macOS and Linux). The browser stopped with us, so it
//! could not answer, and when everything runs again the wait has "timed out" although the browser never had the time.
//! Two live-browser tests failed together that way (Debrief's renderer tests, both with a `Page.enable` timed out at the
//! same moment) after one 10 s stall of the test container.
//!
//! [`Budget`] counts two kinds of time. Work between waits (a DevTools call, reading a file) counts in full, however
//! slow. A wait counts as the time it took, unless it ended more than [`STALL`] later than it asked: then it was a
//! stall, and counts as what it asked for. A wall-clock ceiling ([`CEILING`] times the bound) ends the budget whatever
//! the stalls, so a browser that never answers fails within a few times its bound at worst.
//!
//! The budget has to be told where each wait begins and ends ([`Budget::next_wait_at`], [`Budget::waited_at`]);
//! [`poll`] and [`Budget::wait`] do that for the usual loops.

use std::time::{Duration, Instant};

/// The longest single wait; a stall is seen at the end of the wait it fell in.
pub const SLICE: Duration = Duration::from_millis(100);
/// A wait that ends this much later than it asked for was a stall of the process, not time the browser had. Long
/// enough that a merely busy machine (threads waking a few hundred milliseconds late) is not taken for one.
pub const STALL: Duration = Duration::from_secs(2);
/// The wall-clock ceiling of a budget, as a multiple of its bound: stalls are forgiven up to this.
pub const CEILING: u32 = 3;

/// The time source of a [`poll`] (tests pass their own).
pub trait Clock {
    fn now(&self) -> Instant;
    fn sleep(&self, d: Duration);
}

/// The real clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// What is left of a bounded wait (see the module documentation).
#[derive(Debug, Clone)]
pub struct Budget {
    left: Duration,
    ceiling: Instant,
    /// Up to when time has been counted.
    counted_to: Instant,
    /// The wait that began at `counted_to`, if one did.
    asked: Option<Duration>,
}

impl Budget {
    /// A budget of `total` starting now.
    pub fn new(total: Duration) -> Budget {
        Budget::starting(total, Instant::now())
    }

    /// A budget of `total` starting at `now`.
    pub fn starting(total: Duration, now: Instant) -> Budget {
        Budget {
            left: total,
            ceiling: now + total * CEILING,
            counted_to: now,
            asked: None,
        }
    }

    /// Before a wait, at `now`: counts the time since the last wait ended (work) in full, and returns how long to wait
    /// (at most [`SLICE`]), or `None` once the budget or its ceiling is spent. Tell the end of the wait to
    /// [`Budget::waited_at`]; without it, the wait counts as work.
    pub fn next_wait_at(&mut self, now: Instant) -> Option<Duration> {
        self.count_to(now, false);
        if self.left.is_zero() || now >= self.ceiling {
            return None;
        }
        let wait = self.left.min(SLICE);
        self.asked = Some(wait);
        Some(wait)
    }

    /// After the wait [`Budget::next_wait_at`] returned ended, at `now`: counts it, or only what it asked for when it
    /// ended more than [`STALL`] late.
    pub fn waited_at(&mut self, now: Instant) {
        self.count_to(now, true);
    }

    fn count_to(&mut self, now: Instant, waited: bool) {
        let passed = now.saturating_duration_since(self.counted_to);
        let counted = match self.asked.take() {
            Some(asked) if waited && passed > asked + STALL => {
                log::info!("A stall of {passed:?} (the process did not run) is not counted against a browser wait");
                asked
            }
            _ => passed,
        };
        self.left = self.left.saturating_sub(counted);
        self.counted_to = self.counted_to.max(now);
    }

    /// [`Budget::next_wait_at`] now.
    pub fn next_wait(&mut self) -> Option<Duration> {
        self.next_wait_at(Instant::now())
    }

    /// [`Budget::waited_at`] now.
    pub fn waited(&mut self) {
        self.waited_at(Instant::now());
    }

    /// One wait: `f` waits for at most the duration it is given; `None` (and `f` not called) once the budget is spent.
    pub fn wait<R>(&mut self, f: impl FnOnce(Duration) -> R) -> Option<R> {
        let d = self.next_wait()?;
        let r = f(d);
        self.waited();
        Some(r)
    }

    /// The budget left.
    pub fn left(&self) -> Duration {
        self.left
    }
}

/// Calls `probe` until it finds something (`Ok(Some)`), fails, or `total` is spent (`Ok(None)`), sleeping at most
/// `every` between calls. The probes' own time counts in full; only a stalled sleep is forgiven ([`Budget`]).
pub fn poll<T, E>(
    clock: &dyn Clock,
    total: Duration,
    every: Duration,
    mut probe: impl FnMut() -> Result<Option<T>, E>,
) -> Result<Option<T>, E> {
    let mut budget = Budget::starting(total, clock.now());
    loop {
        if let Some(found) = probe()? {
            return Ok(Some(found));
        }
        let Some(wait) = budget.next_wait_at(clock.now()) else {
            return Ok(None);
        };
        clock.sleep(wait.min(every));
        budget.waited_at(clock.now());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::Cell;

    /// A clock that moves only when told: by a sleep (plus a stall at one of them), or by [`FakeClock::advance`] (work).
    pub(crate) struct FakeClock {
        t: Cell<Instant>,
        /// Extra time added to the sleep with this index (counted from 0): a stall.
        stall: Option<(usize, Duration)>,
        sleeps: Cell<usize>,
    }

    impl FakeClock {
        pub(crate) fn new(stall: Option<(usize, Duration)>) -> FakeClock {
            FakeClock {
                t: Cell::new(Instant::now()),
                stall,
                sleeps: Cell::new(0),
            }
        }
        pub(crate) fn advance(&self, d: Duration) {
            self.t.set(self.t.get() + d);
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            self.t.get()
        }
        fn sleep(&self, d: Duration) {
            let n = self.sleeps.get();
            self.sleeps.set(n + 1);
            let extra = match self.stall {
                Some((i, s)) if i == n => s,
                _ => Duration::ZERO,
            };
            self.advance(d + extra);
        }
    }

    /// Waits of what the budget asks plus `late(i)` each, with `work` between them; returns how many waits fit.
    fn waits(total: Duration, work: Duration, late: impl Fn(usize) -> Duration) -> usize {
        let mut now = Instant::now();
        let mut b = Budget::starting(total, now);
        let mut n = 0;
        while let Some(w) = b.next_wait_at(now) {
            now += w + late(n);
            b.waited_at(now);
            now += work;
            n += 1;
            assert!(n < 10_000, "the budget never ends");
        }
        n
    }

    const NONE: Duration = Duration::ZERO;

    #[test]
    fn waits_on_time_spend_the_whole_budget_in_slices() {
        assert_eq!(waits(Duration::from_secs(1), NONE, |_| NONE), 10);
        assert_eq!(
            waits(Duration::from_millis(250), NONE, |_| NONE),
            3,
            "the last slice is what is left"
        );
    }

    #[test]
    fn a_stall_counts_as_the_wait_it_fell_in_not_as_the_time_that_passed() {
        // A 10 s stall (the CI container got no CPU) in the third wait of a 5 s budget: still fifty waits.
        let n = waits(
            Duration::from_secs(5),
            NONE,
            |i| if i == 2 { Duration::from_secs(10) } else { NONE },
        );
        assert_eq!(n, 50);
    }

    #[test]
    fn ordinary_slowness_counts_in_full() {
        // Every wait ends 400 ms late (a busy machine that still runs): 500 ms each, so a 1 s budget is two waits.
        assert_eq!(waits(Duration::from_secs(1), NONE, |_| Duration::from_millis(400)), 2);
        // Up to the stall bound, lateness is counted: one wait 1.9 s late spends a 1 s budget.
        assert_eq!(waits(Duration::from_secs(1), NONE, |_| Duration::from_millis(1900)), 1);
    }

    /// Regression (review): the time between waits was taken for part of the wait, so 2.5 s of work (a slow DevTools
    /// call) looked like a stall and counted as 0.1 s; a 5 s bound stretched to minutes.
    #[test]
    fn work_between_waits_counts_in_full_however_slow() {
        assert_eq!(waits(Duration::from_secs(5), Duration::from_millis(2500), |_| NONE), 2);
    }

    #[test]
    fn the_ceiling_ends_a_budget_whatever_the_stalls() {
        // Every wait stalls 10 s: each counts as 100 ms, but the wall-clock ceiling (3 x 1 s) ends it after one.
        assert_eq!(waits(Duration::from_secs(1), NONE, |_| Duration::from_secs(10)), 1);
    }

    #[test]
    fn a_wait_not_reported_counts_as_work_and_an_empty_budget_waits_not_at_all() {
        let t0 = Instant::now();
        let mut b = Budget::starting(Duration::from_secs(1), t0);
        assert_eq!(b.next_wait_at(t0), Some(SLICE));
        assert_eq!(b.next_wait_at(t0 + Duration::from_secs(30)), None, "30 s unreported: spent");
        assert_eq!(Budget::starting(Duration::ZERO, t0).next_wait_at(t0), None);
    }

    #[test]
    fn a_clock_that_goes_backwards_counts_nothing() {
        let t0 = Instant::now() + Duration::from_secs(5);
        let mut b = Budget::starting(Duration::from_secs(1), t0);
        assert!(b.next_wait_at(t0 - Duration::from_secs(1)).is_some());
        assert_eq!(b.left(), Duration::from_secs(1));
    }

    #[test]
    fn poll_counts_slow_probes_in_full() {
        // Each probe takes 2.5 s (a slow `Target.getTargets`): a 5 s poll gives up after two of them.
        let clock = FakeClock::new(None);
        let mut probes = 0;
        let r: Result<Option<()>, ()> = poll(&clock, Duration::from_secs(5), Duration::from_millis(50), || {
            probes += 1;
            clock.advance(Duration::from_millis(2500));
            Ok(None)
        });
        assert_eq!((r, probes), (Ok(None), 2));
    }

    #[test]
    fn poll_forgives_a_stalled_sleep() {
        // Instant probes, 50 ms sleeps, and the fourth sleep stalls 8 s: the 5 s budget still gets its full hundred
        // sleeps (the stalled one counted as the 100 ms it asked for), inside the 15 s ceiling.
        let clock = FakeClock::new(Some((3, Duration::from_secs(8))));
        let mut probes = 0;
        let r: Result<Option<()>, ()> = poll(&clock, Duration::from_secs(5), Duration::from_millis(50), || {
            probes += 1;
            Ok(None)
        });
        assert_eq!((r, probes), (Ok(None), 100));
    }

    #[test]
    fn poll_returns_what_the_probe_found_or_its_error() {
        let clock = FakeClock::new(None);
        let mut n = 0;
        assert_eq!(
            poll::<_, ()>(&clock, Duration::from_secs(1), SLICE, || {
                n += 1;
                Ok((n == 3).then_some(n))
            }),
            Ok(Some(3))
        );
        assert_eq!(poll::<(), _>(&clock, Duration::from_secs(1), SLICE, || Err("gone")), Err("gone"));
    }
}
