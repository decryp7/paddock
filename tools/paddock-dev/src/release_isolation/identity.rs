//! The identity of a folder: which object on which volume it is, and when it last changed as an object.
//!
//! Modification times prove nothing about freshness (cargo unpacks sources with the times stored in the package, copies
//! keep their times, a writer can set any time), so release isolation compares *identity*:
//!
//! - Unix: `st_dev`, `st_ino` and `st_ctime`. The change time cannot be set from user space, unlike `st_mtime`.
//! - Windows: the volume serial number and the 64-bit file index of `GetFileInformationByHandle`
//!   (`BY_HANDLE_FILE_INFORMATION`: `dwVolumeSerialNumber`, `nFileIndexHigh`, `nFileIndexLow`; the pair identifies a file
//!   while a handle to it is open, <https://learn.microsoft.com/en-us/windows/win32/api/fileapi/ns-fileapi-by_handle_file_information>,
//!   read 2026-11-10), and the creation time (`ftCreationTime`) in place of a change time, because the same structure has no
//!   change time and a creation time does not change when entries are added.

use std::io;
use std::path::Path;

/// Which object a folder is, and the change time recorded for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    /// The device (Unix `st_dev`) or the volume serial number (Windows).
    pub dev: u64,
    /// The inode (Unix) or the 64-bit file index (Windows).
    pub ino: u64,
    /// Nanoseconds since the Unix epoch of the last inode change (Unix `st_ctime`) or of the creation (Windows).
    pub change_ns: i128,
}

/// Reads the identity of `path` (links are followed: pass the canonical path).
pub fn identity_of(path: &Path) -> io::Result<Identity> {
    imp::identity_of(path)
}

#[cfg(unix)]
mod imp {
    use super::Identity;
    use std::io;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    pub fn identity_of(path: &Path) -> io::Result<Identity> {
        let m = std::fs::metadata(path)?;
        Ok(Identity {
            dev: m.dev(),
            ino: m.ino(),
            change_ns: i128::from(m.ctime()) * 1_000_000_000 + i128::from(m.ctime_nsec()),
        })
    }
}

#[cfg(windows)]
mod imp {
    use super::Identity;
    use std::io;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, GetFileInformationByHandle};

    /// 100 ns ticks between 1601-01-01 (FILETIME) and 1970-01-01.
    const EPOCH_DIFF_TICKS: i128 = 116_444_736_000_000_000;

    #[allow(unsafe_code)] // one Win32 call on a handle this function opened itself
    pub fn identity_of(path: &Path) -> io::Result<Identity> {
        // A folder is opened only with backup semantics.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        // SAFETY: the structure is plain data, written by the call below; the handle stays open until `file` drops.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: `file` is a valid open handle and `info` is a valid out pointer.
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let created = (i128::from(info.ftCreationTime.dwHighDateTime) << 32) | i128::from(info.ftCreationTime.dwLowDateTime);
        Ok(Identity {
            dev: u64::from(info.dwVolumeSerialNumber),
            ino: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
            change_ns: (created - EPOCH_DIFF_TICKS) * 100,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_folder_has_one_identity_and_another_folder_another() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        assert_eq!(identity_of(a.path()).unwrap(), identity_of(a.path()).unwrap());
        let (ia, ib) = (identity_of(a.path()).unwrap(), identity_of(b.path()).unwrap());
        assert!((ia.dev, ia.ino) != (ib.dev, ib.ino));
        assert!(ia.change_ns > 1_700_000_000_000_000_000, "a change time of this decade");
    }

    #[test]
    fn a_missing_path_is_an_error() {
        let a = tempfile::tempdir().unwrap();
        assert!(identity_of(&a.path().join("nope")).is_err());
    }
}
