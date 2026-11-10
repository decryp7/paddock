//! The errors of the private-files type: a refusal names why a folder or file was not trusted, an I/O error names the step.

use crate::folder::Refusal;
use std::fmt;
use std::path::PathBuf;

/// What can go wrong creating, opening or using a private folder or file. Messages say what to fix and never hold file content.
#[derive(Debug)]
pub enum PrivateError {
    /// The folder or file exists but cannot be trusted (another owner, open to other accounts, a link, a bad parent). Nothing was
    /// written there and nothing was changed: tightening an existing folder cannot evict what was planted while it was open.
    Refused {
        /// The folder or file.
        path: PathBuf,
        /// Why.
        why: Refusal,
    },
    /// An operation failed.
    Io {
        /// What was being done ("create folder", "write", ...).
        op: &'static str,
        /// The folder or file.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// The name is not one plain file or folder name (empty, `.`, `..`, a separator or a NUL).
    BadName(String),
    /// A path that must be absolute is not.
    NotAbsolute(PathBuf),
    /// The file is larger than the caller's limit; it was not read.
    TooLarge {
        /// The file.
        path: PathBuf,
        /// The limit in bytes.
        limit: u64,
    },
}

/// The next step for a refusal, per OS (ADR 0092 section 2): tightening is never done for the user.
#[cfg(windows)]
const FIX_IT: &str = "remove the entries for other users in the folder's Security tab, or point the app's data-folder variable at a folder of your own";
/// The next step for a refusal, per OS (ADR 0092 section 2): tightening is never done for the user.
#[cfg(not(windows))]
const FIX_IT: &str = "run `chmod go-w` on the folder (and `chown` it to your account if it is not yours), or point the app's data-folder variable at a folder of your own";

impl fmt::Display for PrivateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused { path, why } => write!(
                f,
                "{} is not used for private data: {why}; {}",
                path.display(),
                FIX_IT
            ),
            Self::Io { op, path, source } => {
                write!(f, "could not {op} {}: {source}", path.display())
            }
            Self::BadName(name) => write!(f, "{name:?} is not a plain file or folder name"),
            Self::NotAbsolute(path) => write!(f, "{} is not an absolute path", path.display()),
            Self::TooLarge { path, limit } => {
                write!(
                    f,
                    "{} is larger than {limit} bytes and was not read",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for PrivateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Refused { why, .. } => Some(why),
            _ => None,
        }
    }
}

/// `Result` with [`PrivateError`].
pub type PrivateResult<T> = Result<T, PrivateError>;

/// A failure inside the platform layer, before a path is attached to it.
#[derive(Debug)]
pub(super) enum Fail {
    /// The OS said no.
    Io(std::io::Error),
    /// The object is not trusted.
    Refused(Refusal),
}

impl From<std::io::Error> for Fail {
    fn from(e: std::io::Error) -> Self {
        Fail::Io(e)
    }
}

impl From<Refusal> for Fail {
    fn from(r: Refusal) -> Self {
        Fail::Refused(r)
    }
}

/// Attaches the operation and path to a platform failure.
pub(super) fn at(op: &'static str, path: &std::path::Path) -> impl FnOnce(Fail) -> PrivateError {
    let path = path.to_path_buf();
    move |f| match f {
        Fail::Io(source) => PrivateError::Io { op, path, source },
        Fail::Refused(why) => PrivateError::Refused { path, why },
    }
}
