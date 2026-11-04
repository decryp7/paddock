//! The few platform calls, in one small file: signals and priority on Unix, process handles and `taskkill` on Windows, and
//! when a process started. Everything here is a single call that hands over to tested code or makes one OS request.
#![allow(unsafe_code)]

#[cfg(unix)]
mod unix {
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    /// Sends `signal` to the process group `pgid`; false when there is no such group.
    #[expect(
        clippy::disallowed_methods,
        reason = "the one implementation: callers checked the group's leader by identity (ADR 0033)"
    )]
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
    #[expect(
        clippy::disallowed_methods,
        reason = "the one implementation: callers checked the process by identity just before (ADR 0033)"
    )]
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

    /// Lowers the priority of `child` to [`LOWERED_NICE`] unless it is already at least that low (a niced parent passes
    /// its level on; this never raises one). The caller still holds the child unreaped, so the pid is still its own
    /// (rule 53). Done after the spawn, never in a `pre_exec` hook: a hook forces fork+exec instead of `posix_spawn`, and
    /// the `nice(1)` binary would drop `DYLD_*` on macOS. The child runs at the parent's priority for the moment
    /// between its start and this call; for work that lasts at least milliseconds that is harmless. True when the child
    /// is at that level or lower afterwards.
    pub fn lower_child(child: &std::process::Child) -> bool {
        let pid = child.id();
        // SAFETY: getpriority takes two integers and has no memory effects.
        if unsafe { libc::getpriority(libc::PRIO_PROCESS, pid) } >= LOWERED_NICE {
            return true;
        }
        // SAFETY: setpriority takes plain integers and has no memory effects; it only changes our own child's priority.
        let main = unsafe { libc::setpriority(libc::PRIO_PROCESS, pid, LOWERED_NICE) == 0 };
        // Linux nice values are per thread: the threads the child already started are lowered too (the ones it starts
        // later inherit from the thread that starts them).
        #[cfg(target_os = "linux")]
        if main {
            lower_other_threads(pid);
        }
        main
    }

    /// Lowers every thread of `pid` but the main one (Linux).
    #[cfg(target_os = "linux")]
    fn lower_other_threads(pid: u32) {
        let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
            return;
        };
        for tid in tasks
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
            .filter(|tid| *tid != pid)
        {
            // SAFETY: as above; a thread that ended meanwhile makes it fail with ESRCH.
            unsafe { libc::setpriority(libc::PRIO_PROCESS, tid, LOWERED_NICE) };
        }
    }

    /// The niceness of a lowered child.
    pub const LOWERED_NICE: i32 = 10;

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

    /// The direct children of `pid` (`proc_listchildpids`: asks the kernel for that parent only, no scan of the process
    /// list). `Some(empty)` for a process without children or one that is gone, `None` when the kernel failed to answer.
    #[cfg(target_os = "macos")]
    pub fn child_pids(pid: u32) -> Option<Vec<u32>> {
        child_pids_in(pid, 1024)
    }

    /// [`child_pids`] starting with room for `room` pids (the buffer grows when it is full; tests start small).
    #[cfg(target_os = "macos")]
    pub fn child_pids_in(pid: u32, room: usize) -> Option<Vec<u32>> {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return Some(Vec::new());
        };
        let mut buf = vec![0 as libc::pid_t; room];
        loop {
            let bytes = libc::c_int::try_from(buf.len() * std::mem::size_of::<libc::pid_t>())
                .unwrap_or(libc::c_int::MAX);
            // SAFETY: the kernel writes at most `bytes` bytes into `buf`, which is that large.
            let n = unsafe { libc::proc_listchildpids(pid, buf.as_mut_ptr().cast(), bytes) };
            match read_listing(n, buf.len()) {
                Listing::Unknown => return None,
                Listing::Grow => buf.resize(buf.len() * 4, 0),
                Listing::Count(n) => {
                    return Some(
                        buf.iter()
                            .take(n)
                            .filter_map(|p| u32::try_from(*p).ok())
                            .filter(|p| *p != 0)
                            .collect(),
                    );
                }
            }
        }
    }

    /// What `proc_listchildpids` answered, for a buffer of `room` pids.
    #[cfg(target_os = "macos")]
    #[derive(Debug, PartialEq, Eq)]
    pub(crate) enum Listing {
        /// The kernel could not say (-1: the call failed), which is not "no children".
        Unknown,
        /// The buffer was full, so the list may have been cut: grow it and ask again.
        Grow,
        /// This many pids were written.
        Count(usize),
    }

    /// Reads the answer `n` (a count of pids, not bytes; -1 on failure) of a call that had room for `room` pids.
    #[cfg(target_os = "macos")]
    pub(crate) fn read_listing(n: libc::c_int, room: usize) -> Listing {
        let Ok(n) = usize::try_from(n) else {
            return Listing::Unknown;
        };
        if n >= room && room < (1 << 20) {
            Listing::Grow
        } else {
            Listing::Count(n)
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

/// A close request to `id` alone (`taskkill` without `/T` or `/F`: never a tree, whose members `taskkill` would find by
/// parent pids that may have been handed on). A handle with `SYNCHRONIZE` is held across the `taskkill` call, after its
/// creation time was checked: while it is open the process object, and so its pid, cannot be handed on, so the request
/// reaches `id` or nobody. Returns who held the pid and whether the request was delivered.
#[cfg(windows)]
pub fn close_request(id: crate::ProcessId) -> (crate::Holder, bool) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };
    // SAFETY: OpenProcess takes plain values; the handle is used only while open and closed below.
    unsafe {
        let h = OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            id.pid(),
        );
        if h.is_null() {
            return (crate::holder(id, start_time(id.pid())), false);
        }
        let found = crate::holder(id, creation_time(h));
        let sent = found == crate::Holder::Same && {
            let mut cmd = crate::command("taskkill");
            cmd.args(["/PID", &id.pid().to_string()]);
            cmd.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            cmd.status().is_ok_and(|s| s.success())
        };
        CloseHandle(h);
        (found, sent)
    }
}

