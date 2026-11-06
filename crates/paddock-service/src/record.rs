//! What this app remembers about the registration (`service-<label>.json` in its data folder, a `paddock-env`
//! [`VersionedFile`]: atomic, damaged or missing gives defaults, a newer format is left alone) and the lock that keeps two
//! copies from repairing at once (ADR 0056).
//!
//! The record is one declared state ([`State`]) next to the version that last saw the service come up. It is written
//! **before** each OS step, so a crash or a quit at any point leaves a state that says what is safe next. A hold is not
//! stored: it is `attempts >= MAX_ATTEMPTS` for the running version (see [`crate::plan`]).
//!
//! The lock is an OS file lock ([`paddock_proc::instance::InstanceLock`]): the kernel drops it when the holder dies, so it
//! is never stale and nothing takes it over by age. The store is a port: the file, or memory (the model test).
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use crate::error::ServiceError;
use paddock_env::VersionedFile;
use paddock_proc::{Claim, InstanceLock};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// The state file's format (1: version and attempt counter; 2: the declared [`State`]; 3: adds `registeredAt`).
pub const FORMAT: u32 = 3;

/// How far a replace has got; written before the step it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// The old registration is being removed (`ours`: it is this version's own, settled or waiting, which a replace asked by
    /// the user removes). Registered afterwards means the removal did not happen: replace, unless it is `ours` and up.
    RemovingOld {
        /// What is being removed was registered by this version.
        ours: bool,
    },
    /// `register` is about to be (or was) called. Registered afterwards means it may have landed: wait, never remove;
    /// absent means it did not: register again without counting another attempt.
    Registering,
    /// The OS accepted the registration; the service has not answered yet.
    Awaiting,
    /// `register` failed (the OS said so); nothing was registered. Retried only under the cap, never as a free finish.
    RegisterFailed(FailKind),
}

/// Why `register` failed, as far as a retry cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FailKind {
    /// The tool or the call failed; worth another try.
    Other,
    /// It was written but reading it back shows something else.
    NotAsWritten,
}

/// What the app is doing about the registration, as of the last durable write.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum State {
    /// Nothing is in progress.
    #[default]
    Idle,
    /// `version` began registering the service `attempts` times (each register call is one) and no answer has been seen
    /// since. `attempts >= MAX_ATTEMPTS` is a hold.
    Attempting {
        /// The app version that is registering.
        version: String,
        /// Register calls begun by that version.
        attempts: u32,
        /// How far the current one got.
        phase: Phase,
    },
    /// An uninstall began (the user's switch went off): finish removing, never register.
    Removing,
}

/// The remembered registration.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recorded {
    /// The app version that last saw the service come up.
    pub registered_version: Option<String>,
    /// The program it registered (when the mechanism has one).
    pub registered_program: Option<String>,
    /// "Quit and stop measuring" happened at this time (milliseconds since the Unix epoch) and the service has not been
    /// resumed since.
    #[serde(default)]
    pub resume_pending: Option<u64>,
    /// What is in progress.
    #[serde(default)]
    pub state: State,
    /// When the register call of the attempt in progress was made (microseconds since the Unix epoch, the unit of a process start
    /// time): a job that started before it is the old build's, not the one this registration started. Cleared with the attempt.
    #[serde(default)]
    pub registered_at: Option<u64>,
}

impl Recorded {
    /// Register calls `version` has begun without an answer since (0 for another version's).
    pub fn attempts_of(&self, version: &str) -> u32 {
        match &self.state {
            State::Attempting {
                version: v,
                attempts,
                ..
            } if v == version => *attempts,
            _ => 0,
        }
    }

    /// The phase of `version`'s attempt, if it has one.
    pub fn phase_of(&self, version: &str) -> Option<Phase> {
        match &self.state {
            State::Attempting {
                version: v, phase, ..
            } if v == version => Some(*phase),
            _ => None,
        }
    }
}

/// Format 1 (`registeredVersion`, `registeredProgram`, `attemptedVersion`, `attempts`) to format 2.
fn migrate_1(mut v: serde_json::Value) -> Result<serde_json::Value, String> {
    let Some(o) = v.as_object_mut() else {
        return Err("the state file is not an object".into());
    };
    let attempted = o.remove("attemptedVersion");
    let attempts = o.remove("attempts").and_then(|a| a.as_u64()).unwrap_or(0);
    if let Some(version) = attempted.filter(|a| a.is_string()) {
        o.insert(
            "state".into(),
            serde_json::json!({"kind": "attempting", "version": version, "attempts": attempts.max(1), "phase": "awaiting"}),
        );
    }
    Ok(v)
}

