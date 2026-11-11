//! Credentials for the Paddock apps (plan X5, decision D6).
//!
//! - [`Secret`]: a value that never prints (`Debug` and `Display` are redacted) and is zeroized on drop.
//! - [`Name`]: the one naming scheme. The service is the app's display name; the account is `<scope>:<key>`.
//! - [`Secrets`]: the port an app's engine codes against; [`MemorySecrets`], an ephemeral store for tests and demos.
//! - [`Vault`]: one raw `(service, account)` store; [`VaultSecrets`] puts the naming scheme and the checks over it.
//!   With the `keyring` feature, [`KeyringSecrets`] is that over the OS credential store.
//! - [`FileSecrets`] (a folder of owner-only files), [`SecretFile`] (one at a chosen path) and [`WithFileFallback`].
//! - [`rename_entry`]: the one-time move of an old entry to the new scheme.
//!
//! Tests never reach the real credential store (rule 7): the native layer refuses unless `PADDOCK_SECRETS_REAL=1`.

mod acl;
mod error;
mod fallback;
mod file;
mod memory;
mod name;
#[cfg(feature = "keyring")]
mod os;
mod secret;
mod vault;

pub use error::SecretsError;
pub use fallback::WithFileFallback;
pub use file::{FileSecrets, SecretFile, make_folder_private, write_owner_only};
pub use memory::{MemorySecrets, MemoryVault};
pub use name::{MAX_NAME_LEN, Name};
#[cfg(feature = "keyring")]
pub use os::{KeyringSecrets, OsVault, real_blocked};
pub use secret::{MAX_SECRET_LEN, Secret};
pub use vault::{Renamed, Secrets, Vault, VaultSecrets, rename_entry};
