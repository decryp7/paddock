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

pub mod float;
pub mod simd;
pub mod sort;
pub mod sustained;

/// One kernel measured on one core and on every core: `(id, title, unit, run for a budget)`.
struct Kernel {
    id: &'static str,
    title: String,
    unit: &'static str,
    run: fn(Duration) -> f64,
}

fn kernels() -> Vec<Kernel> {
    vec![
        Kernel {
            id: "int",
            title: "Integer".into(),
            unit: "M rounds/s",
            run: |b| rounds_per_second(b) / 1e6,
        },
        Kernel {
            id: "sort",
            title: "Sorting".into(),
            unit: "M elements/s",
            run: |b| sort::mega_elements_per_second(b, sort::ELEMENTS),
        },
        Kernel {
            id: "float",
            title: "Float (f64)".into(),
            unit: "GFLOPS",
            run: float::gflops,
        },
        Kernel {
            id: "simd",
            title: format!("Vector (f32, {})", simd::width()),
            unit: "GFLOPS",
            run: simd::gflops,
        },
    ]
}

/// The CPU metrics, each from every repetition: for each kernel one core and all cores, and the scaling of the float kernel
/// between them. The warm-up runs every section for a share of its time and is not counted. "All cores" is one thread per
/// logical CPU, each with its own data (the separate-task model: it shows what the silicon can do, not how an app scales).
pub fn run(budget: Duration, repeats: Repeats, clock: &dyn Clock, pace: &dyn Pace, progress: &dyn Progress) -> Vec<BenchMetric> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(256);
    let ks = kernels();
    progress.note(&format!("Vector unit used: {}.", simd::width()));
    let all = |k: &Kernel, b: Duration| -> f64 {
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..threads).map(|_| s.spawn(|| (k.run)(b))).collect();
            handles.into_iter().filter_map(|h| h.join().ok()).sum()
        })
    };
    let pass = |b: Duration, label: &dyn Fn(&str) -> String, done: u32, of: u32| -> Vec<(f64, f64)> {
        ks.iter()
            .map(|k| {
                progress.report(kind_phase("cpu", &label(&format!("{} · one core", k.title)), done, of));
                let single = (k.run)(b);
                progress.report(kind_phase("cpu", &label(&format!("{} · all {threads} cores", k.title)), done, of));
                (single, all(k, b))
            })
            .collect()
    };
    let reps = repeated(
        clock,
        pace,
        repeats,
        |step| match step {
            Step::Warmup(d) => pass(d / 8, &|s| format!("{WARMING_UP} · {s}"), 0, repeats.reps),
            Step::Rep { n, of } => {
                let r = pass(budget, &|s| pass_label(n, of, s), n - 1, of);
                // A repetition counts when it ends.
                progress.report(kind_phase("cpu", &pass_done_label(n, of), n, of));
                r
            }
        },
        |done, of| progress.report(kind_phase("cpu", &gap_label(done, of), done, of)),
    );
    let mut out = Vec::new();
    for (i, k) in ks.iter().enumerate() {
        let single: Vec<f64> = reps.iter().map(|r| r[i].0).collect();
        let multi: Vec<f64> = reps.iter().map(|r| r[i].1).collect();
        out.push(BenchMetric::measured(
            &format!("cpu-{}-single", k.id),
            &format!("{} · one core", k.title),
            &single,
            k.unit,
            true,
        ));
        out.push(BenchMetric::measured(
            &format!("cpu-{}-multi", k.id),
            &format!("{} · all {threads} cores", k.title),
            &multi,
            k.unit,
            true,
        ));
        if k.id == "float" {
            let scaling: Vec<f64> = reps.iter().map(|r| r[i].1 / r[i].0.max(1e-9)).collect();
            out.push(BenchMetric::measured(
                "cpu-scaling",
                "Multi-core scaling (float)",
                &scaling,
                "x",
                true,
            ));
        }
    }
    out
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
    fn a_short_run_reports_every_kernel_on_one_and_all_cores() {
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
        // Four kernels on one core and on every core, and the scaling.
        assert_eq!(m.len(), 4 * 2 + 1);
        assert!(m.iter().all(|x| x.value > 0.0 && x.higher_is_better));
        assert!(
            m.iter().all(|x| x.samples.len() == 3),
            "every repetition is kept, the warm-up is not"
        );
        assert!(m.iter().any(|x| x.id == "cpu-simd-multi" && x.label.contains("Vector")));
    }
}
