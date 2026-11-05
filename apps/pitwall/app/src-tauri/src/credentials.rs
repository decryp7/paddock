//! The app's copy of the server's admin key. The server owns the key (`<data dir>/admin.key`, owner-only); the app
//! keeps a copy in the OS credential store (Windows Credential Manager, macOS Keychain, Secret Service on Linux) so a
//! later start does not depend on reading the file, and falls back to the file when the store is unavailable (no
//! Secret Service in a container, a locked keychain). The key is never logged and never part of an error message
//! (adapted from Pitcrew's credentials module).

use std::path::PathBuf;
use std::sync::Mutex;

/// The credential service name.
pub const SERVICE: &str = "Pitwall";
/// The credential's account name.
pub const ACCOUNT: &str = "admin key";
/// Longest key accepted.
pub const MAX_KEY_LEN: usize = 256;

/// Whether `k` looks like a key at all: 1 to 256 printable ASCII characters. Everything else (a damaged file, a
/// stray newline in the middle) is refused before it can reach a header.
pub fn parse_key(raw: &str) -> Option<String> {
    let k = raw.trim();
    (!k.is_empty() && k.len() <= MAX_KEY_LEN && k.bytes().all(|b| b.is_ascii_graphic())).then(|| k.to_string())
}

/// The admin key as the client needs it.
pub trait AdminKeys: Send + Sync {
    /// The key to use now (cached).
    fn current(&self) -> Option<String>;
    /// Reads the key again from its source of truth (the server's file) and updates the cache and the store. Called
    /// once after an `Unauthorized` answer.
    fn refresh(&self) -> Option<String>;
}

/// A place the app's copy lives.
pub trait KeyStore: Send + Sync {
    /// The stored key.
    fn get(&self) -> Option<String>;
    /// Stores the key.
    fn set(&self, key: &str) -> Result<(), String>;
}

/// The OS credential store.
pub struct OsKeyStore;

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| format!("the credential store is not available: {e}"))
}

impl KeyStore for OsKeyStore {
    fn get(&self) -> Option<String> {
        entry().ok()?.get_password().ok().and_then(|k| parse_key(&k))
    }

    fn set(&self, key: &str) -> Result<(), String> {
        entry()?.set_password(key).map_err(|e| format!("the key could not be stored: {e}"))
    }
}

/// The key from the OS store with the server's `admin.key` as fallback and as the source of truth on refresh.
pub struct StoredAdminKey {
    store: Box<dyn KeyStore>,
    file: PathBuf,
    cached: Mutex<Option<String>>,
}

impl StoredAdminKey {
    /// Keys in `store`, falling back to `file`.
    pub fn new(store: Box<dyn KeyStore>, file: PathBuf) -> StoredAdminKey {
        StoredAdminKey {
            store,
            file,
            cached: Mutex::new(None),
        }
    }

    fn read_file(&self) -> Option<String> {
        std::fs::metadata(&self.file)
            .ok()
            .filter(|m| m.len() as usize <= MAX_KEY_LEN * 2)
            .and_then(|_| std::fs::read_to_string(&self.file).ok())
            .and_then(|t| parse_key(&t))
    }

    fn remember(&self, key: Option<String>) -> Option<String> {
        *self.cached.lock().unwrap_or_else(|p| p.into_inner()) = key.clone();
        key
    }
}

impl AdminKeys for StoredAdminKey {
    fn current(&self) -> Option<String> {
        if let Some(k) = self.cached.lock().unwrap_or_else(|p| p.into_inner()).clone() {
            return Some(k);
        }
        if let Some(k) = self.store.get() {
            return self.remember(Some(k));
        }
        let from_file = self.read_file();
        if let Some(k) = &from_file {
            // Best effort: a store that refuses leaves the file as the source.
            let _ = self.store.set(k);
        }
        self.remember(from_file)
    }

    fn refresh(&self) -> Option<String> {
        let from_file = self.read_file();
        if let Some(k) = &from_file {
            if self.store.get().as_deref() != Some(k.as_str()) {
                let _ = self.store.set(k);
            }
        }
        // No file (the server is gone or never ran): the stored copy is all there is.
        let key = from_file.or_else(|| self.store.get());
        self.remember(key)
    }
}

