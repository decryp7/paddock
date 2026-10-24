use std::fmt;

/// Why a credential operation failed. Messages say what to fix and never contain a secret value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretsError {
    /// The scope or key is not acceptable.
    InvalidName(String),
    /// The value is empty, too long or has control characters.
    InvalidValue(String),
    /// The credential store cannot be reached (no Secret Service in a container, a locked keychain).
    Unavailable(String),
    /// The store refused this one operation.
    Failed(String),
    /// The fallback file is readable by others; it is refused until fixed.
    InsecureFile(String),
}

impl SecretsError {
    /// The text every app used to write by hand.
    pub fn unavailable(detail: impl fmt::Display) -> SecretsError {
        SecretsError::Unavailable(format!("the credential store is not available: {detail}"))
    }
}

impl fmt::Display for SecretsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretsError::InvalidName(m)
            | SecretsError::InvalidValue(m)
            | SecretsError::Unavailable(m)
            | SecretsError::Failed(m) => f.write_str(m),
            SecretsError::InsecureFile(p) => write!(
                f,
                "{p} can be read by other users; set its mode to 0600 and try again"
            ),
        }
    }
}

impl std::error::Error for SecretsError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_say_what_to_fix() {
        assert_eq!(
            SecretsError::InvalidName("bad name".into()).to_string(),
            "bad name"
        );
        assert_eq!(
            SecretsError::unavailable("locked").to_string(),
            "the credential store is not available: locked"
        );
        assert!(
            SecretsError::InsecureFile("/x/y".into())
                .to_string()
                .contains("0600")
        );
    }
}
