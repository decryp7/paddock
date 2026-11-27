//! Which kind of file system the lock folder is on. The slot locks are kernel file locks: on a network file system (SMB, NFS and the
//! like) they are emulated, unreliable or per-client, and two machines would not exclude each other the way one expects. So the lock
//! folder must be local, and `throttle` refuses a network one with a message that says what to do (the file system is read with
//! `statfs` on macOS and Linux and the drive type on Windows).
//!
//! The classifiers are pure (a name, a magic number, a path text) and tested everywhere; only [`of`] reads the OS.
#![allow(unsafe_code)]

use std::path::Path;

/// What is known about the file system a folder is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsKind {
    /// A local file system.
    Local,
    /// A network file system: the name it was recognised by (`nfs`, `smbfs`, `\\server\share`, ...).
    Network(String),
    /// The OS did not say. Not refused (nothing was measured), but never read as "local" in a log (ER-33).
    Unknown(String),
}

/// macOS `f_fstypename` values that are network file systems.
pub fn classify_name(name: &str) -> FsKind {
    match name {
        "nfs" | "smbfs" | "afpfs" | "webdav" | "cifs" | "ftp" => FsKind::Network(name.to_string()),
        "" => FsKind::Unknown("empty file system name".into()),
        _ => FsKind::Local,
    }
}

/// Linux `statfs` `f_type` values that are network file systems (`linux/magic.h`).
pub fn classify_magic(magic: u64) -> FsKind {
    let name = match magic {
        0x6969 => "nfs",
        0x517B => "smb",
        0xFF53_4D42 => "cifs",
        0xFE53_4D42 => "smb2",
        0x7375_7245 => "coda",
        0x5346_414F => "afs",
        0x564C => "ncp",
        0x0102_1997 => "9p",
        0x00C3_6400 => "ceph",
        _ => return FsKind::Local,
    };
    FsKind::Network(name.to_string())
}

/// Where a Windows path text lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WinRoot {
    /// `\\server\share` or `\\?\UNC\server\share`: always a network share.
    Unc,
    /// A drive letter: `GetDriveTypeW` tells.
    Drive(char),
    /// Anything else (a relative path, a device path).
    Other,
}

/// The root of a Windows path text (pure, so it is tested on every OS).
pub fn win_root(path: &str) -> WinRoot {
    let p = path.replace('/', "\\");
    if let Some(rest) = p.strip_prefix("\\\\?\\") {
        if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case("UNC\\") {
            return WinRoot::Unc;
        }
        return match (rest.chars().next(), rest.as_bytes().get(1)) {
            (Some(c), Some(b':')) if c.is_ascii_alphabetic() => {
                WinRoot::Drive(c.to_ascii_uppercase())
            }
            _ => WinRoot::Other,
        };
    }
    if p.starts_with("\\\\") {
        return WinRoot::Unc;
    }
    match (p.chars().next(), p.as_bytes().get(1)) {
        (Some(c), Some(b':')) if c.is_ascii_alphabetic() => WinRoot::Drive(c.to_ascii_uppercase()),
        _ => WinRoot::Other,
    }
}

/// The kind of the file system `path` is on (the folder must exist).
#[cfg(target_os = "macos")]
pub fn of(path: &Path) -> FsKind {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
        return FsKind::Unknown("path with a NUL byte".into());
    };
    // SAFETY: a zeroed `statfs` is a valid output buffer, `c` is a NUL-terminated path that lives for the call.
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &raw mut st) } != 0 {
        return FsKind::Unknown(format!(
            "statfs failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let bytes: Vec<u8> = st
        .f_fstypename
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| *b as u8)
        .collect();
    classify_name(&String::from_utf8_lossy(&bytes))
}

/// The kind of the file system `path` is on (the folder must exist).
#[cfg(all(unix, not(target_os = "macos")))]
pub fn of(path: &Path) -> FsKind {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
        return FsKind::Unknown("path with a NUL byte".into());
    };
    // SAFETY: a zeroed `statfs` is a valid output buffer, `c` is a NUL-terminated path that lives for the call.
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &raw mut st) } != 0 {
        return FsKind::Unknown(format!(
            "statfs failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    #[allow(
        clippy::unnecessary_cast,
        clippy::cast_sign_loss,
        reason = "f_type is i32, i64 or u32 by target; the magic numbers fit 32 bits"
    )]
    classify_magic(u64::from(st.f_type as u32))
}

