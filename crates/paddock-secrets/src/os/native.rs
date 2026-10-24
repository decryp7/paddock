//! Native glue: the only file that calls `keyring`. Each function makes one call and maps its error; left out of the
//! coverage job (crates/README.md). The gate runs first, so tests cannot reach a real keychain.

use super::real_blocked;
use crate::{Secret, SecretsError, Vault};
use keyring::{Entry, Error};

/// The OS credential store as a [`Vault`].
#[derive(Debug, Default, Clone, Copy)]
pub struct OsVault;

fn gate() {
    assert!(
        !real_blocked(),
        "the real credential store was reached in a test; use MemorySecrets (or set PADDOCK_SECRETS_REAL=1 on purpose)"
    );
}

fn entry(service: &str, account: &str) -> Result<Entry, SecretsError> {
    gate();
    Entry::new(service, account).map_err(SecretsError::unavailable)
}

fn map(e: Error) -> SecretsError {
    match e {
        Error::NoStorageAccess(_) | Error::NoDefaultStore | Error::PlatformFailure(_) => {
            SecretsError::unavailable(e)
        }
        other => SecretsError::Failed(format!("the credential store refused it: {other}")),
    }
}

impl Vault for OsVault {
    fn get(&self, service: &str, account: &str) -> Result<Option<Secret>, SecretsError> {
        match entry(service, account)?.get_password() {
            Ok(v) => Secret::new(v).map(Some),
            Err(Error::NoEntry) => Ok(None),
            Err(e) => Err(map(e)),
        }
    }

    fn set(&self, service: &str, account: &str, value: &Secret) -> Result<(), SecretsError> {
        entry(service, account)?
            .set_password(value.expose())
            .map_err(map)
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), SecretsError> {
        match entry(service, account)?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(e) => Err(map(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KeyringSecrets, Name, Secrets};

    /// Opt-in round trip in the real store:
    /// `PADDOCK_SECRETS_REAL=1 cargo test -p paddock-secrets --features keyring -- --ignored`.
    #[test]
    #[ignore = "uses the real credential store"]
    fn real_round_trip() {
        let store = KeyringSecrets::os("Paddock Secrets Test");
        let name = Name::new("test", "round trip").unwrap();
        store.set(&name, &Secret::new("v").unwrap()).unwrap();
        assert_eq!(store.get(&name).unwrap().unwrap().expose(), "v");
        store.delete(&name).unwrap();
        assert_eq!(store.get(&name).unwrap(), None);
    }
}
