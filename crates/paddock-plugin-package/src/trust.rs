//! Which packages an app installs (ADR 0001, "Signing and packaging"). Three trust levels:
//!
//! - **Builtin**: signed by one of the app's own keys, shipped in the app and not removable;
//! - **Signed**: signed by a publisher key the user trusted ([`TrustSettings::keys`]);
//! - **Unsigned**: not signed, or signed by a key nobody trusted: installable only while the app's **developer mode** is on,
//!   with a warning.
//!
//! A signature that does not match the contents is never installable ([`crate::Package::open`] already refused it). The
//! decision is a pure function of the package's [`Signature`] and the [`TrustPolicy`]; the user's part of the policy
//! ([`TrustSettings`]) is kept by a [`TrustStore`], a versioned state file that tolerates a missing or damaged file by
//! falling back to defaults (nothing trusted, developer mode off).

use crate::PackageError;
use crate::archive::Signature;
use crate::keys::PublicKey;
use paddock_env::{Loaded, VersionedFile};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The format of [`TrustSettings`] on disk.
pub const TRUST_VERSION: u32 = 1;
/// Most keys a user may trust.
pub const MAX_TRUSTED_KEYS: usize = 64;
/// Largest trust file.
const MAX_TRUST_FILE: u64 = 256 * 1024;

/// A key the user trusts, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredKey {
    /// Who the key belongs to, as the user named it.
    pub name: String,
    /// The public key, text form.
    pub public_key: String,
}

/// The user's part of the trust policy: developer mode and the keys they added.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrustSettings {
    /// Unsigned packages and packages signed by unknown keys may be installed (with a warning).
    pub developer_mode: bool,
    /// Publisher keys the user trusts (besides the app's built-in ones).
    pub keys: Vec<StoredKey>,
}

/// A trusted key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedKey {
    /// Who it belongs to.
    pub name: String,
    /// The key.
    pub key: PublicKey,
    /// One of the app's own keys (cannot be removed).
    pub builtin: bool,
}

/// How far an installable package is trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trust {
    /// Signed by one of the app's own keys.
    Builtin {
        /// The key's name.
        publisher: String,
    },
    /// Signed by a publisher key the user trusted.
    Signed {
        /// The key's name.
        publisher: String,
    },
    /// Installable only because developer mode is on.
    Unsigned {
        /// What the user is warned about.
        warning: String,
    },
}

impl Trust {
    /// The level's stable code (`builtin`, `signed`, `unsigned`).
    pub fn code(&self) -> &'static str {
        match self {
            Trust::Builtin { .. } => "builtin",
            Trust::Signed { .. } => "signed",
            Trust::Unsigned { .. } => "unsigned",
        }
    }
}

/// The signature status shown before installing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureStatus {
    /// Signed by one of the app's own keys (its name).
    Builtin(String),
    /// Signed by a key the user trusts (its name).
    Trusted(String),
    /// Signed by a key that is not trusted (its id).
    UnknownKey(String),
    /// Not signed.
    Unsigned,
}

impl SignatureStatus {
    /// The stable code (`builtin`, `trusted`, `unknownKey`, `unsigned`).
    pub fn code(&self) -> &'static str {
        match self {
            SignatureStatus::Builtin(_) => "builtin",
            SignatureStatus::Trusted(_) => "trusted",
            SignatureStatus::UnknownKey(_) => "unknownKey",
            SignatureStatus::Unsigned => "unsigned",
        }
    }
}

/// The app's built-in keys plus the user's [`TrustSettings`]: everything the install decision needs.
#[derive(Debug, Clone)]
pub struct TrustPolicy {
    builtin: Vec<TrustedKey>,
    /// The user's part; change it through the methods below so the checks run, then save it with a [`TrustStore`].
    settings: TrustSettings,
}

impl TrustPolicy {
    /// A policy with the app's own keys (name, public key text) and the user's settings.
    pub fn new(
        builtin: &[(&str, &str)],
        settings: TrustSettings,
    ) -> Result<TrustPolicy, PackageError> {
        let builtin = builtin
            .iter()
            .map(|(name, text)| {
                Ok(TrustedKey {
                    name: (*name).to_string(),
                    key: PublicKey::parse(text)?,
                    builtin: true,
                })
            })
            .collect::<Result<Vec<_>, PackageError>>()?;
        Ok(TrustPolicy { builtin, settings })
    }

    /// The user's settings (to save).
    pub fn settings(&self) -> &TrustSettings {
        &self.settings
    }

    /// Turns developer mode on or off.
    pub fn set_developer_mode(&mut self, on: bool) {
        self.settings.developer_mode = on;
    }

    /// The built-in keys and the user's valid keys (an invalid stored key is skipped, not fatal).
    pub fn trusted_keys(&self) -> Vec<TrustedKey> {
        let mut out = self.builtin.clone();
        for k in &self.settings.keys {
            if let Ok(key) = PublicKey::parse(&k.public_key)
                && !out.iter().any(|t| t.key == key)
            {
                out.push(TrustedKey {
                    name: k.name.clone(),
                    key,
                    builtin: false,
                });
            }
        }
        out
    }

