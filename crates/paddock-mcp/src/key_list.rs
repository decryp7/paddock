//! Where the client key list is kept: an owner-only versioned file in the app's config folder, never the OS credential store.
//!
//! A [`KeyEntry`] holds an id, a client name, the SHA-256 of the key, a time and a revoked flag; the key itself is never kept, so the
//! list is not a secret and needs no credential store. The store was moved out of the Keychain because opening a Keychain item
//! created by another ad-hoc signed build blocks the process on a SecurityAgent prompt before the first window (Dyno 1.1.0).

use crate::keys::KeyEntry;
use paddock_env::{Problem, VersionedFile};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// What [`KeyListStore::load`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeysLoad {
    /// The saved keys. Empty whenever `problem` is set: an unusable list means no key works (failing closed).
    pub entries: Vec<KeyEntry>,
    /// Why the list was not used as saved, with the next step; the app logs it and shows it on the AI access page.
    pub problem: Option<String>,
}

/// Where the key list lives.
pub trait KeyListStore: Send + Sync {
    /// The saved keys. Never fails: a missing list is empty and an unusable one is empty with a [`KeysLoad::problem`].
    fn load(&self) -> KeysLoad;
    /// Saves the list; the message says what to fix. A store that refused an earlier save because of a problem it found at
    /// [`load`](Self::load) looks again, so a fixed cause works without a restart.
    fn save(&self, entries: &[KeyEntry]) -> Result<(), String>;
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct KeyFile {
    keys: Vec<KeyEntry>,
}

/// [`KeyListStore`] over an owner-only versioned JSON file (0600 on Unix; on Windows the per-user config folder is already private,
/// the same rule as every `VersionedFile::private` file of the family). A file that other users can write is refused on Unix, and a
/// damaged file's backup is never used (it could still hold a key revoked since).
pub struct FileKeys {
    path: PathBuf,
    file: VersionedFile<KeyFile>,
}

const NO_CONNECT: &str = "No client can connect until a new key is issued in Settings > AI access";

/// Whether others can write to `path` (Unix: a group or world write bit). `None` when it cannot be told.
#[cfg(unix)]
fn others_can_write(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).ok()?.permissions().mode() & 0o777;
    (mode & 0o022 != 0).then_some(mode)
}

#[cfg(not(unix))]
fn others_can_write(_: &Path) -> Option<u32> {
    None
}

impl FileKeys {
    /// The key list in `path` (for example `dirs.config().join("mcp-keys.json")`).
    pub fn new(path: impl Into<PathBuf>) -> FileKeys {
        let path = path.into();
        FileKeys {
            file: VersionedFile::new(path.clone(), 1).private().no_restore(),
            path,
        }
    }
}

fn describe(problem: &Problem) -> String {
    match problem {
        Problem::Damaged { reason, kept_as, .. } => {
            let kept = kept_as
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| format!(" The damaged file was kept as {}.", n.to_string_lossy()))
                .unwrap_or_default();
            format!("The AI access key list could not be read ({reason}).{kept} Its older copy is not used. {NO_CONNECT}.")
        }
        Problem::Newer { found, supported } => format!(
            "The AI access key list was written by a newer version of this app (format {found}; this version reads up to {supported}). \
             It is left as it is. Update the app; until then no client can connect."
        ),
        Problem::Unreadable { reason } => format!(
            "The AI access key list could not be opened ({reason}). Check the file's permissions, or move it away to start fresh. \
             No client can connect until then."
        ),
        Problem::MissingMigration { from } => format!(
            "The AI access key list is format {from} and this version cannot convert it. Update the app; until then no client can connect."
        ),
    }
}

impl KeyListStore for FileKeys {
    fn load(&self) -> KeysLoad {
        if let Some(mode) = others_can_write(&self.path) {
            return KeysLoad {
                entries: Vec::new(),
                problem: Some(format!(
                    "The AI access key list ({}) can be changed by other users (mode {mode:o}), so it is not trusted. \
                     Run chmod 600 on it, or move it away to start fresh. No client can connect until then.",
                    self.path.display()
                )),
            };
        }
        let loaded = self.file.load();
        match loaded.problem {
            None => KeysLoad {
                entries: loaded.value.keys,
                problem: None,
            },
            Some(problem) => KeysLoad {
                entries: Vec::new(),
                problem: Some(describe(&problem)),
            },
        }
    }

    fn save(&self, entries: &[KeyEntry]) -> Result<(), String> {
        let list = KeyFile {
            keys: entries.to_vec(),
        };
        let first = match self.file.save(&list) {
            Ok(()) => return Ok(()),
            Err(e) => e.to_string(),
        };
        // The refusal was decided at the last load and stays until the next one: look again, in case the cause was fixed since (the
        // folder removed, the file moved away). A file that is now readable and holds keys is not overwritten with ours.
        let again = self.load();
        if again.problem.is_some() {
            return Err(first);
        }
        if !again.entries.is_empty() {
            return Err(
                "The key list file changed while the app was running. Restart the app and try again.".into(),
            );
        }
        self.file.save(&list).map_err(|e| e.to_string())
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
