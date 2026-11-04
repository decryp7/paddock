//! The helper's own log (ADR 0027 §9): one line per record in `helper.log` in the data folder (0600, rotated once at a
//! size), and [`tail`], which `LogTail` answers with for the agent's diagnostics bundle. Messages are the helper's own words;
//! user content never goes in (callers log accounts and counts, not paths or names).

use log::{Level, Log, Metadata, Record};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// The log is rotated once (to `helper.log.1`) when it grows past this.
pub const MAX_LOG_BYTES: u64 = 1024 * 1024;
/// The most bytes read back from the end of the log.
const TAIL_WINDOW: u64 = 128 * 1024;
/// The longest line `tail` returns.
const MAX_LINE: usize = 400;

/// Lets the deferred log open its file once the data folder is verified.
#[derive(Debug, Clone)]
pub struct LogGate(Arc<AtomicBool>);

impl LogGate {
    /// From now on the log is written to its file.
    pub fn arm(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// A [`log::Log`] that appends to a file.
pub struct HelperLog {
    path: PathBuf,
    /// Until armed the log writes to standard error and touches no file: the data folder is not yet known to be protected.
    armed: Arc<AtomicBool>,
    max_bytes: u64,
    now: fn() -> i64,
    lock: Mutex<()>,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

struct Pairs(String);

impl<'kvs> log::kv::VisitSource<'kvs> for Pairs {
    fn visit_pair(&mut self, key: log::kv::Key<'kvs>, value: log::kv::Value<'kvs>) -> Result<(), log::kv::Error> {
        self.0.push_str(&format!(" {key}={value}"));
        Ok(())
    }
}

/// One line of the log.
pub fn format_line(at: i64, level: Level, message: &str, pairs: &str) -> String {
    let flat: String = message.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    format!("{at} {level} {flat}{pairs}")
}

impl HelperLog {
    /// A log at `path`.
    pub fn new(path: PathBuf) -> Self {
        HelperLog {
            path,
            armed: Arc::new(AtomicBool::new(true)),
            max_bytes: MAX_LOG_BYTES,
            now: unix_now,
            lock: Mutex::new(()),
        }
    }

    /// Another size and clock (tests).
    #[must_use]
    pub fn with(mut self, max_bytes: u64, now: fn() -> i64) -> Self {
        self.max_bytes = max_bytes;
        self.now = now;
        self
    }

    /// A log that writes to standard error only, until its [`LogGate`] is armed (the data folder is verified first; no file
    /// in it is opened, created or rotated before that).
    #[must_use]
    pub fn deferred(self) -> Self {
        self.armed.store(false, Ordering::SeqCst);
        self
    }

    /// Makes this the process's logger at Info level. A second install is ignored. The gate arms the file.
    pub fn install(self) -> LogGate {
        let gate = LogGate(self.armed.clone());
        if log::set_boxed_logger(Box::new(self)).is_ok() {
            log::set_max_level(log::LevelFilter::Info);
        }
        gate
    }

    fn append(&self, line: &str) -> std::io::Result<()> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        if std::fs::metadata(&self.path).is_ok_and(|m| m.len() >= self.max_bytes) {
            let mut old = self.path.clone().into_os_string();
            old.push(".1");
            let _ = std::fs::rename(&self.path, old);
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        // Never through a link: the log is opened without following one and must be a regular file.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
            options.custom_flags(OPEN_REPARSE_POINT);
        }
        let mut file = options.open(&self.path)?;
        if !file.metadata()?.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the log is not a regular file",
            ));
        }
        writeln!(file, "{line}")
    }
}

impl Log for HelperLog {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mut pairs = Pairs(String::new());
        let _ = record.key_values().visit(&mut pairs);
        let line = format_line((self.now)(), record.level(), &record.args().to_string(), &pairs.0);
        if !self.armed.load(Ordering::SeqCst) {
            eprintln!("{line}");
            return;
        }
        // A log that cannot be written must not take the daemon down.
        let _ = self.append(&line);
    }

    fn flush(&self) {}
}

