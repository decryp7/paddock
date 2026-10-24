//! Per-client keys for the loopback transport (ADR 0007 rule 1, issued like Pitwall pairing): shown once when issued, only a
//! SHA-256 kept, revocable. Persisting the entries (the OS credential store, a settings file) is the app's job: [`KeyStore::export`]
//! and [`KeyStore::import`] carry them.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{Mutex, MutexGuard};

/// The prefix of every key, so a pasted key is recognisable (and secret scanners find it).
pub const KEY_PREFIX: &str = "pmk_";

/// What is kept about a key. The key itself is never kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyEntry {
    /// The id shown in the app's list, used to revoke.
    pub id: u64,
    /// The client it was issued to ("Pitwall", "my script").
    pub client: String,
    /// SHA-256 of the key, lowercase hex.
    pub sha256: String,
    /// When it was issued (epoch milliseconds, from the app's clock).
    pub created_ms: i64,
    /// Revoked keys are kept in the list but no longer work.
    pub revoked: bool,
}

/// Anything that can say which client a key belongs to.
pub trait KeyVerifier: Send + Sync {
    /// The client name when `key` is a valid, unrevoked key.
    fn verify(&self, key: &str) -> Option<String>;
}

/// The keys of one app.
#[derive(Debug, Default)]
pub struct KeyStore {
    entries: Mutex<Vec<KeyEntry>>,
}

fn hash(key: &str) -> String {
    Sha256::digest(key.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Compares without stopping at the first difference.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

impl KeyStore {
    /// An empty store.
    pub fn new() -> KeyStore {
        KeyStore::default()
    }

    fn lock(&self) -> MutexGuard<'_, Vec<KeyEntry>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Issues a key from 32 bytes of entropy; returns the entry id and the key, which is shown to the user once.
    pub fn issue_with(&self, client: &str, entropy: [u8; 32], now_ms: i64) -> (u64, String) {
        let key = format!(
            "{KEY_PREFIX}{}",
            entropy
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let mut entries = self.lock();
        let id = entries.iter().map(|e| e.id).max().map_or(1, |m| m + 1);
        entries.push(KeyEntry {
            id,
            client: client.chars().take(64).collect(),
            sha256: hash(&key),
            created_ms: now_ms,
            revoked: false,
        });
        (id, key)
    }

    /// [`issue_with`](Self::issue_with) with the OS random source.
    pub fn issue(&self, client: &str, now_ms: i64) -> Result<(u64, String), String> {
        let mut entropy = [0u8; 32];
        getrandom::fill(&mut entropy).map_err(|e| format!("no random source: {e}"))?;
        Ok(self.issue_with(client, entropy, now_ms))
    }

    /// Revokes a key; returns whether it existed.
    pub fn revoke(&self, id: u64) -> bool {
        self.lock()
            .iter_mut()
            .find(|e| e.id == id)
            .map(|e| e.revoked = true)
            .is_some()
    }

    /// The entries (hashes included; they are not secrets), for the app's list and for saving.
    pub fn export(&self) -> Vec<KeyEntry> {
        self.lock().clone()
    }

    /// Replaces the entries with saved ones.
    pub fn import(&self, entries: Vec<KeyEntry>) {
        *self.lock() = entries;
    }
}

impl KeyVerifier for KeyStore {
    fn verify(&self, key: &str) -> Option<String> {
        let h = hash(key);
        self.lock()
            .iter()
            .find(|e| !e.revoked && same(&e.sha256, &h))
            .map(|e| e.client.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_works_until_it_is_revoked_and_only_its_hash_is_kept() {
        let s = KeyStore::new();
        let (id, key) = s.issue_with("Pitwall", [7; 32], 1000);
        assert!(key.starts_with(KEY_PREFIX) && key.len() == 4 + 64);
        assert_eq!(s.verify(&key).as_deref(), Some("Pitwall"));
        assert_eq!(s.verify("pmk_wrong"), None);
        let saved = s.export();
        assert!(!serde_json::to_string(&saved).unwrap().contains(&key));
        assert!(s.revoke(id));
        assert_eq!(s.verify(&key), None);
        assert!(!s.revoke(99));
        let again = KeyStore::new();
        again.import(saved);
        assert_eq!(
            again.verify(&key).as_deref(),
            Some("Pitwall"),
            "a saved store still verifies"
        );
        assert_eq!(
            again.issue_with("b", [8; 32], 2).0,
            2,
            "ids keep counting after an import"
        );
    }

    #[test]
    fn random_keys_differ_and_long_client_names_are_cut() {
        let s = KeyStore::new();
        let (_, a) = s.issue(&"x".repeat(100), 1).unwrap();
        let (_, b) = s.issue("y", 1).unwrap();
        assert_ne!(a, b);
        assert_eq!(s.export()[0].client.len(), 64);
        assert!(!same("ab", "abc") && same("ab", "ab") && !same("ab", "ac"));
    }
}
