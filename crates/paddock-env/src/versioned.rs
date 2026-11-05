use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::atomic::write_atomic;
use crate::error::{EnvError, Result};

/// One step of a migration chain: turns the file's JSON object at version `n` into the object for version `n + 1`
/// (the `version` field itself is set by the chain). Return a message when the data cannot be converted.
pub type Migration = fn(Value) -> std::result::Result<Value, String>;

/// A file without a `version` field is this version (the apps' files from before versioning).
pub const FIRST_VERSION: u32 = 1;
/// Default limit for a state file's size; a larger file is treated as damaged.
pub const DEFAULT_MAX_BYTES: u64 = 8 << 20;
/// Default number of damaged copies kept beside the file.
pub const DEFAULT_KEEP_CORRUPT: usize = 5;

/// Why the file was not used as it is. The app reports it once (a notification), keeping `Display` for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// Not a usable state file; kept as `<name>.corrupt-<time>`. Defaults are in use, or the backup when
    /// `recovered_from_backup`.
    Damaged {
        /// What was wrong.
        reason: String,
        /// Where the damaged file was kept (None when it could not be moved).
        kept_as: Option<PathBuf>,
        /// The `.bak` copy was valid and is in use.
        recovered_from_backup: bool,
    },
    /// Written by a newer version of the app. Left untouched; defaults are in use and saving is refused.
    Newer {
        /// The version in the file.
        found: u32,
        /// The newest version this build reads.
        supported: u32,
    },
    /// The file could not be read (permissions, a folder in its place). Left alone; saving is refused.
    Unreadable {
        /// The cause.
        reason: String,
    },
    /// The file is older than this build but the chain has no step from `from` (a bug in the app). Left untouched;
    /// saving is refused.
    MissingMigration {
        /// The version with no step.
        from: u32,
    },
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Damaged {
                reason,
                kept_as,
                recovered_from_backup,
            } => {
                write!(f, "The file could not be read ({reason}). ")?;
                f.write_str(if *recovered_from_backup {
                    "The last good copy is in use"
                } else {
                    "Defaults are in use"
                })?;
                match kept_as.as_ref().and_then(|p| p.file_name()) {
                    Some(n) => write!(f, "; the damaged file was kept as {}.", n.to_string_lossy()),
                    None => f.write_str("."),
                }
            }
            Self::Newer { found, supported } => write!(
                f,
                "The file was written by a newer version of the app (format {found}, this version reads up to {supported}). \
                 Defaults are in use, the file is left as it is and changes are not saved. Update the app."
            ),
            Self::Unreadable { reason } => write!(
                f,
                "The file could not be opened ({reason}). Defaults are in use and changes are not saved."
            ),
            Self::MissingMigration { from } => {
                write!(
                    f,
                    "The file is format {from} and this version cannot convert it. Defaults are in use and changes are not saved."
                )
            }
        }
    }
}

/// The result of [`VersionedFile::load`]: always a usable value.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded<T> {
    /// The file's content, or `T::default()` when it was missing or not usable.
    pub value: T,
    /// Why the file was not used as it is, if so.
    pub problem: Option<Problem>,
    /// The version the file had before it was migrated (and rewritten) by this load.
    pub migrated_from: Option<u32>,
}

enum ReadOnly {
    Newer(u32),
    Other(String),
}

enum Fail {
    Damaged(String),
    Newer(u32),
    MissingMigration(u32),
}

