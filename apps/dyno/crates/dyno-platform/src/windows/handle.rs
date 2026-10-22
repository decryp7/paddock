//! Owned Win32 handles, opening a process with its identity checked, and Win32 errors as [`PlatformError`]s.

use crate::parse::time::{same_start, started_from_filetime};
use dyno_core::ports::PlatformError;
use dyno_core::process::{Pid, ProcessRef};
use dyno_core::time::Timestamp;
use windows::core::{Error, HRESULT};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, HANDLE, STILL_ACTIVE, WIN32_ERROR,
};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// A handle closed on drop.
#[derive(Debug)]
pub struct Owned(pub HANDLE);

impl Owned {
    /// The raw handle (valid while `self` lives).
    pub fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_invalid() && !self.0 .0.is_null() {
            // SAFETY: the handle was returned by a Win32 open/create call and is closed exactly once, here.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

// SAFETY: a Win32 kernel handle may be used and closed from any thread.
unsafe impl Send for Owned {}

/// Whether `e` is the Win32 error `code`.
pub fn is_win32(e: &Error, code: WIN32_ERROR) -> bool {
    e.code() == HRESULT::from_win32(code.0)
}

/// The message for a process Dyno may not touch.
pub fn denied(pid: Pid) -> PlatformError {
    PlatformError::AccessDenied(format!(
        "Process {pid} runs as administrator or another user: Dyno can't change it without the helper."
    ))
}

/// A Win32 failure while doing `what` to process `pid`.
pub fn failed(pid: Pid, what: &str, e: &Error) -> PlatformError {
    if is_win32(e, ERROR_ACCESS_DENIED) {
        denied(pid)
    } else if is_win32(e, ERROR_INVALID_PARAMETER) {
        crate::processes::gone(pid)
    } else {
        PlatformError::Failed(format!("Could not {what} process {pid}: {}", e.message()))
    }
}

/// The 64-bit value of a FILETIME.
pub fn ft64(ft: FILETIME) -> u64 {
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

/// When the process behind `h` started (second precision), if readable.
pub fn creation_time(h: HANDLE) -> Option<Timestamp> {
    let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: `h` is a live process handle with query rights; the four out-pointers are valid locals.
    unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) }.ok()?;
    Some(started_from_filetime(ft64(c)))
}

/// Opens the process `target` with `access` (plus limited query rights) and checks it is still the same process:
/// alive, and started when `target` says. A reused pid is [`PlatformError::NotFound`].
pub fn open_process(target: ProcessRef, access: PROCESS_ACCESS_RIGHTS) -> Result<Owned, PlatformError> {
    if target.pid == 0 {
        return Err(denied(0));
    }
    // SAFETY: OpenProcess has no pointer arguments; the returned handle is owned by `Owned`.
    let h = unsafe { OpenProcess(access | PROCESS_QUERY_LIMITED_INFORMATION, false, target.pid) }
        .map_err(|e| failed(target.pid, "open", &e))?;
    let h = Owned(h);
    let mut code = 0u32;
    // SAFETY: `h` is a live process handle; `code` is a valid out-pointer.
    let alive = unsafe { GetExitCodeProcess(h.raw(), &mut code) }.is_ok() && code == STILL_ACTIVE.0 as u32;
    let started = creation_time(h.raw());
    match started {
        Some(s) if alive && same_start(s, target.started) => Ok(h),
        _ => Err(crate::processes::gone(target.pid)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_map_to_user_messages() {
        let denied_err = Error::from_hresult(HRESULT::from_win32(ERROR_ACCESS_DENIED.0));
        assert!(matches!(failed(5, "open", &denied_err), PlatformError::AccessDenied(m) if m.contains("helper")));
        let gone_err = Error::from_hresult(HRESULT::from_win32(ERROR_INVALID_PARAMETER.0));
        assert!(matches!(failed(5, "open", &gone_err), PlatformError::NotFound(_)));
        let other = Error::from_hresult(HRESULT::from_win32(31));
        assert!(matches!(failed(5, "open", &other), PlatformError::Failed(m) if m.contains("process 5")));
        assert_eq!(ft64(FILETIME { dwLowDateTime: 1, dwHighDateTime: 1 }), (1 << 32) + 1);
    }

    #[test]
    fn opening_checks_identity() {
        let me = std::process::id();
        // SAFETY: test-only call with a valid pseudo handle.
        let started = creation_time(unsafe { windows::Win32::System::Threading::GetCurrentProcess() }).unwrap();
        let ok = open_process(ProcessRef { pid: me, started }, PROCESS_QUERY_LIMITED_INFORMATION);
        assert!(ok.is_ok());
        let reused = ProcessRef {
            pid: me,
            started: started.minus(10_000),
        };
        assert!(matches!(open_process(reused, PROCESS_QUERY_LIMITED_INFORMATION), Err(PlatformError::NotFound(_))));
        assert!(matches!(open_process(ProcessRef { pid: 0, started }, PROCESS_QUERY_LIMITED_INFORMATION), Err(PlatformError::AccessDenied(_))));
        let missing = ProcessRef {
            pid: 0xFFFF_FFF0,
            started,
        };
        assert!(matches!(open_process(missing, PROCESS_QUERY_LIMITED_INFORMATION), Err(PlatformError::NotFound(_))));
    }
}
