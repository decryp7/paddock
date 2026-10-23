//! UAC: runs one program elevated with `ShellExecuteExW` (verb `runas`) and waits for its exit code (ADR 0016).

use crate::elevate::ElevationError;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::time::Duration;
use windows::core::{w, HRESULT, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

/// `ERROR_CANCELLED`: the user chose No in the UAC dialog.
const ERROR_CANCELLED: u32 = 1223;

/// Runs `program` with the already quoted command line `parameters` as administrator and waits up to `timeout`.
#[allow(unsafe_code)]
pub fn run_elevated(program: &Path, parameters: &str, timeout: Duration) -> Result<(), ElevationError> {
    let file: Vec<u16> = program.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let params: Vec<u16> = parameters.encode_utf16().chain(std::iter::once(0)).collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY: `info` is fully initialised for the fields the flags read; `file` and `params` are NUL-terminated and
    // outlive the call.
    if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
        return Err(if e.code() == HRESULT::from_win32(ERROR_CANCELLED) {
            ElevationError::Declined
        } else {
            ElevationError::Failed(None, format!("Windows could not start the helper installer: {e}"))
        });
    }
    let process = info.hProcess;
    if process.is_invalid() {
        return Err(ElevationError::Failed(None, "Windows did not return the installer process.".into()));
    }
    let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: `process` is a valid process handle we own until the CloseHandle below.
    let waited = unsafe { WaitForSingleObject(process, millis) };
    let mut code = 0u32;
    // SAFETY: as above; `code` is valid for writes.
    let got = if waited == WAIT_OBJECT_0 {
        unsafe { GetExitCodeProcess(process, &mut code) }.is_ok()
    } else {
        false
    };
    // SAFETY: the handle is ours and not used after this.
    unsafe {
        let _ = CloseHandle(process);
    }
    if waited != WAIT_OBJECT_0 {
        return Err(ElevationError::Failed(None, "The helper installer did not finish in time.".into()));
    }
    match (got, code) {
        (true, 0) => Ok(()),
        (true, c) => Err(ElevationError::Failed(
            i32::try_from(c).ok(),
            format!("The helper installer failed (exit code {c}); see the Dyno log for details."),
        )),
        (false, _) => Err(ElevationError::Failed(None, "Could not read the installer's result.".into())),
    }
}
