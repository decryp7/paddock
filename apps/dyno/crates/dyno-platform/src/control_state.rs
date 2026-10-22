//! What Dyno applied to processes that outlives Dyno (CPU caps, suspensions), persisted in a small JSON file so a
//! later run's `release_all` can undo what a crashed run left behind. A missing or corrupt file is an empty state.

use dyno_core::process::{Pid, ProcessRef};
use dyno_core::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file format version.
pub const STATE_VERSION: u32 = 1;
/// Most entries kept (bounded against runaway growth).
pub const MAX_ENTRIES: usize = 4096;

/// One process Dyno changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    /// Process id.
    pub pid: Pid,
    /// Its start time (ms).
    pub started: i64,
    /// Dyno suspended it.
    #[serde(default)]
    pub suspended: bool,
    /// Dyno capped its CPU.
    #[serde(default)]
    pub capped: bool,
}

impl Applied {
    /// The process.
    pub fn target(&self) -> ProcessRef {
        ProcessRef {
            pid: self.pid,
            started: Timestamp(self.started),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StateFileBody {
    version: u32,
    #[serde(default)]
    applied: Vec<Applied>,
}

/// The in-memory state and where it is saved.
#[derive(Debug, Default)]
pub struct ControlState {
    path: Option<PathBuf>,
    entries: Vec<Applied>,
}

impl ControlState {
    /// An unsaved state (tests, or no writable location).
    pub fn in_memory() -> Self {
        ControlState::default()
    }

    /// Loads the state at `path` (empty when missing, unreadable, corrupt or of another version).
    pub fn load(path: PathBuf) -> Self {
        let entries = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<StateFileBody>(&b).ok())
            .filter(|b| b.version == STATE_VERSION)
            .map(|b| b.applied)
            .unwrap_or_default();
        ControlState {
            path: Some(path),
            entries,
        }
    }

    /// Every entry.
    pub fn entries(&self) -> &[Applied] {
        &self.entries
    }

    /// Updates the entry for `target` with `f` (creating it), drops it when nothing is applied any more, and saves.
    pub fn update(&mut self, target: ProcessRef, f: impl FnOnce(&mut Applied)) {
        let idx = match self.entries.iter().position(|e| e.target() == target) {
            Some(i) => i,
            None => {
                self.entries.push(Applied {
                    pid: target.pid,
                    started: target.started.0,
                    suspended: false,
                    capped: false,
                });
                self.entries.len() - 1
            }
        };
        f(&mut self.entries[idx]);
        if !self.entries[idx].suspended && !self.entries[idx].capped {
            self.entries.remove(idx);
        }
        if self.entries.len() > MAX_ENTRIES {
            let extra = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..extra);
        }
        self.save();
    }

    /// Removes every entry and saves.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.save();
    }

    /// Writes the file (temp file + rename); failures are ignored: the state is a best-effort safety net.
    fn save(&self) {
        let Some(path) = &self.path else { return };
        let body = StateFileBody {
            version: STATE_VERSION,
            applied: self.entries.clone(),
        };
        let Ok(bytes) = serde_json::to_vec_pretty(&body) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = tmp_path(path);
        if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(pid: Pid) -> ProcessRef {
        ProcessRef {
            pid,
            started: Timestamp(i64::from(pid) * 1000),
        }
    }

    #[test]
    fn round_trips_and_drops_empty_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("state.json");
        let mut s = ControlState::load(path.clone());
        assert!(s.entries().is_empty());
        s.update(r(1), |a| a.capped = true);
        s.update(r(2), |a| a.suspended = true);
        s.update(r(1), |a| a.suspended = true);
        let back = ControlState::load(path.clone());
        assert_eq!(back.entries().len(), 2);
        assert!(back.entries()[0].capped && back.entries()[0].suspended);
        assert_eq!(back.entries()[0].target(), r(1));
        s.update(r(2), |a| a.suspended = false);
        assert_eq!(ControlState::load(path.clone()).entries().len(), 1);
        s.clear();
        assert!(ControlState::load(path).entries().is_empty());
    }

    #[test]
    fn corrupt_or_foreign_files_are_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, b"{not json").unwrap();
        assert!(ControlState::load(path.clone()).entries().is_empty());
        std::fs::write(&path, br#"{"version":99,"applied":[{"pid":1,"started":1,"capped":true}]}"#).unwrap();
        assert!(ControlState::load(path).entries().is_empty());
        let mut mem = ControlState::in_memory();
        mem.update(r(3), |a| a.capped = true);
        assert_eq!(mem.entries().len(), 1);
    }

    #[test]
    fn stays_bounded() {
        let mut s = ControlState::in_memory();
        for pid in 0..(MAX_ENTRIES as u32 + 10) {
            s.update(r(pid), |a| a.capped = true);
        }
        assert_eq!(s.entries().len(), MAX_ENTRIES);
        assert_eq!(s.entries()[0].pid, 10);
    }
}
