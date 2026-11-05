//! Memory measuring the import tests share.
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// The memory this process holds, in bytes (0 when the OS cannot tell): on macOS the physical footprint, which is what the OS
/// limits and Activity Monitor count and which includes compressed pages (rule 26); elsewhere the resident set.
pub fn rss() -> u64 {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: `info` is a zeroed plain struct the call fills; the pid is this process.
        let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
        let rc = unsafe {
            libc::proc_pid_rusage(
                std::process::id() as i32,
                libc::RUSAGE_INFO_V2,
                (&mut info as *mut libc::rusage_info_v2).cast::<libc::rusage_info_t>(),
            )
        };
        if rc == 0 {
            return info.ri_phys_footprint;
        }
    }
    let pid = sysinfo::Pid::from_u32(std::process::id());
    let mut sys = sysinfo::System::new();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        true,
        sysinfo::ProcessRefreshKind::nothing().with_memory(),
    );
    sys.process(pid).map_or(0, |p| p.memory())
}

/// Polls the process's memory while a test runs and keeps the peak.
pub struct Peak {
    peak: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

impl Peak {
    pub fn start() -> Peak {
        let peak = Arc::new(AtomicU64::new(rss()));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, s) = (peak.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                p.fetch_max(rss(), Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        Peak { peak, stop }
    }

    /// The peak so far, in MB.
    pub fn mb(&self) -> u64 {
        self.peak.fetch_max(rss(), Ordering::Relaxed);
        self.peak.load(Ordering::Relaxed) >> 20
    }
}

impl Drop for Peak {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
