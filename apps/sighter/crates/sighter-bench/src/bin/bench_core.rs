//! Benchmarks the pure core on synthetic data (no model needed) and prints a rule 48 results block.
//!
//! Environment (all but the load are optional text for the block): `SIGHTER_BENCH_LOAD` (required: the current 1-minute
//! load, at most 25), `SIGHTER_BENCH_COMMIT`, `SIGHTER_BENCH_MACHINE`, `SIGHTER_BENCH_DATE`, `SIGHTER_BENCH_BRANCH`.
//!
//! `SIGHTER_BENCH_LOAD=<load> SIGHTER_BENCH_COMMIT=<sha> SIGHTER_BENCH_MACHINE=<text> SIGHTER_BENCH_DATE=<date> \
//!  nice -n 10 cargo run --release -p sighter-bench --bin bench_core`
//!
//! The run refuses to start above the load limit, because a number taken on a busy machine is only an indication. None of
//! the rows has a budget of its own yet (the budgets are model latencies), so each is an indication.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use paddock_bench::{Clock, SystemClock};
use sighter_bench::{env, footprint, stats};
use sighter_core::bench::{ResultRow, RunInfo, budgets, format_duration, load_allows_run, render_results_block, time_runs};
use sighter_core::chain::{Chain, InputSpace};
use sighter_core::detect::{ClassId, Detection, Fraction, Score, nms};
use sighter_core::geometry::{Logical, Pt, Rect, Scale};
use sighter_core::tiling::{GridSpec, tile_grid};
use std::hint::black_box;

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
                tile: 0,
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
    let now = || clock.now();
    let dets = synthetic(500);
    let iou = Fraction::new(0.5).expect("constant threshold");
    let origin = Pt::new(0.0, 0.0).expect("origin");
    let chain = Chain::local(
        Scale::uniform(2.0).expect("constant scale"),
        origin,
        Pt::new(0.0, 0.0).expect("origin"),
        InputSpace::MacPoints,
    )
    .expect("constant chain");
    let map = chain.logical_to_input();
    let spec = GridSpec::default_640();

    let runs = 200;
    let nms_s = time_runs(&now, 20, runs, || drop(black_box(nms(black_box(&dets), iou)))).expect("runs > 0");
    let grid_s = time_runs(&now, 20, runs, || {
        drop(black_box(tile_grid(black_box(3840), black_box(2160), spec)))
    })
    .expect("runs > 0");
    let map_s = time_runs(&now, 20, runs, || {
        let mut acc = 0.0;
        for d in black_box(&dets) {
            acc += map.rect(d.rect).x();
        }
        black_box(acc);
    })
    .expect("runs > 0");

    let rows = [
        ResultRow::indication("NMS, 500 boxes, median", stats(&nms_s).median),
        ResultRow::indication("NMS, 500 boxes, p95", stats(&nms_s).p95),
        ResultRow::indication("Tile grid, 3840 x 2160, median", stats(&grid_s).median),
        ResultRow::indication("Chain map, 500 boxes, median", stats(&map_s).median),
    ];
    let info = RunInfo {
        date: env("SIGHTER_BENCH_DATE", "YYYY-MM-DD"),
        what: "sighter-core geometry, NMS and tile planning".into(),
        branch: env("SIGHTER_BENCH_BRANCH", "sighter/core-wave-0"),
        commit: env("SIGHTER_BENCH_COMMIT", "unknown"),
        machine: env("SIGHTER_BENCH_MACHINE", "unknown"),
        load: format!("1-minute load {load} at start"),
        build: "release; no model, no allocator override".into(),
        corpus: "synthetic: 500 boxes from a fixed-seed generator in the program; a 3840 x 2160 grid".into(),
        command: "SIGHTER_BENCH_LOAD=<load> nice -n 10 cargo run --release -p sighter-bench --bin bench_core".into(),
        runs: format!("single series, 20 warm-up and {runs} timed runs; median and nearest-rank p95 (paddock-bench `summarize`)"),
        memory_measure: footprint(),
        notes: format!(
            "wall time {}; no budget applies to these rows; indicative unless the load is below about 12",
            format_duration(clock.now())
        ),
    };
    print!("{}", render_results_block(&info, &rows));
}
