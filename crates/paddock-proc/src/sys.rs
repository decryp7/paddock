//! The few platform calls, in one small file: signals and priority on Unix, process handles and `taskkill` on Windows, and
//! when a process started. Everything here is a single call that hands over to tested code or makes one OS request.
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

/// A close request to `pid` alone (`taskkill` without `/T` or `/F`: never a tree, whose members `taskkill` would find by
/// parent pids that may have been handed on).
#[cfg(windows)]
pub fn close_request(pid: u32) -> bool {
    let mut cmd = crate::command("taskkill");
    cmd.args(["/PID", &pid.to_string()]);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.status().is_ok_and(|s| s.success())
}

/// When `pid` started (Windows: its creation time), or `None` when there is no such process or it cannot be opened.
#[cfg(windows)]
pub fn start_time(pid: u32) -> Option<crate::StartTime> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: OpenProcess takes plain values; the handle is closed below.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let t = creation_time(h);
        CloseHandle(h);
        t
    }
}

/// The creation time of the process behind `h`.
#[cfg(windows)]
fn creation_time(h: windows_sys::Win32::Foundation::HANDLE) -> Option<crate::StartTime> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetProcessTimes;
    let mut t = [FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    }; 4];
    let [c, e, k, u] = &mut t;
    // SAFETY: a valid process handle and four writable FILETIMEs.
    if unsafe { GetProcessTimes(h, c, e, k, u) } == 0 {
        return None;
    }
    let ft = (u64::from(t[0].dwHighDateTime) << 32) | u64::from(t[0].dwLowDateTime);
    crate::identity::micros_from_filetime(ft).map(crate::StartTime::from_micros)
}

/// Ends `id` if its pid still belongs to it: the check and the kill go through one process handle, which keeps the process
/// object (and so its pid) from being handed on in between.
#[cfg(windows)]
pub fn end_if(id: crate::ProcessId) -> crate::Ending {
    use crate::identity::{Ending, holder};
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, TerminateProcess,
    };
    // SAFETY: OpenProcess takes plain values; the handle is used only while open and closed below.
    unsafe {
        let h = OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            id.pid(),
        );
        if h.is_null() {
            // Gone, or not ours to end: say which without acting.
            return Ending::of(holder(id, start_time(id.pid())), false);
        }
        let found = holder(id, creation_time(h));
        let sent = found == crate::Holder::Same && TerminateProcess(h, 1) != 0;
        CloseHandle(h);
        Ending::of(found, sent)
    }
}

/// When `pid` started, from `pbi_start_tvsec`/`pbi_start_tvusec`, or `None` when there is no such process or it is not
/// ours to look at.
#[cfg(target_os = "macos")]
pub fn start_time(pid: u32) -> Option<crate::StartTime> {
    let pid = libc::c_int::try_from(pid).ok()?;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: proc_pidinfo writes at most `size` bytes into a buffer of that size; the contents are read only when it filled
    // the whole structure.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if n != size {
        return None;
    }
    // SAFETY: zero-initialised plain integers, filled by the call.
    let i = unsafe { info.assume_init() };
    i.pbi_start_tvsec
        .checked_mul(1_000_000)?
        .checked_add(i.pbi_start_tvusec)
        .map(crate::StartTime::from_micros)
}

/// When `pid` started: the boot time plus its start tick (`/proc/<pid>/stat`), or `None` when there is no such process.
#[cfg(target_os = "linux")]
pub fn start_time(pid: u32) -> Option<crate::StartTime> {
    use crate::identity::{micros_from_ticks, parse_btime, parse_stat_start};
    use std::sync::OnceLock;
    static BOOT: OnceLock<Option<u64>> = OnceLock::new();
    let boot = (*BOOT.get_or_init(|| parse_btime(&std::fs::read_to_string("/proc/stat").ok()?)))?;
    // SAFETY: sysconf takes an integer and has no preconditions.
    let hz = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()?;
    let ticks = parse_stat_start(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)?;
    micros_from_ticks(boot, ticks, hz).map(crate::StartTime::from_micros)
}

/// Elsewhere: the start time sysinfo reports, in whole seconds.
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn start_time(pid: u32) -> Option<crate::StartTime> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    let id = Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[id]),
        true,
        ProcessRefreshKind::nothing(),
    );
    sys.process(id)
        .map(|p| crate::StartTime::from_micros(p.start_time().saturating_mul(1_000_000)))
}

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

/// Makes the child lead its own process group (Unix only).
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