/// The in-memory store: the model test's "disk" (cheap, deterministic, shared between the "processes" of one run).
#[derive(Debug)]
pub struct Memory {
    value: Mutex<Recorded>,
    held: AtomicBool,
    fail_writes: AtomicBool,
    /// Writes still allowed before every write fails (negative: no limit).
    writes_left: std::sync::atomic::AtomicI64,
    /// The record is there but cannot be used (what a damaged or newer-format file is): [`RecordFile::peek`] says so until a
    /// write replaces it.
    damaged: AtomicBool,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            value: Mutex::default(),
            held: AtomicBool::new(false),
            fail_writes: AtomicBool::new(false),
            writes_left: std::sync::atomic::AtomicI64::new(-1),
            damaged: AtomicBool::new(false),
        }
    }
}

impl Memory {
    /// Makes every write fail (a full disk, a read-only folder).
    pub fn fail_writes(&self, fail: bool) {
        self.fail_writes.store(fail, Ordering::SeqCst);
    }

    /// Makes [`RecordFile::peek`] report a record that cannot be used (a damaged file), until the next write.
    pub fn set_damaged(&self, damaged: bool) {
        self.damaged.store(damaged, Ordering::SeqCst);
    }

    /// Lets `n` more writes through, then fails every write (a disk that fills up part-way).
    pub fn fail_writes_after(&self, n: i64) {
        self.writes_left.store(n, Ordering::SeqCst);
    }
}

enum Store {
    File {
        file: VersionedFile<Recorded>,
        lock: PathBuf,
        dir: PathBuf,
        label: String,
    },
    Memory(Arc<Memory>),
}

/// The state file of one service.
pub struct RecordFile {
    store: Store,
}

impl RecordFile {
    /// The record of the service `label` in `data_dir`.
    pub fn new(data_dir: &Path, label: &str) -> Self {
        Self {
            store: Store::File {
                file: VersionedFile::new(data_dir.join(format!("service-{label}.json")), FORMAT)
                    .with_migration(1, migrate_1)
                    .with_migration(2, Ok),
                lock: data_dir.join(format!("service-{label}.lock")),
                dir: data_dir.to_path_buf(),
                label: label.to_string(),
            },
        }
    }

    /// A record kept in memory.
    pub fn memory() -> Self {
        Self {
            store: Store::Memory(Arc::new(Memory::default())),
        }
    }

    /// The memory behind a [`RecordFile::memory`] record (to break it in a test).
    pub fn memory_handle(&self) -> Option<Arc<Memory>> {
        match &self.store {
            Store::Memory(m) => Some(m.clone()),
            Store::File { .. } => None,
        }
    }

    /// The same record opened again, as another process would (the file again, or the same memory).
    pub fn reopen(&self) -> RecordFile {
        match &self.store {
            Store::File { dir, label, .. } => RecordFile::new(dir, label),
            Store::Memory(m) => RecordFile {
                store: Store::Memory(m.clone()),
            },
        }
    }

    /// The remembered registration (defaults when missing or damaged; the problem is logged).
    pub fn load(&self) -> Recorded {
        match &self.store {
            Store::File { file, .. } => {
                let loaded = file.load();
                if let Some(p) = &loaded.problem {
                    log::warn!(problem:% = p; "the service state file was not used as it is");
                }
                loaded.value
            }
            Store::Memory(m) => m.value.lock().map_or_else(|e| e.into_inner().clone(), |v| v.clone()),
        }
    }

