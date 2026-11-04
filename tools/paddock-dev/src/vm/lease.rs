//! Leases: one holder per VM, held by a process identity (pid plus start time, rule 53), bounded by a patience and gone when
//! the holder is (ADR 0054 §1.5). They replace the ad hoc `plock` `vm-lock`, which a runner could skip.
//!
//! A lease is a JSON file `<state>/leases/<vm>.json` created with `create_new`, so two callers racing for one VM cannot both
//! win. A lease is stale when its holder process is gone (or its pid now belongs to another process) or its patience ran out;
//! a stale lease is replaced, a live one refuses the caller with the holder's purpose and age. A released lease moves to
//! `<state>/history/<id>.json`, so a result stamped with its id can be checked later ([`LeaseStore::issued`]).

use paddock_proc::{ProcessId, StartTime};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// How long a lease lasts when the caller does not say (ADR 0031's patience for a lifecycle run).
pub const DEFAULT_PATIENCE_SECS: u64 = 4 * 60 * 60;

/// One lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    /// `<vm>.<unix ms>.<holder pid>`: unique per issue, safe in a file name.
    pub id: String,
    /// The VM it covers.
    pub vm: String,
    /// What the caller said it is for.
    pub purpose: String,
    /// The process that holds it.
    pub holder: ProcessId,
    /// When it was issued, ms since the Unix epoch.
    pub issued_ms: i64,
    /// Seconds after which it lapses even if the holder lives.
    pub patience_secs: u64,
}

/// Where a lease stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The holder lives and the patience has not run out.
    Live,
    /// The holder process is gone.
    HolderGone,
    /// The patience ran out.
    Expired,
}

impl State {
    /// Whether the lease still protects the VM.
    pub fn is_live(self) -> bool {
        self == State::Live
    }
}

impl Lease {
    /// The lease's state at `now_ms`, given whether its holder still runs.
    pub fn state(&self, now_ms: i64, holder_alive: bool) -> State {
        if !holder_alive {
            State::HolderGone
        } else if self.age_secs(now_ms) > self.patience_secs {
            State::Expired
        } else {
            State::Live
        }
    }

    /// Seconds since it was issued (never negative).
    pub fn age_secs(&self, now_ms: i64) -> u64 {
        u64::try_from((now_ms - self.issued_ms) / 1000).unwrap_or(0)
    }

    /// The JSON written to disk.
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id, "vm": self.vm, "purpose": self.purpose,
            "holderPid": self.holder.pid(), "holderStartMicros": self.holder.started().as_micros(),
            "issuedMs": self.issued_ms, "patienceSecs": self.patience_secs,
        })
    }

    /// Parses what [`Lease::to_json`] wrote.
    pub fn from_json(v: &Value) -> Option<Lease> {
        Some(Lease {
            id: v.get("id")?.as_str()?.to_string(),
            vm: v.get("vm")?.as_str()?.to_string(),
            purpose: v.get("purpose")?.as_str()?.to_string(),
            holder: ProcessId::new(
                u32::try_from(v.get("holderPid")?.as_u64()?).ok()?,
                StartTime::from_micros(v.get("holderStartMicros")?.as_u64()?),
            ),
            issued_ms: v.get("issuedMs")?.as_i64()?,
            patience_secs: v.get("patienceSecs")?.as_u64()?,
        })
    }
}

/// Why a lease was not issued or accepted.
#[derive(Debug, PartialEq, Eq)]
pub enum LeaseError {
    /// Another live lease covers the VM.
    Held {
        /// Its purpose.
        purpose: String,
        /// Its age.
        age_secs: u64,
        /// Its holder.
        holder: ProcessId,
        /// Its id.
        id: String,
    },
    /// No lease with this id exists (never issued, or released).
    Unknown(String),
    /// The lease exists but is no longer live.
    Stale(String, State),
    /// The lease file could not be read or written.
    Io(String),
}

impl std::fmt::Display for LeaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LeaseError::Held {
                purpose,
                age_secs,
                holder,
                id,
            } => write!(f, "the VM is leased to \"{purpose}\" ({id}, {holder}, held for {age_secs} s)"),
            LeaseError::Unknown(id) => write!(f, "no lease {id} (never issued here, or already released)"),
            LeaseError::Stale(id, s) => write!(f, "lease {id} is no longer live ({s:?})"),
            LeaseError::Io(m) => write!(f, "lease store: {m}"),
        }
    }
}

/// The lease files of one broker state folder.
pub struct LeaseStore {
    leases: PathBuf,
    history: PathBuf,
}

/// A VM name is a file name here: letters, digits, `.`, `-` and `_` only (no path separators, no `..`).
pub fn valid_vm_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

impl LeaseStore {
    /// The store under `state` (the folders are made when first written).
    pub fn new(state: &Path) -> LeaseStore {
        LeaseStore {
            leases: state.join("leases"),
            history: state.join("history"),
        }
    }

    fn file(&self, vm: &str) -> PathBuf {
        self.leases.join(format!("{vm}.json"))
    }

    fn io(e: std::io::Error) -> LeaseError {
        LeaseError::Io(e.to_string())
    }

