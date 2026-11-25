use std::fmt;
use std::path::PathBuf;

/// What can go wrong resolving folders or writing a state file. Messages say what to fix; none contains file content.
#[derive(Debug)]
pub enum EnvError {
    /// The app id or display name is not usable as a folder name.
    InvalidApp(String),
    /// The user's home folder (or `%LOCALAPPDATA%`) cannot be found, so no folder can be chosen.
    NoHome(&'static str),
    /// A file or folder operation failed.
    Io {
        /// What was being done ("write", "create folder", ...).
        op: &'static str,
        /// The path involved.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// The file was written by a newer version of the app; it is left untouched and not overwritten.
    NewerVersion {
        /// The file.
        path: PathBuf,
        /// The version found in the file.
        found: u32,
        /// The newest version this build reads.
        supported: u32,
    },
    /// Saving is refused because loading the file left it untouched on purpose (newer version, unreadable, no migration).
    ReadOnly {
        /// The file.
        path: PathBuf,
        /// Why it is read-only, in words for the user.
        reason: String,
    },
    /// The value did not serialize to a JSON object, so it cannot carry a `version` field.
    NotAnObject,
    /// The value could not be serialized.
    Serialize(String),
}

impl fmt::Display for EnvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidApp(why) => write!(f, "invalid app name: {why}"),
            Self::NoHome(what) => write!(
                f,
                "cannot find the user's {what}; set it or use the app's home override"
            ),
            Self::Io { op, path, source } => {
                write!(f, "could not {op} {}: {source}", path.display())
            }
            Self::NewerVersion {
                path,
                found,
                supported,
            } => write!(
                f,
                "{} was written by a newer version of the app (format {found}, this build reads up to {supported}); \
                 update the app, or move the file away to start fresh",
                path.display()
            ),
            Self::ReadOnly { path, reason } => {
                write!(f, "{} is read-only: {reason}", path.display())
            }
            Self::NotAnObject => f.write_str("a state file must serialize to a JSON object"),
            Self::Serialize(why) => write!(f, "could not serialize the state: {why}"),
        }
    }
}

impl std::error::Error for EnvError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// For a caller that keeps `std::io::Result` (a file writer replaced by [`crate::write_atomic`]): an I/O failure keeps its kind
/// and message (which names the step and the path); anything else is `ErrorKind::Other`.
impl From<EnvError> for std::io::Error {
    fn from(e: EnvError) -> Self {
        match e {
            EnvError::Io { op, path, source } => Self::new(
                source.kind(),
                format!("could not {op} {}: {source}", path.display()),
            ),
            other => Self::other(other.to_string()),
        }
    }
}

/// `Result` with [`EnvError`].
pub type Result<T> = std::result::Result<T, EnvError>;

pub(crate) fn io(
    op: &'static str,
    path: &std::path::Path,
) -> impl FnOnce(std::io::Error) -> EnvError {
    let path = path.to_path_buf();
    move |source| EnvError::Io { op, path, source }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_io_failure_keeps_its_kind_and_names_the_path() {
        let e: std::io::Error = EnvError::Io {
            op: "write",
            path: "/d/x.json".into(),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        }
        .into();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(e.to_string().contains("/d/x.json"), "{e}");
    }

    #[test]
    fn another_failure_is_other() {
        let e: std::io::Error = EnvError::NotAnObject.into();
        assert_eq!(e.kind(), std::io::ErrorKind::Other);
    }
}
