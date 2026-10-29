//! Benchmarks the HUD's overhead on synthetic data (no model, no UI) and prints a rule 48 results block.
//!
//! Two budgets from the design review: HUD work at most 2 ms per frame, and switching the trace on or off may not change
//! step timing by more than 5 %. The second is measured as interleaved A/B runs (trace off, trace on).
//!
//! `SIGHTER_BENCH_LOAD=<1-minute load> SIGHTER_BENCH_COMMIT=<sha> SIGHTER_BENCH_MACHINE=<text> SIGHTER_BENCH_DATE=<date> \
//!  nice -n 10 cargo run --release -p sighter-core --example bench_hud`
#![allow(clippy::expect_used, clippy::unwrap_used)]

use sighter_core::bench::{
    Budget, Row, RunInfo, Stat, SystemClock, budgets, change_percent, format_duration, load_allows_run, measure, measure_ab,
    render_results_block,
};
use sighter_core::chain::{Chain, InputSpace};
use sighter_core::detect::{ClassId, Detection, Fraction, Score, nms};
use sighter_core::geometry::{Logical, Pt, Rect, Scale};
use sighter_core::hud::overlay::{BandThresholds, ClassNames, OverlayContext, OverlayInput, ScoreStyle, build_items};
use sighter_core::hud::sidecar::Track;
use sighter_core::hud::trace::{DetectionEvent, LocateEvent, LocateStatus, Micros, TraceBus, TraceKind};
use sighter_core::tiling::{GridSpec, tile_grid};
use std::hint::black_box;

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

/// A deterministic synthetic detection set (a linear congruential generator).
fn synthetic(n: usize) -> Vec<Detection> {
    let mut state = 7u64;
    let mut next = move |m: u64| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) % m
    };
    (0..n)
        .map(|_| Detection {
            rect: Rect::<Logical>::new(
                next(1800) as f64,
                next(1100) as f64,
                10.0 + next(120) as f64,
                10.0 + next(60) as f64,
            )
            .expect("in range"),
            class: ClassId(next(6) as u32),
            score: Score::new(next(1000) as f64 / 1000.0).expect("in range"),
        })
        .collect()
}

fn events(dets: &[Detection]) -> Vec<TraceKind> {
    let mut v = vec![TraceKind::Locate(LocateEvent::Started { id: 1 })];
    v.extend(dets.iter().enumerate().map(|(i, d)| {
        TraceKind::Detection(DetectionEvent {
            locate: 1,
            rect: d.rect,
            class: d.class,
            score: d.score,
            chosen: i == 0,
        })
    }));
    v.push(TraceKind::Locate(LocateEvent::Finished {
        id: 1,
        status: LocateStatus::Found,
    }));
    v
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
    let frame_dets = synthetic(50);
    let step_dets = synthetic(300);
    let frame_events = events(&frame_dets);
    let step_events = events(&synthetic(12));
    let chain = Chain::local(
        Scale::uniform(2.0).expect("scale"),
        Pt::new(0.0, 0.0).expect("origin"),
        Pt::new(0.0, 0.0).expect("origin"),
        InputSpace::MacPoints,
    )
    .expect("chain");
    let to_capture = chain.logical_to_capture();
    let names = ClassNames::new(&["Button", "Text input", "Tab", "Checkbox", "Menu item", "Icon"]);
    let ctx = OverlayContext {
        thresholds: BandThresholds::default(),
        style: ScoreStyle::Decimal,
        names: &names,
        password_class: None,
        redactions: &[],
        secrets: &[],
    };
    let inputs: Vec<OverlayInput<'_>> = frame_dets
        .iter()
        .enumerate()
        .map(|(i, d)| OverlayInput {
            detection: *d,
            chosen: i == 0,
            ocr_text: Some("Save"),
        })
        .collect();
    let iou = Fraction::new(0.5).expect("threshold");

    // 1. HUD work for one frame of 50 detections: fan the events out to two viewers, build the overlay items, and append
    //    the frame's events to the sidecar track text.
    let frame = measure(&clock, 50, 400, || {
        let mut bus = TraceBus::new(1024, true).expect("bus");
        let a = bus.subscribe(1024).expect("sub");
        let b = bus.subscribe(1024).expect("sub");
        for (i, k) in frame_events.iter().enumerate() {
            bus.publish(Micros(i as u64), k.clone());
        }
        let drained = bus.drain(a).expect("drain");
        black_box(bus.drain(b).expect("drain"));
        black_box(build_items(&inputs, &to_capture, &ctx).expect("items"));
        black_box(Track::new(drained.events).expect("track").to_jsonl());
    })
    .expect("runs");

    // 2. Step timing with the trace off (A) against on (B): the step's own work is NMS, a tile grid and 12 events.
    let spec = GridSpec::default_640();
    let mut off = TraceBus::new(256, false).expect("bus");
    let mut on = TraceBus::new(256, true).expect("bus");
    let sub = on.subscribe(256).expect("sub");
    let work = || {
        black_box(nms(black_box(&step_dets), iou)).ok();
        black_box(tile_grid(1920, 1200, spec)).ok();
    };
    let (a, b) = measure_ab(
        &clock,
        6,
        20,
        100,
        || {
            work();
            for (i, k) in step_events.iter().enumerate() {
                off.publish(Micros(i as u64), k.clone());
            }
        },
        || {
            work();
            for (i, k) in step_events.iter().enumerate() {
                on.publish(Micros(i as u64), k.clone());
            }
            black_box(on.drain(sub).ok());
        },
    )
    .expect("runs");
    let delta = change_percent(a.p50(), b.p50());

    let frame_budget = Budget {
        name: "HUD per frame",
        limit: budgets::HUD_FRAME_OVERHEAD,
        stat: Stat::P95,
    };
    let step_ok = delta <= budgets::HUD_STEP_TIMING_CHANGE_PERCENT;
    let rows = [
        Row::latency("HUD work per frame, 50 detections, 2 viewers", &frame, &frame_budget),
        Row {
            measure: "Step p50, trace off".into(),
            value: format_duration(a.p50()),
            budget: "none (reference)".into(),
            verdict: "reference".into(),
        },
        Row {
            measure: "Step p50, trace on".into(),
            value: format!("{} ({delta:+.1} %)", format_duration(b.p50())),
            budget: format!("change <= {} %", budgets::HUD_STEP_TIMING_CHANGE_PERCENT),
            verdict: if step_ok { "within" } else { "over" }.into(),
        },
    ];
    let info = RunInfo {
        date: env("SIGHTER_BENCH_DATE", "YYYY-MM-DD"),
        what: "HUD overhead (trace bus, overlay items, sidecar text)".into(),
        branch: env("SIGHTER_BENCH_BRANCH", "sighter/core-wave-0"),
        commit: env("SIGHTER_BENCH_COMMIT", "unknown"),
        machine: env("SIGHTER_BENCH_MACHINE", "unknown"),
        load: format!("1-minute load {load} at start"),
        build: "release; no model, no UI rendering".into(),
        corpus: "synthetic: fixed-seed boxes (50 per frame, 300 per step) and 12 trace events per step".into(),
        command: "cargo run --release -p sighter-core --example bench_hud".into(),
        runs: "frame: 50 warm-up and 400 timed runs, p95; step: 6 interleaved A/B pairs of 100 timed runs after 20 warm-up, p50".into(),
        memory_measure: "not measured".into(),
        notes: "Pure-core cost only: drawing, the video sidecar write and the UI are not measured. Indicative unless the load is below about 12.".into(),
    };
    print!("{}", render_results_block(&info, &rows));
}
