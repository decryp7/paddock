//! The helper's own log (ADR 0027 §9): one line per record in `helper.log` in the data folder (0600, rotated once at a
//! size), and [`tail`], which `LogTail` answers with for the agent's diagnostics bundle. Messages are the helper's own words;
//! user content never goes in (callers log accounts and counts, not paths or names).

use log::{Level, Log, Metadata, Record};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// The log is rotated once (to `helper.log.1`) when it grows past this.
pub const MAX_LOG_BYTES: u64 = 1024 * 1024;
/// The most bytes read back from the end of the log.
const TAIL_WINDOW: u64 = 128 * 1024;
/// The longest line `tail` returns.
const MAX_LINE: usize = 400;

/// Opens `path` for appending, creating it (0600 on Unix), never through a link: `O_NOFOLLOW` / `FILE_FLAG_OPEN_REPARSE_POINT`,
/// and the result must be a regular file. Used by the helper log and the audit log.
pub(crate) fn open_append_no_follow(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
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
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the file is not a regular file",
        ));
    }
    Ok(file)
}

/// What the log and its gate share: whether the file may be written, and the lock an append holds while it writes.
#[derive(Debug)]
pub(crate) struct GateState {
    /// Until armed (and again once quiesced) the log writes to standard error and touches no file.
    pub(crate) armed: AtomicBool,
    /// Whether the file has been armed at least once: the early sink only hears the lines of the start, before that.
    pub(crate) ever_armed: AtomicBool,
    pub(crate) lock: Mutex<()>,
}

impl GateState {
    pub(crate) fn open() -> Arc<GateState> {
        Arc::new(GateState {
            armed: AtomicBool::new(true),
            ever_armed: AtomicBool::new(true),
            lock: Mutex::new(()),
        })
    }
}

/// Lets the deferred log open its file once the data folder is verified, and stops it before the folder is removed.
#[derive(Debug, Clone)]
pub struct LogGate(pub(crate) Arc<GateState>);

impl LogGate {
    /// From now on the log is written to its file.
    pub fn arm(&self) {
        self.0.ever_armed.store(true, Ordering::SeqCst);
        self.0.armed.store(true, Ordering::SeqCst);
    }

    /// Stops the file: lines go to standard error from now on, and when this returns no append is writing or about to write
    /// (one in flight has finished). Used before the data folder is removed, so a line from another thread cannot recreate
    /// `helper.log` between the unlink and the folder's removal.
    pub fn quiesce(&self) {
        self.0.armed.store(false, Ordering::SeqCst);
        drop(self.0.lock.lock().unwrap_or_else(|p| p.into_inner()));
    }
}

/// The gate of the installed logger, for code that removes the data folder.
static INSTALLED: OnceLock<LogGate> = OnceLock::new();

/// The gates of the other files the helper writes in the data folder (the audit log), registered by the daemon.
static EXTRA: Mutex<Vec<LogGate>> = Mutex::new(Vec::new());

/// Adds a gate that [`quiesce_installed`] stops too.
pub fn register_gate(gate: LogGate) {
    EXTRA.lock().unwrap_or_else(|p| p.into_inner()).push(gate);
}

/// Stops the installed logger's file (see [`LogGate::quiesce`]); nothing to do when no file logger is installed.
pub fn quiesce_installed() {
    if let Some(gate) = INSTALLED.get() {
        gate.quiesce();
    }
    for gate in EXTRA.lock().unwrap_or_else(|p| p.into_inner()).iter() {
        gate.quiesce();
    }
}

/// A [`log::Log`] that appends to a file.
pub struct HelperLog {
    path: PathBuf,
    /// Until armed the log writes to standard error and touches no file: the data folder is not yet known to be protected.
    state: Arc<GateState>,
    max_bytes: u64,
    now: fn() -> i64,
    /// Hears every line written before the file is first armed (the Windows Event Log: the only place those lines can go that
    /// is not the unverified data folder, and a service has no console).
    early: Option<EarlySink>,
}

