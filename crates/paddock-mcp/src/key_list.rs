//! Where the client key list is kept: an owner-only versioned file in the app's config folder, never the OS credential store.
//!
//! A [`KeyEntry`] holds an id, a client name, the SHA-256 of the key, a time and a revoked flag; the key itself is never kept, so the
//! list is not a secret and needs no credential store. The store was moved out of the Keychain because opening a Keychain item
//! created by another ad-hoc signed build blocks the process on a SecurityAgent prompt before the first window (Dyno 1.1.0).

use crate::keys::KeyEntry;
use paddock_env::{EnvError, Problem, VersionedFile};
use serde::{Deserialize, Serialize};
use std::io::Read as _;
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
    /// The list this store last loaded or saved: a file that parses and holds another one was put there by hand and is never overwritten.
    known: Mutex<Vec<KeyEntry>>,
}

const NO_CONNECT: &str = "No client can connect until a new key is issued in Settings > AI access";
const RESTART: &str =
    "The key list file changed while the app was running. Restart the app to use it";
/// Largest list read to compare (sixteen keys are a few KB).
const MAX_LIST_BYTES: u64 = 1 << 20;

/// Why a Unix list is not trusted: it is not owned by the user running the app, other users can write it, or other users can write its
/// folder (so they could swap the file) and the folder has no sticky bit.
#[cfg(unix)]
fn distrust(mode: u32, owner: u32, running_user: u32, folder_mode: Option<u32>) -> Option<String> {
    if owner != running_user {
        Some("is owned by another user".into())
    } else if mode & 0o022 != 0 {
        Some(format!("can be changed by other users (mode {mode:o})"))
    } else if folder_mode.is_some_and(|m| m & 0o022 != 0 && m & 0o1000 == 0) {
        Some("is in a folder that other users can change".into())
    } else {
        None
    }
}

#[cfg(unix)]
fn untrusted(path: &Path) -> Option<String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let meta = std::fs::metadata(path).ok()?;
    let folder_mode = path
        .parent()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.permissions().mode() & 0o7777);
    distrust(
        meta.permissions().mode() & 0o777,
        meta.uid(),
        paddock_proc::effective_uid(),
        folder_mode,
    )
}

#[cfg(not(unix))]
fn untrusted(_: &Path) -> Option<String> {
    None
}

