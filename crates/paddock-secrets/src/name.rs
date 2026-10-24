use crate::SecretsError;
use std::fmt;

/// Longest scope or key.
pub const MAX_NAME_LEN: usize = 128;

/// What a secret is called: the `scope` it belongs to (a watch id, a server URL, `channel:<id>:<n>`) and its `key`
/// (`token`, `admin key`). The credential store's account is `<scope>:<key>` (decision D6); the service is the app's
/// display name, held by the store, not here.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name {
    scope: String,
    key: String,
}

fn ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_NAME_LEN
        && s.chars().all(|c| c.is_ascii_graphic() || c == ' ')
        && !s.starts_with(' ')
        && !s.ends_with(' ')
}

impl Name {
    /// Parses a name: both parts 1-128 printable ASCII characters (inner spaces allowed), no `:` in the key (the scope
    /// may have them, the account splits at the last one).
    pub fn new(scope: &str, key: &str) -> Result<Name, SecretsError> {
        if !ok(scope) || !ok(key) || key.contains(':') {
            return Err(SecretsError::InvalidName(
                "the secret's name is not valid".into(),
            ));
        }
        Ok(Name {
            scope: scope.to_string(),
            key: key.to_string(),
        })
    }

    /// The scope.
    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// The key.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The credential store's account: `<scope>:<key>`.
    pub fn account(&self) -> String {
        format!("{}:{}", self.scope, self.key)
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.scope, self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_account_is_scope_colon_key() {
        let n = Name::new("channel:w1:2", "token").unwrap();
        assert_eq!(
            (n.scope(), n.key(), n.account().as_str()),
            ("channel:w1:2", "token", "channel:w1:2:token")
        );
        assert_eq!(n.to_string(), "channel:w1:2:token");
        assert_eq!(
            Name::new("https://git.example.com", "read token")
                .unwrap()
                .account(),
            "https://git.example.com:read token"
        );
    }

    #[test]
    fn bad_names_are_refused() {
        for (s, k) in [
            ("", "k"),
            ("s", ""),
            ("s", "a:b"),
            (" s", "k"),
            ("s", "k "),
            ("s\n", "k"),
            ("é", "k"),
        ] {
            assert!(Name::new(s, k).is_err(), "{s:?} {k:?}");
        }
        assert!(Name::new(&"a".repeat(MAX_NAME_LEN + 1), "k").is_err());
        assert!(Name::new(&"a".repeat(MAX_NAME_LEN), "k").is_ok());
    }
}
