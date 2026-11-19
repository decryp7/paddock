//! Which file a path names, and whether it changed, for a cache keyed by path (rule 53's class: a path is a reusable name). The
//! fields are git's index stat data: the device and inode (the file itself, so a file renamed over the path is a different
//! one), the size, the modification time and the inode change time (which any write, `chmod` or rename touches and which
//! `utimes` cannot set back). A cache keeps an answer only while [`FileIdentity::of`] still gives the same value, and reads it
//! again after the work it cached, keeping the answer only when the file did not change meanwhile.
//!
//! On Windows std gives no file index or change time (the stable `MetadataExt` has neither), so the identity there is the size,
//! the modification time and the creation time: weaker, said here, and enough for what calls it there today (nothing).

use std::io;
use std::path::Path;
use std::time::SystemTime;

/// A file's identity and stat data at one moment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    /// The device (Unix `st_dev`); 0 on Windows.
    pub device: u64,
    /// The inode (Unix `st_ino`); 0 on Windows.
    pub inode: u64,
    /// The size in bytes.
    pub len: u64,
    /// The modification time.
    pub modified: Option<SystemTime>,
    /// The inode change time (Unix `st_ctime`, in nanoseconds since 1970), or the creation time on Windows.
    pub changed: Option<i128>,
}

impl FileIdentity {
    /// The identity of the file `path` names now (following links, as opening it would).
    pub fn of(path: &Path) -> io::Result<FileIdentity> {
        let meta = std::fs::metadata(path)?;
        Ok(Self::from_metadata(&meta))
    }

    /// The identity in `meta`.
    pub fn from_metadata(meta: &std::fs::Metadata) -> FileIdentity {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            FileIdentity {
                device: meta.dev(),
                inode: meta.ino(),
                len: meta.len(),
                modified: meta.modified().ok(),
                changed: Some(
                    i128::from(meta.ctime()) * 1_000_000_000 + i128::from(meta.ctime_nsec()),
                ),
            }
        }
        #[cfg(not(unix))]
        {
            let created = meta
                .created()
                .ok()
                .and_then(|c| c.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as i128);
            FileIdentity {
                device: 0,
                inode: 0,
                len: meta.len(),
                modified: meta.modified().ok(),
                changed: created,
            }
        }
    }
}

#[cfg(test)]
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
    #[cfg(unix)]
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
