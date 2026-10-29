/// Why reading the repository failed. Messages say what to do next.
#[derive(Debug)]
pub enum GraphError {
    /// A file could not be read.
    Read {
        /// The repository-relative path.
        path: String,
        /// Why.
        reason: String,
    },
    /// A file is not what its name says (JSON that does not parse).
    Parse {
        /// The repository-relative path.
        path: String,
        /// Why.
        reason: String,
    },
    /// `cargo metadata` failed for a workspace.
    Metadata {
        /// The workspace manifest.
        manifest: String,
        /// The last lines of cargo's complaint.
        reason: String,
    },
    /// A history query (git) failed.
    History(String),
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphError::Read { path, reason } => write!(
                f,
                "{path} could not be read: {reason}. Check that the file is tracked and readable."
            ),
            GraphError::Parse { path, reason } => {
                write!(f, "{path} is not valid: {reason}. Fix the file.")
            }
            GraphError::Metadata { manifest, reason } => {
                write!(
                    f,
                    "cargo metadata failed for {manifest}: {reason}. Run it there to see the whole message."
                )
            }
            GraphError::History(reason) => write!(
                f,
                "{reason}. Check that this is a git repository and the revisions exist."
            ),
        }
    }
}

impl std::error::Error for GraphError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_names_the_next_step() {
        let read = GraphError::Read {
            path: "a".into(),
            reason: "gone".into(),
        }
        .to_string();
        assert!(read.contains("a") && read.contains("gone") && read.contains("tracked"));
        assert!(
            GraphError::History("git failed".into())
                .to_string()
                .contains("git repository")
        );
    }
}
