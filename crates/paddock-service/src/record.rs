//! What this app remembers about the registration (`service-<label>.json` in its data folder, a `paddock-env`
//! [`VersionedFile`]: atomic, damaged or missing gives defaults, a newer format is left alone): the version that
//! registered it last (rule 24's "once per version change", catalogue C21's `service_registered_version`), and a lock
//! so two copies of the app never repair at the same time.
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
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The state file's format.
pub const FORMAT: u32 = 1;
/// A lock older than this is from a copy that died while repairing.
pub const LOCK_STALE_AFTER: Duration = Duration::from_secs(600);

/// The remembered registration.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recorded {
    /// The app version that registered it last.
    pub registered_version: Option<String>,
    /// The program it registered (when the mechanism has one).
    pub registered_program: Option<String>,
    /// The app version that last registered it without the service answering afterwards (the OS accepted the
    /// registration; the service did not come up in time). Optional, so format 1 files without it still load.
    #[serde(default)]
    pub attempted_version: Option<String>,
    /// How many times `attempted_version` registered it that way.
    #[serde(default)]
    pub attempts: u32,
}

/// The state file of one service.
pub struct RecordFile {
    file: VersionedFile<Recorded>,
    lock: PathBuf,
}

impl RecordFile {
    /// The record of the service `label` in `data_dir`.
    pub fn new(data_dir: &Path, label: &str) -> Self {
        Self {
            file: VersionedFile::new(data_dir.join(format!("service-{label}.json")), FORMAT),
            lock: data_dir.join(format!("service-{label}.lock")),
        }
    }

    /// The remembered registration (defaults when missing or damaged; the problem is logged).
    pub fn load(&self) -> Recorded {
        let loaded = self.file.load();
        if let Some(p) = &loaded.problem {
            log::warn!(problem:% = p; "the service state file was not used as it is");
        }
        loaded.value
    }

    /// Remembers that `version` registered `program`.
    pub fn save(&self, version: &str, program: Option<&str>) -> Result<(), ServiceError> {
        self.write(&Recorded {
            registered_version: Some(version.to_string()),
            registered_program: program.map(str::to_string),
            ..Recorded::default()
        })
    }

    /// Remembers that `version` registered the service but it did not answer (the registered version stays as it was).
    /// Returns how many times that now is for this version.
    pub fn attempt(&self, version: &str) -> Result<u32, ServiceError> {
        let mut now = self.load();
        let count = if now.attempted_version.as_deref() == Some(version) {
            now.attempts.saturating_add(1)
        } else {
            1
        };
        now.attempted_version = Some(version.to_string());
        now.attempts = count;
        self.write(&now)?;
        Ok(count)
    }

    /// Forgets the registration (after an uninstall).
    pub fn forget(&self) -> Result<(), ServiceError> {
        self.write(&Recorded::default())
    }

    fn write(&self, r: &Recorded) -> Result<(), ServiceError> {
        self.file
            .save(r)
            .map_err(|e| ServiceError::Failed(format!("The service state could not be saved: {e}")))
    }

    /// Takes the repair lock: `None` when another live copy holds it. A lock older than [`LOCK_STALE_AFTER`] (by the
    /// time written in it, `now_secs` being seconds since the Unix epoch) is taken over.
    pub fn lock(&self, now_secs: u64) -> Option<RepairLock> {
        for _ in 0..2 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&self.lock)
            {
                Ok(mut f) => {
                    if let Err(e) = write!(f, "{} {now_secs}", std::process::id()) {
                        // The lock file exists, so the lock is held; without its time it is only taken over as stale.
                        log::warn!(error:% = e; "the service repair lock's time could not be written");
                    }
                    return Some(RepairLock {
                        path: self.lock.clone(),
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let since = std::fs::read_to_string(&self.lock)
                        .ok()
                        .and_then(|t| t.split_whitespace().nth(1)?.parse::<u64>().ok());
                    let stale = since
                        .is_none_or(|s| now_secs.saturating_sub(s) > LOCK_STALE_AFTER.as_secs());
                    if !stale {
                        return None;
                    }
                    paddock_proc::cleanup::remove_file(&self.lock);
                }
                Err(e) => {
                    // A folder that cannot hold the lock must not stop a repair: run without it.
                    log::warn!(error:% = e; "the service repair lock could not be created; repairing without it");
                    return Some(RepairLock {
                        path: PathBuf::new(),
                    });
                }
            }
        }
        None
    }
}

/// Held while a registration changes; removed on drop.
#[derive(Debug)]
pub struct RepairLock {
    path: PathBuf,
}

impl Drop for RepairLock {
    fn drop(&mut self) {
        if !self.path.as_os_str().is_empty() {
            paddock_proc::cleanup::remove_file(&self.path);
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
        assert!(
            text.contains("\"registeredVersion\"") && text.contains("\"version\""),
            "{text}"
        );
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
        assert!(matches!(
            r.save("1.0.0", None),
            Err(ServiceError::Failed(_))
        ));
        assert!(std::fs::read_to_string(&path).unwrap().contains("99"));
    }

    #[test]
    fn two_copies_never_repair_at_once_and_a_dead_copys_lock_is_taken_over() {
        let d = tempfile::tempdir().unwrap();
        let r = RecordFile::new(d.path(), "net.a.b");
        let held = r.lock(1000).unwrap();
        assert!(r.lock(1000 + 60).is_none(), "another copy holds it");
        let taken = r.lock(1000 + LOCK_STALE_AFTER.as_secs() + 1);
        assert!(taken.is_some(), "a stale lock is taken over");
        drop(taken);
        drop(held);
        assert!(!d.path().join("service-net.a.b.lock").exists());
        std::fs::write(d.path().join("service-net.a.b.lock"), "garbage").unwrap();
        assert!(r.lock(5).is_some(), "an unreadable lock is stale");
        // A data folder that cannot hold the lock does not block the repair.
        let missing = RecordFile::new(&d.path().join("no/such/folder"), "net.a.b");
        assert!(missing.lock(5).is_some());
    }

    #[test]
    fn an_attempt_is_counted_per_version_and_cleared_by_a_success() {
        let d = tempfile::tempdir().unwrap();
        let r = RecordFile::new(d.path(), "net.a.b");
        r.save("1.0.0", None).unwrap();
        assert_eq!(r.attempt("1.1.0").unwrap(), 1);
        assert_eq!(r.attempt("1.1.0").unwrap(), 2);
        let now = r.load();
        assert_eq!(
            now.registered_version.as_deref(),
            Some("1.0.0"),
            "the registered version is not touched"
        );
        assert_eq!(
            (now.attempted_version.as_deref(), now.attempts),
            (Some("1.1.0"), 2)
        );
        assert_eq!(
            r.attempt("1.2.0").unwrap(),
            1,
            "another version starts again"
        );
        r.save("1.2.0", None).unwrap();
        assert_eq!((r.load().attempted_version, r.load().attempts), (None, 0));
        // A format-1 file written before the field existed still loads.
        std::fs::write(
            d.path().join("service-net.a.b.json"),
            r#"{"version": 1, "registeredVersion": "1.0.0"}"#,
        )
        .unwrap();
        assert_eq!(r.load().registered_version.as_deref(), Some("1.0.0"));
    }
}