/// `pid` by identity if it is a member of the job `job`: the membership and the creation time are read through one process
/// handle, so a pid that left the job and was handed on is not mistaken for a member.
#[cfg(windows)]
pub fn identity_in_job(
    pid: u32,
    job: windows_sys::Win32::Foundation::HANDLE,
) -> Option<crate::ProcessId> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::IsProcessInJob;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: OpenProcess takes plain values; the handle is used only while open and closed below; `inside` is a writable
    // BOOL.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut inside = 0;
        let started = if IsProcessInJob(h, job, &raw mut inside) != 0 && inside != 0 {
            creation_time(h)
        } else {
            None
        };
        CloseHandle(h);
        started.map(|s| crate::ProcessId::new(pid, s))
    }
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
pub(crate) fn creation_time(h: windows_sys::Win32::Foundation::HANDLE) -> Option<crate::StartTime> {
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
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        TerminateProcess,
    };
    /// `STILL_ACTIVE`: the exit code of a process that runs.
    const STILL_ACTIVE: u32 = 259;
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
        // Terminating a process that already exited fails with ACCESS_DENIED: that is gone, not refused.
        let mut code = 0u32;
        let exited = !sent && GetExitCodeProcess(h, &raw mut code) != 0 && code != STILL_ACTIVE;
        CloseHandle(h);
        crate::identity::ending_after_kill(found, sent, exited)
    }
}

/// When `pid` started: `kp_proc.p_starttime` of `sysctl(CTL_KERN, KERN_PROC, KERN_PROC_PID, pid)`, or `None` when there is
/// no such process. Unlike `proc_pidinfo(PROC_PIDTBSDINFO)`, which refuses a process of another effective user (a setuid
/// child such as `/usr/bin/top` gives EPERM), the sysctl answers for every process, zombies included.
#[cfg(target_os = "macos")]
pub fn start_time(pid: u32) -> Option<crate::StartTime> {
    let pid = libc::c_int::try_from(pid).ok()?;
    let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid];
    // `struct kinfo_proc` is 648 bytes on 64-bit macOS; a larger, u64-aligned buffer leaves room for growth.
    let mut buf = [0u64; 128];
    let mut len = std::mem::size_of_val(&buf);
    // SAFETY: the kernel writes at most `len` bytes into `buf`, which is that large, and sets `len` to what it wrote.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            buf.as_mut_ptr().cast(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    start_from_kinfo(rc, len, &buf)
}

/// The start time in a `kinfo_proc` the sysctl answered with `rc` and `len` bytes: `kp_proc` comes first, and its first
/// member is the union whose `p_starttime` (a `timeval`: seconds as `i64`, then microseconds as `i32`) sits at offset 0.
/// No process gives `len` 0.
#[cfg(target_os = "macos")]
pub(crate) fn start_from_kinfo(
    rc: libc::c_int,
    len: usize,
    buf: &[u64],
) -> Option<crate::StartTime> {
    if rc != 0 || len < 16 {
        return None;
    }
    let secs = u64::try_from(buf[0] as i64).ok()?;
    let micros = u64::try_from((buf[1] as u32) as i32).ok()?;
    secs.checked_mul(1_000_000)?
        .checked_add(micros)
        .filter(|&t| t > 0)
        .map(crate::StartTime::from_micros)
}

