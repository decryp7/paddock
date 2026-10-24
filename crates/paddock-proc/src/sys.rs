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
#[cfg(not(unix))]
pub fn own_group(_cmd: &mut std::process::Command) {}
