//! The owner map: which agent may write in which locked worktree (ADR 0083 §1, D9).
//!
//! It lives in `<git-common-dir>/paddock/owners.json`. Every update is a read-check-write under an exclusive lock on the
//! sidecar `owners.lock` and the write is atomic (temp file, then rename), so two agents claiming at once cannot both win.
//! The `./dev worktree` commands and the guard hook (RT-7) use this one module, so the format has one writer.
//!
//! A missing file is an empty map. A file that cannot be read as a map is an error, never an empty map: falling back to
//! "nobody owns anything" would let the next claim take over a worktree that an agent holds.

use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The format version written to the file.
pub const VERSION: u64 = 1;
/// The longest an update waits for the lock before it gives up and names the next step.
pub const LOCK_WAIT: Duration = Duration::from_secs(15);
const LOCK_POLL: Duration = Duration::from_millis(15);

/// Who holds a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The Claude Code `agent_id` (or any id the caller passes) that claimed it.
    pub agent_id: String,
    /// The agent's type (`implementer`, `reviewer`, ...), or empty when unknown.
    pub agent_type: String,
    /// When it was claimed, `YYYY-MM-DDTHH:MM:SSZ`.
    pub claimed: String,
}

/// What [`OwnerMap::claim`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// The worktree had no owner; the caller now has it.
    Granted,
    /// The caller already had it.
    Already,
    /// Another agent has it.
    Held(Entry),
}

/// Worktree path to its owner.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnerMap {
    entries: BTreeMap<String, Entry>,
}

/// Two paths name one worktree: equal, ignoring case where the file system does (macOS, Windows).
pub fn same_path(a: &str, b: &str) -> bool {
    if cfg!(any(target_os = "macos", windows)) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

impl OwnerMap {
    fn key_of(&self, path: &str) -> Option<String> {
        self.entries.keys().find(|k| same_path(k, path)).cloned()
    }

    /// The owner of `path`, if any.
    pub fn get(&self, path: &str) -> Option<&Entry> {
        self.entries.iter().find(|(k, _)| same_path(k, path)).map(|(_, e)| e)
    }

    /// Binds `path` to `agent_id` unless another agent holds it. The same agent claiming again changes nothing.
    pub fn claim(&mut self, path: &str, agent_id: &str, agent_type: &str, claimed: &str) -> Claim {
        match self.get(path) {
            Some(e) if e.agent_id == agent_id => Claim::Already,
            Some(e) => Claim::Held(e.clone()),
            None => {
                self.entries.insert(
                    path.to_string(),
                    Entry {
                        agent_id: agent_id.to_string(),
                        agent_type: agent_type.to_string(),
                        claimed: claimed.to_string(),
                    },
                );
                Claim::Granted
            }
        }
    }

    /// Forgets `path`; returns who held it.
    pub fn remove(&mut self, path: &str) -> Option<Entry> {
        let key = self.key_of(path)?;
        self.entries.remove(&key)
    }

    /// Every entry, ordered by path.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Entry)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// How many worktrees have an owner.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// No worktree has an owner.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn to_json(&self) -> Value {
        let mut owners = Map::new();
        for (k, e) in &self.entries {
            owners.insert(
                k.clone(),
                json!({"agent_id": e.agent_id, "agent_type": e.agent_type, "claimed": e.claimed}),
            );
        }
        json!({"version": VERSION, "owners": Value::Object(owners)})
    }

    fn from_json(v: &Value) -> Result<OwnerMap, String> {
        let version = v.get("version").and_then(Value::as_u64).ok_or("no `version`")?;
        if version != VERSION {
            return Err(format!("version {version}, this tool reads version {VERSION}"));
        }
        let owners = v.get("owners").and_then(Value::as_object).ok_or("no `owners` object")?;
        let mut entries = BTreeMap::new();
        for (path, e) in owners {
            let text = |k: &str| e.get(k).and_then(Value::as_str).map(str::to_string);
            let agent_id = text("agent_id")
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("`{path}` has no agent_id"))?;
            entries.insert(
                path.clone(),
                Entry {
                    agent_id,
                    agent_type: text("agent_type").unwrap_or_default(),
                    claimed: text("claimed").unwrap_or_default(),
                },
            );
        }
        Ok(OwnerMap { entries })
    }
}

