//! Integer work: a xorshift-and-multiply mix whose result is consumed, so it can't be optimised away. Reported as
//! million rounds per second on one core and on every core at once.

use dyno_core::bench::BenchMetric;
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

/// The CPU metrics: one core, all cores and the scaling between them.
pub fn run(budget: Duration) -> Vec<BenchMetric> {
    let single = rounds_per_second(budget);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(256);
    let multi: f64 = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads).map(|_| s.spawn(|| rounds_per_second(budget))).collect();
        handles.into_iter().filter_map(|h| h.join().ok()).sum()
    });
    vec![
        BenchMetric::higher("cpu-single", "Single core", single / 1e6, "M rounds/s"),
        BenchMetric::higher("cpu-multi", &format!("All {threads} cores"), multi / 1e6, "M rounds/s"),
        BenchMetric::higher("cpu-scaling", "Multi-core scaling", multi / single.max(1.0), "x"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mix_is_deterministic_and_moves() {
        assert_eq!(mix(1), mix(1));
        assert_ne!(mix(1), mix(2));
        assert_ne!(mix(1), 1);
    }

    #[test]
    fn a_short_run_reports_three_positive_metrics() {
        let m = run(Duration::from_millis(30));
        assert_eq!(m.len(), 3);
        assert!(m.iter().all(|x| x.value > 0.0 && x.higher_is_better));
        assert!(m[1].label.starts_with("All "));
    }
}
