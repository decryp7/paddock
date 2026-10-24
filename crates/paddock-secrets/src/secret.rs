use crate::SecretsError;
use std::fmt;
use zeroize::Zeroizing;

/// Longest value stored (a cookie header can be long; nothing legitimate is larger).
pub const MAX_SECRET_LEN: usize = 8192;

/// A credential value. `Debug` and `Display` never show it; it is wiped from memory on drop. Read it with
/// [`Secret::expose`] at the one place that must use it (a header, a login).
#[derive(Clone)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    /// Parses a value: 1 to [`MAX_SECRET_LEN`] bytes on one line (no control characters).
    pub fn new(value: impl Into<String>) -> Result<Secret, SecretsError> {
        let value = Zeroizing::new(value.into());
        if value.is_empty() || value.len() > MAX_SECRET_LEN || value.chars().any(char::is_control) {
            return Err(SecretsError::InvalidValue(format!(
                "a secret must be 1-{MAX_SECRET_LEN} characters on one line"
            )));
        }
        Ok(Secret(value))
    }

    /// The value. Never log it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Secret) -> bool {
        // Compare every byte whatever the first difference is.
        let (a, b) = (self.0.as_bytes(), other.0.as_bytes());
        a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }
}

impl Eq for Secret {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_display_never_show_the_value() {
        let s = Secret::new("hunter2-very-secret").unwrap();
        for text in [
            format!("{s:?}"),
            format!("{s:#?}"),
            format!("{s}"),
            format!("{:?}", Some(&s)),
            format!("{:>20}", s.to_string()),
        ] {
            assert!(!text.contains("hunter2"), "{text}");
        }
        assert_eq!(format!("{s:?}"), "Secret(***)");
        assert_eq!(s.to_string(), "***");
        assert_eq!(s.expose(), "hunter2-very-secret");
    }

    #[test]
    fn values_are_checked() {
        assert!(Secret::new("").is_err());
        assert!(Secret::new("two\nlines").is_err());
        assert!(Secret::new("x".repeat(MAX_SECRET_LEN + 1)).is_err());
        assert!(Secret::new("x".repeat(MAX_SECRET_LEN)).is_ok());
        let e = Secret::new("bad\nvalue").unwrap_err();
        assert!(!e.to_string().contains("bad"));
    }

    #[test]
    fn equality_compares_values() {
        let a = Secret::new("abc").unwrap();
        assert_eq!(a, a.clone());
        assert_ne!(a, Secret::new("abd").unwrap());
        assert_ne!(a, Secret::new("abcd").unwrap());
    }
}
