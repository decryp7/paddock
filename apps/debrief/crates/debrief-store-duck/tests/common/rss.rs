//! Resident-set sampling (from the store spike's `rss.rs`): a thread polls the process RSS every 20 ms and
//! keeps the peak of the current phase.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Current resident set size in bytes (0 when the platform cannot tell).
pub fn current() -> u64 {
    memory_stats::memory_stats().map_or(0, |m| m.physical_mem as u64)
}

/// What this process really uses (rule 26), in bytes: the physical footprint on macOS (`ri_phys_footprint`), the
/// proportional set size on Linux (`/proc/self/smaps_rollup`); elsewhere the resident size (0 when unknown).
#[allow(dead_code)]
pub fn real() -> u64 {
    #[cfg(target_os = "macos")]
    {
        let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
        // SAFETY: proc_pid_rusage fills a buffer of the flavour's size; it is read only when the call succeeded.
        let rc = unsafe { libc::proc_pid_rusage(std::process::id() as i32, libc::RUSAGE_INFO_V2, info.as_mut_ptr().cast()) };
        if rc == 0 {
            // SAFETY: filled by the call.
            return unsafe { info.assume_init() }.ri_phys_footprint;
        }
    }
    #[cfg(target_os = "linux")]
    {
        let pss = std::fs::read_to_string("/proc/self/smaps_rollup").ok().and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("Pss:"))
                .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
        });
        if let Some(kb) = pss {
            return kb * 1024;
        }
    }
    current()
}

pub struct Sampler {
    peak: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    measure: fn() -> u64,
}

impl Sampler {
    /// Samples the resident size.
    pub fn start() -> Sampler {
        Sampler::start_with(current)
    }

    /// Samples `measure` (e.g. [`real`]).
    pub fn start_with(measure: fn() -> u64) -> Sampler {
        let peak = Arc::new(AtomicU64::new(measure()));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, s) = (peak.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                p.fetch_max(measure(), Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        Sampler { peak, stop, measure }
    }

    /// Peak since the start or the last call, in MB, and starts a new phase.
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
