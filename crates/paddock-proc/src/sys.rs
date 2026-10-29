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

#[cfg(not(unix))]
pub fn own_group(_cmd: &mut std::process::Command) {}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

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
