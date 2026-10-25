//! Validated names.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Longest runner name Pitcrew accepts.
pub const MAX_NAME_LEN: usize = 48;

/// A runner's name in Pitcrew: 1-48 characters of `a-z`, `0-9` and `-`, starting with a letter or digit. It names the
/// runner's folder, log files and service resources, so it must be safe in paths on every platform.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RunnerName(String);

/// Why a runner name was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    /// The name is empty.
    #[error("the runner name is empty")]
    Empty,
    /// The name is too long.
    #[error("the runner name is longer than {MAX_NAME_LEN} characters")]
    TooLong,
    /// The name contains a character outside `a-z`, `0-9` and `-`, or starts with `-`.
    #[error("the runner name {0:?} may only contain a-z, 0-9 and '-' and must not start with '-'")]
    BadCharacter(String),
}

impl RunnerName {
    /// Parses a name (see the type's documentation).
    pub fn parse(s: &str) -> Result<Self, NameError> {
        if s.is_empty() {
            return Err(NameError::Empty);
        }
        if s.len() > MAX_NAME_LEN {
            return Err(NameError::TooLong);
        }
        let ok = s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !s.starts_with('-');
        if !ok {
            return Err(NameError::BadCharacter(s.to_string()));
        }
        Ok(RunnerName(s.to_string()))
    }

    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RunnerName {
    type Error = NameError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        RunnerName::parse(&s)
    }
}

impl From<RunnerName> for String {
    fn from(n: RunnerName) -> Self {
        n.0
    }
}

impl fmt::Display for RunnerName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_path_safe_names_only() {
        assert_eq!(RunnerName::parse("build-pc-1").unwrap().as_str(), "build-pc-1");
        assert_eq!(RunnerName::parse(""), Err(NameError::Empty));
        assert_eq!(RunnerName::parse(&"a".repeat(49)), Err(NameError::TooLong));
        for bad in ["-x", "A", "a b", "../x", r"a\b", "ü"] {
            assert!(matches!(RunnerName::parse(bad), Err(NameError::BadCharacter(_))), "{bad}");
        }
    }

    #[test]
    fn deserialises_through_validation() {
        let ok: RunnerName = serde_json::from_str("\"main\"").unwrap();
        assert_eq!(ok.to_string(), "main");
        assert!(serde_json::from_str::<RunnerName>("\"Main\"").is_err());
    }
}
