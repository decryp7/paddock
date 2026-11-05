//! Where the client key list is kept: an owner-only versioned file in the app's config folder, never the OS credential store.
//!
//! A [`KeyEntry`] holds an id, a client name, the SHA-256 of the key, a time and a revoked flag; the key itself is never kept, so the
//! list is not a secret and needs no credential store. The store was moved out of the Keychain because opening a Keychain item
//! created by another ad-hoc signed build blocks the process on a SecurityAgent prompt before the first window (Dyno 1.1.0).

use crate::keys::KeyEntry;
use paddock_env::{Problem, VersionedFile};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

/// What [`KeyListStore::load`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeysLoad {
    /// The saved keys. Empty whenever `problem` is set: an unusable list means no key works (failing closed).
    pub entries: Vec<KeyEntry>,
    /// Why the list was not used as saved, with the next step; the app logs it.
    pub problem: Option<String>,
}

/// Where the key list lives.
pub trait KeyListStore: Send + Sync {
    /// The saved keys. Never fails: a missing list is empty and an unusable one is empty with a [`KeysLoad::problem`].
    fn load(&self) -> KeysLoad;
    /// Saves the list; the message says what to fix.
    fn save(&self, entries: &[KeyEntry]) -> Result<(), String>;
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct KeyFile {
    keys: Vec<KeyEntry>,
}

/// [`KeyListStore`] over an owner-only versioned JSON file (0600 on Unix; on Windows the per-user config folder is already private,
/// the same rule as every `VersionedFile::private` file of the family).
pub struct FileKeys(VersionedFile<KeyFile>);

impl FileKeys {
    /// The key list in `path` (for example `dirs.config().join("mcp-keys.json")`).
    pub fn new(path: impl Into<PathBuf>) -> FileKeys {
        FileKeys(VersionedFile::new(path, 1).private())
    }
}

impl KeyListStore for FileKeys {
    fn load(&self) -> KeysLoad {
        let loaded = self.0.load();
        let Some(problem) = loaded.problem else {
            return KeysLoad {
                entries: loaded.value.keys,
                problem: None,
            };
        };
        let mut note = String::new();
        if matches!(
            problem,
            Problem::Damaged {
                recovered_from_backup: true,
                ..
            }
        ) {
            // The backup is the list before the last change: it may still hold a key that was revoked since. Never use it, and
            // replace the restored copy so that the next start does not read it as good.
            if let Err(e) = self.0.save(&KeyFile::default()) {
                note = format!(" The older copy could not be cleared: {e}.");
            }
        }
        KeysLoad {
            entries: Vec::new(),
            problem: Some(format!(
                "The AI access client keys could not be read. {problem}{note} No client can connect until a new key is issued in \
                 Settings > AI access (or the app is updated, if the file is from a newer version)."
            )),
        }
    }

    fn save(&self, entries: &[KeyEntry]) -> Result<(), String> {
        self.0
            .save(&KeyFile {
                keys: entries.to_vec(),
            })
            .map_err(|e| e.to_string())
    }
}

/// [`KeyListStore`] in memory, for tests.
#[derive(Default)]
pub struct MemoryKeys {
    value: Mutex<Vec<KeyEntry>>,
    /// When set, [`KeyListStore::save`] fails with this message.
    pub fail: Mutex<Option<String>>,
}

impl KeyListStore for MemoryKeys {
    fn load(&self) -> KeysLoad {
        KeysLoad {
            entries: self
                .value
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            problem: None,
        }
    }

    fn save(&self, entries: &[KeyEntry]) -> Result<(), String> {
        if let Some(why) = self
            .fail
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        {
            return Err(why);
        }
        *self.value.lock().unwrap_or_else(PoisonError::into_inner) = entries.to_vec();
        Ok(())
    }
}
