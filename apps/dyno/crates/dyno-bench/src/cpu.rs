//! Integer work: a xorshift-and-multiply mix whose result is consumed, so it can't be optimised away. Reported as
//! million rounds per second on one core and on every core at once.

use crate::emit::{gap_label, kind_phase, pass_done_label, pass_label, Progress, WARMING_UP};
use crate::repeat::{repeated, Pace, Repeats, Step};
use dyno_core::bench::BenchMetric;
use paddock_bench::Clock;
use std::hint::black_box;
use std::time::{Duration, Instant};

/// Rounds done per timing check.
const CHUNK: u64 = 200_000;

/// One round of the mix.
#[inline]
pub fn mix(mut x: u64) -> u64 {
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(23)
}

/// Runs the mix for about `budget` and returns the rounds per second.
pub fn rounds_per_second(budget: Duration) -> f64 {
    let start = Instant::now();
    let mut x = 0x1234_5678_9ABC_DEF1_u64;
    let mut rounds = 0_u64;
    while start.elapsed() < budget {
        for _ in 0..CHUNK {
            x = mix(x);
        }
        rounds += CHUNK;
    }
    black_box(x);
    rounds as f64 / start.elapsed().as_secs_f64().max(1e-9)
}

/// The CPU metrics, each from every repetition: one core, all cores and the scaling between them. The warm-up runs both for
/// half its time each and is not counted.
pub fn run(budget: Duration, repeats: Repeats, clock: &dyn Clock, pace: &dyn Pace, progress: &dyn Progress) -> Vec<BenchMetric> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(256);
    let both = |b: Duration, label: &dyn Fn(&str) -> String, done: u32, of: u32| {
        progress.report(kind_phase("cpu", &label("Single core"), done, of));
        let single = rounds_per_second(b);
        progress.report(kind_phase("cpu", &label(&format!("All {threads} cores")), done, of));
        let multi: f64 = std::thread::scope(|s| {
            let handles: Vec<_> = (0..threads).map(|_| s.spawn(|| rounds_per_second(b))).collect();
            handles.into_iter().filter_map(|h| h.join().ok()).sum()
        });
        (single, multi)
    };
    let reps = repeated(
        clock,
        pace,
        repeats,
        |step| match step {
            Step::Warmup(d) => both(d / 2, &|s| format!("{WARMING_UP} · {s}"), 0, repeats.reps),
            Step::Rep { n, of } => {
                let r = both(budget, &|s| pass_label(n, of, s), n - 1, of);
                // A repetition counts when it ends.
                progress.report(kind_phase("cpu", &pass_done_label(n, of), n, of));
                r
            }
        },
        |done, of| progress.report(kind_phase("cpu", &gap_label(done, of), done, of)),
    );
    let single: Vec<f64> = reps.iter().map(|(s, _)| s / 1e6).collect();
    let multi: Vec<f64> = reps.iter().map(|(_, m)| m / 1e6).collect();
    let scaling: Vec<f64> = reps.iter().map(|(s, m)| m / s.max(1.0)).collect();
    vec![
        BenchMetric::measured("cpu-single", "Single core", &single, "M rounds/s", true),
        BenchMetric::measured("cpu-multi", &format!("All {threads} cores"), &multi, "M rounds/s", true),
        BenchMetric::measured("cpu-scaling", "Multi-core scaling", &scaling, "x", true),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emit::NoProgress;

    #[test]
    fn the_mix_is_deterministic_and_moves() {
        assert_eq!(mix(1), mix(1));
        assert_ne!(mix(1), mix(2));
        assert_ne!(mix(1), 1);
    }

    #[test]
    fn a_short_run_reports_three_positive_metrics() {
        let repeats = Repeats {
            warmup: Duration::from_millis(10),
            reps: 3,
            gap: Duration::ZERO,
        };
        let m = run(
            Duration::from_millis(10),
            repeats,
            &paddock_bench::SystemClock::new(),
            &crate::repeat::SleepPace,
            &NoProgress,
        );
        assert_eq!(m.len(), 3);
        assert!(m.iter().all(|x| x.value > 0.0 && x.higher_is_better));
        assert!(
            m.iter().all(|x| x.samples.len() == 3),
            "every repetition is kept, the warm-up is not"
        );
        assert!(m[1].label.starts_with("All "));
    }
}