/// A JSON state file with a `version`: one object, `version` at the top level and the value's fields beside it.
///
/// - **Missing** gives defaults. **Damaged** (not JSON, not an object, wrong shape, too large, a migration failing) gives
///   defaults, or the `.bak` copy when that is valid, and the damaged file is kept as `<name>.corrupt-<time>` (the newest
///   few are kept) with a [`Problem`].
/// - **Newer** than this build gives defaults and a [`Problem`]; the file is not touched and [`save`](Self::save) is
///   refused until the app is updated, so a downgrade never destroys data.
/// - **Older** goes through the migration chain (`with_migration(from, step)`, one step per version) in memory; the result
///   is written back once, and the old file is kept as `<name>.v<from>.bak`.
/// - **Saves** are atomic with a backup: the previous good file becomes `<name>.bak` (a damaged one never replaces a
///   good backup), then the new content is written beside it, flushed and renamed over the file.
///
/// `T` is deserialized from the object without its `version` field, so it needs no such field.
pub struct VersionedFile<T> {
    path: PathBuf,
    current: u32,
    migrations: Vec<(u32, Migration)>,
    max_bytes: u64,
    keep_corrupt: usize,
    private: bool,
    restore: bool,
    clock: fn() -> u64,
    read_only: Mutex<Option<ReadOnly>>,
    _value: PhantomData<fn() -> T>,
}

impl<T: Serialize + DeserializeOwned + Default> VersionedFile<T> {
    /// A state file at `path` whose current format is `current` (at least 1).
    pub fn new(path: impl Into<PathBuf>, current: u32) -> Self {
        Self {
            path: path.into(),
            current: current.max(FIRST_VERSION),
            migrations: Vec::new(),
            max_bytes: DEFAULT_MAX_BYTES,
            keep_corrupt: DEFAULT_KEEP_CORRUPT,
            private: false,
            restore: true,
            clock: unix_seconds,
            read_only: Mutex::new(None),
            _value: PhantomData,
        }
    }

    /// Adds the step from version `from` to `from + 1`.
    #[must_use]
    pub fn with_migration(mut self, from: u32, step: Migration) -> Self {
        self.migrations.retain(|(v, _)| *v != from);
        self.migrations.push((from, step));
        self
    }

    /// The largest file read (default [`DEFAULT_MAX_BYTES`]).
    #[must_use]
    pub fn max_bytes(mut self, max: u64) -> Self {
        self.max_bytes = max;
        self
    }

    /// How many damaged copies stay beside the file (default [`DEFAULT_KEEP_CORRUPT`]).
    #[must_use]
    pub fn keep_corrupt(mut self, keep: usize) -> Self {
        self.keep_corrupt = keep.max(1);
        self
    }

    /// Writes the file (and its backup) owner-only on Unix, for files that hold anything private.
    #[must_use]
    pub fn private(mut self) -> Self {
        self.private = true;
        self
    }

    /// A damaged file is never replaced by its `.bak` copy: it is moved aside and defaults are used, with nothing written back. For
    /// files where an older copy could be wrong in a way that matters (a list of revoked keys).
    #[must_use]
    pub fn no_restore(mut self) -> Self {
        self.restore = false;
        self
    }

    /// Replaces the clock used to name damaged copies (seconds since the Unix epoch); for tests.
    #[must_use]
    pub fn with_clock(mut self, clock: fn() -> u64) -> Self {
        self.clock = clock;
        self
    }

    /// The file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The backup of the previous good content: `<file>.bak`.
    pub fn backup_path(&self) -> PathBuf {
        sibling(&self.path, ".bak")
    }

    /// Reads the file. Never fails: every problem gives a value and a [`Problem`] (see the type's rules).
    pub fn load(&self) -> Loaded<T> {
        *self.lock() = None;
        let bytes = match self.read(&self.path) {
            Ok(None) => {
                return Loaded {
                    value: T::default(),
                    problem: None,
                    migrated_from: None,
                };
            }
            Ok(Some(bytes)) => bytes,
            Err(Read::Large) => {
                return self.damaged(format!("larger than {} KB", self.max_bytes / 1024));
            }
            Err(Read::Io(reason)) => {
                *self.lock() = Some(ReadOnly::Other(format!("it could not be read ({reason})")));
                return self.defaults(Problem::Unreadable { reason });
            }
        };
        match self.parse(&bytes) {
            Ok((value, from)) => {
                if let Some(from) = from {
                    self.persist_migration(&bytes, from, &value);
                }
                Loaded {
                    value,
                    problem: None,
                    migrated_from: from,
                }
            }
            Err(Fail::Newer(found)) => {
                *self.lock() = Some(ReadOnly::Newer(found));
                self.defaults(Problem::Newer {
                    found,
                    supported: self.current,
                })
            }
            Err(Fail::MissingMigration(from)) => {
                *self.lock() = Some(ReadOnly::Other(format!("no step converts format {from}")));
                self.defaults(Problem::MissingMigration { from })
            }
            Err(Fail::Damaged(reason)) => self.damaged(reason),
        }
    }