    /// Adds a key (checked: valid, named, not already trusted, within [`MAX_TRUSTED_KEYS`]). Returns its id.
    pub fn add_key(&mut self, name: &str, public_key: &str) -> Result<String, PackageError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
            return Err(PackageError::InvalidKey(
                "name the key's publisher (1-120 characters)".into(),
            ));
        }
        let key = PublicKey::parse(public_key)?;
        if self.trusted_keys().iter().any(|t| t.key == key) {
            return Err(PackageError::InvalidKey(format!(
                "key {} is already trusted",
                key.id()
            )));
        }
        if self.settings.keys.len() >= MAX_TRUSTED_KEYS {
            return Err(PackageError::InvalidKey(format!(
                "at most {MAX_TRUSTED_KEYS} keys can be trusted"
            )));
        }
        self.settings.keys.push(StoredKey {
            name: name.to_string(),
            public_key: key.to_text(),
        });
        Ok(key.id())
    }

    /// Removes the user's key with id `key_id`; built-in keys cannot be removed. Returns whether one was.
    pub fn remove_key(&mut self, key_id: &str) -> bool {
        let before = self.settings.keys.len();
        self.settings.keys.retain(|k| {
            PublicKey::parse(&k.public_key)
                .map(|p| p.id() != key_id)
                .unwrap_or(false)
        });
        self.settings.keys.len() != before
    }

    /// What the user sees about `signature`.
    pub fn status(&self, signature: &Signature) -> SignatureStatus {
        match signature {
            Signature::Unsigned => SignatureStatus::Unsigned,
            Signature::Signed { key } => {
                match self.trusted_keys().into_iter().find(|t| t.key == *key) {
                    Some(t) if t.builtin => SignatureStatus::Builtin(t.name),
                    Some(t) => SignatureStatus::Trusted(t.name),
                    None => SignatureStatus::UnknownKey(key.id()),
                }
            }
        }
    }

    /// Whether a package signed as `signature` may be installed, and at which trust level.
    pub fn decide(&self, signature: &Signature) -> Result<Trust, PackageError> {
        let developer = self.settings.developer_mode;
        match self.status(signature) {
            SignatureStatus::Builtin(publisher) => Ok(Trust::Builtin { publisher }),
            SignatureStatus::Trusted(publisher) => Ok(Trust::Signed { publisher }),
            SignatureStatus::Unsigned if developer => Ok(Trust::Unsigned {
                warning: "This plugin is not signed: nobody vouches for who made it or that it was not changed. Install it only if you built it yourself or trust where it came from.".into(),
            }),
            SignatureStatus::UnknownKey(id) if developer => Ok(Trust::Unsigned {
                warning: format!("This plugin is signed with key {id}, which you have not trusted. Install it only if you trust where it came from, or add the publisher's key first."),
            }),
            SignatureStatus::Unsigned => Err(PackageError::Unsigned),
            SignatureStatus::UnknownKey(key_id) => Err(PackageError::UnknownKey { key_id }),
        }
    }
}

/// The user's [`TrustSettings`] on disk: a versioned, owner-only state file. A missing or damaged file gives the defaults
/// (nothing trusted, developer mode off) and a [`paddock_env::Problem`] to report; a file from a newer version is left as it
/// is and saving is refused until the app is updated.
pub struct TrustStore {
    file: VersionedFile<TrustSettings>,
}

impl TrustStore {
    /// The store at `path`.
    pub fn new(path: impl Into<std::path::PathBuf>) -> TrustStore {
        TrustStore {
            file: VersionedFile::new(path, TRUST_VERSION)
                .max_bytes(MAX_TRUST_FILE)
                .private(),
        }
    }

    /// The file's path.
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    /// Reads the settings. Never fails: every problem gives defaults and a [`paddock_env::Problem`].
    pub fn load(&self) -> Loaded<TrustSettings> {
        self.file.load()
    }

