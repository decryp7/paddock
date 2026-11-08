//! The audit log: one JSON line per refusal or removal (ADR 0016). Root-owned and world-readable; it holds summaries,
//! never file contents or secrets.
#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]

use crate::logfile::{open_append_no_follow, GateState, LogGate};
use dyno_core::helper_wire::{parse_audit_line, AuditEntry};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, MutexGuard, PoisonError};

/// The log is rotated once when it grows past this.
pub const MAX_LOG_BYTES: u64 = 1024 * 1024;
/// The most bytes read back from the end of the log.
const TAIL_BYTES: u64 = 256 * 1024;

/// The log file and its lock (writes from concurrent connections must not interleave).
pub struct AuditLog {
    path: PathBuf,
    max_bytes: u64,
    /// The gate and lock of the file: once quiesced (before the data folder is removed) nothing is written any more.
    state: Arc<GateState>,
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
            state: GateState::open(),
        }
    }

    fn guard(&self) -> MutexGuard<'_, ()> {
        self.state.lock.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The gate of this log (to stop its file before the data folder is removed).
    pub fn gate(&self) -> LogGate {
        LogGate(self.state.clone())
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
        // Checked under the lock: after the gate is quiesced nothing is written (the helper is being removed).
        if !self.state.armed.load(Ordering::SeqCst) {
            return Ok(());
        }
        if std::fs::metadata(&self.path).is_ok_and(|m| m.len() > self.max_bytes) {
            if let Err(e) = std::fs::rename(&self.path, self.rotated()) {
                log::warn!(error = e.to_string().as_str(); "Helper: the audit log could not be rotated; it keeps growing until it can");
            }
        }
        let line = serde_json::to_string(entry).map_err(|e| e.to_string())?;
        let mut f =
            open_append_no_follow(&self.path).map_err(|e| format!("The audit log {} cannot be written: {e}", self.path.display()))?;
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
    fn a_quiesced_audit_log_writes_nothing_more_and_a_racing_request_never_writes_after_it() {
        use std::sync::atomic::AtomicBool;
        for round in 0..30 {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("audit.jsonl");
            let log = Arc::new(AuditLog::new(path.clone()));
            log.record(&entry(0, "first", AuditVerdict::Done)).unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            struct StopOnDrop(Arc<AtomicBool>);
            impl Drop for StopOnDrop {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::SeqCst);
                }
            }
            let _stop_on_exit = StopOnDrop(stop.clone());
            let worker = {
                let (log, stop) = (log.clone(), stop.clone());
                std::thread::spawn(move || {
                    let mut at = 1;
                    while !stop.load(Ordering::SeqCst) {
                        // An audited request: it is told "recorded" whether the file took the line or not.
                        log.record(&entry(at, "racing", AuditVerdict::Refused)).unwrap();
                        at += 1;
                    }
                })
            };
            std::thread::yield_now();
            log.gate().quiesce();
            let size = std::fs::metadata(&path).map_or(0, |m| m.len());
            std::thread::sleep(std::time::Duration::from_millis(3));
            assert_eq!(
                std::fs::metadata(&path).map_or(0, |m| m.len()),
                size,
                "round {round}: nothing is written after the quiesce"
            );
            // And the folder can be removed.
            stop.store(true, Ordering::SeqCst);
            worker.join().unwrap();
            std::fs::remove_file(&path).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_audit_log_is_never_written_through_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere");
        std::fs::write(&target, b"keep").unwrap();
        let path = dir.path().join("audit.jsonl");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let log = AuditLog::new(path);
        assert!(log.record(&entry(1, "x", AuditVerdict::Done)).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
    }

    #[test]
    fn entries_are_appended_and_read_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let log = AuditLog::new(dir.path().join("audit.jsonl"));
        assert!(log.tail(5).is_empty());
        for i in 0..5 {
            log.record(&entry(i, &format!("change {i}"), AuditVerdict::Done)).unwrap();
        }
        let t = log.tail(3);
        assert_eq!(t.iter().map(|e| e.at).collect::<Vec<_>>(), [4, 3, 2]);
        assert_eq!(log.tail(100).len(), 5);
    }

    #[test]
    fn a_damaged_line_is_skipped_and_the_log_rotates_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        let log = AuditLog::with_limit(path.clone(), 100);
        log.record(&entry(1, "a", AuditVerdict::Done)).unwrap();
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
            .record(&entry(1, "x", AuditVerdict::Done))
            .unwrap_err()
            .contains("cannot be written"));
    }
}