    /// Reads the record without writing anything (queries must not quarantine, restore or migrate a file: only a writer under
    /// the lock does). The flag says the file is there but cannot be used (damaged, unreadable, or written by a newer app), so
    /// the caller adopts a service that answers instead of replacing it.
    pub fn peek(&self) -> (Recorded, bool) {
        match &self.store {
            Store::Memory(m) => (self.load(), m.damaged.load(Ordering::SeqCst)),
            Store::File { file, .. } => {
                let bytes = match std::fs::read(file.path()) {
                    Ok(b) => b,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        return (Recorded::default(), false);
                    }
                    Err(_) => return (Recorded::default(), true),
                };
                let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                    return (Recorded::default(), true);
                };
                let version = value.get("version").and_then(serde_json::Value::as_u64).unwrap_or(1);
                if version > u64::from(FORMAT) {
                    return (Recorded::default(), true);
                }
                if version == 1 {
                    match migrate_1(value) {
                        Ok(v) => value = v,
                        Err(_) => return (Recorded::default(), true),
                    }
                }
                match serde_json::from_value::<Recorded>(value) {
                    Ok(r) => (r, false),
                    Err(_) => (Recorded::default(), true),
                }
            }
        }
    }

    /// Whether a write cannot succeed (a memory store that refuses writes, a file written by a newer app, which is left alone).
    /// Reads only, so a query can ask it.
    pub fn unwritable(&self) -> bool {
        match &self.store {
            Store::Memory(m) => m.fail_writes.load(Ordering::SeqCst) || m.writes_left.load(Ordering::SeqCst) == 0,
            Store::File { file, .. } => std::fs::read(file.path())
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .and_then(|v| v.get("version").and_then(serde_json::Value::as_u64))
                .is_some_and(|v| v > u64::from(FORMAT)),
        }
    }

    /// Writes the whole record.
    ///
    /// # Errors
    /// [`ServiceError::Failed`] when it could not be saved.
    pub fn store(&self, r: &Recorded) -> Result<(), ServiceError> {
        match &self.store {
            Store::File { file, .. } => file.save(r).map_err(|e| {
                ServiceError::Failed(format!("The service state could not be saved: {e}"))
            }),
            Store::Memory(m) => {
                let left = m.writes_left.load(Ordering::SeqCst);
                if left == 0 {
                    m.fail_writes.store(true, Ordering::SeqCst);
                } else if left > 0 {
                    m.writes_left.store(left - 1, Ordering::SeqCst);
                }
                if m.fail_writes.load(Ordering::SeqCst) {
                    return Err(ServiceError::Failed(
                        "The service state could not be saved: the disk refused".into(),
                    ));
                }
                *m.value.lock().unwrap_or_else(|e| e.into_inner()) = r.clone();
                m.damaged.store(false, Ordering::SeqCst);
                Ok(())
            }
        }
    }

    /// Remembers that `version` registered `program` and saw it come up: nothing is in progress any more.
    pub fn save(&self, version: &str, program: Option<&str>) -> Result<(), ServiceError> {
        let mut r = self.load();
        r.registered_version = Some(version.to_string());
        r.registered_program = program.map(str::to_string);
        r.state = State::Idle;
        r.registered_at = None;
        self.store(&r)
    }

    /// Writes `state`, keeping the rest.
    ///
    /// # Errors
    /// [`ServiceError::Failed`] when it could not be saved.
    pub fn set_state(&self, state: State) -> Result<(), ServiceError> {
        let mut r = self.load();
        r.state = state;
        self.store(&r)
    }

    /// "Quit and stop measuring" happened at this time (milliseconds since the Unix epoch) and the service was not resumed since.
    pub fn resume_pending(&self) -> Option<u64> {
        self.load().resume_pending
    }

    /// Records (or, with `None`, clears) that the service was stopped on request, keeping the rest.
    ///
    /// # Errors
    /// [`ServiceError::Failed`] when it could not be saved.
    pub fn set_resume_pending(&self, at: Option<u64>) -> Result<(), ServiceError> {
        let mut r = self.load();
        r.resume_pending = at;
        self.store(&r)
    }

    /// Writes `state` and, when `program` is known, the program this copy is about to register (ADR 0056 section 8: written
    /// with `RemovingOld`, so the next copy to look knows whose registration it sees).
    ///
    /// # Errors
    /// [`ServiceError::Failed`] when it could not be saved.
    pub fn set_state_and_program(&self, state: State, program: Option<&str>) -> Result<(), ServiceError> {
        let mut r = self.load();
        r.state = state;
        if let Some(p) = program {
            r.registered_program = Some(p.to_string());
        }
        self.store(&r)
    }

    /// Records (or clears) when the register call of the attempt in progress was made, keeping the rest.
    ///
    /// # Errors
    /// [`ServiceError::Failed`] when it could not be saved.
    pub fn set_registered_at(&self, at: Option<u64>) -> Result<(), ServiceError> {
        let mut r = self.load();
        r.registered_at = at;
        self.store(&r)
    }

    /// Forgets the registration (after an uninstall): nothing is registered, nothing is in progress.
    pub fn forget(&self) -> Result<(), ServiceError> {
        let mut r = self.load();
        r.registered_version = None;
        r.registered_program = None;
        r.state = State::Idle;
        r.registered_at = None;
        self.store(&r)
    }

    /// Takes the repair lock: `None` when another live copy holds it. The OS drops it when its holder dies; there is no
    /// age and no pid to trust (rule 53). A folder that cannot hold a lock does not stop a repair: it runs without one,
    /// and the log says so.
    pub fn lock(&self) -> Option<RepairLock> {
        match &self.store {
            Store::Memory(m) => (!m.held.swap(true, Ordering::SeqCst)).then(|| RepairLock::Memory(m.clone())),
            Store::File { lock, .. } => match InstanceLock::claim(lock) {
                Ok(Claim::Acquired(l)) => Some(RepairLock::File(l)),
                Ok(Claim::Held) => None,
                Ok(Claim::Unsupported) => {
                    log::warn!("this folder cannot hold a lock; repairing without one");
                    Some(RepairLock::None)
                }
                Err(e) => {
                    log::warn!(error:% = e; "the service repair lock could not be created; repairing without it");
                    Some(RepairLock::None)
                }
            },
        }
    }
}

