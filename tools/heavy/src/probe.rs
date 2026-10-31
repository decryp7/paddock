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

/// Identity of a file or directory, to notice that it was replaced: (device or volume, inode or file index).
#[cfg(unix)]
pub fn file_id(p: &std::path::Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p).ok().map(|m| (m.dev(), m.ino()))
}

/// Identity of a file or directory: the volume serial number and the 64-bit file index (`GetFileInformationByHandle`).
#[cfg(windows)]
pub fn file_id(p: &std::path::Path) -> Option<(u64, u64)> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000; // needed to open a directory
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(p)
        .ok()?;
    // SAFETY: a valid handle owned by `f` for the whole call, and a writable, zeroed output struct.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(f.as_raw_handle().cast(), &raw mut info) } == 0 {
        return None;
    }
    Some((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}
