//! The few operating-system calls the helper makes itself: which program a process is, and whether this process
//! is elevated. Each is a small function with a `SAFETY` comment on every `unsafe` block (AGENTS.md).
#![allow(unsafe_code)]

use std::path::PathBuf;

/// The program file of process `pid`, when the OS tells (Linux `/proc/<pid>/exe`).
#[cfg(target_os = "linux")]
pub fn process_image(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

/// The program file of process `pid`, when the OS tells (`proc_pidpath`).
#[cfg(target_os = "macos")]
pub fn process_image(pid: u32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    // PROC_PIDPATHINFO_MAXSIZE is 4 * MAXPATHLEN.
    let mut buf = vec![0u8; 4096];
    // SAFETY: `buf` is writable for the length passed; the call writes at most that many bytes.
    let n = unsafe { libc::proc_pidpath(i32::try_from(pid).ok()?, buf.as_mut_ptr().cast(), buf.len() as u32) };
    let n = usize::try_from(n).ok().filter(|n| *n > 0)?;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(buf.get(..n)?)))
}

/// The program file of process `pid`, when the OS tells (`QueryFullProcessImageNameW`).
#[cfg(windows)]
pub fn process_image(pid: u32) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: a plain query for a handle; the result is checked and closed below.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = vec![0u16; 4096];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` is writable for `len` UTF-16 units; `len` is updated to the number written.
    let ok = unsafe { QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok();
    // SAFETY: the handle was opened above and is not used after this.
    unsafe {
        let _ = CloseHandle(handle);
    }
    ok.then(|| PathBuf::from(std::ffi::OsString::from_wide(buf.get(..len as usize).unwrap_or_default())))
}

/// Other systems: unknown (the caller then refuses, failing closed).
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn process_image(_: u32) -> Option<PathBuf> {
    None
}

/// Whether this process runs as root.
#[cfg(unix)]
pub fn is_elevated() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

/// Windows cannot be asked cheaply here; the install steps fail with "access denied" when it is not elevated.
#[cfg(windows)]
pub fn is_elevated() -> bool {
    true
}
