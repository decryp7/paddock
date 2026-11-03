//! Memory: copy bandwidth over a buffer much larger than the caches, and random-access latency by chasing a
//! shuffled cycle through a buffer of the same size.

use crate::repeat::{repeated, Pace, Repeats, Step};
use dyno_core::bench::BenchMetric;
use paddock_bench::Clock;
use std::hint::black_box;
use std::time::{Duration, Instant};

/// Copy buffer size, bytes (two of them are allocated).
const COPY_BYTES: usize = 64 * 1024 * 1024;
/// Number of 8-byte slots in the latency chain (64 MiB).
const CHAIN_SLOTS: usize = 8 * 1024 * 1024;

/// Copies a buffer repeatedly for about `budget`; returns GB/s of data copied.
pub fn copy_bandwidth(bytes: usize, budget: Duration) -> f64 {
    let src = vec![0xA5_u8; bytes];
    let mut dst = vec![0_u8; bytes];
    let start = Instant::now();
    let mut copied = 0_u64;
    while start.elapsed() < budget || copied == 0 {
        dst.copy_from_slice(black_box(&src));
        black_box(&mut dst);
        copied += bytes as u64;
    }
    copied as f64 / start.elapsed().as_secs_f64().max(1e-9) / 1e9
}

/// A single random cycle over `n` slots (Sattolo's algorithm with a fixed-seed generator).
pub fn random_cycle(n: usize) -> Vec<u32> {
    let mut next: Vec<u32> = (0..n as u32).collect();
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    for i in (1..n).rev() {
        state = crate::cpu::mix(state);
        let j = (state % i as u64) as usize;
        next.swap(i, j);
    }
    next
}

/// Follows the cycle for about `budget`; returns nanoseconds per dependent load.
pub fn latency_ns(slots: usize, budget: Duration) -> f64 {
    let next = random_cycle(slots);
    let start = Instant::now();
    let mut at = 0_u32;
    let mut loads = 0_u64;
    while start.elapsed() < budget || loads == 0 {
        for _ in 0..100_000 {
            at = next[at as usize];
        }
        loads += 100_000;
    }
    black_box(at);
    start.elapsed().as_secs_f64() * 1e9 / loads as f64
}

/// The memory metrics, each from every repetition. The warm-up runs both for half its time each and is not counted.
pub fn run(budget: Duration, repeats: Repeats, clock: &dyn Clock, pace: &dyn Pace) -> Vec<BenchMetric> {
    let reps = repeated(clock, pace, repeats, |step| {
        let b = match step {
            Step::Warmup(d) => d / 2,
            Step::Rep { .. } => budget,
        };
        (copy_bandwidth(COPY_BYTES, b), latency_ns(CHAIN_SLOTS, b))
    });
    let copy: Vec<f64> = reps.iter().map(|(c, _)| *c).collect();
    let latency: Vec<f64> = reps.iter().map(|(_, l)| *l).collect();
    vec![
        BenchMetric::measured("mem-copy", "Copy bandwidth", &copy, "GB/s", true),
        BenchMetric::measured("mem-latency", "Random access latency", &latency, "ns", false),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cycle_visits_every_slot_once() {
        let next = random_cycle(1000);
        let mut seen = vec![false; 1000];
        let mut at = 0_usize;
        for _ in 0..1000 {
            assert!(!seen[at]);
            seen[at] = true;
            at = next[at] as usize;
        }
        assert_eq!(at, 0);
        assert!(seen.iter().all(|s| *s));
    }

    #[test]
    fn small_runs_give_positive_numbers() {
        assert!(copy_bandwidth(1 << 20, Duration::from_millis(10)) > 0.0);
        assert!(latency_ns(1 << 12, Duration::from_millis(10)) > 0.0);
    }
}