/// Held while a registration changes; released on drop (and by the OS when the process ends).
#[derive(Debug)]
pub enum RepairLock {
    /// An OS lock on the lock file.
    File(InstanceLock),
    /// The memory store's flag.
    Memory(Arc<Memory>),
    /// No lock could be taken here.
    None,
}

impl Drop for RepairLock {
    fn drop(&mut self) {
        if let RepairLock::Memory(m) = self {
            m.held.store(false, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registered_version_is_remembered_and_forgotten() {
        let d = tempfile::tempdir().unwrap();
        let r = RecordFile::new(d.path(), "net.a.b");
        assert_eq!(r.load(), Recorded::default());
        r.save("1.0.4", Some("/p")).unwrap();
        let back = r.load();
        assert_eq!(back.registered_version.as_deref(), Some("1.0.4"));
        assert_eq!(back.registered_program.as_deref(), Some("/p"));
        let text = std::fs::read_to_string(d.path().join("service-net.a.b.json")).unwrap();
        assert!(text.contains("\"registeredVersion\"") && text.contains("\"version\": 2"), "{text}");
        r.forget().unwrap();
        assert_eq!(r.load(), Recorded::default());
    }

    #[test]
    fn a_damaged_or_newer_state_file_gives_defaults_and_a_newer_one_is_not_overwritten() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("service-net.a.b.json");
        std::fs::write(&path, "{ not json").unwrap();
        let r = RecordFile::new(d.path(), "net.a.b");
        assert_eq!(r.load(), Recorded::default());
        std::fs::write(&path, r#"{"version": 99, "registeredVersion": "9.0.0"}"#).unwrap();
        let r = RecordFile::new(d.path(), "net.a.b");
        assert_eq!(r.load(), Recorded::default());
        assert!(matches!(r.save("1.0.0", None), Err(ServiceError::Failed(_))));
        assert!(std::fs::read_to_string(&path).unwrap().contains("99"));
    }

    #[test]
    fn a_format_1_file_becomes_the_declared_state() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("service-net.a.b.json");
        std::fs::write(&path, r#"{"version": 1, "registeredVersion": "1.0.0", "registeredProgram": "/p"}"#).unwrap();
        let r = RecordFile::new(d.path(), "net.a.b").load();
        assert_eq!(r.registered_version.as_deref(), Some("1.0.0"));
        assert_eq!(r.state, State::Idle);
        std::fs::write(&path, r#"{"version": 1, "registeredVersion": "1.0.0", "attemptedVersion": "1.1.0", "attempts": 2}"#).unwrap();
        let r = RecordFile::new(d.path(), "net.a.b").load();
        assert_eq!(
            r.state,
            State::Attempting { version: "1.1.0".into(), attempts: 2, phase: Phase::Awaiting }
        );
        assert_eq!((r.attempts_of("1.1.0"), r.attempts_of("1.0.0")), (2, 0));
    }

    #[test]
    fn each_state_survives_a_write_and_a_read_and_a_save_clears_the_attempts() {
        let d = tempfile::tempdir().unwrap();
        let r = RecordFile::new(d.path(), "net.a.b");
        for phase in [
            Phase::RemovingOld { ours: false },
            Phase::RemovingOld { ours: true },
            Phase::Registering,
            Phase::Awaiting,
            Phase::RegisterFailed(FailKind::Other),
            Phase::RegisterFailed(FailKind::NotAsWritten),
        ] {
            let s = State::Attempting { version: "1.1.0".into(), attempts: 3, phase };
            r.set_state(s.clone()).unwrap();
            assert_eq!(r.load().state, s);
        }
        r.set_state(State::Removing).unwrap();
        assert_eq!(r.load().state, State::Removing);
        r.save("1.1.0", None).unwrap();
        assert_eq!(r.load().state, State::Idle);
    }

    #[test]
    fn two_copies_never_repair_at_once_and_the_lock_goes_with_its_holder() {
        let d = tempfile::tempdir().unwrap();
        let r = RecordFile::new(d.path(), "net.a.b");
        let held = r.lock().unwrap();
        assert!(r.reopen().lock().is_none(), "another copy holds it");
        drop(held);
        let again = r.reopen().lock();
        assert!(again.is_some(), "released with its holder, nothing to take over");
        // Garbage in the file (an old time stamp from a dead copy) means nothing: only the OS lock counts.
        drop(again);
        std::fs::write(d.path().join("service-net.a.b.lock"), "999999 1").unwrap();
        assert!(r.lock().is_some());
        // The memory store behaves the same.
        let m = RecordFile::memory();
        let held = m.lock().unwrap();
        assert!(m.reopen().lock().is_none());
        drop(held);
        assert!(m.reopen().lock().is_some());
        // A folder that cannot hold a lock does not block a repair.
        let missing = RecordFile::new(&d.path().join("lock-file"), "net.a.b");
        std::fs::write(d.path().join("lock-file"), "x").unwrap();
        assert!(missing.lock().is_some());
    }

    #[test]
    fn peeking_never_writes_and_says_when_the_file_cannot_be_used() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("service-net.a.b.json");
        let r = RecordFile::new(d.path(), "net.a.b");
        assert_eq!(r.peek(), (Recorded::default(), false));
        assert!(!path.exists());
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(r.peek(), (Recorded::default(), true));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json", "nothing quarantined or restored");
        std::fs::write(&path, r#"{"version": 99}"#).unwrap();
        assert!(r.peek().1, "a newer app's file");
        std::fs::write(&path, r#"{"version": 1, "registeredVersion": "1.0.0", "attemptedVersion": "1.1.0", "attempts": 2}"#).unwrap();
        let (rec, damaged) = r.peek();
        assert!(!damaged);
        assert_eq!(rec.attempts_of("1.1.0"), 2);
        assert!(std::fs::read_to_string(&path).unwrap().contains("attemptedVersion"), "not migrated by a query");
    }

    #[test]
    fn a_file_that_cannot_be_used_is_reported_damaged_and_a_write_clears_a_memory_flag() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("service-net.a.b.json");
        let r = RecordFile::new(d.path(), "net.a.b");
        std::fs::create_dir(&path).unwrap();
        assert!(r.peek().1, "unreadable");
        std::fs::remove_dir(&path).unwrap();
        std::fs::write(&path, r#"{"version": 2, "state": 5}"#).unwrap();
        assert!(r.peek().1, "the wrong shape");
        std::fs::write(&path, "[1]").unwrap();
        assert!(r.peek().1, "format 1 that is not an object");
        std::fs::write(&path, r#"{"version": 1}"#).unwrap();
        assert!(!r.peek().1);
        assert!(r.memory_handle().is_none());
        let m = RecordFile::memory();
        m.memory_handle().unwrap().set_damaged(true);
        assert!(m.peek().1);
        m.save("1.0.0", None).unwrap();
        assert!(!m.peek().1, "a write replaces the damaged record");
    }

    #[test]
    fn a_memory_store_that_cannot_be_written_says_so() {
        let m = RecordFile::memory();
        m.memory_handle().unwrap().fail_writes(true);
        assert!(m.save("1.0.0", None).is_err());
        assert_eq!(m.load(), Recorded::default());
    }
}
