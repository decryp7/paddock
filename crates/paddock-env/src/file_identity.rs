//! Which file a path names, and whether it changed, for a cache keyed by path (rule 53's class: a path is a reusable name). The
//! fields are git's index stat data: the device and inode (the file itself, so a file renamed over the path is a different
//! one), the size, the modification time and the inode change time (which any write, `chmod` or rename touches and which
//! `utimes` cannot set back). A cache keeps an answer only while [`FileIdentity::of`] still gives the same value, and reads it
//! again after the work it cached, keeping the answer only when the file did not change meanwhile.
//!
//! Unix only for now: stable std has no file index or change time on Windows. The family's Windows identity (volume serial
//! number and file index from `GetFileInformationByHandle`) is in Debrief's tail (`debrief-core/src/workspace/tail/identity.rs`);
//! moving it here, with the change time from `FILE_BASIC_INFO`, is registered in `docs/known-copies.toml` (`file-identity`).

#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::path::Path;
use std::time::SystemTime;

/// A file's identity and stat data at one moment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    /// The device (`st_dev`).
    pub device: u64,
    /// The inode (`st_ino`).
    pub inode: u64,
    /// The size in bytes.
    pub len: u64,
    /// The modification time.
    pub modified: Option<SystemTime>,
    /// The inode change time (`st_ctime`, in nanoseconds since 1970).
    pub changed: Option<i128>,
}

#[cfg(unix)]
impl FileIdentity {
    /// The identity of the file `path` names now (following links, as opening it would).
    pub fn of(path: &Path) -> io::Result<FileIdentity> {
        let meta = std::fs::metadata(path)?;
        Ok(Self::from_metadata(&meta))
    }

    /// The identity in `meta`.
    pub fn from_metadata(meta: &std::fs::Metadata) -> FileIdentity {
        use std::os::unix::fs::MetadataExt;
        FileIdentity {
            device: meta.dev(),
            inode: meta.ino(),
            len: meta.len(),
            modified: meta.modified().ok(),
            changed: Some(i128::from(meta.ctime()) * 1_000_000_000 + i128::from(meta.ctime_nsec())),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn the_same_untouched_file_has_the_same_identity() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a");
        std::fs::write(&f, b"one").unwrap();
        assert_eq!(FileIdentity::of(&f).unwrap(), FileIdentity::of(&f).unwrap());
        assert!(FileIdentity::of(&dir.path().join("missing")).is_err());
    }

    /// A file renamed over the path with the same size and its modification time set back is still a different file.
    #[test]
    fn a_file_replaced_with_the_same_size_and_time_is_told_apart() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a");
        std::fs::write(&f, b"one").unwrap();
        let before = FileIdentity::of(&f).unwrap();
        let other = dir.path().join("b");
        std::fs::write(&other, b"two").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&other)
            .unwrap()
            .set_modified(before.modified.unwrap())
            .unwrap();
        std::fs::rename(&other, &f).unwrap();
        let after = FileIdentity::of(&f).unwrap();
        assert_eq!(
            (after.len, after.modified),
            (before.len, before.modified),
            "precondition: size and time look the same"
        );
        assert_ne!(after, before);
    }
}
