use std::path::PathBuf;

/// Why a walk or a hash failed. Messages say what to do next.
#[derive(Debug)]
pub enum RepoFilesError {
    /// The root is not a folder.
    NotAFolder(PathBuf),
    /// More files than [`crate::WalkLimits::max_files`].
    TooManyFiles(usize),
    /// A file is bigger than the hash limit.
    TooLarge {
        /// The file.
        path: PathBuf,
        /// The limit in bytes.
        limit: u64,
    },
    /// Reading failed.
    Io {
        /// The file or folder.
        path: PathBuf,
        /// What failed.
        source: std::io::Error,
    },
}

impl std::fmt::Display for RepoFilesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RepoFilesError::NotAFolder(p) => write!(
                f,
                "{} is not a folder. Pass the repository root.",
                p.display()
            ),
            RepoFilesError::TooManyFiles(n) => {
                write!(
                    f,
                    "More than {n} files. Point at a smaller folder or add the big one to .gitignore."
                )
            }
            RepoFilesError::TooLarge { path, limit } => {
                write!(
                    f,
                    "{} is larger than {limit} bytes and was not hashed. Ignore it or raise the limit.",
                    path.display()
                )
            }
            RepoFilesError::Io { path, source } => write!(
                f,
                "{}: {source}. Check that it exists and is readable.",
                path.display()
            ),
        }
    }
}

impl std::error::Error for RepoFilesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RepoFilesError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
