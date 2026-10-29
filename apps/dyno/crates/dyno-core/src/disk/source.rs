//! The port the disk scan reads folders through. The core owns it; `dyno-agent` supplies the real file system and the
//! tests supply fakes (a tree in memory, a link that drops, a call that never returns).

use crate::time::Timestamp;
use std::path::Path;

/// What a directory entry is. Links are reported, never followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A folder.
    Dir,
    /// A regular file.
    File,
    /// A symbolic link, junction or other reparse point.
    Link,
    /// A device, socket, pipe or anything else without a size worth counting.
    Other,
}

/// One entry of a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntryInfo {
    /// File name exactly as the file system gave it (not necessarily valid text).
    pub name: std::ffi::OsString,
    /// Kind.
    pub kind: EntryKind,
    /// Bytes the entry occupies on the volume (allocated: blocks on Unix, clusters on Windows; the length where the OS
    /// can't say).
    pub bytes: u64,
    /// Bytes of content (the length).
    pub logical: u64,
    /// The volume the entry is on (`st_dev`; the volume serial on Windows).
    pub dev: u64,
    /// The entry's file id, with `dev` its identity.
    pub ino: u64,
    /// How many names the file has; above 1 it is a hard link.
    pub links: u64,
    /// Last modification.
    pub mtime: Timestamp,
}

/// Why a folder or an entry could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    /// The OS refused (no rights, or macOS Full Disk Access is off).
    #[error("access denied")]
    PermissionDenied,
    /// "Operation not permitted" (EPERM): on macOS the privacy protection (Full Disk Access and kin), not file modes.
    #[error("operation not permitted")]
    NotPermitted,
    /// Gone while the scan ran.
    #[error("not found")]
    NotFound,
    /// The volume stopped answering: a dropped network share, an unplugged disk.
    #[error("the volume is unreachable: {0}")]
    Unreachable(String),
    /// Anything else the OS said.
    #[error("{0}")]
    Other(String),
}

/// Reads folders. Implementations must be safe to call from several threads.
pub trait DirSource: Send + Sync {
    /// The entry for `path` itself (not following a final link).
    fn stat(&self, path: &Path) -> Result<DirEntryInfo, ReadError>;
    /// Calls `each` for every entry of the folder `path`, in any order, until it returns false. An entry that could
    /// not be read is passed as an `Err` so the scan can count it.
    fn read_dir(&self, path: &Path, each: &mut dyn FnMut(Result<DirEntryInfo, ReadError>) -> bool) -> Result<(), ReadError>;
}
