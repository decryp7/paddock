use paddock_jobs::{CancelReason, Cancelled, JobError};
use paddock_limits::LimitExceeded;
use std::fmt;
use std::path::PathBuf;

/// Why an archive was not unpacked. Typed; the app edge turns it into a message. Nothing is left behind: the staging folder is
/// removed and the destination is untouched.
#[derive(Debug)]
pub enum ArchiveError {
    /// A file or folder operation failed.
    Io {
        /// What was being done.
        op: &'static str,
        /// The path involved.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// Not a format this crate reads.
    Unsupported(String),
    /// The archive is damaged or truncated.
    Corrupt(String),
    /// An entry would escape the folder or is not allowed (`..`, absolute path, drive letter, a link out of the tree, a
    /// link where links are refused, a device file).
    Unsafe {
        /// The entry's name as the archive spells it.
        entry: String,
        /// Why it is refused.
        reason: String,
    },
    /// A limit was exceeded: entries, file size, total size, expansion ratio, path length or depth. The id is in
    /// [`crate::limits`]; the type is kept by the job the unpack ran in ([`JobError::Limit`]).
    Limit(LimitExceeded),
    /// Two entries have the same path (or differ only in case, which a case-insensitive disk cannot hold).
    Duplicate(String),
    /// Stopped on request or at the deadline.
    Cancelled(CancelReason),
}

/// Result of this crate.
pub type Result<T> = std::result::Result<T, ArchiveError>;

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArchiveError::Io { op, path, source } => write!(
                f,
                "cannot {op} {}: {source}. Check the disk space and permissions.",
                path.display()
            ),
            ArchiveError::Unsupported(m) => write!(f, "{m}"),
            ArchiveError::Corrupt(m) => {
                write!(f, "the archive is damaged: {m}. Download it again.")
            }
            ArchiveError::Unsafe { entry, reason } => {
                write!(f, "the archive is refused: {entry:?} {reason}.")
            }
            ArchiveError::Limit(l) => write!(
                f,
                "the archive is refused. {}",
                l.message(&crate::limits::REGISTRY)
            ),
            ArchiveError::Duplicate(name) => {
                write!(f, "the archive is refused: {name:?} appears twice.")
            }
            ArchiveError::Cancelled(r) => write!(f, "cancelled ({r})"),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<Cancelled> for ArchiveError {
    fn from(c: Cancelled) -> Self {
        ArchiveError::Cancelled(c.0)
    }
}

impl From<ArchiveError> for JobError {
    fn from(e: ArchiveError) -> Self {
        match e {
            ArchiveError::Cancelled(r) => JobError::Cancelled(r),
            ArchiveError::Limit(l) => JobError::Limit(l),
            other => JobError::Failed(other.to_string()),
        }
    }
}

pub(crate) fn io(
    op: &'static str,
    path: &std::path::Path,
) -> impl FnOnce(std::io::Error) -> ArchiveError {
    let path = path.to_path_buf();
    move |source| ArchiveError::Io { op, path, source }
}