    /// The lease on `vm`, live or not.
    pub fn current(&self, vm: &str) -> Result<Option<Lease>, LeaseError> {
        match std::fs::read_to_string(self.file(vm)) {
            Ok(text) => Ok(serde_json::from_str::<Value>(&text).ok().as_ref().and_then(Lease::from_json)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Self::io(e)),
        }
    }

    /// Every lease on disk.
    pub fn all(&self) -> Vec<Lease> {
        let Ok(dir) = std::fs::read_dir(&self.leases) else {
            return Vec::new();
        };
        let mut out: Vec<Lease> = dir
            .flatten()
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .filter_map(|t| serde_json::from_str::<Value>(&t).ok())
            .filter_map(|v| Lease::from_json(&v))
            .collect();
        out.sort_by(|a, b| a.vm.cmp(&b.vm));
        out
    }

    /// Issues a lease on `vm` to `holder`. A live lease of another holder refuses; a stale one is replaced.
    pub fn acquire(
        &self,
        vm: &str,
        purpose: &str,
        holder: ProcessId,
        patience_secs: u64,
        now_ms: i64,
        alive: &dyn Fn(ProcessId) -> bool,
    ) -> Result<Lease, LeaseError> {
        if !valid_vm_name(vm) {
            return Err(LeaseError::Io(format!("\"{vm}\" is not a usable VM name")));
        }
        std::fs::create_dir_all(&self.leases).map_err(Self::io)?;
        let lease = Lease {
            id: format!("{vm}.{now_ms}.{}", holder.pid()),
            vm: vm.to_string(),
            purpose: purpose.to_string(),
            holder,
            issued_ms: now_ms,
            patience_secs,
        };
        let body = serde_json::to_string_pretty(&lease.to_json()).map_err(|e| LeaseError::Io(e.to_string()))?;
        // Two tries: the second after a stale lease was moved aside. Anything else that exists is a live holder.
        for _ in 0..2 {
            match create_new(&self.file(vm), body.as_bytes()) {
                Ok(()) => return Ok(lease),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    match self.current(vm)? {
                        Some(old) if old.state(now_ms, alive(old.holder)).is_live() => {
                            let age_secs = old.age_secs(now_ms);
                            return Err(LeaseError::Held {
                                purpose: old.purpose,
                                age_secs,
                                holder: old.holder,
                                id: old.id,
                            });
                        }
                        Some(old) => self.archive(&old).map_err(Self::io)?,
                        // Unreadable or half-written: leave it to a person rather than overwrite it.
                        None => {
                            return Err(LeaseError::Io(format!(
                                "{} exists but is not a lease; remove it by hand",
                                self.file(vm).display()
                            )));
                        }
                    }
                }
                Err(e) => return Err(Self::io(e)),
            }
        }
        Err(LeaseError::Io(
            "another caller took the lease while this one was replacing a stale one".into(),
        ))
    }

    /// The live lease `id`, or why it is not usable.
    pub fn require(&self, id: &str, now_ms: i64, alive: &dyn Fn(ProcessId) -> bool) -> Result<Lease, LeaseError> {
        let vm = vm_of(id).ok_or_else(|| LeaseError::Unknown(id.to_string()))?;
        match self.current(vm)? {
            Some(l) if l.id == id => {
                let s = l.state(now_ms, alive(l.holder));
                if s.is_live() {
                    Ok(l)
                } else {
                    Err(LeaseError::Stale(id.to_string(), s))
                }
            }
            _ => Err(LeaseError::Unknown(id.to_string())),
        }
    }

    /// Ends lease `id` (moves it to the history). Releasing one that is gone is not an error.
    pub fn release(&self, id: &str) -> Result<bool, LeaseError> {
        let Some(vm) = vm_of(id) else { return Ok(false) };
        match self.current(vm)? {
            Some(l) if l.id == id => {
                self.archive(&l).map_err(Self::io)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Whether this broker issued `id` (live or released): what a release gate checks on a result file's stamp.
    pub fn issued(&self, id: &str) -> bool {
        vm_of(id).is_some()
            && (self.history.join(format!("{id}.json")).is_file()
                || self.current(vm_of(id).unwrap_or("")).ok().flatten().is_some_and(|l| l.id == id))
    }

    fn archive(&self, l: &Lease) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.history)?;
        let dest = self.history.join(format!("{}.json", l.id));
        std::fs::write(&dest, serde_json::to_string_pretty(&l.to_json()).unwrap_or_default())?;
        match std::fs::remove_file(self.file(&l.vm)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// The VM an id belongs to: ids are `<vm>.<ms>.<pid>`, and a VM name may hold dots, so strip the last two parts.
fn vm_of(id: &str) -> Option<&str> {
    let (rest, pid) = id.rsplit_once('.')?;
    let (vm, ms) = rest.rsplit_once('.')?;
    (pid.bytes().all(|b| b.is_ascii_digit()) && ms.bytes().all(|b| b.is_ascii_digit()) && valid_vm_name(vm)).then_some(vm)
}

fn create_new(path: &Path, body: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pid(n: u32) -> ProcessId {
        ProcessId::new(n, StartTime::from_micros(u64::from(n) * 1000))
    }
    fn alive_all(_: ProcessId) -> bool {
        true
    }

    #[test]
    fn a_second_lease_on_a_live_one_is_refused_with_the_holders_purpose_and_age() {
        let dir = tempfile::tempdir().unwrap();
        let s = LeaseStore::new(dir.path());
        let first = s.acquire("win", "dyno lifecycle", pid(10), 3600, 1_000_000, &alive_all).unwrap();
        let err = s.acquire("win", "other", pid(11), 3600, 1_090_000, &alive_all).unwrap_err();
        assert_eq!(
            err,
            LeaseError::Held {
                purpose: "dyno lifecycle".into(),
                age_secs: 90,
                holder: pid(10),
                id: first.id
            }
        );
        assert!(err.to_string().contains("dyno lifecycle") && err.to_string().contains("90 s"));
    }

    #[test]
    fn different_vms_do_not_block_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let s = LeaseStore::new(dir.path());
        s.acquire("win", "a", pid(10), 3600, 0, &alive_all).unwrap();
        s.acquire("linux", "b", pid(11), 3600, 0, &alive_all).unwrap();
        assert_eq!(s.all().len(), 2);
    }

    #[test]
    fn a_lease_whose_holder_is_gone_is_replaced_and_archived() {
        let dir = tempfile::tempdir().unwrap();
        let s = LeaseStore::new(dir.path());
        let old = s.acquire("win", "a", pid(10), 3600, 0, &alive_all).unwrap();
        let new = s.acquire("win", "b", pid(11), 3600, 5000, &|p| p != pid(10)).unwrap();
        assert_eq!(new.purpose, "b");
        assert!(s.issued(&old.id), "the replaced lease stays in the history");
        assert!(matches!(s.require(&old.id, 5000, &alive_all), Err(LeaseError::Unknown(_))));
    }

    #[test]
    fn a_lease_past_its_patience_lapses_even_if_the_holder_lives() {
        let dir = tempfile::tempdir().unwrap();
        let s = LeaseStore::new(dir.path());
        let l = s.acquire("win", "a", pid(10), 60, 0, &alive_all).unwrap();
        assert_eq!(l.state(61_000, true), State::Expired);
        assert!(s.acquire("win", "b", pid(11), 60, 61_000, &alive_all).is_ok());
    }

    #[test]
    fn require_accepts_only_a_live_lease_with_that_id() {
        let dir = tempfile::tempdir().unwrap();
        let s = LeaseStore::new(dir.path());
        let l = s.acquire("win", "a", pid(10), 3600, 0, &alive_all).unwrap();
        assert!(s.require(&l.id, 1000, &alive_all).is_ok());
        assert!(matches!(
            s.require(&l.id, 1000, &|_| false),
            Err(LeaseError::Stale(_, State::HolderGone))
        ));
        assert!(matches!(s.require("win.1.2", 1000, &alive_all), Err(LeaseError::Unknown(_))));
        assert!(matches!(s.require("nonsense", 1000, &alive_all), Err(LeaseError::Unknown(_))));
    }

    #[test]
    fn release_frees_the_vm_and_keeps_the_id_checkable() {
        let dir = tempfile::tempdir().unwrap();
        let s = LeaseStore::new(dir.path());
        let l = s.acquire("win", "a", pid(10), 3600, 0, &alive_all).unwrap();
        assert!(s.release(&l.id).unwrap());
        assert!(!s.release(&l.id).unwrap(), "releasing twice is harmless");
        assert!(s.issued(&l.id) && !s.issued("win.5.6"));
        assert!(s.acquire("win", "b", pid(11), 3600, 1, &alive_all).is_ok());
    }

    #[test]
    fn vm_names_with_dots_round_trip_and_path_tricks_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let s = LeaseStore::new(dir.path());
        let l = s.acquire("linux-gnome-1.2", "a", pid(10), 3600, 7, &alive_all).unwrap();
        assert_eq!(vm_of(&l.id), Some("linux-gnome-1.2"));
        for bad in ["", "../x", "a/b", ".hidden", "a b"] {
            assert!(s.acquire(bad, "a", pid(10), 1, 0, &alive_all).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_file_that_is_not_a_lease_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let s = LeaseStore::new(dir.path());
        std::fs::create_dir_all(dir.path().join("leases")).unwrap();
        std::fs::write(dir.path().join("leases/win.json"), "garbage").unwrap();
        assert!(matches!(s.acquire("win", "a", pid(10), 1, 0, &alive_all), Err(LeaseError::Io(_))));
        assert_eq!(std::fs::read_to_string(dir.path().join("leases/win.json")).unwrap(), "garbage");
    }

    #[test]
    fn json_round_trips() {
        let l = Lease {
            id: "a.1.2".into(),
            vm: "a".into(),
            purpose: "p".into(),
            holder: pid(2),
            issued_ms: 1,
            patience_secs: 3,
        };
        assert_eq!(Lease::from_json(&l.to_json()), Some(l));
        assert_eq!(Lease::from_json(&json!({"id": 1})), None);
    }
}
