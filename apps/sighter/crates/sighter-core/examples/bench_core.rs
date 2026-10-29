//! Benchmarks the pure core on synthetic data (no model needed) and prints a rule 48 results block.
//!
//! `SIGHTER_BENCH_LOAD=<1-minute load> SIGHTER_BENCH_COMMIT=<sha> SIGHTER_BENCH_MACHINE=<text> \
//!  nice -n 10 cargo run --release -p sighter-core --example bench_core`
//!
//! The run refuses to start above the load limit, because a number taken on a busy machine is only an indication.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use sighter_core::bench::{Budget, Clock, Row, RunInfo, Stat, SystemClock, budgets, load_allows_run, measure, render_results_block};
use sighter_core::chain::{Chain, InputSpace};
use sighter_core::detect::{ClassId, Detection, Fraction, Score, nms};
use sighter_core::geometry::{Logical, Pt, Rect, Scale};
use sighter_core::tiling::{GridSpec, tile_grid};
use std::hint::black_box;

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

/// A deterministic synthetic detection set (a linear congruential generator, no randomness crate).
fn synthetic(n: usize) -> Vec<Detection> {
    let mut state = 42u64;
    let mut next = move |m: u64| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) % m
    };
    (0..n)
        .filter_map(|_| {
            let r = Rect::<Logical>::new(
                next(1800) as f64,
                next(1100) as f64,
                10.0 + next(120) as f64,
                10.0 + next(60) as f64,
            )
            .ok()?;
            Some(Detection {
                rect: r,
                class: ClassId(next(8) as u32),
                score: Score::new(next(1000) as f64 / 1000.0).ok()?,
            })
        })
        .collect()
}

fn main() {
    let load: f64 = env("SIGHTER_BENCH_LOAD", "nan").parse().unwrap_or(f64::NAN);
    if !load_allows_run(load) {
        eprintln!(
            "refusing to run: set SIGHTER_BENCH_LOAD to the current 1-minute load, at most {}",
            budgets::RUN_LOAD_LIMIT
        );
        std::process::exit(2);
    }
    let clock = SystemClock::new();
    let dets = synthetic(500);
    let iou = Fraction::new(0.5).expect("constant threshold");
    let chain = Chain::local(
        Scale::uniform(2.0).expect("constant scale"),
        Pt::new(0.0, 0.0).expect("origin"),
        Pt::new(0.0, 0.0).expect("origin"),
        InputSpace::MacPoints,
    )
    .expect("constant chain");
    let map = chain.logical_to_input();
    let spec = GridSpec::default_640();

    // Generous guard budgets: this skeleton records numbers, it does not gate the core (the gates are model latencies).
    let budget = |name, limit, stat| Budget { name, limit, stat };
    let runs = 200;
    let nms_s = measure(&clock, 20, runs, || drop(black_box(nms(black_box(&dets), iou)))).expect("runs > 0");
    let grid_s = measure(&clock, 20, runs, || drop(black_box(tile_grid(3840, 2160, spec)))).expect("runs > 0");
    let map_s = measure(&clock, 20, runs, || {
        for d in &dets {
            black_box(map.rect(d.rect));
        }
    })
    .expect("runs > 0");

    let rows = [
        Row::latency(
            "NMS, 500 boxes",
            &nms_s,
            &budget("locate target CPU", budgets::LOCATE_TARGET_CPU_P95, Stat::P95),
        ),
        Row::latency(
            "Tile grid, 3840 x 2160",
            &grid_s,
            &budget("HUD frame", budgets::HUD_FRAME_OVERHEAD, Stat::P95),
        ),
        Row::latency(
            "Chain map, 500 boxes",
            &map_s,
            &budget("HUD frame", budgets::HUD_FRAME_OVERHEAD, Stat::P95),
        ),
    ];
    let info = RunInfo {
        date: env("SIGHTER_BENCH_DATE", "YYYY-MM-DD"),
        what: "sighter-core geometry, NMS and tile planning".into(),
        branch: env("SIGHTER_BENCH_BRANCH", "sighter/core-wave-0"),
        commit: env("SIGHTER_BENCH_COMMIT", "unknown"),
        machine: env("SIGHTER_BENCH_MACHINE", "unknown"),
        load: format!("1-minute load {load} at start"),
        build: "release; no model, no allocator override".into(),
        corpus: "synthetic: 500 boxes from a fixed-seed generator in the example; a 3840 x 2160 grid".into(),
        command: "cargo run --release -p sighter-core --example bench_core".into(),
        runs: format!("single series, 20 warm-up and {runs} timed runs; median and p95 (sorted[floor(p x (n - 1))])"),
        memory_measure: "not measured".into(),
        notes: format!(
            "wall time {}; indicative only unless the load is below about 12",
            sighter_core::bench::format_duration(clock.now())
        ),
    };
    print!("{}", render_results_block(&info, &rows));
}
