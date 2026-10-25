//! Secrets (access tokens) live in the shell's credential store, never in the configuration or the database. The
//! engine reaches them through this trait; tests use [`MemorySecrets`].
use std::collections::BTreeMap;
use std::sync::Mutex;

/// A credential store keyed by scope (a watch id) and key (`token`).
///
/// Contract: never panics; `get` answers `None` for a missing or unreadable secret; `set` and `delete` report a
/// failure as a message the user can act on. Values must never be logged.
pub trait SecretStore: Send + Sync {
    /// The secret, if there is one.
    fn get(&self, scope: &str, key: &str) -> Option<String>;
    /// Stores a secret.
    fn set(&self, scope: &str, key: &str, value: &str) -> Result<(), String>;
    /// Removes a secret (no error when there is none).
    fn delete(&self, scope: &str, key: &str) -> Result<(), String>;
}

/// An in-memory store for tests.
#[derive(Debug, Default)]
pub struct MemorySecrets {
    map: Mutex<BTreeMap<(String, String), String>>,
}

impl MemorySecrets {
    /// An empty store.
    pub fn new() -> MemorySecrets {
        MemorySecrets::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<(String, String), String>> {
        self.map.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl SecretStore for MemorySecrets {
    fn get(&self, scope: &str, key: &str) -> Option<String> {
        self.lock().get(&(scope.to_string(), key.to_string())).cloned()
    }

    fn set(&self, scope: &str, key: &str, value: &str) -> Result<(), String> {
        self.lock().insert((scope.to_string(), key.to_string()), value.to_string());
        Ok(())
    }

    fn delete(&self, scope: &str, key: &str) -> Result<(), String> {
        self.lock().remove(&(scope.to_string(), key.to_string()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_scoped() {
        let s = MemorySecrets::new();
        assert_eq!(s.get("a", "token"), None);
        s.set("a", "token", "x").unwrap();
        s.set("b", "token", "y").unwrap();
        assert_eq!(s.get("a", "token").as_deref(), Some("x"));
        s.delete("a", "token").unwrap();
        s.delete("a", "token").unwrap();
        assert_eq!((s.get("a", "token"), s.get("b", "token").as_deref()), (None, Some("y")));
    }

    #[test]
    fn a_poisoned_lock_recovers() {
        let s = std::sync::Arc::new(MemorySecrets::new());
        let s2 = s.clone();
        let _ = std::thread::spawn(move || {
            let _g = s2.lock();
            panic!("poison");
        })
        .join();
        s.set("a", "k", "v").unwrap();
        assert_eq!(s.get("a", "k").as_deref(), Some("v"));
    }
}
