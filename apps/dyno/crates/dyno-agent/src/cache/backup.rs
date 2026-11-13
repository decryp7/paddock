//! Keeping the cache folder out of backups (ADR 0029 section 2). `~/Library/Caches` is commonly left out of Time Machine, which
//! is not relied on: on macOS the folder and the file get the backup-exclusion attribute explicitly. Linux's cache folder is
//! outside the usual backup sets and Windows' `%LOCALAPPDATA%` is not roamed; neither has anything to set.
#![allow(unsafe_code)]

use std::path::Path;

/// The value of `com.apple.metadata:com_apple_backup_excludeItem`: a binary property list holding the string
/// `com.apple.backupd`, what `NSURLIsExcludedFromBackupKey` writes.
pub fn exclusion_value() -> Vec<u8> {
    const WHO: &[u8] = b"com.apple.backupd";
    let mut v = Vec::new();
    v.extend_from_slice(b"bplist00");
    // An ASCII string longer than 14 bytes: type 0x5F, then its length as a 1-byte integer.
    v.push(0x5F);
    v.push(0x10);
    v.push(WHO.len() as u8);
    v.extend_from_slice(WHO);
    let table_at = v.len() as u64;
    v.push(8); // the one object starts after the 8-byte magic
    v.extend_from_slice(&[0; 5]);
    v.push(0); // sort version
    v.push(1); // offset size
    v.push(1); // object reference size
    v.extend_from_slice(&1u64.to_be_bytes()); // objects
    v.extend_from_slice(&0u64.to_be_bytes()); // top object
    v.extend_from_slice(&table_at.to_be_bytes());
    v
}

/// Sets the exclusion on `path` where the OS has one; `Ok(false)` where there is nothing to set.
pub fn exclude_from_backup(path: &Path) -> std::io::Result<bool> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        let name = c"com.apple.metadata:com_apple_backup_excludeItem";
        let value = exclusion_value();
        // SAFETY: both strings are NUL-terminated and live through the call; the value pointer and length describe `value`.
        let rc = unsafe { libc::setxattr(c.as_ptr(), name.as_ptr(), value.as_ptr().cast(), value.len(), 0, 0) };
        if rc == 0 {
            Ok(true)
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Ok(false)
    }
}

/// Reads the attribute back (tests): the bytes set, or `None`.
#[cfg(target_os = "macos")]
pub fn read_exclusion(path: &Path) -> Option<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let name = c"com.apple.metadata:com_apple_backup_excludeItem";
    let mut buf = vec![0u8; 256];
    // SAFETY: the buffer is 256 bytes and the call is told so; the strings are NUL-terminated.
    let n = unsafe { libc::getxattr(c.as_ptr(), name.as_ptr(), buf.as_mut_ptr().cast(), buf.len(), 0, 0) };
    (n >= 0).then(|| {
        buf.truncate(n as usize);
        buf
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_value_is_a_property_list_holding_the_backup_daemon_name() {
        let v = plist::Value::from_reader(std::io::Cursor::new(exclusion_value())).unwrap();
        assert_eq!(v.as_string(), Some("com.apple.backupd"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_file_and_its_folder_are_excluded_on_macos() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let f = d.path().join("cache.db");
        std::fs::write(&f, b"x").unwrap();
        assert!(exclude_from_backup(&f).unwrap());
        assert!(exclude_from_backup(d.path()).unwrap());
        assert_eq!(read_exclusion(&f), Some(exclusion_value()));
        assert_eq!(read_exclusion(d.path()), Some(exclusion_value()));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn nothing_is_set_elsewhere() {
        assert!(!exclude_from_backup(Path::new("/nope")).unwrap());
    }
}