    /// Saves the settings atomically.
    pub fn save(&self, settings: &TrustSettings) -> Result<(), PackageError> {
        Ok(self.file.save(settings)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::SecretKey;

    fn app_key() -> (SecretKey, String) {
        let sk = SecretKey::generate().unwrap();
        let text = sk.public().to_text();
        (sk, text)
    }

    fn policy_with_builtin() -> (SecretKey, TrustPolicy) {
        let (sk, text) = app_key();
        (
            sk,
            TrustPolicy::new(&[("The app", &text)], TrustSettings::default()).unwrap(),
        )
    }

    #[test]
    fn the_builtin_key_is_always_trusted_and_not_removable() {
        let (sk, mut p) = policy_with_builtin();
        let keys = p.trusted_keys();
        assert_eq!(keys.len(), 1);
        assert!(keys[0].builtin);
        let signed = Signature::Signed { key: sk.public() };
        assert_eq!(
            p.decide(&signed).unwrap(),
            Trust::Builtin {
                publisher: "The app".into()
            }
        );
        assert_eq!(p.status(&signed).code(), "builtin");
        assert_eq!(p.decide(&signed).unwrap().code(), "builtin");
        assert!(
            !p.remove_key(&sk.public().id()),
            "built-in keys cannot be removed"
        );
        assert!(p.add_key("again", &sk.public().to_text()).is_err());
        assert_eq!(
            TrustPolicy::new(&[("bad", "nope")], TrustSettings::default())
                .unwrap_err()
                .code(),
            "invalidKey"
        );
    }

    #[test]
    fn unsigned_and_unknown_keys_need_developer_mode() {
        let (_, mut p) = policy_with_builtin();
        let key = SecretKey::generate().unwrap().public();
        let signed = Signature::Signed { key };
        assert_eq!(
            p.decide(&Signature::Unsigned).unwrap_err(),
            PackageError::Unsigned
        );
        assert_eq!(p.decide(&signed).unwrap_err().code(), "unknownKey");
        assert_eq!(p.status(&signed), SignatureStatus::UnknownKey(key.id()));
        p.set_developer_mode(true);
        assert!(
            matches!(p.decide(&Signature::Unsigned).unwrap(), Trust::Unsigned { warning } if warning.contains("not signed"))
        );
        let unknown = p.decide(&signed).unwrap();
        assert!(matches!(&unknown, Trust::Unsigned { warning } if warning.contains(&key.id())));
        assert_eq!(unknown.code(), "unsigned");
        // Trusting the key makes it a normal install, with or without developer mode.
        p.set_developer_mode(false);
        assert_eq!(
            p.add_key("  Example Ltd ", &key.to_text()).unwrap(),
            key.id()
        );
        assert_eq!(
            p.decide(&signed).unwrap(),
            Trust::Signed {
                publisher: "Example Ltd".into()
            }
        );
        assert_eq!(p.decide(&signed).unwrap().code(), "signed");
        assert_eq!(p.status(&signed).code(), "trusted");
        assert!(
            p.add_key("Example", &key.to_text())
                .unwrap_err()
                .to_string()
                .contains("already trusted")
        );
        assert!(p.remove_key(&key.id()) && !p.remove_key(&key.id()));
        assert_eq!(p.decide(&signed).unwrap_err().code(), "unknownKey");
        assert_eq!(SignatureStatus::Unsigned.code(), "unsigned");
        assert_eq!(SignatureStatus::Builtin("x".into()).code(), "builtin");
        assert_eq!(SignatureStatus::UnknownKey("k".into()).code(), "unknownKey");
    }

    #[test]
    fn stored_keys_are_checked_and_bad_ones_skipped() {
        let (_, mut p) = policy_with_builtin();
        assert!(
            p.add_key("", &SecretKey::generate().unwrap().public().to_text())
                .is_err()
        );
        assert!(p.add_key("x", "not a key").is_err());
        let mut settings = p.settings().clone();
        settings.keys.push(StoredKey {
            name: "broken".into(),
            public_key: "garbage".into(),
        });
        let mut p = TrustPolicy::new(&[], settings).unwrap();
        assert_eq!(
            p.trusted_keys().len(),
            0,
            "the broken key is skipped, not fatal"
        );
        for i in 0..MAX_TRUSTED_KEYS - 1 {
            p.add_key(
                &format!("k{i}"),
                &SecretKey::generate().unwrap().public().to_text(),
            )
            .unwrap();
        }
        assert!(
            p.add_key(
                "one too many",
                &SecretKey::generate().unwrap().public().to_text()
            )
            .is_err()
        );
        let json = serde_json::to_string(p.settings()).unwrap();
        assert_eq!(
            &serde_json::from_str::<TrustSettings>(&json).unwrap(),
            p.settings()
        );
        assert_eq!(
            serde_json::from_str::<TrustSettings>("{}").unwrap(),
            TrustSettings::default()
        );
    }

    #[test]
    fn the_store_roundtrips_and_tolerates_a_missing_or_damaged_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = TrustStore::new(dir.path().join("state").join("trust.json"));
        let loaded = store.load();
        assert_eq!(
            (loaded.value, loaded.problem),
            (TrustSettings::default(), None),
            "missing gives defaults"
        );
        let (_, mut p) = policy_with_builtin();
        p.set_developer_mode(true);
        p.add_key(
            "Example",
            &SecretKey::generate().unwrap().public().to_text(),
        )
        .unwrap();
        store.save(p.settings()).unwrap();
        assert_eq!(&store.load().value, p.settings());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(store.path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        // Damaged: defaults (developer mode off, nothing trusted), reported, and the next save works.
        std::fs::write(store.path(), "{ not json").unwrap();
        let damaged = store.load();
        assert_eq!(damaged.value, TrustSettings::default());
        assert!(damaged.problem.is_some());
        store.save(&TrustSettings::default()).unwrap();
        // Newer than this build: left alone, saving refused.
        std::fs::write(store.path(), r#"{"version": 99, "developerMode": true}"#).unwrap();
        let newer = store.load();
        assert_eq!(
            newer.value,
            TrustSettings::default(),
            "a newer file never turns developer mode on"
        );
        assert!(newer.problem.is_some());
        assert_eq!(
            store.save(&TrustSettings::default()).unwrap_err().code(),
            "io"
        );
    }
}
