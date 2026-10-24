use crate::{Name, Secret, SecretsError};

/// The port an app's engine codes against.
///
/// Contract: never panics; a missing secret is `Ok(None)`; deleting a missing secret succeeds; errors are messages the
/// user can act on and never contain a value; values are never logged.
pub trait Secrets: Send + Sync {
    /// The secret, if there is one.
    fn get(&self, name: &Name) -> Result<Option<Secret>, SecretsError>;
    /// Stores (or replaces) a secret.
    fn set(&self, name: &Name, value: &Secret) -> Result<(), SecretsError>;
    /// Removes a secret; no error when there is none.
    fn delete(&self, name: &Name) -> Result<(), SecretsError>;
}

/// A raw store of `(service, account)` entries: the OS credential store or a memory map. Same contract as [`Secrets`].
pub trait Vault: Send + Sync {
    /// The entry, if there is one.
    fn get(&self, service: &str, account: &str) -> Result<Option<Secret>, SecretsError>;
    /// Stores (or replaces) an entry.
    fn set(&self, service: &str, account: &str, value: &Secret) -> Result<(), SecretsError>;
    /// Removes an entry; no error when there is none.
    fn delete(&self, service: &str, account: &str) -> Result<(), SecretsError>;
}

/// [`Secrets`] over a [`Vault`] for one app: the service is the app's display name, the account `<scope>:<key>`.
#[derive(Debug, Clone)]
pub struct VaultSecrets<V> {
    service: String,
    vault: V,
}

impl<V: Vault> VaultSecrets<V> {
    /// Secrets of the app called `service` (its display name, as the user sees it in Keychain Access).
    pub fn new(service: impl Into<String>, vault: V) -> VaultSecrets<V> {
        VaultSecrets {
            service: service.into(),
            vault,
        }
    }

    /// The service name.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// The vault underneath, for [`rename_entry`].
    pub fn vault(&self) -> &V {
        &self.vault
    }
}

impl<V: Vault> Secrets for VaultSecrets<V> {
    fn get(&self, name: &Name) -> Result<Option<Secret>, SecretsError> {
        self.vault.get(&self.service, &name.account())
    }

    fn set(&self, name: &Name, value: &Secret) -> Result<(), SecretsError> {
        self.vault.set(&self.service, &name.account(), value)
    }

    fn delete(&self, name: &Name) -> Result<(), SecretsError> {
        self.vault.delete(&self.service, &name.account())
    }
}

/// What [`rename_entry`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renamed {
    /// There was no old entry.
    NothingToMove,
    /// The old entry was written under the new name and deleted.
    Moved,
    /// The new name already had a value; the old entry was only deleted.
    AlreadyThere,
}

/// The one-time move of an entry to the new scheme: read old, write new, delete old, in that order, so a failure never
/// loses the secret (the old entry stays until the new one is written). Safe to run on every start.
pub fn rename_entry(
    vault: &dyn Vault,
    from: (&str, &str),
    to: (&str, &str),
) -> Result<Renamed, SecretsError> {
    let Some(value) = vault.get(from.0, from.1)? else {
        return Ok(Renamed::NothingToMove);
    };
    if from == to {
        return Ok(Renamed::AlreadyThere);
    }
    if vault.get(to.0, to.1)?.is_some() {
        vault.delete(from.0, from.1)?;
        return Ok(Renamed::AlreadyThere);
    }
    vault.set(to.0, to.1, &value)?;
    vault.delete(from.0, from.1)?;
    Ok(Renamed::Moved)
}
