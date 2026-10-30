//! The real file system behind the disk scan's [`DirSource`] port: reads sizes as allocated bytes, never follows a link,
//! reports the volume and file id of every entry. Nothing here writes.

use dyno_core::disk::source::{DirEntryInfo, DirSource, EntryKind, ReadError};
use std::ffi::OsString;
use std::fs::Metadata;
use std::io;
use std::path::Path;

/// The operating system's file system.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsDirSource;

/// Whether an OS error number means the volume went away (a dropped share, an unplugged disk).
pub fn is_unreachable_errno(code: i32) -> bool {
    #[cfg(unix)]
    {
        [
            libc::ENOTCONN,
            libc::ETIMEDOUT,
            libc::EHOSTDOWN,
            libc::EHOSTUNREACH,
            libc::ESTALE,
            libc::ENXIO,
            libc::ENETDOWN,
            libc::ENETUNREACH,
        ]
        .contains(&code)
    }
    #[cfg(windows)]
    {
        // ERROR_BAD_NETPATH 53, ERROR_DEV_NOT_EXIST 55, ERROR_UNEXP_NET_ERR 59, ERROR_NETNAME_DELETED 64,
        // ERROR_NETWORK_UNREACHABLE 1231, ERROR_HOST_UNREACHABLE 1232.
        matches!(code, 53 | 55 | 59 | 64 | 1231 | 1232)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = code;
        false
    }
}

/// `EPERM`, "operation not permitted" (the same number on macOS, Linux and the BSDs; Windows never reports it).
const EPERM: i32 = 1;

/// Maps an I/O error to the port's typed error.
pub fn read_error(e: &io::Error) -> ReadError {
    if e.raw_os_error().is_some_and(is_unreachable_errno) {
        return ReadError::Unreachable(e.to_string());
    }
    match e.kind() {
        io::ErrorKind::PermissionDenied if e.raw_os_error() == Some(EPERM) => ReadError::NotPermitted,
        io::ErrorKind::PermissionDenied => ReadError::PermissionDenied,
        io::ErrorKind::NotFound => ReadError::NotFound,
        _ => ReadError::Other(e.to_string()),
    }
}

/// (allocated bytes, device, file id, links) of an entry; the length is read from the metadata.
#[cfg(unix)]
fn identity(meta: &Metadata, _path: &Path) -> (u64, u64, u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (meta.blocks().saturating_mul(512), meta.dev(), meta.ino(), meta.nlink())
}

/// On Windows the identity and the allocation come from an open handle (volume serial plus file index, link count,
/// allocation size). An entry that cannot be opened (a sharing violation, no rights, a path the API refuses) is counted by its
/// length with device 0, which the walk reads as "volume unknown": the entry is neither lost, nor taken for a mount point,
/// nor treated as a hard link of anything.
#[cfg(windows)]
fn identity(meta: &Metadata, path: &Path) -> (u64, u64, u64, u64) {
    match dyno_platform::fileid::file_facts(path) {
        Ok(f) => (f.allocated, f.volume, f.index, f.links),
        Err(_) => (meta.len(), 0, 0, 1),
    }
}

#[cfg(not(any(unix, windows)))]
fn identity(meta: &Metadata, _path: &Path) -> (u64, u64, u64, u64) {
    (meta.len(), 0, 0, 1)
}

