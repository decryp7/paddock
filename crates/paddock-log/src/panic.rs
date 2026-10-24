//! The panic hook and the crash marker. A panic on any thread (UI, async runtime, worker pools, helper threads) is written
//! to the log at Error with `Severity = "Fatal"`, the thread, the location, a bounded backtrace and the enclosing span's
//! `Window` and `OperationId`; the log then attaches the latest breadcrumbs. The hook also writes a crash marker, a small
//! JSON file the next start reads to offer Restore or Safe Mode (plan X3), and runs the previous hook afterwards (so stderr
//! still gets the message in a terminal).

use crate::clock::{Clock, now_utc};
use paddock_env::write_atomic;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Longest backtrace logged with a panic (bytes, cut at a line boundary).
pub const MAX_BACKTRACE: usize = 8 * 1024;
/// Panics logged with a backtrace per run (capturing one is slow; a panic loop must not stall the app).
const MAX_BACKTRACES: u32 = 20;
/// Longest panic message kept in the marker (characters).
const MAX_MARKER_MESSAGE: usize = 300;
static PANICS: AtomicU32 = AtomicU32::new(0);

/// The marker file left by a crashed process (`crash-<process>.json`); its presence at the next start means "the last run
/// crashed". Removing it is the reader's job ([`CrashMarker::clear`]) once the user was told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashMarker {
    /// Format version of the marker.
    pub version: u32,
    /// The process that crashed.
    pub process: String,
    /// Its process id.
    pub pid: u32,
    /// When (RFC 3339, UTC).
    pub time: String,
    /// The panicking thread.
    pub thread: String,
    /// The panic message, cut; may name data, so it is for the report preview, not for display without care.
    pub message: String,
    /// `file:line` of the panic.
    pub location: String,
    /// The log file that has the full entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_file: Option<String>,
}

impl CrashMarker {
    /// Current marker format version.
    pub const VERSION: u32 = 1;

    /// Where the marker of `process` lives under the app's data folder.
    pub fn path_for(data_dir: &Path, process: &str) -> PathBuf {
        data_dir.join(format!("crash-{process}.json"))
    }

    /// The marker at `path`; a missing or damaged file gives `None` (a damaged marker is still a crash: callers that
    /// care use [`Path::exists`] too).
    pub fn read(path: &Path) -> Option<CrashMarker> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    /// Removes the marker (missing is fine).
    pub fn clear(path: &Path) -> std::io::Result<()> {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// What the hook found out about a panic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PanicFacts {
    pub thread: String,
    pub location: String,
    pub message: String,
}

impl PanicFacts {
    pub(crate) fn of(info: &std::panic::PanicHookInfo<'_>) -> Self {
        let t = std::thread::current();
        PanicFacts {
            thread: t
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("{:?}", t.id())),
            location: info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_default(),
            message: payload_message(info.payload()),
        }
    }
}

/// The text of a panic payload (`&str` or `String`; anything else is named, never printed).
pub fn payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "(a panic with a non-text payload)".to_string()
    }
}

/// `text` cut to at most `max` bytes at a line boundary, with a note of what was left out.
pub fn bounded_lines(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut cut = max;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let cut = text[..cut].rfind('\n').unwrap_or(cut);
    format!("{}\n… ({} more bytes)", &text[..cut], text.len() - cut)
}

/// Where the hook reports to.
pub(crate) struct PanicReport {
    pub process: String,
    pub marker: Option<PathBuf>,
    pub log_file: Option<PathBuf>,
    pub clock: Arc<dyn Clock>,
}

