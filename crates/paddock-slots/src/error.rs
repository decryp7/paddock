use std::path::PathBuf;

/// Why a slot operation failed. Messages say what to do next.
#[derive(Debug)]
pub enum SlotError {
    /// Not a slot name (`s1`, `s2`, ...).
    Invalid(String),
    /// A lock, owner or marker file could not be used.
    Io {
        /// The file.
        path: PathBuf,
        /// What failed.
        source: std::io::Error,
    },
    /// A release did not take effect in time (the holder is still alive).
    ReleaseTimeout(String),
}

impl std::fmt::Display for SlotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SlotError::Invalid(s) => write!(
                f,
                "{s:?} is not a slot name. Use s1, s2, ... (a number from 1)."
            ),
            SlotError::Io { path, source } => {
                write!(
                    f,
                    "{}: {source}. Check that the folder exists and is writable.",
                    path.display()
                )
            }
            SlotError::ReleaseTimeout(s) => {
                write!(
                    f,
                    "Slot {s} was asked to release but its holder is still running. Check the holder process."
                )
            }
        }
    }
}

impl std::error::Error for SlotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SlotError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn every_error_names_the_next_step() {
        let io = SlotError::Io {
            path: "x.lock".into(),
            source: std::io::Error::other("disk"),
        };
        assert!(io.to_string().contains("writable") && io.source().is_some());
        let invalid = SlotError::Invalid("q".into());
        assert!(invalid.to_string().contains("s1") && invalid.source().is_none());
        assert!(
            SlotError::ReleaseTimeout("s2".into())
                .to_string()
                .contains("holder")
        );
    }
}
