//! Why a scan or a clear was refused as a whole (a category that cannot be cleared is a result, not an error).

use paddock_commands::CommandError;
use std::fmt;

/// A refused request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    /// Nothing was measured yet, or the preview was used by an earlier clear: scan first, so the user sees what goes.
    NoPreview,
    /// More categories than [`crate::MAX_CATEGORIES`] in one request.
    TooMany,
    /// A category the app does not declare (or an id that is not one).
    UnknownCategory(String),
    /// The same category twice in one request.
    Duplicate(String),
    /// The user cancelled; nothing was changed by a scan, and a clear reports what it did instead.
    Cancelled,
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::NoPreview => {
                f.write_str("Measure the storage first, so you can see what would be removed.")
            }
            StorageError::TooMany => f.write_str("Too many categories in one request."),
            StorageError::UnknownCategory(id) => write!(
                f,
                "\"{}\" is not something this app keeps on disk.",
                id.chars().take(40).collect::<String>()
            ),
            StorageError::Duplicate(id) => write!(f, "\"{id}\" is listed twice."),
            StorageError::Cancelled => f.write_str("Stopped."),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<StorageError> for CommandError {
    fn from(e: StorageError) -> CommandError {
        match e {
            StorageError::NoPreview => CommandError::invalid(e.to_string()).kind("noPreview"),
            StorageError::TooMany | StorageError::Duplicate(_) => {
                CommandError::invalid(e.to_string())
            }
            StorageError::UnknownCategory(_) => CommandError::not_found(e.to_string()),
            StorageError::Cancelled => CommandError::cancelled(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_commands::ErrorCode;

    #[test]
    fn every_refusal_has_words_and_a_code() {
        let cases = [
            (StorageError::NoPreview, ErrorCode::Invalid),
            (StorageError::TooMany, ErrorCode::Invalid),
            (
                StorageError::UnknownCategory("x".repeat(100)),
                ErrorCode::NotFound,
            ),
            (StorageError::Duplicate("logs".into()), ErrorCode::Invalid),
            (StorageError::Cancelled, ErrorCode::Cancelled),
        ];
        for (e, code) in cases {
            assert!(!e.to_string().is_empty());
            assert_eq!(CommandError::from(e).code, code);
        }
        assert!(
            StorageError::UnknownCategory("x".repeat(100))
                .to_string()
                .len()
                < 90,
            "a long id is cut"
        );
        assert_eq!(
            CommandError::from(StorageError::NoPreview).kind.as_deref(),
            Some("noPreview")
        );
    }
}