/// When `pid` started: the boot time plus its start tick (`/proc/<pid>/stat`), or `None` when there is no such process.
#[cfg(target_os = "linux")]
pub fn start_time(pid: u32) -> Option<crate::StartTime> {
    let ticks = crate::identity::parse_stat_start(
        &std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?,
    )?;
    start_from_ticks(ticks)
}

/// The start time of a process that started `ticks` clock ticks after boot (field 22 of `/proc/<pid>/stat`).
#[cfg(target_os = "linux")]
fn start_from_ticks(ticks: u64) -> Option<crate::StartTime> {
    use crate::identity::{micros_from_ticks, parse_btime};
    use std::sync::OnceLock;
    static BOOT: OnceLock<Option<u64>> = OnceLock::new();
    let boot = (*BOOT.get_or_init(|| {
        let b = std::fs::read_to_string("/proc/stat").ok().and_then(|t| parse_btime(&t));
        if b.is_none() {
            log::warn!("The boot time could not be read from /proc/stat: no process can be identified, so none is ended by pid");
        }
        b
    }))?;
    // SAFETY: sysconf takes an integer and has no preconditions.
    let hz = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()?;
    micros_from_ticks(boot, ticks, hz).map(crate::StartTime::from_micros)
}

/// Sends `signal` to `id` if its pid still belongs to it, through a pidfd (Linux 5.3 and later): the pidfd is opened first and names one
/// process for good; the start time read afterwards is that process's as long as the pidfd says it still runs, and the
/// signal goes through the pidfd, so a pid handed on in between is never signalled. `None` when the kernel has no pidfds
/// (the caller checks and signals instead).
#[cfg(target_os = "linux")]
pub fn signal_by_pidfd(id: crate::ProcessId, signal: i32) -> Option<crate::Ending> {
    use crate::{Ending, Holder, holder};
    use std::os::fd::{FromRawFd, OwnedFd};
    let pid = libc::pid_t::try_from(id.pid()).ok()?;
    // SAFETY: pidfd_open takes a pid and flags and returns a new descriptor or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        let e = std::io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::ESRCH) => Some(Ending::Gone),
            _ => None,
        };
    }
    // SAFETY: a fresh descriptor this function owns.
    let fd = unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) };
    let send = |sig: libc::c_int| {
        use std::os::fd::AsRawFd;
        // SAFETY: pidfd_send_signal takes the descriptor, a signal, no siginfo and no flags.
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd.as_raw_fd(),
                sig,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            ) == 0
        }
    };
    let now = start_time(id.pid());
    // Signal 0 checks that the pidfd's process still runs: then `now` was read from it, not from a newer holder.
    if !send(0) {
        return Some(Ending::Gone);
    }
    let found = holder(id, now);
    let sent = found == Holder::Same && send(signal);
    Some(Ending::of(found, sent))
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

/// `(pid, parent)` of every process, from one snapshot (`CreateToolhelp32Snapshot`: Windows has no per-parent call). The
/// snapshot is a full list of the machine's processes and threads, but reads no memory, command lines or times, so it is
/// lighter than a full refresh; callers take it at most once per tree walk.
#[cfg(windows)]
pub fn process_parents() -> Option<Vec<(u32, u32)>> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    // SAFETY: the snapshot handle is closed below; the entry is a zero-initialised struct whose size field is set as the API
    // requires, and it is read only after a successful call.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut list = Vec::new();
        let mut ok = Process32FirstW(snap, &raw mut entry);
        while ok != 0 {
            list.push((entry.th32ProcessID, entry.th32ParentProcessID));
            ok = Process32NextW(snap, &raw mut entry);
        }
        CloseHandle(snap);
        Some(list)
    }
}

/// Makes the child lead its own process group (Unix only).
#[cfg(not(unix))]
pub fn own_group(_cmd: &mut std::process::Command) {}