/// Why the owner map could not be read or updated.
#[derive(Debug)]
pub enum OwnerError {
    /// A file operation failed.
    Io(String, std::io::Error),
    /// The file is not a valid map; it is left untouched.
    Corrupt(PathBuf, String),
    /// Another process held the lock for longer than [`LOCK_WAIT`].
    LockTimeout(PathBuf),
}

impl fmt::Display for OwnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OwnerError::Io(what, e) => write!(f, "{what}: {e}."),
            OwnerError::Corrupt(p, why) => write!(
                f,
                "The owner map {} is not valid ({why}); it was not changed. Move it aside after checking which agents hold which worktree, then claim again.",
                p.display()
            ),
            OwnerError::LockTimeout(p) => write!(
                f,
                "Another process has held {} for {} s. Run the command again; if it keeps failing, check for a hung `./dev worktree`.",
                p.display(),
                LOCK_WAIT.as_secs()
            ),
        }
    }
}

impl std::error::Error for OwnerError {}

/// The owner map of one repository.
#[derive(Debug, Clone)]
pub struct OwnerStore {
    dir: PathBuf,
}

impl OwnerStore {
    /// The store under `<common_dir>/paddock/` (the folder is made on the first update).
    pub fn open(common_dir: &Path) -> OwnerStore {
        OwnerStore {
            dir: common_dir.join("paddock"),
        }
    }

    /// The map file.
    pub fn file(&self) -> PathBuf {
        self.dir.join("owners.json")
    }

    fn lock_file(&self) -> PathBuf {
        self.dir.join("owners.lock")
    }

