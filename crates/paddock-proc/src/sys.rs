//! The few platform calls, in one small file: signals and priority on Unix, `taskkill` on Windows. Everything here is a
//! single call that hands over to tested code or makes one OS request.
#![allow(unsafe_code)]

#[cfg(unix)]
mod unix {
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    /// Sends `signal` to the process group `pgid`; false when there is no such group.
    pub fn signal_group(pgid: u32, signal: i32) -> bool {
        let Ok(pgid) = libc::pid_t::try_from(pgid) else {
            return false;
        };
        if pgid <= 1 {
            return false;
        }
        // SAFETY: killpg takes two integers; a group that does not exist makes it fail with ESRCH.
        unsafe { libc::killpg(pgid, signal) == 0 }
    }

    /// Sends `signal` to one process; false when there is no such process.
    pub fn signal_pid(pid: u32, signal: i32) -> bool {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        if pid <= 1 {
            return false;
        }
        // SAFETY: kill takes two integers; a process that does not exist makes it fail with ESRCH.
        unsafe { libc::kill(pid, signal) == 0 }
    }

    pub const SIGTERM: i32 = libc::SIGTERM;
    pub const SIGKILL: i32 = libc::SIGKILL;

    /// Makes the child lead its own process group, so the whole tree can be signalled at once.
    pub fn own_group(cmd: &mut Command) {
        cmd.process_group(0);
    }

    /// Lowers the child's priority between fork and exec (the `nice` binary would drop `DYLD_*` on macOS).
    pub fn lower_priority_before_exec(cmd: &mut Command) {
        // SAFETY: setpriority is async-signal-safe and the closure touches no memory of the parent.
        unsafe {
            cmd.pre_exec(|| {
                libc::setpriority(libc::PRIO_PROCESS, 0, 10);
                Ok(())
            });
        }
    }

    /// The niceness of this process (tests).
    #[cfg(test)]
    pub fn own_niceness() -> i32 {
        // SAFETY: getpriority takes two integers.
        unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) }
    }

    /// The physical footprint of `pid` in bytes (`ri_phys_footprint`, the figure of Activity Monitor's "Memory" column), or
    /// `None` when the process is gone or not ours to look at. Unlike the resident size it leaves out pages that were
    /// given back with `MADV_FREE_REUSABLE` (mimalloc does this) but still sit in the process until the kernel needs them.
    #[cfg(target_os = "macos")]
    pub fn phys_footprint(pid: u32) -> Option<u64> {
        let pid = libc::c_int::try_from(pid).ok()?;
        let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
        // SAFETY: proc_pid_rusage fills a buffer of the size of the flavour asked for (RUSAGE_INFO_V2 is rusage_info_v2); the
        // buffer is that struct, and the contents are read only when the call returned 0.
        let rc =
            unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, info.as_mut_ptr().cast()) };
        // SAFETY: zero-initialised plain integers, filled by the call on success.
        (rc == 0).then(|| unsafe { info.assume_init() }.ri_phys_footprint)
    }

    /// Touches `bytes` of fresh pages and gives them back the way mimalloc does (tests).
    #[cfg(all(test, target_os = "macos"))]
    pub fn touch_and_free(bytes: usize) {
        // SAFETY: an anonymous private mapping of `bytes`, written inside its bounds, then advised and left mapped.
        unsafe {
            let p = libc::mmap(
                std::ptr::null_mut(),
                bytes,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_ANON | libc::MAP_PRIVATE,
                -1,
                0,
            );
            if p == libc::MAP_FAILED {
                return;
            }
            let base = p.cast::<u8>();
            for off in (0..bytes).step_by(4096) {
                base.add(off).write_volatile(1);
            }
            libc::madvise(p, bytes, libc::MADV_FREE_REUSABLE);
        }
    }

    /// Ignores SIGTERM in this process (tests).
    #[cfg(test)]
    pub fn ignore_sigterm() {
        // SAFETY: setting a signal disposition to SIG_IGN.
        unsafe {
            libc::signal(libc::SIGTERM, libc::SIG_IGN);
        }
    }
}

#[cfg(unix)]
pub use unix::*;

/// `taskkill` for `pid` and its tree, politely (`force` false: a close request) or at once.
#[cfg(windows)]
pub fn taskkill(pid: u32, force: bool) -> bool {
    let mut cmd = crate::command("taskkill");
    cmd.args(["/T", "/PID", &pid.to_string()]);
    if force {
        cmd.arg("/F");
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.status().is_ok_and(|s| s.success())
}

/// Makes the child lead its own process group (Unix); Windows trees are ended by `taskkill /T`.
/// The private working set of `pid` in bytes (the part of the working set that is not shared with other processes), or
/// `None` when the process is gone or cannot be opened.
#[cfg(windows)]
pub fn private_working_set(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: OpenProcess takes plain values; the handle is closed below. GetProcessMemoryInfo fills at most `cb` bytes of a
    // zero-initialised struct of that size, and it is read only when the call succeeded.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut c: PROCESS_MEMORY_COUNTERS_EX2 = std::mem::zeroed();
        let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32;
        let ok = GetProcessMemoryInfo(h, (&raw mut c).cast::<PROCESS_MEMORY_COUNTERS>(), cb);
        CloseHandle(h);
        (ok != 0).then_some(c.PrivateWorkingSetSize as u64)
    }
}

#[cfg(not(unix))]
pub fn own_group(_cmd: &mut std::process::Command) {}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn ids_that_could_hit_the_caller_or_everything_are_refused() {
        // 0 and 1 would signal our own group or init; ids beyond pid_t are not processes at all.
        for id in [0, 1, u32::MAX, u32::MAX / 2 + 2] {
            assert!(!signal_group(id, SIGTERM), "group {id}");
            assert!(!signal_pid(id, SIGTERM), "pid {id}");
        }
    }
}