/// Where the lines of a start go before the data folder is verified (besides standard error): the level and the line.
pub type EarlySink = Arc<dyn Fn(Level, &str) + Send + Sync>;

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
            state: GateState::open(),
            max_bytes: MAX_LOG_BYTES,
            now: unix_now,
            early: None,
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
        self.state.armed.store(false, Ordering::SeqCst);
        self.state.ever_armed.store(false, Ordering::SeqCst);
        self
    }

    /// Also hands every line written before the file is first armed to `sink` (never a file in the unverified folder).
    #[must_use]
    pub fn with_early_sink(mut self, sink: EarlySink) -> Self {
        self.early = Some(sink);
        self
    }

    /// The gate of this log (to stop its file before the data folder is removed).
    pub fn gate(&self) -> LogGate {
        LogGate(self.state.clone())
    }

    /// Makes this the process's logger at Info level. A second install is ignored. The gate arms the file.
    pub fn install(self) -> LogGate {
        let gate = LogGate(self.state.clone());
        if log::set_boxed_logger(Box::new(self)).is_ok() {
            log::set_max_level(log::LevelFilter::Info);
            let _ = INSTALLED.set(gate.clone());
        }
        gate
    }

    fn append(&self, line: &str) -> std::io::Result<()> {
        let _guard = self.state.lock.lock().unwrap_or_else(|p| p.into_inner());
        // Checked under the lock: after `quiesce` returns, nothing is written (a line that raced it is dropped).
        if !self.state.armed.load(Ordering::SeqCst) {
            return Ok(());
        }
        if std::fs::metadata(&self.path).is_ok_and(|m| m.len() >= self.max_bytes) {
            let mut old = self.path.clone().into_os_string();
            old.push(".1");
            let _ = std::fs::rename(&self.path, old);
        }
        let mut file = open_append_no_follow(&self.path)?;
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
        if !self.state.armed.load(Ordering::SeqCst) {
            eprintln!("{line}");
            if let Some(sink) = self.early.as_ref().filter(|_| !self.state.ever_armed.load(Ordering::SeqCst)) {
                sink(record.level(), &line);
            }
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
    fn a_quiesced_log_writes_nothing_more_to_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("helper.log");
        let log = HelperLog::new(path.clone()).with(1000, || 7);
        let write = |text: &str| log.log(&Record::builder().args(format_args!("{text}")).level(Level::Info).build());
        write("before");
        log.gate().quiesce();
        std::fs::remove_file(&path).unwrap();
        write("after");
        assert!(!path.exists(), "no line recreates the file once quiesced");
        log.gate().arm();
        write("armed again");
        assert!(std::fs::read_to_string(&path).unwrap().contains("armed again"));
        // No logger installed in a test run: nothing to stop, nothing breaks.
        quiesce_installed();
    }

    #[test]
    fn a_deferred_log_touches_no_file_until_it_is_armed_and_never_follows_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("helper.log");
        let log = HelperLog::new(path.clone()).with(1000, || 7).deferred();
        let gate = LogGate(log.state.clone());
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
    fn the_early_sink_hears_the_lines_of_the_start_and_no_file_is_touched_until_the_gate_arms() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("helper.log");
        let heard = Arc::new(Mutex::new(Vec::<(Level, String)>::new()));
        let h = heard.clone();
        let log = HelperLog::new(path.clone())
            .with(1000, || 7)
            .deferred()
            .with_early_sink(Arc::new(move |level, line| h.lock().unwrap().push((level, line.to_string()))));
        let gate = log.gate();
        let write = |level, text: &str| log.log(&Record::builder().args(format_args!("{text}")).level(level).build());
        write(Level::Info, "phase one");
        write(Level::Error, "refused");
        assert!(!path.exists(), "the unverified folder is not written");
        assert_eq!(
            *heard.lock().unwrap(),
            [
                (Level::Info, "7 INFO phase one".to_string()),
                (Level::Error, "7 ERROR refused".to_string())
            ]
        );
        gate.arm();
        write(Level::Info, "serving");
        gate.quiesce();
        write(Level::Info, "removing");
        assert_eq!(
            heard.lock().unwrap().len(),
            2,
            "after the folder is verified the file takes the lines, not the sink"
        );
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