    /// The map as of now (no lock: the writer replaces the file atomically).
    pub fn read(&self) -> Result<OwnerMap, OwnerError> {
        let path = self.file();
        let mut text = String::new();
        match File::open(&path) {
            Ok(mut f) => {
                f.read_to_string(&mut text)
                    .map_err(|e| OwnerError::Io(format!("Reading {}", path.display()), e))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(OwnerMap::default()),
            Err(e) => return Err(OwnerError::Io(format!("Opening {}", path.display()), e)),
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| OwnerError::Corrupt(path.clone(), e.to_string()))?;
        OwnerMap::from_json(&v).map_err(|why| OwnerError::Corrupt(path, why))
    }

    /// Read, change and write the map under the exclusive lock. The file is written only when `f` changed the map.
    pub fn update<T>(&self, f: impl FnOnce(&mut OwnerMap) -> T) -> Result<T, OwnerError> {
        std::fs::create_dir_all(&self.dir).map_err(|e| OwnerError::Io(format!("Making {}", self.dir.display()), e))?;
        let lock_path = self.lock_file();
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|e| OwnerError::Io(format!("Opening {}", lock_path.display()), e))?;
        let started = Instant::now();
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if started.elapsed() < LOCK_WAIT => std::thread::sleep(LOCK_POLL),
                Err(TryLockError::WouldBlock) => return Err(OwnerError::LockTimeout(lock_path)),
                Err(TryLockError::Error(e)) => return Err(OwnerError::Io(format!("Locking {}", lock_path.display()), e)),
            }
        }
        // The lock is released when `lock` is dropped, on every path out of here.
        let before = self.read()?;
        let mut map = before.clone();
        let out = f(&mut map);
        if map != before {
            self.write(&map)?;
        }
        Ok(out)
    }

    fn write(&self, map: &OwnerMap) -> Result<(), OwnerError> {
        let tmp = self.dir.join(format!("owners.json.{}.tmp", std::process::id()));
        let io = |what: &str, e| OwnerError::Io(format!("{what} {}", tmp.display()), e);
        let mut f = File::create(&tmp).map_err(|e| io("Creating", e))?;
        f.write_all(format!("{:#}\n", map.to_json()).as_bytes())
            .map_err(|e| io("Writing", e))?;
        f.sync_all().map_err(|e| io("Syncing", e))?;
        drop(f);
        std::fs::rename(&tmp, self.file()).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            OwnerError::Io(format!("Replacing {}", self.file().display()), e)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, OwnerStore) {
        let d = tempfile::tempdir().unwrap();
        let s = OwnerStore::open(d.path());
        (d, s)
    }

    #[test]
    fn claim_grants_once_and_refuses_another_agent() {
        let mut m = OwnerMap::default();
        assert_eq!(m.claim("/w/a", "A", "implementer", "t"), Claim::Granted);
        assert_eq!(m.claim("/w/a", "A", "implementer", "t2"), Claim::Already);
        match m.claim("/w/a", "B", "implementer", "t3") {
            Claim::Held(e) => assert_eq!((e.agent_id.as_str(), e.claimed.as_str()), ("A", "t")),
            other => panic!("{other:?}"),
        }
        assert_eq!(m.len(), 1);
        assert!(m.remove("/w/a").is_some());
        assert!(m.is_empty());
        assert_eq!(m.claim("/w/a", "B", "", "t4"), Claim::Granted);
    }

    #[test]
    fn a_missing_file_is_an_empty_map_and_an_update_round_trips() {
        let (_d, s) = store();
        assert!(s.read().unwrap().is_empty());
        assert_eq!(
            s.update(|m| m.claim("/w/a", "A", "reviewer", "2026-11-10T00:00:00Z")).unwrap(),
            Claim::Granted
        );
        let m = s.read().unwrap();
        let e = m.get("/w/a").unwrap();
        assert_eq!(
            (e.agent_id.as_str(), e.agent_type.as_str(), e.claimed.as_str()),
            ("A", "reviewer", "2026-11-10T00:00:00Z")
        );
        let v: Value = serde_json::from_str(&std::fs::read_to_string(s.file()).unwrap()).unwrap();
        assert_eq!(v["version"], VERSION);
    }

    #[test]
    fn an_update_that_changes_nothing_writes_nothing() {
        let (_d, s) = store();
        s.update(|_| ()).unwrap();
        assert!(!s.file().exists());
    }

    #[test]
    fn a_corrupt_or_foreign_file_is_an_error_and_is_left_alone() {
        for text in [
            "not json",
            "{}",
            r#"{"version":9,"owners":{}}"#,
            r#"{"version":1,"owners":{"/w":{}}}"#,
        ] {
            let (_d, s) = store();
            std::fs::create_dir_all(s.dir.clone()).unwrap();
            std::fs::write(s.file(), text).unwrap();
            assert!(matches!(s.read(), Err(OwnerError::Corrupt(..))), "{text}");
            assert!(s.update(|m| m.claim("/w", "A", "", "t")).is_err(), "{text}");
            assert_eq!(std::fs::read_to_string(s.file()).unwrap(), text);
        }
    }

    #[test]
    fn threads_claiming_one_path_give_exactly_one_winner() {
        let (_d, s) = store();
        for round in 0..10 {
            let path = format!("/w/{round}");
            let wins: usize = std::thread::scope(|sc| {
                let handles: Vec<_> = (0..6)
                    .map(|i| {
                        let (s, path) = (s.clone(), path.clone());
                        sc.spawn(move || s.update(|m| m.claim(&path, &format!("agent-{i}"), "", "t")).unwrap())
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().unwrap())
                    .filter(|c| *c == Claim::Granted)
                    .count()
            });
            assert_eq!(wins, 1, "round {round}");
        }
        assert_eq!(s.read().unwrap().len(), 10);
    }

    #[test]
    fn paths_compare_like_the_file_system() {
        assert!(same_path("/a/b", "/a/b"));
        assert_eq!(same_path("/a/B", "/a/b"), cfg!(any(target_os = "macos", windows)));
    }
}
