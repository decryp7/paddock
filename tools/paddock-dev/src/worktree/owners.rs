//! The owner map: which agent may write in which locked worktree (ADR 0083 §1, D9).
//!
//! It lives in `<git-common-dir>/paddock-worktree-owners.json` (ADR 0083 §1). Every update is a read-check-write under an exclusive lock on the
//! sidecar `paddock-worktree-owners.lock` and the write is atomic (temp file, then rename), so two agents claiming at once cannot both win.
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
    /// Paths an `add` is making right now (path to seconds since the epoch). `add` reserves the path under the lock, makes
    /// the worktree without it, then confirms; a reservation older than [`PENDING_TTL`] is a crashed `add` and is ignored.
    pending: BTreeMap<String, u64>,
}

/// How long a reservation counts.
pub const PENDING_TTL: u64 = 600;

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

    /// A live reservation for `path`.
    pub fn is_pending(&self, path: &str, now: u64) -> bool {
        self.pending
            .iter()
            .any(|(k, t)| same_path(k, path) && now.saturating_sub(*t) < PENDING_TTL)
    }

    /// Reserves `path` for an `add` in progress; `false` when another reservation is alive.
    pub fn reserve(&mut self, path: &str, now: u64) -> bool {
        if self.is_pending(path, now) {
            return false;
        }
        self.pending.retain(|k, _| !same_path(k, path));
        self.pending.insert(path.to_string(), now);
        true
    }

    /// Ends a reservation (the worktree was made, or git failed).
    pub fn unreserve(&mut self, path: &str) {
        self.pending.retain(|k, _| !same_path(k, path));
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
        let pending: Map<String, Value> = self.pending.iter().map(|(k, t)| (k.clone(), json!(t))).collect();
        json!({"version": VERSION, "owners": Value::Object(owners), "pending": Value::Object(pending)})
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
        let pending = v
            .get("pending")
            .and_then(Value::as_object)
            .map(|o| o.iter().filter_map(|(k, t)| t.as_u64().map(|t| (k.clone(), t))).collect())
            .unwrap_or_default();
        Ok(OwnerMap { entries, pending })
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
    LockTimeout(PathBuf, String, Duration),
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
            OwnerError::LockTimeout(p, holder, waited) => write!(
                f,
                "Waited {:.1} s for {}, held by {}. Run the command again; if that process is gone or stuck, end it (it holds the lock only while it runs).",
                waited.as_secs_f32(),
                p.display(),
                if holder.is_empty() {
                    "an unknown process".to_string()
                } else {
                    holder.clone()
                }
            ),
        }
    }
}

impl std::error::Error for OwnerError {}

/// The owner map of one repository.
#[derive(Debug, Clone)]
pub struct OwnerStore {
    dir: PathBuf,
    wait: Duration,
}

impl OwnerStore {
    /// The store beside the files of `<common_dir>`.
    pub fn open(common_dir: &Path) -> OwnerStore {
        OwnerStore {
            dir: common_dir.to_path_buf(),
            wait: LOCK_WAIT,
        }
    }

    /// The same store with another lock wait (tests).
    pub fn with_wait(mut self, wait: Duration) -> OwnerStore {
        self.wait = wait;
        self
    }

    /// The map file.
    pub fn file(&self) -> PathBuf {
        self.dir.join("paddock-worktree-owners.json")
    }

    fn lock_file(&self) -> PathBuf {
        self.dir.join("paddock-worktree-owners.lock")
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
        self.update_as("update", f)
    }

    /// Like [`OwnerStore::update`]; `what` names the caller in the lock file, so a waiter that times out can say who holds it.
    pub fn update_as<T>(&self, what: &str, f: impl FnOnce(&mut OwnerMap) -> T) -> Result<T, OwnerError> {
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
                Err(TryLockError::WouldBlock) if started.elapsed() < self.wait => std::thread::sleep(LOCK_POLL),
                Err(TryLockError::WouldBlock) => {
                    let holder = std::fs::read_to_string(&lock_path).unwrap_or_default();
                    return Err(OwnerError::LockTimeout(lock_path, holder.trim().to_string(), self.wait));
                }
                Err(TryLockError::Error(e)) => return Err(OwnerError::Io(format!("Locking {}", lock_path.display()), e)),
            }
        }
        // Say who holds it (the file is only written by the holder).
        let holder = format!(
            "pid {} running `{what}` since {}",
            std::process::id(),
            super::note::format_utc(super::note::now_secs())
        );
        let _ = lock.set_len(0);
        let _ = (&lock).write_all(holder.as_bytes());
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
        let tmp = self.dir.join(format!("paddock-worktree-owners.json.{}.tmp", std::process::id()));
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
    fn a_reservation_blocks_a_second_one_until_it_ends_or_goes_stale() {
        let mut m = OwnerMap::default();
        assert!(m.reserve("/w/a", 1000));
        assert!(m.is_pending("/w/a", 1001));
        assert!(!m.reserve("/w/a", 1001));
        m.unreserve("/w/a");
        assert!(m.reserve("/w/a", 1002));
        // A crashed add's reservation stops counting after the TTL.
        assert!(!m.is_pending("/w/a", 1002 + PENDING_TTL));
        assert!(m.reserve("/w/a", 1002 + PENDING_TTL));
    }

    #[test]
    fn a_reservation_survives_a_round_trip_and_an_old_file_without_one_still_reads() {
        let (_d, s) = store();
        s.update(|m| m.reserve("/w/a", 5)).unwrap();
        assert!(s.read().unwrap().is_pending("/w/a", 6));
        std::fs::write(s.file(), r#"{"version":1,"owners":{}}"#).unwrap();
        assert!(s.read().unwrap().is_empty());
    }

    #[test]
    fn a_lock_timeout_names_the_holder_and_never_says_hung() {
        let (_d, s) = store();
        let s = s.with_wait(Duration::from_millis(80));
        let held = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(s.lock_file())
            .unwrap();
        held.lock().unwrap();
        std::fs::write(
            s.lock_file(),
            "pid 4242 running `worktree add (confirm)` since 2026-11-10T00:00:00Z",
        )
        .unwrap();
        let err = s.update(|_| ()).unwrap_err();
        let text = err.to_string();
        assert!(matches!(err, OwnerError::LockTimeout(..)));
        assert!(
            text.contains("pid 4242") && text.contains("worktree add (confirm)") && !text.contains("hung"),
            "{text}"
        );
        drop(held);
        assert!(s.update(|_| ()).is_ok(), "once the holder is gone the lock is free");
    }

    #[test]
    fn the_lock_file_names_the_holder_while_an_update_runs() {
        let (_d, s) = store();
        let text = s
            .update_as("worktree claim", |_| std::fs::read_to_string(s.lock_file()).unwrap())
            .unwrap();
        assert!(
            text.contains(&format!("pid {}", std::process::id())) && text.contains("worktree claim"),
            "{text}"
        );
    }

    #[test]
    fn paths_compare_like_the_file_system() {
        assert!(same_path("/a/b", "/a/b"));
        assert_eq!(same_path("/a/B", "/a/b"), cfg!(any(target_os = "macos", windows)));
    }
}
