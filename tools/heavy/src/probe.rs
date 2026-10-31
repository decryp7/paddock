//! Whether a pid exists at all. Unix: signal 0, where EPERM means "exists, not ours to signal" (alive). Nothing is ever
//! sent: signal 0 is the existence probe, and the pid is range-checked by the caller (never 0 or negative: a group).
#![allow(unsafe_code)]

/// Whether `pid` (1..=i32::MAX) names an existing process.
#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "signal 0 only probes existence and sends nothing; EPERM counts as alive"
)]
pub fn exists(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else { return false };
    if pid <= 0 {
        return false;
    }
    // SAFETY: kill with signal 0 takes plain integers and delivers nothing.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Whether `pid` names an existing process.
#[cfg(not(unix))]
pub fn exists(pid: u32) -> bool {
    paddock_proc::is_alive(pid) || paddock_proc::ProcessId::of(pid).is_some()
}
