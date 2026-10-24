//! The app's Pitwall key (`pw_...`, Pitwall ADR 0007). It lives only in the credential store through `paddock-secrets`
//! (never in a file, a log or the UI), under the one naming scheme: the service is the app's display name and the account is
//! `Pitwall:API key`. The key is returned by Pitwall exactly once, stored at once, and deleted when Pitwall says it was
//! revoked.

use paddock_secrets::{Name, Renamed, Secret, Secrets, SecretsError, Vault, rename_entry};
use std::sync::Arc;

/// Longest key accepted (Pitwall's limit for credentials).
pub const MAX_KEY_LEN: usize = 256;
/// The scope of the entry.
pub const SCOPE: &str = "Pitwall";
/// The key of the entry.
pub const KEY: &str = "API key";
/// The account Debrief 1.1 used before the shared naming scheme (`Pitwall API key`); [`adopt_legacy`] moves it.
pub const LEGACY_ACCOUNT: &str = "Pitwall API key";

/// A usable key: 1 to 256 printable ASCII characters after trimming.
pub fn parse_key(raw: &str) -> Option<Secret> {
    let k = raw.trim();
    if k.is_empty() || k.len() > MAX_KEY_LEN || !k.bytes().all(|b| b.is_ascii_graphic()) {
        return None;
    }
    Secret::new(k).ok()
}

/// The app's key in a [`Secrets`] store.
#[derive(Clone)]
pub struct KeyVault {
    secrets: Arc<dyn Secrets>,
    name: Name,
}

impl std::fmt::Debug for KeyVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyVault")
            .field("name", &self.name.to_string())
            .finish()
    }
}

impl KeyVault {
    /// The key entry in `secrets` (the store of the app: its service is the app's display name).
    pub fn new(secrets: Arc<dyn Secrets>) -> KeyVault {
        // The scope and key are constants that satisfy the naming rules (a test covers it).
        let name = Name::new(SCOPE, KEY).expect("the constant name is valid");
        KeyVault { secrets, name }
    }

    /// The stored key. A store that cannot be reached reads as no key: the app then asks to connect again, and the error is
    /// reported by [`KeyVault::set`] if storing fails too.
    pub fn get(&self) -> Option<Secret> {
        self.secrets
            .get(&self.name)
            .ok()
            .flatten()
            .filter(|s| parse_key(s.expose()).is_some())
    }

    /// Stores the key Pitwall delivered. An unusable key is refused without touching the store.
    pub fn set(&self, key: &str) -> Result<(), SecretsError> {
        let secret = parse_key(key).ok_or_else(|| {
            SecretsError::InvalidValue("the key Pitwall sent is not usable".into())
        })?;
        self.secrets.set(&self.name, &secret)
    }

    /// Deletes the key (Forget Pitwall, or after a revoke). Deleting a missing key succeeds.
    pub fn delete(&self) -> Result<(), SecretsError> {
        self.secrets.delete(&self.name)
    }
}

/// Moves Debrief 1.1's entry (service = the app, account `Pitwall API key`) to the shared scheme. Safe to run at every start;
/// the old entry stays until the new one is written.
pub fn adopt_legacy(vault: &dyn Vault, service: &str) -> Result<Renamed, SecretsError> {
    rename_entry(
        vault,
        (service, LEGACY_ACCOUNT),
        (service, &format!("{SCOPE}:{KEY}")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_secrets::{MemorySecrets, MemoryVault};

    #[test]
    fn keys_are_checked_and_kept_under_the_shared_name() {
        assert_eq!(
            parse_key(" pw_abc \n")
                .map(|s| s.expose().to_string())
                .as_deref(),
            Some("pw_abc")
        );
        assert!(
            parse_key("").is_none()
                && parse_key("has space").is_none()
                && parse_key(&"k".repeat(MAX_KEY_LEN + 1)).is_none()
        );
        assert!(parse_key("é").is_none());
        let store = Arc::new(MemorySecrets::new("Debrief"));
        let vault = KeyVault::new(store.clone());
        assert!(vault.get().is_none());
        assert!(vault.set("bad key").is_err());
        assert!(vault.get().is_none(), "a refused key stores nothing");
        vault.set("pw_1").unwrap();
        assert_eq!(vault.get().unwrap().expose(), "pw_1");
        assert_eq!(
            store
                .get(&Name::new("Pitwall", "API key").unwrap())
                .unwrap()
                .unwrap()
                .expose(),
            "pw_1"
        );
        vault.delete().unwrap();
        vault.delete().unwrap();
        assert!(vault.get().is_none());
        assert!(!format!("{vault:?}").contains("pw_"));
    }

    #[test]
    fn an_unreachable_store_reads_as_no_key_and_refuses_to_store() {
        let store = Arc::new(MemorySecrets::new("Debrief"));
        let vault = KeyVault::new(store.clone());
        vault.set("pw_1").unwrap();
        store.set_unavailable(true);
        assert!(vault.get().is_none());
        assert!(matches!(
            vault.set("pw_2"),
            Err(SecretsError::Unavailable(_))
        ));
    }

    #[test]
    fn debriefs_old_entry_moves_once() {
        let store = MemorySecrets::new("Debrief");
        let v: &MemoryVault = store.vault();
        v.set("Debrief", LEGACY_ACCOUNT, &Secret::new("pw_old").unwrap())
            .unwrap();
        assert_eq!(adopt_legacy(v, "Debrief").unwrap(), Renamed::Moved);
        assert_eq!(adopt_legacy(v, "Debrief").unwrap(), Renamed::NothingToMove);
        let vault = KeyVault::new(Arc::new(store));
        assert_eq!(vault.get().unwrap().expose(), "pw_old");
    }
}