/// A fixed key, for tests.
pub struct StaticKeys(String);

impl StaticKeys {
    /// Always `key`.
    pub fn new(key: &str) -> StaticKeys {
        StaticKeys(key.to_string())
    }
}

impl AdminKeys for StaticKeys {
    fn current(&self) -> Option<String> {
        Some(self.0.clone())
    }
    fn refresh(&self) -> Option<String> {
        Some(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_key_never_panics_on_hostile_text() {
        paddock_testkit::assert_never_panics(&[b"abc123_key", b" key \n"], |bytes| parse_key(&String::from_utf8_lossy(bytes)));
    }
    use std::sync::Arc;

    #[derive(Default, Clone)]
    struct MemStore(Arc<Mutex<Option<String>>>, bool);

    impl KeyStore for MemStore {
        fn get(&self) -> Option<String> {
            self.0.lock().unwrap().clone()
        }
        fn set(&self, key: &str) -> Result<(), String> {
            if self.1 {
                return Err("locked".into());
            }
            *self.0.lock().unwrap() = Some(key.to_string());
            Ok(())
        }
    }

    #[test]
    fn keys_are_checked_before_use() {
        assert_eq!(parse_key("  pw_abc123\n").as_deref(), Some("pw_abc123"));
        assert_eq!(parse_key(""), None);
        assert_eq!(parse_key("has space"), None);
        assert_eq!(parse_key("two\nlines"), None);
        assert_eq!(parse_key(&"a".repeat(MAX_KEY_LEN + 1)), None);
        assert_eq!(parse_key("pw_\u{e9}"), None);
    }

    #[test]
    fn the_file_is_the_fallback_and_is_copied_into_the_store() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("admin.key");
        std::fs::write(&file, "from-file\n").unwrap();
        let store = MemStore::default();
        let keys = StoredAdminKey::new(Box::new(store.clone()), file);
        assert_eq!(keys.current().as_deref(), Some("from-file"));
        assert_eq!(store.get().as_deref(), Some("from-file"), "the store now has its copy");
    }

    #[test]
    fn the_store_wins_over_the_file_until_a_refresh() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("admin.key");
        std::fs::write(&file, "new-key").unwrap();
        let store = MemStore::default();
        store.set("old-key").unwrap();
        let keys = StoredAdminKey::new(Box::new(store.clone()), file);
        assert_eq!(keys.current().as_deref(), Some("old-key"));
        // The server made a new key: a refresh trusts the file and updates the store and the cache.
        assert_eq!(keys.refresh().as_deref(), Some("new-key"));
        assert_eq!(store.get().as_deref(), Some("new-key"));
        assert_eq!(keys.current().as_deref(), Some("new-key"));
    }

    #[test]
    fn a_locked_store_or_a_missing_file_degrades_quietly() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("admin.key");
        std::fs::write(&file, "k1").unwrap();
        let locked = MemStore(Arc::default(), true);
        let keys = StoredAdminKey::new(Box::new(locked), file.clone());
        assert_eq!(keys.current().as_deref(), Some("k1"), "works without a store");
        std::fs::remove_file(&file).unwrap();
        assert_eq!(keys.refresh(), None, "no file and no store copy: no key");
        let store = MemStore::default();
        store.set("kept").unwrap();
        let only_store = StoredAdminKey::new(Box::new(store), d.path().join("none"));
        assert_eq!(only_store.refresh().as_deref(), Some("kept"));
    }

    #[test]
    fn a_damaged_key_file_is_ignored() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("admin.key");
        std::fs::write(&file, "bad key with spaces").unwrap();
        let keys = StoredAdminKey::new(Box::new(MemStore::default()), file.clone());
        assert_eq!(keys.current(), None);
        std::fs::write(&file, "x".repeat(MAX_KEY_LEN * 3)).unwrap();
        assert_eq!(keys.refresh(), None);
        assert_eq!(StaticKeys::new("s").current().as_deref(), Some("s"));
        assert_eq!(StaticKeys::new("s").refresh().as_deref(), Some("s"));
    }
}
