use std::fmt;
use std::path::{Path, PathBuf};

/// Why a report could not be written. Messages say what to change; none contains log content.
#[derive(Debug)]
pub enum DiagError {
    /// The destination the user chose cannot be used (not a `.zip` file name, no such folder, a folder, ...).
    BadDestination(String),
    /// A file name inside the report is not allowed (the app's section produced it).
    BadName(String),
    /// A file name was used twice.
    Duplicate(String),
    /// Writing failed (disk full, folder locked, ...).
    Io {
        /// What was being done.
        action: &'static str,
        /// The file or folder.
        path: PathBuf,
        /// The system's reason.
        source: std::io::Error,
    },
    /// The zip writer failed.
    Zip(String),
    /// The user cancelled; nothing was written.
    Cancelled,
}

impl fmt::Display for DiagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiagError::BadDestination(why) => f.write_str(why),
            DiagError::BadName(name) => {
                write!(f, "The report file name \"{name}\" is not allowed.")
            }
            DiagError::Duplicate(name) => {
                write!(f, "The report already has a file called \"{name}\".")
            }
            DiagError::Io {
                action,
                path,
                source,
            } => {
                write!(
                    f,
                    "Could not {action} {}: {source}. Choose another folder or free some space.",
                    path.display()
                )
            }
            DiagError::Zip(why) => write!(f, "The report could not be packed: {why}."),
            DiagError::Cancelled => f.write_str("Cancelled; nothing was saved."),
        }
    }
}

impl std::error::Error for DiagError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DiagError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Result of writing a report.
pub type Result<T> = std::result::Result<T, DiagError>;

pub(crate) fn io<'a>(
    action: &'static str,
    path: &'a Path,
) -> impl FnOnce(std::io::Error) -> DiagError + 'a {
    move |source| DiagError::Io {
        action,
        path: path.to_path_buf(),
        source,
    }
}
