//! The identity of a folder: which object on which volume it is, and when it was born.
//!
//! Modification times prove nothing about freshness (cargo unpacks sources with the times stored in the package, copies keep
//! their times, a writer can set any time), and a directory's change time moves whenever an entry is added. Release
//! isolation therefore compares:
//!
//! - **device and inode** for equality (ADR 0085 §5 item 2 and its Rust-tool amendment): a replaced folder is another object;
//! - **birth time** (`Metadata::created()`: `statx` btime on Linux, the creation date on APFS, `ftCreationTime` on Windows) as
//!   a floor against the *job's* start. A birth time never changes for an object and cannot be set from user space on Unix.
//!   A file system that cannot report it makes the check *broken*, never a pass.
//!
//! Windows: the volume serial number (64-bit) and the 128-bit file id of `GetFileInformationByHandleEx(FileIdInfo)`
//! (`FILE_ID_INFO`, <https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info>, read 2026-11-10);
//! file systems without 128-bit ids answer `ERROR_INVALID_PARAMETER`, and then the 64-bit file index of
//! `GetFileInformationByHandle` is used (`BY_HANDLE_FILE_INFORMATION`).

use std::io;
use std::path::Path;

/// Which object a folder is, and when it was born.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    /// The device (Unix `st_dev`) or the volume serial number (Windows).
    pub dev: u64,
    /// The inode (Unix) or the file id (Windows, up to 128 bits).
    pub ino: u128,
    /// Nanoseconds since the Unix epoch of the birth (creation) of the object.
    pub birth_ns: i128,
}

/// Reads the identity of `path` (links are followed: pass the canonical path). An error of kind `Unsupported` means the file
/// system reports no birth time.
pub fn identity_of(path: &Path) -> io::Result<Identity> {
    let meta = std::fs::metadata(path)?;
    let birth = meta.created().map_err(|e| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            format!("the file system reports no birth time for {}: {e}", path.display()),
        )
    })?;
    let birth_ns = match birth.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => i128::try_from(d.as_nanos()).unwrap_or(i128::MAX),
        Err(e) => -i128::try_from(e.duration().as_nanos()).unwrap_or(i128::MAX),
    };
    let (dev, ino) = imp::device_and_inode(path, &meta)?;
    Ok(Identity { dev, ino, birth_ns })
}

#[cfg(unix)]
mod imp {
    use std::io;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    pub fn device_and_inode(_path: &Path, m: &std::fs::Metadata) -> io::Result<(u64, u128)> {
        Ok((m.dev(), u128::from(m.ino())))
    }
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO, FileIdInfo, GetFileInformationByHandle,
        GetFileInformationByHandleEx,
    };

    const ERROR_INVALID_PARAMETER: i32 = 87;

    #[allow(unsafe_code)] // two Win32 calls on a handle this function opened itself
    pub fn device_and_inode(path: &Path, _m: &std::fs::Metadata) -> io::Result<(u64, u128)> {
        // A folder is opened only with backup semantics.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        // SAFETY: plain data written by the call below.
        let mut id: FILE_ID_INFO = unsafe { std::mem::zeroed() };
        // SAFETY: `file` is a valid open handle; the buffer is a FILE_ID_INFO of the size passed.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                std::ptr::addr_of_mut!(id).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        };
        if ok != 0 {
            return Ok((id.VolumeSerialNumber, u128::from_le_bytes(id.FileId.Identifier)));
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(ERROR_INVALID_PARAMETER) {
            return Err(err);
        }
        // SAFETY: plain data written by the call below.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: `file` is a valid open handle and `info` is a valid out pointer.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((
            u64::from(info.dwVolumeSerialNumber),
            (u128::from(info.nFileIndexHigh) << 32) | u128::from(info.nFileIndexLow),
        ))
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
        assert!(ia.birth_ns > 1_700_000_000_000_000_000, "a birth time of this decade");
    }

    #[test]
    fn the_birth_time_does_not_move_when_entries_are_added() {
        let a = tempfile::tempdir().unwrap();
        let before = identity_of(a.path()).unwrap();
        std::fs::write(a.path().join("f"), "x").unwrap();
        assert_eq!(before, identity_of(a.path()).unwrap());
    }

    #[test]
    fn a_missing_path_is_an_error() {
        let a = tempfile::tempdir().unwrap();
        assert!(identity_of(&a.path().join("nope")).is_err());
    }
}