/// Windows lowers the priority with the creation flags set by `lower_priority`; nothing is left to do after the spawn.
#[cfg(not(unix))]
pub fn lower_child(_child: &std::process::Child) -> bool {
    true
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// macOS: the start time comes from `kinfo_proc` (offset 0: seconds, then microseconds) and agrees with what sysinfo
    /// reads through `proc_pidinfo` for a process of our own.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_start_time_is_read_from_kinfo_proc() {
        let mut buf = [0u64; 128];
        buf[0] = 1_700_000_000;
        buf[1] = 250_000 | (0xdead_u64 << 32);
        assert_eq!(
            start_from_kinfo(0, 648, &buf).map(crate::StartTime::as_micros),
            Some(1_700_000_000_250_000)
        );
        assert_eq!(start_from_kinfo(-1, 648, &buf), None, "the call failed");
        assert_eq!(start_from_kinfo(0, 0, &buf), None, "no such process");
        assert_eq!(start_from_kinfo(0, 648, &[0; 2]), None, "a zero start");
        assert_eq!(
            start_from_kinfo(0, 648, &[u64::MAX, 0]),
            None,
            "a negative start"
        );
        let me = std::process::id();
        let ours = start_time(me).unwrap().as_micros();
        let mut sys = sysinfo::System::new();
        let pid = sysinfo::Pid::from_u32(me);
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        assert_eq!(ours / 1_000_000, sys.process(pid).unwrap().start_time());
        assert_eq!(start_time(u32::MAX), None, "not a pid_t");
        assert_eq!(start_time(i32::MAX as u32), None, "no such process");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_failed_listing_is_unknown_not_an_empty_list() {
        assert_eq!(
            read_listing(-1, 1024),
            Listing::Unknown,
            "-1 is the call failing"
        );
        assert_eq!(read_listing(0, 1024), Listing::Count(0), "0 is no children");
        assert_eq!(read_listing(3, 1024), Listing::Count(3));
        assert_eq!(
            read_listing(4, 4),
            Listing::Grow,
            "a full buffer may have cut the list"
        );
        assert_eq!(
            read_listing(1 << 20, 1 << 20),
            Listing::Count(1 << 20),
            "the buffer's ceiling"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn children_are_listed_for_one_pid_and_the_buffer_grows() {
        let mut kids: Vec<_> = (0..3)
            .map(|_| {
                std::process::Command::new("sleep")
                    .arg("60")
                    .spawn()
                    .unwrap()
            })
            .collect();
        let me = std::process::id();
        for room in [1, 2, 3, 1024] {
            let mut got = child_pids_in(me, room).unwrap();
            got.retain(|p| kids.iter().any(|k| k.id() == *p));
            assert_eq!(got.len(), 3, "room {room}");
        }
        assert_eq!(child_pids_in(u32::MAX, 4), Some(vec![]), "not a pid_t");
        for k in &mut kids {
            let _ = k.kill();
            let _ = k.wait();
        }
    }

    #[test]
    fn ids_that_could_hit_the_caller_or_everything_are_refused() {
        // 0 and 1 would signal our own group or init; ids beyond pid_t are not processes at all.
        for id in [0, 1, u32::MAX, u32::MAX / 2 + 2] {
            assert!(!signal_group(id, SIGTERM), "group {id}");
            assert!(!signal_pid(id, SIGTERM), "pid {id}");
        }
    }
}

/// The CPU time (user + system) `pid` has used, in microseconds, with the start time of that process from the same OS call
/// where the OS gives both (rule 53); from a read of that pid only. `None` when there is no such process or the OS did not
/// say.
#[cfg(target_os = "linux")]
pub fn cpu_sample(pid: u32) -> Option<crate::usage::CpuSample> {
    // SAFETY: sysconf takes an integer and has no preconditions.
    let hz = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()?;
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    Some(crate::usage::CpuSample {
        micros: crate::usage::cpu_micros_from_stat(&stat, hz)?,
        started: crate::identity::parse_stat_start(&stat).and_then(start_from_ticks),
        stopped: crate::runstate::parse_stat_state(&stat).map(crate::runstate::is_stop_letter),
    })
}

/// See the Linux version: `proc_pidinfo(PROC_PIDTASKALLINFO)` gives the BSD info (start time) and the task totals (in Mach
/// absolute time units, not nanoseconds on Apple silicon, converted with `mach_timebase_info`) in one call. A process
/// that call refuses (a setuid child) is read with `PROC_PIDTASKINFO` and has no start time in the sample.
#[cfg(target_os = "macos")]
#[allow(
    deprecated,
    reason = "libc marks the Mach timebase for mach2; one call, no new dependency"
)]
pub fn cpu_sample(pid: u32) -> Option<crate::usage::CpuSample> {
    use std::sync::OnceLock;
    static TIMEBASE: OnceLock<Option<(u32, u32)>> = OnceLock::new();
    let (numer, denom) = (*TIMEBASE.get_or_init(|| {
        let mut tb = libc::mach_timebase_info { numer: 0, denom: 0 };
        // SAFETY: mach_timebase_info fills the struct it is given.
        let rc = unsafe { libc::mach_timebase_info(&raw mut tb) };
        (rc == 0 && tb.denom != 0).then_some((tb.numer, tb.denom))
    }))?;
    let raw = pid;
    let pid = libc::c_int::try_from(pid).ok()?;
    let mut all = std::mem::MaybeUninit::<libc::proc_taskallinfo>::zeroed();
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_taskallinfo>()).ok()?;
    // SAFETY: proc_pidinfo fills at most `size` bytes of the zero-initialised struct; it is read only when the call
    // returned the full size.
    let got = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKALLINFO,
            0,
            all.as_mut_ptr().cast(),
            size,
        )
    };
    let (task, started, stopped) = if got == size {
        // SAFETY: filled by the call, which returned its whole size.
        let all = unsafe { all.assume_init() };
        let secs: u64 = all.pbsd.pbi_start_tvsec;
        (
            all.ptinfo,
            secs.checked_mul(1_000_000)?
                .checked_add(all.pbsd.pbi_start_tvusec)?,
            Some(all.pbsd.pbi_status == SSTOP),
        )
    } else {
        // This call carries no identity: the start time is read before it and after it, and the sample is dropped if the
        // pid changed hands in between (rule 53).
        let before = start_time(raw)?;
        let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
        let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_taskinfo>()).ok()?;
        // SAFETY: as above, for the smaller struct.
        let got = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTASKINFO,
                0,
                info.as_mut_ptr().cast(),
                size,
            )
        };
        if got != size {
            return None;
        }
        // SAFETY: filled by the call, which returned its whole size.
        let micros = task_micros(unsafe { info.assume_init() }, numer, denom);
        return (start_time(raw)? == before).then_some(crate::usage::CpuSample {
            micros,
            started: Some(before),
            stopped: None,
        });
    };
    Some(crate::usage::CpuSample {
        micros: task_micros(task, numer, denom),
        started: Some(crate::StartTime::from_micros(started)).filter(|t| t.as_micros() > 0),
        stopped,
    })
}

