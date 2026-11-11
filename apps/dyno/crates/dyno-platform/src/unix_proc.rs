//! What Dyno reads about a process through `libc` and sysinfo on macOS and Linux: its nice value and its start time.
//! Nothing here changes a process.

use crate::parse::time::started_from_unix_secs;
use dyno_core::ports::PlatformError;
use dyno_core::process::Pid;
use dyno_core::time::Timestamp;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

fn errno_error(what: &str) -> PlatformError {
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::ESRCH) => PlatformError::NotFound("The process has exited.".into()),
        Some(libc::EPERM) | Some(libc::EACCES) => {
            PlatformError::AccessDenied(format!("Dyno can't {what}: it belongs to another user or the system."))
        }
        _ => PlatformError::Failed(format!("Could not {what}: {e}")),
    }
}

/// The nice value of `pid`.
pub fn get_nice(pid: libc::pid_t) -> Result<i32, PlatformError> {
    // SAFETY: getpriority's errno is cleared first because -1 is also a valid nice value.
    let (v, err) = unsafe {
        *errno_ptr() = 0;
        let v = libc::getpriority(libc::PRIO_PROCESS, pid as libc::id_t);
        (v, *errno_ptr())
    };
    if err != 0 {
        return Err(errno_error("read the priority"));
    }
    Ok(v)
}

#[cfg(target_os = "macos")]
unsafe fn errno_ptr() -> *mut libc::c_int {
    libc::__error()
}
#[cfg(not(target_os = "macos"))]
unsafe fn errno_ptr() -> *mut libc::c_int {
    libc::__errno_location()
}

/// The start time sysinfo reports for `pid`, as a [`Timestamp`].
pub fn started_of(pid: Pid) -> Option<Timestamp> {
    let mut sys = System::new()
        .inspect_err(|why| log::warn!("the OS process table is unavailable (sysinfo): {why}; the start time of pid {pid} is unknown"))
        .ok()?;
    let sp = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[sp]), true, ProcessRefreshKind::nothing());
    sys.process(sp).map(|p| started_from_unix_secs(p.start_time()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_has_a_nice_value_and_a_start_time() {
        let pid = std::process::id();
        assert!(get_nice(pid as libc::pid_t).is_ok());
        assert!(started_of(pid).is_some());
    }

    #[test]
    fn a_pid_that_does_not_exist_is_not_found() {
        assert!(started_of(u32::MAX - 1).is_none());
    }
}