impl PanicReport {
    /// Logs the panic and writes the marker. Never panics itself.
    pub(crate) fn record(&self, facts: &PanicFacts) {
        let n = PANICS.fetch_add(1, Ordering::Relaxed);
        let backtrace = if n < MAX_BACKTRACES {
            bounded_lines(
                &std::backtrace::Backtrace::force_capture().to_string(),
                MAX_BACKTRACE,
            )
        } else {
            "(not captured: too many panics in this run)".to_string()
        };
        tracing::error!(
            Severity = "Fatal",
            Thread = facts.thread.as_str(),
            Location = facts.location.as_str(),
            Backtrace = backtrace.as_str(),
            "{} crashed on thread {}: {}",
            self.process,
            facts.thread,
            facts.message
        );
        if let Some(path) = &self.marker {
            let marker = CrashMarker {
                version: CrashMarker::VERSION,
                process: self.process.clone(),
                pid: std::process::id(),
                time: now_utc(&*self.clock).to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                thread: facts.thread.clone(),
                message: crate::redact::cap(&facts.message, MAX_MARKER_MESSAGE),
                location: facts.location.clone(),
                log_file: self.log_file.as_ref().map(|p| p.display().to_string()),
            };
            if let Ok(bytes) = serde_json::to_vec_pretty(&marker) {
                let _ = write_atomic(path, &bytes, false);
            }
        }
    }
}

/// Installs the process-wide hook (chains the previous one).
pub(crate) fn install_hook(report: PanicReport) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        report.record(&PanicFacts::of(info));
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use chrono::{TimeZone, Utc};

    #[test]
    fn marker_round_trips_and_clears() {
        let t = tempfile::tempdir().unwrap();
        let path = CrashMarker::path_for(t.path(), "app");
        assert_eq!(path, t.path().join("crash-app.json"));
        assert_eq!(CrashMarker::read(&path), None);
        CrashMarker::clear(&path).unwrap();
        let m = CrashMarker {
            version: 1,
            process: "app".into(),
            pid: 7,
            time: "t".into(),
            thread: "main".into(),
            message: "m".into(),
            location: "a.rs:1".into(),
            log_file: None,
        };
        std::fs::write(&path, serde_json::to_vec(&m).unwrap()).unwrap();
        assert_eq!(CrashMarker::read(&path), Some(m));
        CrashMarker::clear(&path).unwrap();
        assert!(!path.exists());
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(
            CrashMarker::read(&path),
            None,
            "a damaged marker reads as none; the file still exists"
        );
        assert!(path.exists());
    }

    #[test]
    fn payloads_and_backtraces_are_bounded() {
        assert_eq!(payload_message(&"boom"), "boom");
        assert_eq!(payload_message(&String::from("owned")), "owned");
        assert!(payload_message(&42u32).contains("non-text"));
        let text = "line\n".repeat(100);
        let cut = bounded_lines(&text, 52);
        assert!(cut.starts_with("line\nline") && cut.contains("more bytes"));
        assert_eq!(bounded_lines("short", 100), "short");
        let multi = "é".repeat(50);
        assert!(
            bounded_lines(&multi, 11).contains("more bytes"),
            "never cuts inside a character"
        );
    }

    #[test]
    fn recording_writes_the_marker_with_the_facts() {
        let t = tempfile::tempdir().unwrap();
        let path = CrashMarker::path_for(t.path(), "app");
        let report = PanicReport {
            process: "app".into(),
            marker: Some(path.clone()),
            log_file: Some(t.path().join("app.log")),
            clock: Arc::new(FakeClock::at_wall(
                Utc.with_ymd_and_hms(2026, 10, 20, 9, 0, 0).unwrap().into(),
            )),
        };
        report.record(&PanicFacts {
            thread: "worker-3".into(),
            location: "x.rs:9".into(),
            message: "m".repeat(1000),
        });
        let m = CrashMarker::read(&path).unwrap();
        assert_eq!(
            (m.thread.as_str(), m.location.as_str(), m.pid),
            ("worker-3", "x.rs:9", std::process::id())
        );
        assert_eq!(m.time, "2026-10-20T09:00:00.000Z");
        assert_eq!(m.message.chars().count(), MAX_MARKER_MESSAGE + 1);
        assert!(m.log_file.unwrap().ends_with("app.log"));
        // An unwritable marker location must not make the hook fail.
        PanicReport {
            marker: Some(
                t.path()
                    .join("crash-app.json")
                    .join("nested")
                    .join("x.json"),
            ),
            ..report
        }
        .record(&PanicFacts {
            thread: "t".into(),
            location: String::new(),
            message: "m".into(),
        });
    }
}
