use std::fmt;
use std::path::PathBuf;

/// What can go wrong; each message says what to fix.
#[derive(Debug)]
pub enum BenchError {
    /// A file could not be read or written.
    Io {
        /// What was being done.
        op: &'static str,
        /// The file.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// A file is larger than the limit.
    TooLarge {
        /// The file.
        path: PathBuf,
        /// The limit in bytes.
        limit: u64,
    },
    /// A file does not parse.
    Parse {
        /// The file (or "text").
        path: PathBuf,
        /// What is wrong.
        message: String,
    },
    /// The results file is from a newer harness.
    NewerVersion {
        /// The file.
        path: PathBuf,
        /// The version found.
        found: u32,
    },
    /// A value is not usable (a negative or non-finite number, a duplicate or empty name, too many entries).
    Invalid(String),
    /// Writing the results file failed.
    Write(String),
}

/// Result of this crate.
pub type Result<T> = std::result::Result<T, BenchError>;

impl fmt::Display for BenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BenchError::Io { op, path, source } => write!(
                f,
                "cannot {op} {}: {source}. Check the path and its permissions.",
                path.display()
            ),
            BenchError::TooLarge { path, limit } => write!(
                f,
                "{} is larger than {limit} bytes; it is not a benchmark file.",
                path.display()
            ),
            BenchError::Parse { path, message } => {
                write!(f, "{} is not valid: {message}", path.display())
            }
            BenchError::NewerVersion { path, found } => {
                write!(
                    f,
                    "{} is version {found}, newer than this paddock-bench reads. Update paddock-bench.",
                    path.display()
                )
            }
            BenchError::Invalid(m) => f.write_str(m),
            BenchError::Write(m) => write!(f, "cannot save the results: {m}"),
        }
    }
}

impl std::error::Error for BenchError {}