/// What an entry is. Only a name-surrogate reparse point (a symlink or a junction, which `FileType::is_symlink` reports) is a
/// link that is not followed. Other reparse points are ordinary files and folders to the walk: a OneDrive placeholder, a
/// deduplicated file, a compressed or cloud file all keep their size and their place in the tree.
fn classify(is_symlink: bool, is_dir: bool, is_file: bool) -> EntryKind {
    if is_symlink {
        EntryKind::Link
    } else if is_dir {
        EntryKind::Dir
    } else if is_file {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

fn info(name: OsString, path: &Path, meta: &Metadata) -> DirEntryInfo {
    let ft = meta.file_type();
    let kind = classify(ft.is_symlink(), meta.is_dir(), meta.is_file());
    let (bytes, dev, ino, links) = if kind == EntryKind::Link {
        (0, 0, 0, 1)
    } else {
        identity(meta, path)
    };
    DirEntryInfo {
        name,
        kind,
        bytes: if kind == EntryKind::File { bytes } else { 0 },
        logical: if kind == EntryKind::File { meta.len() } else { 0 },
        dev,
        ino,
        links,
        mtime: super::walk::modified(meta),
    }
}

impl DirSource for OsDirSource {
    fn stat(&self, path: &Path) -> Result<DirEntryInfo, ReadError> {
        let meta = std::fs::symlink_metadata(path).map_err(|e| read_error(&e))?;
        let name = path.file_name().map_or_else(|| path.as_os_str().to_owned(), OsString::from);
        Ok(info(name, path, &meta))
    }

    fn read_dir(&self, path: &Path, each: &mut dyn FnMut(Result<DirEntryInfo, ReadError>) -> bool) -> Result<(), ReadError> {
        for entry in std::fs::read_dir(path).map_err(|e| read_error(&e))? {
            let item = entry.and_then(|e| e.metadata().map(|m| info(e.file_name(), &e.path(), &m)));
            if !each(item.map_err(|e| read_error(&e))) {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_link_is_a_link_and_other_reparse_points_stay_files_and_folders() {
        // A OneDrive placeholder or a deduplicated file is a reparse point whose type is not a symlink: `is_symlink` is false.
        assert_eq!(classify(false, false, true), EntryKind::File);
        assert_eq!(classify(false, true, false), EntryKind::Dir);
        assert_eq!(classify(true, false, true), EntryKind::Link);
        assert_eq!(
            classify(true, true, false),
            EntryKind::Link,
            "a junction or symlink to a folder is not entered"
        );
        assert_eq!(classify(false, false, false), EntryKind::Other);
    }

    #[test]
    fn entries_have_kind_size_and_identity() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir(t.path().join("sub")).unwrap();
        std::fs::write(t.path().join("file.bin"), vec![1u8; 5000]).unwrap();
        let mut seen = Vec::new();
        OsDirSource
            .read_dir(t.path(), &mut |e| {
                seen.push(e.unwrap());
                true
            })
            .unwrap();
        seen.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(seen[0].name, OsString::from("file.bin"));
        assert_eq!(seen[0].kind, EntryKind::File);
        assert!(seen[0].bytes >= 5000, "allocated bytes are at least the length");
        assert_eq!(seen[0].logical, 5000, "the length is kept next to the allocation");
        assert_eq!(seen[1].logical, 0);
        assert_eq!(seen[1].kind, EntryKind::Dir);
        assert_eq!(seen[1].bytes, 0, "a folder's own entry counts nothing");
        let me = OsDirSource.stat(t.path()).unwrap();
        assert_eq!(me.kind, EntryKind::Dir);
        #[cfg(unix)]
        assert_eq!(me.dev, seen[1].dev);
    }

    #[cfg(unix)]
    #[test]
    fn a_sparse_file_occupies_less_than_its_length() {
        let t = tempfile::tempdir().unwrap();
        let f = std::fs::File::create(t.path().join("sparse.img")).unwrap();
        f.set_len(64 * 1024 * 1024).unwrap();
        let e = OsDirSource.stat(&t.path().join("sparse.img")).unwrap();
        assert_eq!(e.logical, 64 * 1024 * 1024);
        assert!(e.bytes < e.logical, "holes take no blocks on a file system that supports them");
    }

    #[test]
    fn reading_can_stop_early() {
        let t = tempfile::tempdir().unwrap();
        for n in 0..5 {
            std::fs::write(t.path().join(format!("f{n}")), b"x").unwrap();
        }
        let mut count = 0;
        OsDirSource
            .read_dir(t.path(), &mut |_| {
                count += 1;
                false
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn links_are_reported_not_followed() {
        let t = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(target.path(), t.path().join("shortcut")).unwrap();
        assert_eq!(OsDirSource.stat(&t.path().join("shortcut")).unwrap().kind, EntryKind::Link);
    }

    #[cfg(unix)]
    #[test]
    fn a_socket_is_neither_file_nor_folder() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("s");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        assert_eq!(OsDirSource.stat(&path).unwrap().kind, EntryKind::Other);
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_share_an_identity() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("a"), b"hello").unwrap();
        std::fs::hard_link(t.path().join("a"), t.path().join("b")).unwrap();
        let (a, b) = (
            OsDirSource.stat(&t.path().join("a")).unwrap(),
            OsDirSource.stat(&t.path().join("b")).unwrap(),
        );
        assert_eq!((a.dev, a.ino), (b.dev, b.ino));
        assert_eq!(a.links, 2);
    }

    #[test]
    fn errors_are_typed() {
        let missing = Path::new("no-such-folder-for-dyno-disk-tests");
        assert_eq!(OsDirSource.stat(missing), Err(ReadError::NotFound));
        assert_eq!(OsDirSource.read_dir(missing, &mut |_| true), Err(ReadError::NotFound));
        assert_eq!(
            read_error(&io::Error::from(io::ErrorKind::PermissionDenied)),
            ReadError::PermissionDenied
        );
        #[cfg(unix)]
        {
            assert_eq!(read_error(&io::Error::from_raw_os_error(EPERM)), ReadError::NotPermitted, "EPERM");
            assert_eq!(read_error(&io::Error::from_raw_os_error(13)), ReadError::PermissionDenied, "EACCES");
        }
        assert!(matches!(read_error(&io::Error::other("odd")), ReadError::Other(m) if m == "odd"));
        #[cfg(unix)]
        {
            let gone = io::Error::from_raw_os_error(libc::ENOTCONN);
            assert!(matches!(read_error(&gone), ReadError::Unreachable(_)));
            assert!(!is_unreachable_errno(libc::EACCES));
        }
        #[cfg(windows)]
        assert!(is_unreachable_errno(64) && !is_unreachable_errno(5));
    }

    #[cfg(unix)]
    #[allow(unsafe_code)]
    #[test]
    fn a_folder_without_rights_is_a_permission_error() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let locked = t.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let r = OsDirSource.read_dir(&locked, &mut |_| true);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        // SAFETY: `geteuid` has no arguments and cannot fail.
        if unsafe { libc::geteuid() } != 0 {
            assert_eq!(r, Err(ReadError::PermissionDenied));
        }
    }
}
