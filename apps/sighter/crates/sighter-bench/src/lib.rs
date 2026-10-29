//! Sighter's benchmark programs (`bench_core`, `bench_hud`) share these helpers: statistics from `paddock-bench`, memory from
//! `paddock-proc`. The programs are binaries because a benchmark is run, not tested; the harness logic they call
//! (`sighter_core::bench`) is covered by sighter-core's tests.
#![allow(clippy::expect_used)]

use paddock_bench::summarize;
use std::time::Duration;

/// median, nearest-rank p95 and max of timed runs (paddock-bench's definitions).
#[derive(Debug, Clone, Copy)]
pub struct Stats {
    /// The median run.
    pub median: Duration,
    /// The nearest-rank 95th percentile.
    pub p95: Duration,
    /// The slowest run.
    pub max: Duration,
}

/// Summarises timed runs with `paddock-bench`'s definitions (one sample at least).
pub fn stats(samples: &[Duration]) -> Stats {
    let secs: Vec<f64> = samples.iter().map(Duration::as_secs_f64).collect();
    let s = summarize(&secs).expect("at least one sample");
    Stats {
        median: Duration::from_secs_f64(s.median),
        p95: Duration::from_secs_f64(s.p95),
        max: Duration::from_secs_f64(s.max),
    }
}

/// An environment variable, or `default` when it is unset.
pub fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

/// This process's real memory (physical footprint on macOS, PSS on Linux, private working set on Windows) as text.
pub fn footprint() -> String {
    match paddock_proc::process_memory(std::process::id()) {
        Some(b) => format!(
            "physical footprint at the end of the run {:.1} MB (paddock-proc::process_memory)",
            b as f64 / (1024.0 * 1024.0)
        ),
        None => "not available on this platform".to_owned(),
    }
}
