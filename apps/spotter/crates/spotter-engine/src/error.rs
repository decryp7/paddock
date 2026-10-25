//! The engine's errors, in the shape the shell maps to the UI's `CommandError`.
use thiserror::Error;

/// Why an engine operation failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EngineError {
    /// The request itself is wrong (an unknown format, a bad range).
    #[error("{0}")]
    BadRequest(String),
    /// There is no such watch.
    #[error("{0}")]
    NotFound(String),
    /// The input is not valid; every problem is listed.
    #[error("{}", .0.join(" "))]
    Invalid(Vec<String>),
    /// Something the operation needs is not available (a renderer, the network); the message says what to do.
    #[error("{0}")]
    Unavailable(String),
    /// A bug or an I/O failure on Spotter's own files.
    #[error("{0}")]
    Internal(String),
    /// The user cancelled the operation.
    #[error("Cancelled.")]
    Cancelled,
}

impl EngineError {
    /// The machine-readable code the UI switches on (`badRequest`, `notFound`, `invalid`, `unavailable`, `internal`).
    pub fn code(&self) -> &'static str {
        match self {
            EngineError::BadRequest(_) => "badRequest",
            EngineError::NotFound(_) => "notFound",
            EngineError::Invalid(_) => "invalid",
            EngineError::Unavailable(_) => "unavailable",
            EngineError::Internal(_) => "internal",
            EngineError::Cancelled => "cancelled",
        }
    }

    /// Every problem, for `Invalid`; empty for the others.
    pub fn problems(&self) -> &[String] {
        match self {
            EngineError::Invalid(p) => p,
            _ => &[],
        }
    }
}

/// The engine's result type.
pub type Result<T> = std::result::Result<T, EngineError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_messages() {
        let all = [
            EngineError::BadRequest("a".into()),
            EngineError::NotFound("b".into()),
            EngineError::Invalid(vec!["c".into(), "d".into()]),
            EngineError::Unavailable("e".into()),
            EngineError::Internal("f".into()),
        ];
        let codes: Vec<&str> = all.iter().map(EngineError::code).collect();
        assert_eq!(codes, ["badRequest", "notFound", "invalid", "unavailable", "internal"]);
        assert_eq!(all[2].to_string(), "c d");
        assert_eq!(all[2].problems().len(), 2);
        assert!(all[0].problems().is_empty());
    }
}
