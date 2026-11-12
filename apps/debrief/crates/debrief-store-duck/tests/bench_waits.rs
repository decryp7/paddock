//! Measurement (docs/performance.md, rule 48): what the wait phase logs (`debrief_core::wait_log`) cost a store call.
//! `summary()` and a 500-row `page()` on the samples store, per call, on one thread and on eight, and the raw cost of
//! one watch. Off by default: `DEBRIEF_BENCH_WAITS=<calls per thread>` (for example 2000).
//!
//! Run with `--release --test bench_waits -- --nocapture`; the same measurement ran on the commit before the phase logs.

mod common;

#[global_allocator]
static GLOBAL: debrief_alloc::MiMalloc = debrief_alloc::MiMalloc;

use common::{log_samples, TempDir};
use debrief_core::ports::{Filter, PageQuery, Query};
use debrief_core::{PluginRegistry, Profile};
use debrief_store_duck::DuckStore;
use std::time::Instant;

fn per_call_us(store: &DuckStore, threads: usize, calls: usize, op: &(dyn Fn(&DuckStore) + Sync)) -> f64 {
    let started = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| (0..calls).for_each(|_| op(store)));
        }
    });
    started.elapsed().as_secs_f64() * 1e6 / (threads * calls) as f64
}

#[test]
fn what_the_wait_logs_cost_a_store_call() {
    let Some(calls) = std::env::var("DEBRIEF_BENCH_WAITS").ok().and_then(|v| v.parse::<usize>().ok()) else {
        eprintln!("skipped: set DEBRIEF_BENCH_WAITS=<calls per thread>");
        return;
    };
    let dir = TempDir::new("waits");
    let store = DuckStore::create(&dir.0, Profile::default_profile(), PluginRegistry::new()).unwrap();
    debrief_core::ports::Ingest::open(&store, &log_samples()).unwrap().wait().unwrap();
    let summary = |s: &DuckStore| {
        s.summary().unwrap();
    };
    let page = |s: &DuckStore| {
        s.page(&PageQuery {
            query: Filter::default(),
            limit: 500,
            ..PageQuery::default()
        })
        .unwrap();
    };
    summary(&store);
    for (name, op) in [("summary()", &summary as &(dyn Fn(&DuckStore) + Sync)), ("page(500)", &page)] {
        for threads in [1, 8] {
            eprintln!(
                "{name}, {threads} thread(s): {:.1} us per call",
                per_call_us(&store, threads, calls, op)
            );
        }
    }
    // The cost of one watch alone (absent before the phase logs): open and drop, static label.
    for threads in [1, 8] {
        let n = calls * 50;
        let started = Instant::now();
        std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| (0..n).for_each(|_| drop(debrief_core::wait_log::activity("bench"))));
            }
        });
        eprintln!(
            "one watch, {threads} thread(s): {:.0} ns",
            started.elapsed().as_secs_f64() * 1e9 / (threads * n) as f64
        );
    }
}