/// Makes the list's folder private: created 0700 when missing (whatever the umask: 002 would make 0775, which is then refused as
/// writable by the group), and made 0700 when this user owns it and group or others can write to it without a sticky bit. A folder
/// that someone else owns is left alone and judged by [`distrust`].
#[cfg(unix)]
fn secure_folder(path: &Path) {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) else {
        return;
    };
    let _ = std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir);
    if let Ok(meta) = std::fs::metadata(dir) {
        let mode = meta.permissions().mode();
        if meta.uid() == paddock_proc::effective_uid() && mode & 0o022 != 0 && mode & 0o1000 == 0 {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
}

#[cfg(not(unix))]
fn secure_folder(_: &Path) {}

impl FileKeys {
    /// The key list in `path` (for example `dirs.config().join("mcp-keys.json")`).
    pub fn new(path: impl Into<PathBuf>) -> FileKeys {
        let path = path.into();
        FileKeys {
            file: VersionedFile::new(path.clone(), 1).private().no_restore(),
            path,
            known: Mutex::new(Vec::new()),
        }
    }

    /// Whether the file parses and holds a list other than the one this store knows.
    fn changed_by_hand(&self) -> bool {
        let mut bytes = Vec::new();
        let read = std::fs::File::open(&self.path)
            .and_then(|f| f.take(MAX_LIST_BYTES).read_to_end(&mut bytes));
        let keys = read
            .ok()
            .and_then(|_| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|v| serde_json::from_value::<Vec<KeyEntry>>(v.get("keys")?.clone()).ok());
        keys.is_some_and(|k| k != *self.known.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn remember(&self, entries: &[KeyEntry]) {
        *self.known.lock().unwrap_or_else(PoisonError::into_inner) = entries.to_vec();
    }

    /// The saved list as [`KeyListStore::load`] reports it, without changing what this store knows.
    fn read_list(&self) -> KeysLoad {
        secure_folder(&self.path);
        if let Some(why) = untrusted(&self.path) {
            return KeysLoad {
                entries: Vec::new(),
                problem: Some(format!(
                    "The AI access key list ({}) {why}, so it is not trusted. Fix its owner or mode (chmod 600), or its folder's mode, or move it away, \
                     then restart the app. No client can connect until then.",
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
}

fn describe(problem: &Problem) -> String {
    match problem {
        Problem::Damaged {
            reason, kept_as, ..
        } => {
            let kept = kept_as
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| format!(" The damaged file was kept as {}.", n.to_string_lossy()))
                .unwrap_or_default();
            format!(
                "The AI access key list could not be read ({reason}).{kept} Its older copy is not used. {NO_CONNECT}."
            )
        }
        Problem::Newer { found, supported } => format!(
            "The AI access key list was written by a newer version of this app (format {found}; this version reads up to {supported}). \
             It is left as it is. Update the app; until then no client can connect."
        ),
        Problem::Unreadable { reason } => format!(
            "The AI access key list could not be opened ({reason}). Check the file's permissions, or move it away to start fresh, then restart the app. \
             No client can connect until then."
        ),
        Problem::MissingMigration { from } => format!(
            "The AI access key list is format {from} and this version cannot convert it. Update the app; until then no client can connect."
        ),
    }
}

impl KeyListStore for FileKeys {
    fn load(&self) -> KeysLoad {
        let loaded = self.read_list();
        self.remember(&loaded.entries);
        loaded
    }

    fn save(&self, entries: &[KeyEntry]) -> Result<(), String> {
        secure_folder(&self.path);
        // A list put there by hand (restored, or fixed with chmod) is never overwritten, however often this is tried.
        if self.changed_by_hand() {
            return Err(RESTART.into());
        }
        let list = KeyFile {
            keys: entries.to_vec(),
        };
        let saved = match self.file.save(&list) {
            // The refusal was decided at the last load and stays until the next one: look again, in case the cause was fixed since
            // (the folder removed, the file moved away). Any other failure is reported as it is.
            Err(refused @ (EnvError::NewerVersion { .. } | EnvError::ReadOnly { .. })) => {
                let again = self.read_list();
                if again.problem.is_some() {
                    return Err(refused.to_string());
                }
                if !again.entries.is_empty() {
                    return Err(RESTART.into());
                }
                self.file.save(&list)
            }
            other => other,
        };
        saved
            .map(|()| self.remember(entries))
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

#[cfg(all(test, unix))]
mod tests {
    use super::distrust;

    #[test]
    fn a_list_is_trusted_only_for_its_user_with_no_write_for_others_in_it_or_its_folder() {
        assert_eq!(distrust(0o600, 501, 501, Some(0o700)), None);
        assert_eq!(
            distrust(0o644, 501, 501, Some(0o755)),
            None,
            "others may read a list of hashes"
        );
        assert_eq!(
            distrust(0o600, 501, 501, None),
            None,
            "a folder that cannot be read says nothing"
        );
        assert!(
            distrust(0o666, 501, 501, Some(0o700))
                .unwrap()
                .contains("other users")
        );
        assert!(
            distrust(0o620, 501, 501, Some(0o700))
                .unwrap()
                .contains("other users")
        );
        assert!(
            distrust(0o644, 502, 501, Some(0o700))
                .unwrap()
                .contains("owned by another user"),
            "not the running user"
        );
        assert!(
            distrust(0o600, 501, 0, Some(0o700))
                .unwrap()
                .contains("owned by another user"),
            "root running, user's file"
        );
        assert!(
            distrust(0o600, 501, 501, Some(0o777))
                .unwrap()
                .contains("folder")
        );
        assert!(
            distrust(0o600, 501, 501, Some(0o770))
                .unwrap()
                .contains("folder")
        );
        assert_eq!(
            distrust(0o600, 501, 501, Some(0o1777)),
            None,
            "a sticky folder keeps others from swapping the file"
        );
    }
}
