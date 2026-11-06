//! Resident-set sampling (from the store spike's `rss.rs`): a thread polls the process RSS every 20 ms and
//! keeps the peak of the current phase.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Current resident set size in bytes (0 when the platform cannot tell).
pub fn current() -> u64 {
    memory_stats::memory_stats().map_or(0, |m| m.physical_mem as u64)
}

/// Real memory of this process in bytes (rule 26): the physical footprint on macOS, PSS on Linux, the private working set
/// on Windows (`paddock-proc`); the resident size where that cannot be read.
pub fn footprint() -> u64 {
    paddock_proc::process_memory(std::process::id()).unwrap_or_else(current)
}

/// Whether the real-memory gauge works here. A harness that prints "footprint" asserts it first (ER-33: a gauge that
/// silently falls back to the resident size would be reported under the wrong name).
pub fn footprint_gauge_works() -> bool {
    paddock_proc::process_memory(std::process::id()).is_some_and(|b| b > 0)
}

pub struct Sampler {
    measure: fn() -> u64,
    peak: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

impl Sampler {
    pub fn start() -> Sampler {
        Sampler::start_with(current)
    }

    /// A sampler of the real memory ([`footprint`]), not the resident size.
    pub fn start_footprint() -> Sampler {
        Sampler::start_with(footprint)
    }

    fn start_with(measure: fn() -> u64) -> Sampler {
        let peak = Arc::new(AtomicU64::new(measure()));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, s) = (peak.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                p.fetch_max(measure(), Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        Sampler { measure, peak, stop }
    }

    /// Peak RSS since the start or the last call, in MB, and starts a new phase.
    pub fn take_peak_mb(&self) -> u64 {
        let now = (self.measure)();
        self.peak.swap(now, Ordering::Relaxed).max(now) >> 20
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