/// The last `n` lines of the log at `path`, oldest first, each cut at 400 characters. A missing log is no lines.
pub fn tail(path: &Path, n: usize) -> Vec<String> {
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let len = file.metadata().map_or(0, |m| m.len());
    let start = len.saturating_sub(TAIL_WINDOW);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut bytes = Vec::new();
    if file.take(TAIL_WINDOW).read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.lines().collect();
    // The window can start in the middle of a line.
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    let from = lines.len().saturating_sub(n);
    lines[from..].iter().map(|l| l.chars().take(MAX_LINE).collect()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deferred_log_touches_no_file_until_it_is_armed_and_never_follows_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("helper.log");
        let log = HelperLog::new(path.clone()).with(1000, || 7).deferred();
        let gate = LogGate(log.armed.clone());
        let write = |log: &HelperLog, text: &str| {
            log.log(&Record::builder().args(format_args!("{text}")).level(Level::Info).build());
        };
        write(&log, "before");
        assert!(
            !path.exists() && std::fs::read_dir(dir.path()).unwrap().count() == 0,
            "nothing is created before the gate"
        );
        gate.arm();
        write(&log, "after");
        assert!(std::fs::read_to_string(&path).unwrap().contains("after"));
        #[cfg(unix)]
        {
            let target = dir.path().join("elsewhere");
            std::fs::write(&target, b"keep").unwrap();
            let linked = dir.path().join("linked.log");
            std::os::unix::fs::symlink(&target, &linked).unwrap();
            let l2 = HelperLog::new(linked).with(1000, || 7);
            write(&l2, "must not land in the target");
            assert_eq!(std::fs::read(&target).unwrap(), b"keep");
        }
    }

    #[test]
    fn lines_are_one_line_each_and_keep_their_pairs() {
        assert_eq!(format_line(5, Level::Info, "a\nb", " k=1"), "5 INFO a b k=1");
    }

    #[test]
    fn the_log_appends_rotates_and_is_read_back_from_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("helper.log");
        let log = HelperLog::new(path.clone()).with(120, || 7);
        for i in 0..10 {
            log.log(
                &Record::builder()
                    .args(format_args!("line {i}"))
                    .level(Level::Info)
                    .key_values(&[("n", i)] as &dyn log::kv::Source)
                    .build(),
            );
        }
        // Debug is not written.
        log.log(&Record::builder().args(format_args!("noise")).level(Level::Debug).build());
        let last = tail(&path, 3);
        assert!(last.last().unwrap().ends_with("line 9 n=9"), "{last:?}");
        assert!(dir.path().join("helper.log.1").exists(), "rotated once at the size");
        assert!(tail(&path, 100).iter().all(|l| !l.contains("noise")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        log.flush();
    }

    #[test]
    fn the_tail_is_bounded_and_a_missing_log_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(tail(&dir.path().join("none"), 5).is_empty());
        let path = dir.path().join("big.log");
        let line = "x".repeat(1000);
        std::fs::write(&path, (0..400).map(|i| format!("{i} {line}\n")).collect::<String>()).unwrap();
        let t = tail(&path, 2);
        assert_eq!(t.len(), 2);
        assert!(t[1].starts_with("399 ") && t[1].chars().count() == 400);
        assert!(tail(&path, 100_000).len() < 400, "only the window is read");
        // A unreadable path (a folder) gives nothing.
        assert!(tail(dir.path(), 5).is_empty());
    }

    #[test]
    fn a_log_that_cannot_be_written_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let log = HelperLog::new(dir.path().join("missing").join("h.log"));
        log.log(&Record::builder().args(format_args!("x")).level(Level::Info).build());
        assert!(log.enabled(&Metadata::builder().level(Level::Warn).build()));
        assert!(!log.enabled(&Metadata::builder().level(Level::Trace).build()));
    }
}
