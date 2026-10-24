//! The name of a path grant, shared by the view model (a path field carries one) and the plugin manifest (a folder
//! permission lists them): one type, one rule, so a name is valid wherever it came from.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Most characters of a name that is a list item (grants, programs, operations, contribution ids).
pub const MAX_NAME: usize = 64;

/// Whether `s` is 1–[`MAX_NAME`] characters from `A–Z a–z 0–9 . - _`, starting with a letter or digit.
pub fn valid_ident(s: &str) -> bool {
    (1..=MAX_NAME).contains(&s.len())
        && s.starts_with(|c: char| c.is_ascii_alphanumeric())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c))
}

/// The name of a grant the user chose (a folder in the app's grant list), never a raw path: 1–64 characters from
/// `A–Z a–z 0–9 . - _`. The only constructor checks the value and `Deserialize` goes through it, so a value of the type is
/// valid whether it came from a manifest file, a JSON view or a postcard frame. On the wire it is a plain string (its
/// TypeScript is `string`; ts-rs cannot read the validating serde attributes, so the generated file declares it by hand).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GrantRef(String);

impl GrantRef {
    /// Checks and wraps `value`.
    pub fn new(value: impl Into<String>) -> Result<GrantRef, String> {
        let value = value.into();
        if valid_ident(&value) {
            Ok(GrantRef(value))
        } else {
            let shown: String = value.chars().take(40).collect();
            Err(format!(
                "grant name \"{shown}\" is not valid: use 1-64 characters from A-Z, a-z, 0-9, '.', '-', '_'; it names a grant \
                 the user chooses, it is not a path"
            ))
        }
    }

    /// The value.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for GrantRef {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        GrantRef::new(value)
    }
}

impl From<GrantRef> for String {
    fn from(n: GrantRef) -> String {
        n.0
    }
}

impl FromStr for GrantRef {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        GrantRef::new(s)
    }
}

impl std::fmt::Display for GrantRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for GrantRef {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_names_are_validated_everywhere() {
        for ok in ["symbols", "a.b-c_d", "X1", &"x".repeat(64)] {
            assert_eq!(GrantRef::new(ok).unwrap().as_str(), ok);
            assert!(valid_ident(ok));
        }
        for bad in ["", "a/b", "..", "-x", "C:\\x", "a b", &"x".repeat(65)] {
            assert!(GrantRef::new(bad).is_err(), "{bad}");
            assert!(bad.parse::<GrantRef>().is_err(), "{bad}");
        }
        let e = GrantRef::new("/etc/passwd").unwrap_err();
        assert!(e.contains("not a path"), "{e}");
        let long = GrantRef::new("é".repeat(50)).unwrap_err();
        assert!(
            long.contains(&"é".repeat(40)) && !long.contains(&"é".repeat(41)),
            "{long}"
        );
    }

    #[test]
    fn it_is_a_string_on_the_wire_and_revalidates_when_read() {
        let g = GrantRef::new("symbols").unwrap();
        assert_eq!(g.to_string(), "symbols");
        assert_eq!(g.as_ref() as &str, "symbols");
        assert_eq!(String::from(g.clone()), "symbols");
        assert_eq!(serde_json::to_string(&g).unwrap(), "\"symbols\"");
        assert_eq!(serde_json::from_str::<GrantRef>("\"symbols\"").unwrap(), g);
        assert!(serde_json::from_str::<GrantRef>("\"../x\"").is_err());
        let wire = postcard::to_stdvec(&"symbols").unwrap();
        assert_eq!(postcard::to_stdvec(&g).unwrap(), wire);
        assert_eq!(postcard::from_bytes::<GrantRef>(&wire).unwrap(), g);
        // A frame cannot smuggle an invalid name in.
        assert!(postcard::from_bytes::<GrantRef>(&postcard::to_stdvec(&"../x").unwrap()).is_err());
    }
}