    /// Writes `value` atomically (see the type's rules). Refused with [`EnvError::NewerVersion`] or
    /// [`EnvError::ReadOnly`] when the last [`load`](Self::load) left the file untouched on purpose.
    pub fn save(&self, value: &T) -> Result<()> {
        match &*self.lock() {
            Some(ReadOnly::Newer(found)) => {
                return Err(EnvError::NewerVersion {
                    path: self.path.clone(),
                    found: *found,
                    supported: self.current,
                });
            }
            Some(ReadOnly::Other(reason)) => {
                return Err(EnvError::ReadOnly {
                    path: self.path.clone(),
                    reason: reason.clone(),
                });
            }
            None => {}
        }
        let bytes = self.encode(value)?;
        // Keep the previous file as the backup only when it is itself good, so a damaged file never replaces a good one.
        if let Ok(Some(old)) = self.read(&self.path)
            && self.parse(&old).is_ok()
        {
            write_atomic(&self.backup_path(), &old, self.private)?;
        }
        write_atomic(&self.path, &bytes, self.private)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<ReadOnly>> {
        self.read_only
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn defaults(&self, problem: Problem) -> Loaded<T> {
        Loaded {
            value: T::default(),
            problem: Some(problem),
            migrated_from: None,
        }
    }

    fn encode(&self, value: &T) -> Result<Vec<u8>> {
        let Value::Object(fields) =
            serde_json::to_value(value).map_err(|e| EnvError::Serialize(e.to_string()))?
        else {
            return Err(EnvError::NotAnObject);
        };
        let mut doc = Map::with_capacity(fields.len() + 1);
        doc.insert("version".into(), self.current.into());
        doc.extend(fields.into_iter().filter(|(k, _)| k != "version"));
        let mut bytes = serde_json::to_vec_pretty(&Value::Object(doc))
            .map_err(|e| EnvError::Serialize(e.to_string()))?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    fn read(&self, path: &Path) -> std::result::Result<Option<Vec<u8>>, Read> {
        use std::io::Read as _;
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            // Windows refuses to open a folder as a file ("access denied"); say what is wrong instead.
            Err(e) => {
                return Err(Read::Io(if path.is_dir() {
                    "it is not a file".into()
                } else {
                    e.to_string()
                }));
            }
        };
        if file.metadata().is_ok_and(|m| !m.is_file()) {
            return Err(Read::Io("it is not a file".into()));
        }
        let mut bytes = Vec::new();
        // Bounded: the size may change between opening and reading.
        file.take(self.max_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| Read::Io(e.to_string()))?;
        if bytes.len() as u64 > self.max_bytes {
            Err(Read::Large)
        } else {
            Ok(Some(bytes))
        }
    }

    /// The value in `bytes` and the version it was migrated from, if it was.
    fn parse(&self, bytes: &[u8]) -> std::result::Result<(T, Option<u32>), Fail> {
        let mut doc: Value = serde_json::from_slice(bytes)
            .map_err(|e| Fail::Damaged(format!("not valid JSON: {e}")))?;
        if !doc.is_object() {
            return Err(Fail::Damaged("not a JSON object".into()));
        }
        let found = match doc.get("version") {
            None => FIRST_VERSION,
            Some(v) => v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n >= FIRST_VERSION)
                .ok_or_else(|| Fail::Damaged("the version is not a positive number".into()))?,
        };
        if found > self.current {
            return Err(Fail::Newer(found));
        }
        for from in found..self.current {
            let (_, step) = self
                .migrations
                .iter()
                .find(|(v, _)| *v == from)
                .ok_or(Fail::MissingMigration(from))?;
            doc = step(doc)
                .map_err(|why| Fail::Damaged(format!("converting format {from} failed: {why}")))?;
            if !doc.is_object() {
                return Err(Fail::Damaged(format!(
                    "converting format {from} did not give an object"
                )));
            }
        }
        if let Value::Object(fields) = &mut doc {
            fields.remove("version");
        }
        let value = serde_json::from_value(doc).map_err(|e| Fail::Damaged(e.to_string()))?;
        Ok((value, (found < self.current).then_some(found)))
    }

    /// After a migration: keep the old file as `<name>.v<from>.bak`, then write the new format. Best effort; the value
    /// is already in memory and the next save retries.
    fn persist_migration(&self, old: &[u8], from: u32, value: &T) {
        let keep = sibling(&self.path, &format!(".v{from}.bak"));
        let result = write_atomic(&keep, old, self.private)
            .and_then(|()| write_atomic(&self.path, &self.encode(value)?, self.private));
        if let Err(e) = result {
            log::warn!("could not save the migrated {}: {e}", self.path.display());
        }
    }

    fn damaged(&self, reason: String) -> Loaded<T> {
        let kept_as = self.keep_aside();
        let backup = match self.read(&self.backup_path()) {
            Ok(Some(bytes)) if self.restore => self.parse(&bytes).ok(),
            _ => None,
        };
        let problem = |recovered_from_backup| Problem::Damaged {
            reason,
            kept_as,
            recovered_from_backup,
        };
        match backup {
            Some((value, _)) => {
                if let Err(e) = write_atomic(
                    &self.path,
                    &self.encode(&value).unwrap_or_default(),
                    self.private,
                ) {
                    log::warn!(
                        "could not restore {} from its backup: {e}",
                        self.path.display()
                    );
                }
                Loaded {
                    value,
                    problem: Some(problem(true)),
                    migrated_from: None,
                }
            }
            None => Loaded {
                value: T::default(),
                problem: Some(problem(false)),
                migrated_from: None,
            },
        }
    }

    /// Moves the damaged file to `<name>.corrupt-<time>[-n]` and prunes the oldest beyond `keep_corrupt`.
    fn keep_aside(&self) -> Option<PathBuf> {
        let stamp = (self.clock)();
        // The next free number for this stamp is above every existing one, so a pruned slot is never reused.
        let n = self
            .corrupt_copies()
            .iter()
            .filter(|((s, _), _)| *s == stamp)
            .map(|((_, n), _)| n + 1)
            .max();
        let suffix = n.map_or_else(
            || format!(".corrupt-{stamp}"),
            |n| format!(".corrupt-{stamp}-{}", n.max(1)),
        );
        let target = sibling(&self.path, &suffix);
        if let Err(e) = std::fs::rename(&self.path, &target) {
            log::warn!(
                "could not keep the damaged {} aside: {e}",
                self.path.display()
            );
            return None;
        }
        let copies = self.corrupt_copies();
        let excess = copies.len().saturating_sub(self.keep_corrupt);
        for (_, path) in copies.into_iter().take(excess) {
            let _ = std::fs::remove_file(path);
        }
        Some(target)
    }

    /// The kept damaged copies, oldest first (by stamp, then by the collision number).
    fn corrupt_copies(&self) -> Vec<((u64, u32), PathBuf)> {
        let (Some(dir), Some(name)) = (self.path.parent(), self.path.file_name()) else {
            return Vec::new();
        };
        let prefix = format!("{}.corrupt-", name.to_string_lossy());
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut copies: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let file = e.file_name().to_string_lossy().into_owned();
                let rest = file.strip_prefix(&prefix)?;
                let (stamp, n) = rest.split_once('-').unwrap_or((rest, "0"));
                Some(((stamp.parse().ok()?, n.parse().ok()?), e.path()))
            })
            .collect();
        copies.sort();
        copies
    }
}

enum Read {
    Large,
    Io(String),
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
