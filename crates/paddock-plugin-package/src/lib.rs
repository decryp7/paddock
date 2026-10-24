//! The `.paddockplugin` package of out-of-process plugins (ADR 0001, "Signing and packaging"): the manifest a plugin
//! carries, the archive that holds it, the signature that vouches for it, the keys and trust decision, and the check
//! that an installed binary is still the one that was verified. Pure library: no process launching, no app state and no
//! app domain types; the host (`paddock-plugin-host`) installs and runs plugins, `paddock-plugin-api` re-exports the
//! manifest types a plugin links.
//!
//! - [`manifest`]: `manifest.toml` ([`Manifest`], [`Permission`], [`Contribution`]), parsed once with limits;
//! - [`names`]: the validated names the manifest uses ([`PluginId`], [`GrantRef`], [`ProgramRef`], [`HostPattern`],
//!   [`Namespace`]);
//! - [`ProtocolVersion`]: `paddock_ipc`'s, the one compatibility rule (platform protocol and contract minors alike);
//! - [`archive`]: reading, verifying, packing and extracting a package ([`Package`], [`archive::build`],
//!   [`archive::verify_installed`]);
//! - [`keys`]: Ed25519 publisher keys ([`SecretKey`], [`PublicKey`]);
//! - [`trust`]: which packages may be installed ([`TrustPolicy`], [`TrustStore`], [`Trust`]).

#![deny(missing_docs)]

pub mod archive;
mod error;
pub mod keys;
pub mod manifest;
pub mod names;
pub mod trust;

pub use archive::{Package, Signature};
pub use error::PackageError;
pub use keys::{PublicKey, SecretKey};
pub use manifest::{Action, Contribution, Manifest, Permission, PermissionRequest};
pub use names::{GrantRef, HostPattern, Namespace, PluginId, ProgramRef};
pub use paddock_ipc::ProtocolVersion;
pub use trust::{SignatureStatus, Trust, TrustPolicy, TrustSettings, TrustStore, TrustedKey};
