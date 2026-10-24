use crate::{Name, Secret, Secrets, SecretsError, Vault, VaultSecrets};
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

#[derive(Default)]
struct State {
    map: BTreeMap<(String, String), Secret>,
    unavailable: bool,
}

/// An in-memory [`Vault`] for tests. It can be made unavailable, like a locked keychain.
#[derive(Default)]
pub struct MemoryVault(Mutex<State>);

impl MemoryVault {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Makes every later call fail with [`SecretsError::Unavailable`] (or work again).
    pub fn set_unavailable(&self, unavailable: bool) {
        self.lock().unavailable = unavailable;
    }

    fn check(s: &State) -> Result<(), SecretsError> {
        if s.unavailable {
            Err(SecretsError::unavailable("it is locked"))
        } else {
            Ok(())
        }
    }
}

impl Vault for MemoryVault {
    fn get(&self, service: &str, account: &str) -> Result<Option<Secret>, SecretsError> {
        let s = self.lock();
        Self::check(&s)?;
        Ok(s.map
            .get(&(service.to_string(), account.to_string()))
            .cloned())
    }

    fn set(&self, service: &str, account: &str, value: &Secret) -> Result<(), SecretsError> {
        let mut s = self.lock();
        Self::check(&s)?;
        s.map
            .insert((service.to_string(), account.to_string()), value.clone());
        Ok(())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), SecretsError> {
        let mut s = self.lock();
        Self::check(&s)?;
        s.map.remove(&(service.to_string(), account.to_string()));
        Ok(())
    }
}

/// In-memory [`Secrets`] for tests, with the same naming as the real store.
pub struct MemorySecrets(VaultSecrets<MemoryVault>);

impl MemorySecrets {
    /// An empty store for the app called `service`.
    pub fn new(service: &str) -> MemorySecrets {
        MemorySecrets(VaultSecrets::new(service, MemoryVault::default()))
    }

    /// Makes the store unavailable (or available again).
    pub fn set_unavailable(&self, unavailable: bool) {
        self.0.vault().set_unavailable(unavailable);
    }

    /// The vault, to seed old entries for [`crate::rename_entry`].
    pub fn vault(&self) -> &MemoryVault {
        self.0.vault()
    }
}

impl Secrets for MemorySecrets {
    fn get(&self, name: &Name) -> Result<Option<Secret>, SecretsError> {
        self.0.get(name)
    }
    fn set(&self, name: &Name, value: &Secret) -> Result<(), SecretsError> {
        self.0.set(name, value)
    }
    fn delete(&self, name: &Name) -> Result<(), SecretsError> {
        self.0.delete(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Renamed, rename_entry};
    use std::sync::Arc;

    fn n(k: &str) -> Name {
        Name::new("w1", k).unwrap()
    }
    fn s(v: &str) -> Secret {
        Secret::new(v).unwrap()
    }

    #[test]
    fn secrets_are_scoped_and_stored_under_the_scheme() {
        let m = MemorySecrets::new("Spotter");
        assert_eq!(m.get(&n("token")).unwrap(), None);
        m.set(&n("token"), &s("x")).unwrap();
        m.set(&Name::new("w2", "token").unwrap(), &s("y")).unwrap();
        assert_eq!(m.get(&n("token")).unwrap(), Some(s("x")));
        assert_eq!(
            m.vault().get("Spotter", "w1:token").unwrap(),
            Some(s("x")),
            "service = app name, account = scope:key"
        );
        m.delete(&n("token")).unwrap();
        m.delete(&n("token")).unwrap();
        assert_eq!(m.get(&n("token")).unwrap(), None);
        assert_eq!(
            m.get(&Name::new("w2", "token").unwrap()).unwrap(),
            Some(s("y"))
        );
    }

    #[test]
    fn an_unavailable_store_reports_it() {
        let m = MemorySecrets::new("App");
        m.set_unavailable(true);
        for e in [
            m.get(&n("k")).unwrap_err(),
            m.set(&n("k"), &s("v")).unwrap_err(),
            m.delete(&n("k")).unwrap_err(),
        ] {
            assert!(matches!(e, SecretsError::Unavailable(_)));
            assert!(
                e.to_string()
                    .starts_with("the credential store is not available")
            );
        }
        m.set_unavailable(false);
        assert!(m.set(&n("k"), &s("v")).is_ok());
    }

    #[test]
    fn a_poisoned_lock_recovers() {
        let m = Arc::new(MemorySecrets::new("App"));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.vault().0.lock().unwrap();
            panic!("poison");
        })
        .join();
        m.set(&n("k"), &s("v")).unwrap();
        assert_eq!(m.get(&n("k")).unwrap(), Some(s("v")));
    }

    #[test]
    fn an_old_entry_moves_once() {
        let m = MemorySecrets::new("Pitcrew");
        let v = m.vault();
        let old = ("Pitcrew notification channel", "smtp");
        let new = ("Pitcrew", "channel:smtp:password");
        assert_eq!(rename_entry(v, old, new).unwrap(), Renamed::NothingToMove);
        v.set(old.0, old.1, &s("pw")).unwrap();
        assert_eq!(rename_entry(v, old, new).unwrap(), Renamed::Moved);
        assert_eq!(
            (v.get(old.0, old.1).unwrap(), v.get(new.0, new.1).unwrap()),
            (None, Some(s("pw")))
        );
        assert_eq!(
            rename_entry(v, old, new).unwrap(),
            Renamed::NothingToMove,
            "idempotent"
        );
        // The new name wins; the old copy is dropped.
        v.set(old.0, old.1, &s("older")).unwrap();
        assert_eq!(rename_entry(v, old, new).unwrap(), Renamed::AlreadyThere);
        assert_eq!(
            (v.get(old.0, old.1).unwrap(), v.get(new.0, new.1).unwrap()),
            (None, Some(s("pw")))
        );
        // Same name: untouched.
        assert_eq!(rename_entry(v, new, new).unwrap(), Renamed::AlreadyThere);
        assert_eq!(v.get(new.0, new.1).unwrap(), Some(s("pw")));
    }

    #[test]
    fn a_failed_rename_keeps_the_old_entry() {
        let v = MemoryVault::default();
        v.set("Old", "a", &s("pw")).unwrap();
        struct FailsWrites<'a>(&'a MemoryVault);
        impl Vault for FailsWrites<'_> {
            fn get(&self, s: &str, a: &str) -> Result<Option<Secret>, SecretsError> {
                self.0.get(s, a)
            }
            fn set(&self, _: &str, _: &str, _: &Secret) -> Result<(), SecretsError> {
                Err(SecretsError::Failed("no".into()))
            }
            fn delete(&self, s: &str, a: &str) -> Result<(), SecretsError> {
                self.0.delete(s, a)
            }
        }
        assert!(rename_entry(&FailsWrites(&v), ("Old", "a"), ("New", "b")).is_err());
        FailsWrites(&v).delete("None", "x").unwrap();
        assert_eq!(v.get("Old", "a").unwrap(), Some(s("pw")));
        v.set_unavailable(true);
        assert!(rename_entry(&v, ("Old", "a"), ("New", "b")).is_err());
    }
}
