//! The sustained run (ADR 0030 section 1): every core runs the float kernel for the chosen length while this thread reads the
//! work done once a second and writes one `sample` line. The agent adds frequency and temperature to those points, judges heat
//! and stops the run if it must; here the trace and its summary (peak, sustained, kept share, first drop) are produced.

use super::float;
use crate::emit::{kind_phase, Progress};
use crate::repeat::Pace;
use dyno_core::bench::cpu::{summarize, TracePoint};
use dyno_core::bench::BenchMetric;
use paddock_bench::Clock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// The longest run the runner accepts, seconds (20 minutes).
pub const MAX_SECS: u32 = 20 * 60;

/// Reads `total` (cumulative work in operations) once a second by `clock` for `secs` seconds; `on` gets each second's rate in
/// giga-operations per second. Returns the rates. A pause that moves nothing does not spin: the second is cut short.
pub fn ticker(secs: u32, clock: &dyn Clock, pace: &dyn Pace, mut total: impl FnMut() -> u64, mut on: impl FnMut(u32, f64)) -> Vec<f64> {
    let start = clock.now();
    let (mut last_at, mut last_total) = (start, total());
    let mut out = Vec::with_capacity(secs as usize);
    for s in 1..=secs {
        let due = start + Duration::from_secs(u64::from(s));
        loop {
            let now = clock.now();
            if now >= due {
                break;
            }
            pace.pause(due - now);
            if clock.now() <= now {
                break;
            }
        }
        let (now, t) = (clock.now(), total());
        let dt = now.saturating_sub(last_at).as_secs_f64().max(1e-9);
        let rate = t.saturating_sub(last_total) as f64 / dt / 1e9;
        (last_at, last_total) = (now, t);
        out.push(rate);
        on(s, rate);
    }
    out
}

/// Runs every logical CPU on the float kernel for `secs` seconds. Returns the metrics: peak, sustained, share kept and, when the
/// throughput fell, when.
pub fn run(secs: u32, clock: &dyn Clock, pace: &dyn Pace, progress: &dyn Progress) -> Vec<BenchMetric> {
    let secs = secs.clamp(1, MAX_SECS);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(256);
    let ops = AtomicU64::new(0);
    let stop = AtomicBool::new(false);
    let steps = secs.div_ceil(5).max(1);
    let rates = std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let (a, b) = float::matrices(float::N);
                let mut c = vec![0.0; float::N * float::N];
                let per = 2 * (float::N as u64).pow(3);
                while !stop.load(Ordering::Relaxed) {
                    c.iter_mut().for_each(|x| *x = 0.0);
                    float::matmul(float::N, std::hint::black_box(&a), std::hint::black_box(&b), &mut c);
                    std::hint::black_box(&c);
                    ops.fetch_add(per, Ordering::Relaxed);
                }
            });
        }
        let rates = ticker(
            secs,
            clock,
            pace,
            || ops.load(Ordering::Relaxed),
            |s, rate| {
                // The live rate is shown once, from the samples; the phase says only where the run is.
                let mut p = kind_phase(
                    "cpu-sustained",
                    // Where the run is in time is the page's own line (count and total); the label does not repeat it.
                    "Sustained run, all cores",
                    (s / 5).min(steps),
                    steps,
                );
                p.count = u64::from(s);
                p.total = Some(u64::from(secs));
                progress.report(p);
                progress.sample(s, rate);
            },
        );
        stop.store(true, Ordering::Relaxed);
        rates
    });
    metrics(&rates, threads)
}

