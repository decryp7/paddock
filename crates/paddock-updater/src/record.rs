//! `updates.json` in the app's config folder: the update preferences, when the app last checked (so the daily automatic
//! check survives restarts), an install that failed while quitting (shown at the next start), the last version whose
//! release notes were shown, and the last version whose services were registered (rule 24). Versioned; a missing,
//! oversized, newer or corrupt file means "nothing recorded" (defaults), never an error.

use crate::error::UpdateError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The file name in the folder the app gives.
pub const FILE: &str = "updates.json";
/// The format version written.
pub const VERSION: u32 = 1;
const MAX_BYTES: u64 = 64 * 1024;
const MAX_TEXT: usize = 2048;

/// What is kept between runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    pub version: u32,
    /// Seconds since 1970 of the last finished check.
    pub last_checked: Option<u64>,
    /// "Check for updates automatically" (off by default).
    pub auto_check: bool,
    /// The release channel's id (the app's first channel when unknown).
    pub channel: Option<String>,
    pub failed_install: Option<FailedInstall>,
    /// The last version whose release notes the user was shown (or which started for the first time).
    pub whats_new_seen: Option<String>,
    /// The last version whose services were registered successfully (rule 24: once per version change).
    pub services_version: Option<String>,
}

impl Default for Record {
    fn default() -> Self {
        Record {
            version: VERSION,
            last_checked: None,
            auto_check: false,
            channel: None,
            failed_install: None,
            whats_new_seen: None,
            services_version: None,
        }
    }
}

/// An install that failed when the app quit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedInstall {
    pub version: String,
    pub detail: String,
}

impl FailedInstall {
    pub fn of(version: &str, e: &UpdateError) -> FailedInstall {
        FailedInstall {
            version: clip(version),
            detail: clip(&e.detail().unwrap_or_else(|| e.message())),
        }
    }

    pub fn error(&self) -> UpdateError {
        UpdateError::InstallFailed {
            detail: format!("version {}: {}", self.version, self.detail),
        }
    }
}

/// The record file, read and written under one lock (the updater and About share it).
#[derive(Debug)]
pub struct RecordStore {
    path: Option<PathBuf>,
    lock: Mutex<()>,
}

impl RecordStore {
    /// The record in `dir` (`None`: kept in memory only, nothing persists).
    pub fn new(dir: Option<&Path>) -> RecordStore {
        RecordStore {
            path: dir.map(|d| d.join(FILE)),
            lock: Mutex::new(()),
        }
    }

    fn guard(&self) -> MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The record (defaults when there is none or it is unusable).
    pub fn load(&self) -> Record {
        let _g = self.guard();
        self.read()
    }

    /// Changes the record and writes it (temporary file and rename). A failed write is returned: losing it only means an
    /// extra check or notes shown again.
    pub fn update<T>(&self, change: impl FnOnce(&mut Record) -> T) -> (T, std::io::Result<()>) {
        let _g = self.guard();
        let mut rec = self.read();
        let out = change(&mut rec);
        rec.version = VERSION;
        (out, self.write(&rec))
    }

    fn read(&self) -> Record {
        let Some(path) = &self.path else {
            return Record::default();
        };
        let read = || -> Option<Record> {
            let meta = std::fs::metadata(path).ok()?;
            if !meta.is_file() || meta.len() > MAX_BYTES {
                return None;
            }
            serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
        };
        match read() {
            Some(r) if r.version <= VERSION => r,
            _ => Record::default(),
        }
    }

    fn write(&self, rec: &Record) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        let text = serde_json::to_vec_pretty(rec).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }
}

fn clip(s: &str) -> String {
    s.chars().take(MAX_TEXT).collect()
}
