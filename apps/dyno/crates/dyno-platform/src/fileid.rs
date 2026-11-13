//! A file's identity and its two sizes, for the disk map on Windows: the volume serial number and the 64-bit file index
//! say which file a name is (a hard link is counted once, a folder is entered once), the link count says whether to
//! bother, and the allocation size is what the file occupies on the volume (clusters, compression and sparseness
//! included) next to its length. The numbers come from an open handle (`GetFileInformationByHandle` and
//! `GetFileInformationByHandleEx`); folding them into [`FileFacts`] is pure and tested with invented numbers on every OS.
#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]
#![allow(unsafe_code)]

/// What the disk map needs to know about one file or folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileFacts {
    /// The volume's serial number (the "device").
    pub volume: u64,
    /// The file index on that volume; with `volume` it names the file.
    pub index: u64,
    /// How many names the file has.
    pub links: u64,
    /// Bytes allocated on the volume.
    pub allocated: u64,
    /// Bytes of content.
    pub length: u64,
}

/// The raw numbers of one handle, as the two Win32 calls give them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawHandleInfo {
    /// `dwVolumeSerialNumber`.
    pub volume_serial: u32,
    /// `nFileIndexHigh`.
    pub index_high: u32,
    /// `nFileIndexLow`.
    pub index_low: u32,
    /// `nNumberOfLinks`.
    pub links: u32,
    /// `FILE_STANDARD_INFO::AllocationSize`.
    pub allocation_size: i64,
    /// `FILE_STANDARD_INFO::EndOfFile`.
    pub end_of_file: i64,
}

/// Folds the raw numbers into [`FileFacts`]. A negative size (the OS never sends one) counts as 0; a file system that
/// reports no allocation (a network share that cannot say) occupies its length, so nothing is under-counted.
pub fn facts_from(raw: RawHandleInfo) -> FileFacts {
    let length = u64::try_from(raw.end_of_file).unwrap_or(0);
    let allocated = match u64::try_from(raw.allocation_size) {
        Ok(a) if a > 0 => a,
        _ => length,
    };
    FileFacts {
        volume: u64::from(raw.volume_serial),
        index: (u64::from(raw.index_high) << 32) | u64::from(raw.index_low),
        links: u64::from(raw.links.max(1)),
        allocated,
        length,
    }
}

