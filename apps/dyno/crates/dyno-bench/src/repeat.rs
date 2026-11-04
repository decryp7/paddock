//! Warm-up and repetitions (ADR 0030 section 3): one warm-up that is not counted, then `reps` measured repetitions with a
//! gap between them so the heat of one does not carry into the next. The driver takes the clock and the pause as ports,
//! so its tests move a [`ManualClock`](paddock_bench::ManualClock) instead of waiting (rule 60).

use dyno_core::bench::{GAP_MILLIS, REPETITIONS, WARMUP_MILLIS};
use paddock_bench::Clock;
use std::time::Duration;

/// How a kernel is repeated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Repeats {
    /// The time the uncounted warm-up runs for (zero: none).
    pub warmup: Duration,
    /// Measured repetitions, at least one.
    pub reps: u32,
    /// The pause between two repetitions.
    pub gap: Duration,
}

impl Repeats {
    /// The method of ADR 0030: 1 s warm-up, 5 repetitions, 2 s gap.
    pub const STANDARD: Repeats = Repeats {
        warmup: Duration::from_millis(WARMUP_MILLIS),
        reps: REPETITIONS,
        gap: Duration::from_millis(GAP_MILLIS),
    };

    /// One repetition and nothing else (smoke runs and tests of the real binary).
    pub const SINGLE: Repeats = Repeats {
        warmup: Duration::ZERO,
        reps: 1,
        gap: Duration::ZERO,
    };
}

/// Where a repetition is: what the kernel closure is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The uncounted warm-up; the kernel runs for the given time.
    Warmup(Duration),
    /// Repetition `n` (from 1) of `of`.
    Rep {
        /// This repetition, from 1.
        n: u32,
        /// How many there are.
        of: u32,
    },
}

/// Waits. A port so a test needs no real time.
pub trait Pace {
    /// Pauses for about `d` (it may return early; the driver asks again for what is left).
    fn pause(&self, d: Duration);
}

/// Sleeps the thread.
pub struct SleepPace;

impl Pace for SleepPace {
    fn pause(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// Runs `step` for the warm-up (its result is dropped) and then for each repetition, keeping `gap` between repetitions by the
/// clock; `gap(done, of)` is called as each gap begins. Returns the results of the repetitions in order.
pub fn repeated<T>(
    clock: &dyn Clock,
    pace: &dyn Pace,
    r: Repeats,
    mut step: impl FnMut(Step) -> T,
    mut gap: impl FnMut(u32, u32),
) -> Vec<T> {
    if !r.warmup.is_zero() {
        drop(step(Step::Warmup(r.warmup)));
    }
    let of = r.reps.max(1);
    let mut out = Vec::with_capacity(of as usize);
    for n in 1..=of {
        out.push(step(Step::Rep { n, of }));
        if n < of {
            // Told first, so the gap is a phase of its own that the person sees (ER-18).
            gap(n, of);
            keep_gap(clock, pace, r.gap);
        }
    }
    out
}

fn keep_gap(clock: &dyn Clock, pace: &dyn Pace, gap: Duration) {
    let from = clock.now();
    loop {
        let waited = clock.now().saturating_sub(from);
        if waited >= gap {
            return;
        }
        let before = clock.now();
        pace.pause(gap - waited);
        // A pause that moved nothing (a broken port) must not spin forever.
        if clock.now() <= before {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_bench::ManualClock;
    use std::cell::RefCell;

    /// Advances the manual clock by `fraction` of what it is asked to wait (so the driver has to ask again) and records the asks.
    struct FakePace<'a> {
        clock: &'a ManualClock,
        asked: RefCell<Vec<Duration>>,
        fraction: u32,
    }

    impl Pace for FakePace<'_> {
        fn pause(&self, d: Duration) {
            self.asked.borrow_mut().push(d);
            self.clock.advance(d / self.fraction);
        }
    }

    fn pace(clock: &ManualClock, fraction: u32) -> FakePace<'_> {
        FakePace {
            clock,
            asked: RefCell::new(Vec::new()),
            fraction,
        }
    }

    #[test]
    fn warm_up_is_not_counted_and_the_repetitions_come_in_order() {
        let clock = ManualClock::new();
        let p = pace(&clock, 1);
        let mut steps = Vec::new();
        let out = repeated(
            &clock,
            &p,
            Repeats::STANDARD,
            |s| {
                steps.push(s);
                steps.len()
            },
            |_, _| {},
        );
        assert_eq!(steps[0], Step::Warmup(Duration::from_secs(1)));
        assert_eq!(steps.len(), 6);
        assert_eq!(steps[1], Step::Rep { n: 1, of: 5 });
        assert_eq!(steps[5], Step::Rep { n: 5, of: 5 });
        assert_eq!(out, vec![2, 3, 4, 5, 6], "the warm-up's result is dropped");
    }

    #[test]
    fn the_gap_is_kept_between_repetitions_and_not_after_the_last() {
        let clock = ManualClock::new();
        let p = pace(&clock, 1);
        let mut gaps = Vec::new();
        repeated(&clock, &p, Repeats::STANDARD, |_| (), |done, of| gaps.push((done, of)));
        assert_eq!(gaps, vec![(1, 5), (2, 5), (3, 5), (4, 5)], "each gap is announced as it begins");
        assert_eq!(*p.asked.borrow(), vec![Duration::from_secs(2); 4]);
        assert_eq!(clock.now(), Duration::from_secs(8));
    }

    #[test]
    fn a_pause_that_returns_early_is_asked_again_for_what_is_left() {
        let clock = ManualClock::new();
        let p = pace(&clock, 2);
        repeated(
            &clock,
            &p,
            Repeats {
                warmup: Duration::ZERO,
                reps: 2,
                gap: Duration::from_secs(2),
            },
            |_| (),
            |_, _| {},
        );
        // Asked for 2 s, got 1 s; asked for the 1 s left, got 0.5 s; and so on until the gap is (all but) covered.
        assert!(clock.now() > Duration::from_millis(1999));
        let asked = p.asked.borrow();
        assert_eq!(asked[0], Duration::from_secs(2));
        assert_eq!(asked[1], Duration::from_secs(1));
    }

    #[test]
    fn a_pause_that_moves_nothing_does_not_hang_the_run() {
        struct Stuck;
        impl Pace for Stuck {
            fn pause(&self, _: Duration) {}
        }
        let clock = ManualClock::new();
        let out = repeated(&clock, &Stuck, Repeats::STANDARD, |s| matches!(s, Step::Rep { .. }), |_, _| {});
        assert_eq!(out.len(), 5);
    }

    #[test]
    fn single_and_zero_repetitions_run_once_without_warm_up_or_gap() {
        let clock = ManualClock::new();
        let p = pace(&clock, 1);
        let mut n = 0;
        let out = repeated(
            &clock,
            &p,
            Repeats::SINGLE,
            |_| {
                n += 1;
                n
            },
            |_, _| {},
        );
        assert_eq!(out, vec![1]);
        assert!(p.asked.borrow().is_empty());
        let zero = Repeats {
            reps: 0,
            ..Repeats::SINGLE
        };
        assert_eq!(repeated(&clock, &p, zero, |_| (), |_, _| {}).len(), 1, "at least one repetition");
    }
}
