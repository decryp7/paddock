//! Secrets in the OS credential store: Windows Credential Manager, macOS Keychain (Secret Service on Linux). Access
//! tokens of sources, channel credentials (ntfy token, webhook secret, SMTP password) and site cookies of watched
//! pages are kept here and nowhere else: never in `spotter.toml`, the database, the logs or the UI.
//!
//! Entries are `Spotter` / `<scope>/<key>`, where the scope is a watch id (or `channel:<watch id>:<n>`) and the key
//! names the secret (`token`, `cookie`, ...).
//!
//! The store is `paddock-secrets`' [`Vault`] port (ADR 0077 WP-4): the app uses [`OsVault`], tests a `MemoryVault`, so a
//! test never reaches the real credential store. The entries keep the names Spotter wrote before.

use paddock_secrets::{OsVault, Secret, Vault};
use spotter_engine::secrets::SecretStore;

/// The credential store's service name.
pub const SERVICE: &str = "Spotter";
/// Longest value stored (a cookie header can be long; nothing legitimate is larger).
pub const MAX_SECRET_LEN: usize = 8192;
const MAX_NAME_LEN: usize = 128;

/// A credential store as the engine's [`SecretStore`]: the OS one by default ([`OsVault`]), a `MemoryVault` in tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringSecrets<V = OsVault>(pub V);

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

impl<V: Vault> SecretStore for KeyringSecrets<V> {
    fn get(&self, scope: &str, key: &str) -> Option<String> {
        let account = account(scope, key).ok()?;
        self.0.get(SERVICE, &account).ok().flatten().map(|s| s.expose().to_string())
    }

    fn set(&self, scope: &str, key: &str, value: &str) -> Result<(), String> {
        check_value(value)?;
        let value = Secret::new(value).map_err(|e| e.to_string())?;
        self.0
            .set(SERVICE, &account(scope, key)?, &value)
            .map_err(|e| format!("it could not be stored: {e}"))
    }

    fn delete(&self, scope: &str, key: &str) -> Result<(), String> {
        self.0
            .delete(SERVICE, &account(scope, key)?)
            .map_err(|e| format!("it could not be deleted: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_secrets::MemoryVault;

    // Tests never use the real credential store: a MemoryVault stands in for it.
    #[test]
    fn names_and_values_are_checked_before_the_store_is_touched() {
        assert_eq!(account("w1", "token").unwrap(), "w1/token");
        assert!(account("", "token").is_err());
        assert!(account("w/1", "token").is_err());
        assert!(account("w1", "to ken").is_err());
        assert!(account(&"a".repeat(129), "k").is_err());
        let v = MemoryVault::default();
        v.set_unavailable(true);
        let s = KeyringSecrets(v);
        assert!(s.set("w1", "token", "").is_err());
        assert!(s.set("w1", "token", "two\nlines").is_err());
        assert!(s.set("w1", "token", &"x".repeat(MAX_SECRET_LEN + 1)).is_err());
        assert!(s.set("bad scope", "token", "v").is_err());
        assert!(s.delete("bad scope", "token").is_err());
        assert_eq!(s.get("bad scope", "token"), None);
    }

    /// ADR 0077 WP-4: the entries stay where Spotter wrote them, `Spotter` / `<scope>/<key>`, so a stored secret is read after
    /// the update; a store that cannot be reached gives no secret and a message, never the value.
    #[test]
    fn secrets_keep_the_entries_spotter_wrote() {
        let s = KeyringSecrets(MemoryVault::default());
        s.set("channel:w1:2", "token", "s3cr").unwrap();
        assert_eq!(
            s.0.get("Spotter", "channel:w1:2/token").unwrap().map(|v| v.expose().to_string()),
            Some("s3cr".to_string())
        );
        assert_eq!(s.get("channel:w1:2", "token").as_deref(), Some("s3cr"));
        s.delete("channel:w1:2", "token").unwrap();
        s.delete("channel:w1:2", "token").unwrap();
        assert_eq!(s.get("channel:w1:2", "token"), None);
        s.0.set_unavailable(true);
        let e = s.set("w1", "cookie", "s3cr").unwrap_err();
        assert!(e.starts_with("it could not be stored: ") && !e.contains("s3cr"), "{e}");
        assert_eq!(s.get("w1", "cookie"), None);
        assert!(s.delete("w1", "cookie").is_err());
    }
}