/// The metrics of a sustained trace of `rates`.
pub fn metrics(rates: &[f64], threads: usize) -> Vec<BenchMetric> {
    let trace: Vec<TracePoint> = rates
        .iter()
        .enumerate()
        .map(|(i, t)| TracePoint {
            secs: u32::try_from(i + 1).unwrap_or(u32::MAX),
            throughput: *t,
            freq_mhz: None,
            temp_c: None,
        })
        .collect();
    let Some(s) = summarize(&trace) else {
        return Vec::new();
    };
    let mut out = vec![
        BenchMetric::higher("cpu-sustained-peak", &format!("Peak, all {threads} cores"), s.peak, "GFLOPS"),
        BenchMetric::higher(
            "cpu-sustained-steady",
            &format!("Sustained, all {threads} cores"),
            s.sustained,
            "GFLOPS",
        ),
        BenchMetric::higher("cpu-sustained-kept", "Sustained as a share of the peak", s.kept_percent, "%"),
    ];
    if let Some(d) = s.first_drop_secs {
        out.push(BenchMetric::lower(
            "cpu-sustained-drop",
            "Time until it fell more than 10 % below the peak",
            f64::from(d),
            "s",
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_bench::ManualClock;
    use std::cell::{Cell, RefCell};

    struct Pace<'a>(&'a ManualClock);
    impl crate::repeat::Pace for Pace<'_> {
        fn pause(&self, d: Duration) {
            self.0.advance(d);
        }
    }

    #[test]
    fn the_ticker_reads_the_work_once_a_second_and_reports_each_rate() {
        let clock = ManualClock::new();
        let work = Cell::new(0_u64);
        // The "machine" does 2 G operations in the first second, 1 G in each later one: the clock moves with the pause.
        let calls = Cell::new(0);
        let got = RefCell::new(Vec::new());
        let rates = ticker(
            3,
            &clock,
            &Pace(&clock),
            || {
                calls.set(calls.get() + 1);
                let n = calls.get();
                // read 1 is the start; reads 2.. come after each second
                if n >= 2 {
                    work.set(work.get() + if n == 2 { 2_000_000_000 } else { 1_000_000_000 });
                }
                work.get()
            },
            |s, r| got.borrow_mut().push((s, r)),
        );
        assert_eq!(rates, vec![2.0, 1.0, 1.0]);
        assert_eq!(*got.borrow(), vec![(1, 2.0), (2, 1.0), (3, 1.0)]);
        assert_eq!(clock.now(), Duration::from_secs(3));
    }

    #[test]
    fn a_pause_that_moves_nothing_does_not_hang() {
        struct Stuck;
        impl crate::repeat::Pace for Stuck {
            fn pause(&self, _: Duration) {}
        }
        let clock = ManualClock::new();
        assert_eq!(ticker(4, &clock, &Stuck, || 0, |_, _| {}).len(), 4);
    }

    #[test]
    fn the_metrics_name_peak_sustained_share_and_drop() {
        let mut r = vec![200.0; 60];
        r.extend(vec![150.0; 120]);
        let m = metrics(&r, 14);
        let by = |id: &str| m.iter().find(|x| x.id == id).map(|x| x.value);
        assert_eq!(by("cpu-sustained-peak"), Some(200.0));
        assert_eq!(by("cpu-sustained-steady"), Some(150.0));
        assert_eq!(by("cpu-sustained-kept"), Some(75.0));
        assert!(by("cpu-sustained-drop").is_some());
        assert!(metrics(&[1.0; 3], 4).is_empty(), "too short to judge");
        assert!(
            metrics(&[100.0; 60], 4).iter().all(|x| x.id != "cpu-sustained-drop"),
            "a steady machine has no drop"
        );
    }

    #[test]
    fn a_real_two_second_run_gives_two_samples() {
        struct Collect(std::sync::Mutex<Vec<u32>>);
        impl Progress for Collect {
            fn report(&self, _: dyno_core::bench::wire::PhaseProgress) {}
            fn sample(&self, s: u32, _: f64) {
                self.0.lock().unwrap().push(s);
            }
        }
        let c = Collect(Default::default());
        let _ = run(2, &paddock_bench::SystemClock::new(), &crate::repeat::SleepPace, &c);
        assert_eq!(*c.0.lock().unwrap(), vec![1, 2]);
    }
}
