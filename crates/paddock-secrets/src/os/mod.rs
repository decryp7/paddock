//! The OS credential store through `keyring` (feature `keyring`). The calls themselves are in `native.rs`.

mod native;

pub use native::OsVault;

use crate::VaultSecrets;

/// [`crate::Secrets`] in the OS credential store: `KeyringSecrets::os("Spotter")`.
pub type KeyringSecrets = VaultSecrets<OsVault>;

impl KeyringSecrets {
    /// The app called `service` (its display name) in the OS credential store.
    pub fn os(service: &str) -> KeyringSecrets {
        VaultSecrets::new(service, OsVault)
    }
}

/// Whether the real credential store must not be reached now: in this crate's tests, or when the app's tests set
/// `PADDOCK_SECRETS_FORBID_REAL=1`, unless `PADDOCK_SECRETS_REAL=1` allows it (rule 7). `OsVault` panics when this is
/// true, so a test that reaches `keyring` fails instead of touching the user's keychain.
pub fn real_blocked() -> bool {
    blocked(
        cfg!(test),
        env_is_one("PADDOCK_SECRETS_FORBID_REAL"),
        env_is_one("PADDOCK_SECRETS_REAL"),
    )
}

fn env_is_one(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1")
}

fn blocked(in_own_tests: bool, forbidden: bool, real: bool) -> bool {
    (in_own_tests || forbidden) && !real
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Name, Secret, Secrets, Vault};

    #[test]
    fn the_real_store_is_blocked_in_tests_unless_asked_for() {
        assert!(blocked(true, false, false));
        assert!(blocked(false, true, false));
        assert!(!blocked(true, true, true));
        assert!(
            !blocked(false, false, false),
            "an app outside tests is never blocked"
        );
        assert!(!blocked(false, true, true));
        assert_eq!(real_blocked(), !env_is_one("PADDOCK_SECRETS_REAL"));
    }

    #[test]
    fn reaching_keyring_in_a_test_fails_the_test() {
        // With PADDOCK_SECRETS_REAL=1 the caller chose the real store and there is nothing to check.
        if real_blocked() {
            let name = Name::new("scope", "key").unwrap();
            let secret = Secret::new("v").unwrap();
            let store = KeyringSecrets::os("Paddock Secrets Test");
            assert_eq!(store.service(), "Paddock Secrets Test");
            let calls: [&dyn Fn(); 4] = [
                &|| drop(store.get(&name)),
                &|| drop(store.set(&name, &secret)),
                &|| drop(store.delete(&name)),
                &|| drop(OsVault.get("s", "a")),
            ];
            for call in calls {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(call));
                let msg = r.expect_err("keyring must not be reached");
                let text = msg
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| msg.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                assert!(text.contains("PADDOCK_SECRETS_REAL=1"), "{text}");
            }
        }
    }
}
