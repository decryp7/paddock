//! What differs on Linux in process control: efficiency mode (nice + idle disk priority) and CPU affinity. Both are
//! per **thread** on Linux (`setpriority` and `sched_setaffinity` take a thread id), so they are applied to every
//! thread of the process; threads started later inherit from their creator.

use super::paths::{read_text, HostPaths};
use super::sys;
use crate::parse::linux_proc::cpu_list_mask;
use crate::unix_control::{get_nice, set_nice, OsControl};
use dyno_core::ports::PlatformError;
use dyno_core::process::Pid;

/// Most threads of one process touched by an action.
const MAX_THREADS: usize = 16_384;
/// The nice value of efficiency mode ("below normal").
pub const EFFICIENCY_NICE: i32 = 10;

/// Linux efficiency mode and affinity.
pub struct LinuxControl {
    paths: HostPaths,
}

impl LinuxControl {
    /// Control reading thread lists and the online CPUs from `paths`.
    pub fn new(paths: HostPaths) -> Self {
        LinuxControl { paths }
    }

    /// The thread ids of `pid`, the main thread first. Falls back to the pid alone when the list is unreadable.
    fn threads(&self, pid: Pid) -> Vec<u32> {
        let mut tids: Vec<u32> = std::fs::read_dir(self.paths.pid(pid).join("task"))
            .map(|d| {
                d.flatten()
                    .take(MAX_THREADS)
                    .filter_map(|e| e.file_name().to_str()?.parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        tids.retain(|t| *t != pid);
        tids.insert(0, pid);
        tids
    }

    /// The CPUs that exist and are online, as a mask.
    fn online_mask(&self) -> u64 {
        read_text(&self.paths.sys.join("devices/system/cpu/online"))
            .map(|t| cpu_list_mask(&t))
            .filter(|m| *m != 0)
            .unwrap_or_else(|| {
                let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(64);
                if n >= 64 {
                    u64::MAX
                } else {
                    (1u64 << n) - 1
                }
            })
    }
}

impl OsControl for LinuxControl {
    fn set_efficiency(&mut self, pid: Pid, on: bool) -> Result<(), PlatformError> {
        let main = libc::pid_t::try_from(pid).map_err(|_| PlatformError::NotFound(format!("Process {pid} has exited.")))?;
        if on {
            // Never raise a priority that is already lower (nice 15 stays 15).
            let target = get_nice(main)?.max(EFFICIENCY_NICE);
            set_nice(main, target)?;
        }
        // The main thread decides success; the others are best effort (they may exit while we walk them).
        for tid in self.threads(pid).into_iter().skip(1) {
            if on {
                let _ = libc::pid_t::try_from(tid).map(|t| get_nice(t).and_then(|n| set_nice(t, n.max(EFFICIENCY_NICE))));
            }
            let _ = sys::set_io_idle(tid, on);
        }
        // Idle disk priority is a bonus: kernels before 5.1 require privileges for it.
        let _ = sys::set_io_idle(pid, on);
        Ok(())
    }

    fn set_affinity(&mut self, pid: Pid, mask: u64) -> Result<(), PlatformError> {
        let online = self.online_mask();
        if mask == 0 {
            return Err(PlatformError::Failed("Choose at least one core.".into()));
        }
        if mask & !online != 0 {
            let bad = (mask & !online).trailing_zeros();
            return Err(PlatformError::Failed(format!(
                "This computer has no core {bad} online; pick from the cores it has."
            )));
        }
        sys::set_thread_affinity(pid, mask)?;
        for tid in self.threads(pid).into_iter().skip(1) {
            let _ = sys::set_thread_affinity(tid, mask);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::linux_proc::{cpus_allowed, stat_nice};
    use std::process::{Child, Command, Stdio};

    /// A throwaway `sleep` that is always killed, even when the test fails.
    struct Dummy(Child);
    impl Dummy {
        fn spawn() -> Dummy {
            #[allow(clippy::disallowed_methods)]
            let c = Command::new("sleep")
                .arg("300")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            Dummy(c)
        }
        fn pid(&self) -> u32 {
            self.0.id()
        }
    }
    impl Drop for Dummy {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn proc_text(pid: u32, file: &str) -> String {
        std::fs::read_to_string(format!("/proc/{pid}/{file}")).unwrap()
    }

    #[test]
    fn efficiency_mode_lowers_priority_and_never_raises_it() {
        let d = Dummy::spawn();
        let mut c = LinuxControl::new(HostPaths::host());
        c.set_efficiency(d.pid(), true).unwrap();
        assert_eq!(stat_nice(&proc_text(d.pid(), "stat")), Some(10));
        // Already nicer: stays.
        set_nice(d.pid() as libc::pid_t, 15).unwrap();
        c.set_efficiency(d.pid(), true).unwrap();
        assert_eq!(stat_nice(&proc_text(d.pid(), "stat")), Some(15));
        c.set_efficiency(d.pid(), false).unwrap();
        let gone = u32::MAX - 3;
        assert!(c.set_efficiency(gone, true).is_err());
    }

    #[test]
    fn affinity_is_applied_checked_and_reported() {
        let d = Dummy::spawn();
        let mut c = LinuxControl::new(HostPaths::host());
        let before = cpus_allowed(&proc_text(d.pid(), "status")).unwrap();
        assert_ne!(before, 0);
        let one = 1u64 << before.trailing_zeros();
        c.set_affinity(d.pid(), one).unwrap();
        assert_eq!(cpus_allowed(&proc_text(d.pid(), "status")), Some(one));
        c.set_affinity(d.pid(), before).unwrap();
        assert_eq!(cpus_allowed(&proc_text(d.pid(), "status")), Some(before));
        assert!(matches!(c.set_affinity(d.pid(), 0), Err(PlatformError::Failed(_))));
        assert!(matches!(c.set_affinity(d.pid(), 1 << 63), Err(PlatformError::Failed(m)) if m.contains("no core 63")));
    }

    #[test]
    fn online_cpus_come_from_sys_or_the_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let c = LinuxControl::new(HostPaths::under(dir.path()));
        assert_ne!(c.online_mask(), 0);
        std::fs::create_dir_all(dir.path().join("sys/devices/system/cpu")).unwrap();
        std::fs::write(dir.path().join("sys/devices/system/cpu/online"), "0-2,5\n").unwrap();
        assert_eq!(c.online_mask(), 0b100111);
        assert_eq!(c.threads(4242)[0], 4242);
    }
}
