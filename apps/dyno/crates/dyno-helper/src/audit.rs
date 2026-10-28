//! The audit log: one JSON line per refusal or removal (ADR 0016). Root-owned and world-readable; it holds summaries,
//! never file contents or secrets.

use dyno_core::helper_wire::{parse_audit_line, AuditEntry};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The log is rotated once when it grows past this.
pub const MAX_LOG_BYTES: u64 = 1024 * 1024;
/// The most bytes read back from the end of the log.
const TAIL_BYTES: u64 = 256 * 1024;

/// The log file and its lock (writes from concurrent connections must not interleave).
pub struct AuditLog {
    path: PathBuf,
    max_bytes: u64,
    lock: Mutex<()>,
}

impl AuditLog {
    /// A log at `path`.
    pub fn new(path: PathBuf) -> AuditLog {
        AuditLog::with_limit(path, MAX_LOG_BYTES)
    }

    /// A log that rotates at `max_bytes`.
    pub fn with_limit(path: PathBuf, max_bytes: u64) -> AuditLog {
        AuditLog {
            path,
            max_bytes,
            lock: Mutex::new(()),
        }
    }

    fn guard(&self) -> MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn rotated(&self) -> PathBuf {
        let mut name = self.path.file_stem().map(|s| s.to_os_string()).unwrap_or_default();
        name.push(".1.");
        name.push(self.path.extension().unwrap_or_default());
        self.path.with_file_name(name)
    }

    /// Appends `entry`. A log that cannot be written is reported to the caller, who must then refuse to remove anything.
    pub fn record(&self, entry: &AuditEntry) -> Result<(), String> {
        let _g = self.guard();
        if std::fs::metadata(&self.path).is_ok_and(|m| m.len() > self.max_bytes) {
            let _ = std::fs::rename(&self.path, self.rotated());
        }
        let line = serde_json::to_string(entry).map_err(|e| e.to_string())?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("The audit log {} cannot be written: {e}", self.path.display()))?;
        f.write_all(line.as_bytes())
            .and_then(|_| f.write_all(b"\n"))
            .and_then(|_| f.sync_data())
            .map_err(|e| format!("The audit log {} cannot be written: {e}", self.path.display()))
    }

    /// The last `n` entries, newest first. Damaged lines are skipped.
    pub fn tail(&self, n: usize) -> Vec<AuditEntry> {
        let _g = self.guard();
        let mut entries = self.read_current();
        if entries.len() < n {
            // Fill from the rotated file.
            let mut older = read_file(&self.rotated());
            older.append(&mut entries);
            entries = older;
        }
        entries.into_iter().rev().take(n).collect()
    }

    fn read_current(&self) -> Vec<AuditEntry> {
        read_file(&self.path)
    }
}

fn read_file(path: &PathBuf) -> Vec<AuditEntry> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else { return vec![] };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let skipped = len.saturating_sub(TAIL_BYTES);
    let mut text = Vec::new();
    if f.seek(SeekFrom::Start(skipped))
        .and_then(|_| f.take(TAIL_BYTES).read_to_end(&mut text))
        .is_err()
    {
        return vec![];
    }
    let text = String::from_utf8_lossy(&text);
    let mut lines = text.lines();
    if skipped > 0 {
        // The first line may be cut in half.
        lines.next();
    }
    lines.filter_map(parse_audit_line).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::helper_wire::AuditVerdict;

    fn entry(at: i64, what: &str, verdict: AuditVerdict) -> AuditEntry {
        AuditEntry {
            at,
            peer: Some(7),
            what: what.into(),
            verdict,
            detail: String::new(),
        }
    }

    #[test]
    fn entries_are_appended_and_read_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let log = AuditLog::new(dir.path().join("audit.jsonl"));
        assert!(log.tail(5).is_empty());
        for i in 0..5 {
            log.record(&entry(i, &format!("change {i}"), AuditVerdict::Applied)).unwrap();
        }
        let t = log.tail(3);
        assert_eq!(t.iter().map(|e| e.at).collect::<Vec<_>>(), [4, 3, 2]);
        assert_eq!(log.tail(100).len(), 5);
    }

    #[test]
    fn a_damaged_line_is_skipped_and_the_log_rotates_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        let log = AuditLog::with_limit(path.clone(), 150);
        log.record(&entry(1, "a", AuditVerdict::Applied)).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{garbage\n")
            .unwrap();
        log.record(&entry(2, "b", AuditVerdict::Refused)).unwrap();
        assert_eq!(log.tail(10).iter().map(|e| e.at).collect::<Vec<_>>(), [2, 1]);
        // The file is now over the limit: the next record starts a new file and keeps the old one.
        log.record(&entry(3, "c", AuditVerdict::Failed)).unwrap();
        assert!(dir.path().join("audit.1.jsonl").exists());
        assert_eq!(log.tail(10).iter().map(|e| e.at).collect::<Vec<_>>(), [3, 2, 1]);
    }

    #[test]
    fn an_unwritable_log_is_an_error_for_the_caller() {
        let dir = tempfile::tempdir().unwrap();
        let log = AuditLog::new(dir.path().join("no-such-folder").join("audit.jsonl"));
        assert!(log
            .record(&entry(1, "x", AuditVerdict::Applied))
            .unwrap_err()
            .contains("cannot be written"));
    }
}
