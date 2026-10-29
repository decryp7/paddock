//! Shared by the benchmark examples: statistics from `paddock-bench`, memory from `paddock-proc`.
#![allow(dead_code)]

use paddock_bench::summarize;
use std::time::Duration;

/// median, nearest-rank p95 and max of timed runs (paddock-bench's definitions).
#[derive(Debug, Clone, Copy)]
pub struct Stats {
    pub median: Duration,
    pub p95: Duration,
    pub max: Duration,
}

pub fn stats(samples: &[Duration]) -> Stats {
    let secs: Vec<f64> = samples.iter().map(Duration::as_secs_f64).collect();
    let s = summarize(&secs).expect("at least one sample");
    Stats {
        median: Duration::from_secs_f64(s.median),
        p95: Duration::from_secs_f64(s.p95),
        max: Duration::from_secs_f64(s.max),
    }
}

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