/// `path` in the form the Win32 file calls accept past 260 characters (`\\?\C:\...`, `\\?\UNC\server\share\...`). A path that is
/// not absolute, or already in that form, is returned as it is. Slashes become backslashes, which the verbatim form requires.
pub fn verbatim(path: &std::path::Path) -> std::path::PathBuf {
    let mut b = path.as_os_str().as_encoded_bytes().to_vec();
    if b.starts_with(br"\\?\") {
        return path.to_path_buf();
    }
    for c in &mut b {
        if *c == b'/' {
            *c = b'\\';
        }
    }
    let joined = if b.starts_with(br"\\") {
        [&br"\\?\UNC\"[..], &b[2..]].concat()
    } else if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\' {
        [&br"\\?\"[..], &b[..]].concat()
    } else {
        return path.to_path_buf();
    };
    // SAFETY: `joined` is the encoded bytes of `path` with ASCII bytes replaced by ASCII bytes and an ASCII prefix added, which
    // keeps it a valid encoding of the same platform.
    std::path::PathBuf::from(unsafe { std::ffi::OsString::from_encoded_bytes_unchecked(joined) })
}

/// Opens `path` without following a link (and without needing any access to the content) and reads its facts.
#[cfg(windows)]
pub fn file_facts(path: &std::path::Path) -> std::io::Result<FileFacts> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FileStandardInfo, GetFileInformationByHandle, GetFileInformationByHandleEx, BY_HANDLE_FILE_INFORMATION,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FILE_STANDARD_INFO, OPEN_EXISTING,
    };

    /// Closes the handle on every path out.
    struct Open(HANDLE);
    impl Drop for Open {
        fn drop(&mut self) {
            // SAFETY: the handle was opened by `CreateFileW` below and is closed only here.
            unsafe {
                paddock_proc::cleanup::logged("closing a file handle", CloseHandle(self.0));
            }
        }
    }

    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = verbatim(path).as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call; no security attributes and no template file are passed.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF))?;
    let handle = Open(handle);

    let mut by_handle = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open and `by_handle` is valid for the call.
    unsafe { GetFileInformationByHandle(handle.0, &mut by_handle) }.map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF))?;
    let mut standard = FILE_STANDARD_INFO::default();
    // SAFETY: the handle is open; the buffer is a `FILE_STANDARD_INFO` and its size is passed.
    unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            std::mem::size_of::<FILE_STANDARD_INFO>() as u32,
        )
    }
    .map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF))?;
    Ok(facts_from(RawHandleInfo {
        volume_serial: by_handle.dwVolumeSerialNumber,
        index_high: by_handle.nFileIndexHigh,
        index_low: by_handle.nFileIndexLow,
        links: by_handle.nNumberOfLinks,
        allocation_size: standard.AllocationSize,
        end_of_file: standard.EndOfFile,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw() -> RawHandleInfo {
        RawHandleInfo {
            volume_serial: 0xA1B2_C3D4,
            index_high: 0x0001_0002,
            index_low: 0x0000_0003,
            links: 2,
            allocation_size: 8192,
            end_of_file: 5000,
        }
    }

    #[test]
    fn the_file_index_joins_its_two_halves_and_the_sizes_are_kept_apart() {
        let f = facts_from(raw());
        assert_eq!(f.volume, 0xA1B2_C3D4);
        assert_eq!(f.index, 0x0001_0002_0000_0003);
        assert_eq!((f.links, f.allocated, f.length), (2, 8192, 5000));
    }

    #[test]
    fn a_missing_allocation_falls_back_to_the_length_and_odd_numbers_never_wrap() {
        let none = facts_from(RawHandleInfo {
            allocation_size: 0,
            ..raw()
        });
        assert_eq!(none.allocated, 5000);
        let negative = facts_from(RawHandleInfo {
            allocation_size: -1,
            end_of_file: -7,
            links: 0,
            ..raw()
        });
        assert_eq!((negative.allocated, negative.length, negative.links), (0, 0, 1));
        let big = facts_from(RawHandleInfo {
            allocation_size: i64::MAX,
            ..raw()
        });
        assert_eq!(big.allocated, i64::MAX as u64);
    }

    #[test]
    fn long_windows_paths_get_the_verbatim_prefix() {
        use std::path::{Path, PathBuf};
        let v = |s: &str| verbatim(Path::new(s));
        assert_eq!(v(r"C:\Users\alex\file"), PathBuf::from(r"\\?\C:\Users\alex\file"));
        assert_eq!(v(r"\\nas\media\films"), PathBuf::from(r"\\?\UNC\nas\media\films"));
        assert_eq!(v(r"\\?\C:\already"), PathBuf::from(r"\\?\C:\already"));
        assert_eq!(v("relative\\path"), PathBuf::from("relative\\path"));
        assert_eq!(v("/unix/path"), PathBuf::from("/unix/path"), "not a Windows path: untouched");
    }

    #[cfg(windows)]
    #[test]
    fn a_real_file_reports_its_length_and_a_hard_link_shares_its_identity() {
        let t = paddock_testkit::private_tempdir().unwrap();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        std::fs::write(&a, vec![1u8; 5000]).unwrap();
        std::fs::hard_link(&a, &b).unwrap();
        let (fa, fb) = (file_facts(&a).unwrap(), file_facts(&b).unwrap());
        assert_eq!((fa.volume, fa.index), (fb.volume, fb.index));
        assert_eq!((fa.links, fa.length), (2, 5000));
        assert!(fa.allocated >= 5000);
        assert!(file_facts(t.path()).unwrap().index != 0, "a folder has an identity too");
        assert!(file_facts(&t.path().join("missing")).is_err());
    }
}
