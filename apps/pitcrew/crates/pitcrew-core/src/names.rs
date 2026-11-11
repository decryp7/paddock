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
    /// The name is one Windows reserves for a device (`con`, `nul`, `com1`, ...): as a folder or file name it would not name a folder.
    #[error("the runner name {0:?} is reserved by Windows for a device (con, prn, aux, nul, com1-9, lpt1-9); choose another")]
    Reserved(String),
}

/// Whether `name` is a device name Windows reserves (`CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`), whatever the case.
fn is_windows_device(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(n.as_str(), "con" | "prn" | "aux" | "nul")
        || ((n.starts_with("com") || n.starts_with("lpt")) && n.len() == 4 && matches!(n.as_bytes()[3], b'1'..=b'9'))
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
        if is_windows_device(s) {
            return Err(NameError::Reserved(s.to_string()));
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

    /// `data\runners\con` is not a folder on Windows (it opens the console); a name that is a device is refused everywhere, so a
    /// configuration written on one OS is valid on every other.
    #[test]
    fn windows_device_names_are_refused() {
        for bad in ["con", "prn", "aux", "nul", "com1", "com9", "lpt1", "lpt9"] {
            assert_eq!(RunnerName::parse(bad), Err(NameError::Reserved(bad.to_string())), "{bad}");
        }
        for ok in ["com0", "com10", "lpt", "console", "nul2", "my-con", "com", "aux1"] {
            assert!(RunnerName::parse(ok).is_ok(), "{ok}");
        }
        assert!(serde_json::from_str::<RunnerName>("\"nul\"").is_err());
    }

    #[test]
    fn deserialises_through_validation() {
        let ok: RunnerName = serde_json::from_str("\"main\"").unwrap();
        assert_eq!(ok.to_string(), "main");
        assert!(serde_json::from_str::<RunnerName>("\"Main\"").is_err());
    }
}