/// `pbi_status` of a process stopped by a signal (`SSTOP` in `<sys/proc.h>`).
#[cfg(target_os = "macos")]
const SSTOP: u32 = 4;

#[cfg(target_os = "macos")]
fn task_micros(info: libc::proc_taskinfo, numer: u32, denom: u32) -> u64 {
    crate::usage::mach_to_micros(
        info.pti_total_user.saturating_add(info.pti_total_system),
        numer,
        denom,
    )
}

/// See the Linux version: `GetProcessTimes` gives the creation time and the CPU times (kernel + user, in 100 ns units) in one
/// call.
#[cfg(windows)]
pub fn cpu_sample(pid: u32) -> Option<crate::usage::CpuSample> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: OpenProcess takes plain values; the handle is closed below; four writable FILETIMEs.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut t = [FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        }; 4];
        let [c, e, k, u] = &mut t;
        let ok = GetProcessTimes(h, c, e, k, u);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        let ft = |f: &FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
        Some(crate::usage::CpuSample {
            micros: ft(&t[2]).saturating_add(ft(&t[3])) / 10,
            started: crate::identity::micros_from_filetime(ft(&t[0]))
                .map(crate::StartTime::from_micros),
            stopped: None,
        })
    }
}

/// Elsewhere: not read.
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn cpu_sample(_pid: u32) -> Option<crate::usage::CpuSample> {
    None
}

/// The run state of `id`: `/proc/<pid>/stat` field 3 (`T` or `t` is stopped), read once with the start tick that names the
/// process.
#[cfg(target_os = "linux")]
pub fn run_state(id: crate::ProcessId) -> crate::RunState {
    let read = std::fs::read_to_string(format!("/proc/{}/stat", id.pid()));
    crate::runstate::from_read(id, read, start_from_ticks)
}

/// The run state of `id` from the CPU sample (macOS: `pbi_status` of the same `proc_pidinfo` call as the CPU time and the
/// start time); `Unknown` where the sample carries no state (Windows).
#[cfg(not(target_os = "linux"))]
pub fn run_state(id: crate::ProcessId) -> crate::RunState {
    crate::runstate::from_sample(id, cpu_sample(id.pid()), || start_time(id.pid()))
}
