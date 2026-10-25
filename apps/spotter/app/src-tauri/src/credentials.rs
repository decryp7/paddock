//! Secrets in the OS credential store: Windows Credential Manager, macOS Keychain (Secret Service on Linux). Access
//! tokens of sources, channel credentials (ntfy token, webhook secret, SMTP password) and site cookies of watched
//! pages are kept here and nowhere else: never in `spotter.toml`, the database, the logs or the UI.
//!
//! Entries are `Spotter` / `<scope>/<key>`, where the scope is a watch id (or `channel:<watch id>:<n>`) and the key
//! names the secret (`token`, `cookie`, ...).

use spotter_engine::secrets::SecretStore;

/// The credential store's service name.
pub const SERVICE: &str = "Spotter";
/// Longest value stored (a cookie header can be long; nothing legitimate is larger).
pub const MAX_SECRET_LEN: usize = 8192;
const MAX_NAME_LEN: usize = 128;

/// The OS credential store as the engine's [`SecretStore`].
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringSecrets;

/// The account name of a secret, or why the scope or key is not acceptable.
pub fn account(scope: &str, key: &str) -> Result<String, String> {
    let ok = |s: &str| !s.is_empty() && s.len() <= MAX_NAME_LEN && s.chars().all(|c| c.is_ascii_graphic());
    if !ok(scope) || !ok(key) || scope.contains('/') || key.contains('/') {
        return Err("the secret's name is not valid".into());
    }
    Ok(format!("{scope}/{key}"))
}

fn check_value(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > MAX_SECRET_LEN || value.chars().any(char::is_control) {
        return Err(format!("a secret must be 1-{MAX_SECRET_LEN} characters on one line"));
    }
    Ok(())
}

fn entry(scope: &str, key: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, &account(scope, key)?).map_err(|e| format!("the credential store is not available: {e}"))
}

impl SecretStore for KeyringSecrets {
    fn get(&self, scope: &str, key: &str) -> Option<String> {
        entry(scope, key).ok()?.get_password().ok()
    }

    fn set(&self, scope: &str, key: &str, value: &str) -> Result<(), String> {
        check_value(value)?;
        entry(scope, key)?
            .set_password(value)
            .map_err(|e| format!("it could not be stored: {e}"))
    }

    fn delete(&self, scope: &str, key: &str) -> Result<(), String> {
        match entry(scope, key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("it could not be deleted: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Only the checks that run before the store is touched: tests never use the real credential store.
    #[test]
    fn names_and_values_are_checked_before_the_store_is_touched() {
        assert_eq!(account("w1", "token").unwrap(), "w1/token");
        assert!(account("", "token").is_err());
        assert!(account("w/1", "token").is_err());
        assert!(account("w1", "to ken").is_err());
        assert!(account(&"a".repeat(129), "k").is_err());
        let s = KeyringSecrets;
        assert!(s.set("w1", "token", "").is_err());
        assert!(s.set("w1", "token", "two\nlines").is_err());
        assert!(s.set("w1", "token", &"x".repeat(MAX_SECRET_LEN + 1)).is_err());
        assert!(s.set("bad scope", "token", "v").is_err());
        assert!(s.delete("bad scope", "token").is_err());
        assert_eq!(s.get("bad scope", "token"), None);
    }
}
