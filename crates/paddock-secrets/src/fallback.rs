use crate::{Name, Secret, Secrets, SecretsError};

/// `primary` (the credential store) with an owner-only file as the fallback. **Opting in is constructing this**: an app
/// that wants no file keeps using the primary alone.
///
/// - `get`: the primary; when it has no value or is unavailable, the file.
/// - `set`: the primary; only when it is *unavailable* (no Secret Service, a locked keychain) the value goes to the file
///   instead. A value written to the primary removes an older file copy, so the two never disagree.
/// - `delete`: both.
pub struct WithFileFallback<P, F> {
    primary: P,
    file: F,
}

impl<P: Secrets, F: Secrets> WithFileFallback<P, F> {
    /// The primary store and its fallback.
    pub fn new(primary: P, file: F) -> WithFileFallback<P, F> {
        WithFileFallback { primary, file }
    }
}

impl<P: Secrets, F: Secrets> Secrets for WithFileFallback<P, F> {
    fn get(&self, name: &Name) -> Result<Option<Secret>, SecretsError> {
        match self.primary.get(name) {
            Ok(Some(v)) => Ok(Some(v)),
            Ok(None) | Err(SecretsError::Unavailable(_)) => self.file.get(name),
            Err(e) => Err(e),
        }
    }

    fn set(&self, name: &Name, value: &Secret) -> Result<(), SecretsError> {
        match self.primary.set(name, value) {
            Ok(()) => {
                // Best effort: a stale copy would otherwise win after the store becomes unavailable.
                let _ = self.file.delete(name);
                Ok(())
            }
            Err(SecretsError::Unavailable(_)) => self.file.set(name, value),
            Err(e) => Err(e),
        }
    }

    fn delete(&self, name: &Name) -> Result<(), SecretsError> {
        let primary = self.primary.delete(name);
        let file = self.file.delete(name);
        match primary {
            Err(SecretsError::Unavailable(_)) => file,
            other => other.and(file),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileSecrets, MemorySecrets};

    fn n() -> Name {
        Name::new("server", "admin key").unwrap()
    }
    fn s(v: &str) -> Secret {
        Secret::new(v).unwrap()
    }

    fn setup() -> (
        tempfile::TempDir,
        WithFileFallback<MemorySecrets, FileSecrets>,
    ) {
        let d = crate::file::private_tempdir();
        let f = WithFileFallback::new(MemorySecrets::new("Pitwall"), FileSecrets::new(d.path()));
        (d, f)
    }

    #[test]
    fn the_primary_is_used_while_it_works() {
        let (d, f) = setup();
        f.set(&n(), &s("k")).unwrap();
        assert_eq!(f.get(&n()).unwrap(), Some(s("k")));
        assert_eq!(
            std::fs::read_dir(d.path()).unwrap().count(),
            0,
            "no file is written while the store works"
        );
    }

    #[test]
    fn an_unavailable_store_falls_back_to_the_file_and_back() {
        let (d, f) = setup();
        f.primary.set_unavailable(true);
        f.set(&n(), &s("in-file")).unwrap();
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
        assert_eq!(f.get(&n()).unwrap(), Some(s("in-file")));
        f.primary.set_unavailable(false);
        assert_eq!(
            f.get(&n()).unwrap(),
            Some(s("in-file")),
            "no value in the store: the file answers"
        );
        f.set(&n(), &s("in-store")).unwrap();
        assert_eq!(
            std::fs::read_dir(d.path()).unwrap().count(),
            0,
            "the stale copy is gone"
        );
        assert_eq!(f.get(&n()).unwrap(), Some(s("in-store")));
    }

    #[test]
    fn delete_removes_both_and_other_errors_are_not_hidden() {
        let (_d, f) = setup();
        f.primary.set_unavailable(true);
        f.set(&n(), &s("k")).unwrap();
        f.delete(&n()).unwrap();
        assert_eq!(f.get(&n()).unwrap(), None);
        f.primary.set_unavailable(false);
        f.set(&n(), &s("k")).unwrap();
        f.delete(&n()).unwrap();
        assert_eq!(f.get(&n()).unwrap(), None);

        struct Broken;
        impl Secrets for Broken {
            fn get(&self, _: &Name) -> Result<Option<Secret>, SecretsError> {
                Err(SecretsError::Failed("denied".into()))
            }
            fn set(&self, _: &Name, _: &Secret) -> Result<(), SecretsError> {
                Err(SecretsError::Failed("denied".into()))
            }
            fn delete(&self, _: &Name) -> Result<(), SecretsError> {
                Err(SecretsError::Failed("denied".into()))
            }
        }
        let d = crate::file::private_tempdir();
        let b = WithFileFallback::new(Broken, FileSecrets::new(d.path()));
        assert!(b.get(&n()).is_err() && b.set(&n(), &s("k")).is_err() && b.delete(&n()).is_err());
        assert_eq!(
            std::fs::read_dir(d.path()).unwrap().count(),
            0,
            "a refusal is not a reason to write a file"
        );
    }
}