/// The kind of the file system `path` is on (the folder must exist).
#[cfg(windows)]
pub fn of(path: &Path) -> FsKind {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    const DRIVE_REMOTE: u32 = 4;
    // The resolved path in its plain form (`C:\...`, `\\server\share\...`); win_root reads either form.
    let canon = paddock_env::winpath::real(path).unwrap_or_else(|_| path.to_path_buf());
    let text = canon.to_string_lossy().into_owned();
    match win_root(&text) {
        WinRoot::Unc => FsKind::Network(text),
        WinRoot::Drive(d) => {
            let wide: Vec<u16> = format!("{d}:\\")
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            // SAFETY: `wide` is a NUL-terminated UTF-16 root path that lives for the call.
            if unsafe { GetDriveTypeW(wide.as_ptr()) } == DRIVE_REMOTE {
                FsKind::Network(format!("{d}: (a mapped network drive)"))
            } else {
                FsKind::Local
            }
        }
        WinRoot::Other => FsKind::Unknown(format!("no drive in {text}")),
    }
}

/// `Err` with the message to print when the lock folder `root` is on a network file system; `Ok` for a local one and for one the OS did
/// not name (the caller logs an `Unknown`).
pub fn check_root(kind: &FsKind, root: &Path) -> Result<(), String> {
    match kind {
        FsKind::Network(n) => Err(format!(
            "the lock folder {} is on a network file system ({n}): file locks there are not reliable between machines or after a drop of the connection. \
             Set THROTTLE_DIR (or `dir` in throttle's config.toml) to a folder on a local disk, one per machine",
            root.display()
        )),
        FsKind::Local | FsKind::Unknown(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_names_are_classified() {
        for n in ["nfs", "smbfs", "afpfs", "webdav", "cifs"] {
            assert_eq!(classify_name(n), FsKind::Network(n.into()), "{n}");
        }
        for n in ["apfs", "hfs", "msdos", "exfat", "tmpfs"] {
            assert_eq!(classify_name(n), FsKind::Local, "{n}");
        }
        assert!(matches!(classify_name(""), FsKind::Unknown(_)));
    }

    #[test]
    fn linux_magics_are_classified() {
        for m in [0x6969, 0x517B, 0xFF53_4D42, 0xFE53_4D42, 0x0102_1997] {
            assert!(matches!(classify_magic(m), FsKind::Network(_)), "{m:#x}");
        }
        for m in [
            0xEF53,      /* ext4 */
            0x0102_1994, /* tmpfs */
            0x9123_683E, /* btrfs */
            0x5846_5342, /* xfs */
        ] {
            assert_eq!(classify_magic(m), FsKind::Local, "{m:#x}");
        }
    }

    #[test]
    fn windows_roots_are_read_from_the_path_text() {
        assert_eq!(win_root(r"\\nas\share\x"), WinRoot::Unc);
        assert_eq!(win_root(r"\\?\UNC\nas\share"), WinRoot::Unc);
        assert_eq!(win_root(r"\\?\C:\a"), WinRoot::Drive('C'));
        assert_eq!(win_root(r"d:/a/b"), WinRoot::Drive('D'));
        assert_eq!(win_root(r"relative\a"), WinRoot::Other);
    }

    #[test]
    fn a_network_folder_is_refused_with_what_to_do_and_a_local_or_unknown_one_is_not() {
        let root = &paddock_testkit::path::abs(["Volumes", "share", "throttle"]);
        let e = check_root(&FsKind::Network("smbfs".into()), root).expect_err("refused");
        assert!(
            e.contains("network file system") && e.contains("smbfs") && e.contains("THROTTLE_DIR"),
            "{e}"
        );
        assert!(check_root(&FsKind::Local, root).is_ok());
        assert!(check_root(&FsKind::Unknown("x".into()), root).is_ok());
    }

    #[test]
    fn a_temp_folder_on_this_machine_is_local_or_unknown_never_network() {
        let d = tempfile::tempdir().expect("tempdir");
        assert!(
            !matches!(of(d.path()), FsKind::Network(_)),
            "{:?}",
            of(d.path())
        );
    }
}
