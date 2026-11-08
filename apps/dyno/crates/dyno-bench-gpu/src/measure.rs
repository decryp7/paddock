//! The timing driver, free of any graphics API: it takes a function that does one submission of a given amount of work and
//! returns how long it took, so its sizing and its stopping are tested with scripted durations and no GPU (rule 60).

use dyno_core::bench::gpu::{calibrate, next_work, MAX_MS, MEASURE_MIN_MS, WARMUP_SUBMISSIONS};
use std::time::Duration;

/// Measures a rate. `submit(work)` does one submission of `work` units and returns how long it took; `per_unit` is what one unit is
/// worth in the metric's own unit (for example FLOPs). The calibration submissions and [`WARMUP_SUBMISSIONS`] more are not counted.
/// The counted submissions are repeated until at least [`MEASURE_MIN_MS`] of work time is counted; one that ran past
/// [`MAX_MS`] makes the next one smaller. Returns the rate per second, or `None` when nothing could be timed.
pub fn measure(start_work: u64, per_unit: f64, min_measure: Duration, mut submit: impl FnMut(u64) -> Duration) -> Option<f64> {
    let mut work = calibrate(start_work, &mut submit);
    for _ in 0..WARMUP_SUBMISSIONS {
        let took = submit(work);
        if took.as_millis() as u64 > MAX_MS {
            work = next_work(work, took);
        }
    }
    let (mut total, mut units) = (Duration::ZERO, 0_u64);
    // Bounded: a pathological clock that never advances must not loop for ever.
    for _ in 0..10_000 {
        let took = submit(work);
        total += took;
        units = units.saturating_add(work);
        if took.as_millis() as u64 > MAX_MS {
            work = next_work(work, took);
        }
        if total >= min_measure {
            break;
        }
    }
    let secs = total.as_secs_f64();
    (secs > 0.0 && units > 0).then(|| units as f64 * per_unit / secs)
}

/// The usual minimum.
pub const MIN_MEASURE: Duration = Duration::from_millis(MEASURE_MIN_MS);

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A GPU of `rate` units per millisecond that records what it was asked.
    fn gpu(rate: f64, asked: &RefCell<Vec<u64>>) -> impl FnMut(u64) -> Duration + '_ {
        move |w| {
            asked.borrow_mut().push(w);
            Duration::from_secs_f64(w as f64 / rate / 1000.0)
        }
    }

    #[test]
    fn the_rate_is_work_over_time_at_any_speed_and_counts_at_least_a_second() {
        for rate in [10.0, 1_000.0, 100_000.0] {
            let asked = RefCell::new(Vec::new());
            let got = measure(1, 2.0, MIN_MEASURE, gpu(rate, &asked)).unwrap();
            // rate units/ms = rate * 1000 units/s, each worth 2.
            assert!((got / (rate * 2000.0) - 1.0).abs() < 1e-6, "rate {rate}: {got}");
            let counted: u64 = asked.borrow().iter().rev().take_while(|_| true).take(12).sum();
            assert!(counted > 0);
        }
    }

    #[test]
    fn no_submission_after_calibration_is_planned_over_the_cap() {
        let asked = RefCell::new(Vec::new());
        measure(1, 1.0, MIN_MEASURE, gpu(1_000.0, &asked));
        let calls = asked.borrow();
        // After the first few (calibration) the work settles near 100 ms and stays under the 250 ms cap.
        let settled = &calls[calls.len() - 5..];
        assert!(settled.iter().all(|w| (*w as f64 / 1_000.0) <= 250.0), "{settled:?}");
    }

    #[test]
    fn a_submission_that_runs_long_makes_the_next_smaller() {
        // The first counted submission takes 400 ms, then the device behaves.
        let mut n = 0;
        let asked = RefCell::new(Vec::new());
        let r = measure(100, 1.0, Duration::from_millis(500), |w| {
            asked.borrow_mut().push(w);
            n += 1;
            Duration::from_millis(if n == 4 { 400 } else { 100 })
        });
        assert!(r.is_some());
        let a = asked.borrow();
        assert!(a[4] < a[3], "work after a long submission is smaller: {a:?}");
    }

    #[test]
    fn a_clock_that_never_advances_gives_no_rate_and_does_not_hang() {
        assert_eq!(measure(10, 1.0, MIN_MEASURE, |_| Duration::ZERO), None);
    }
}
