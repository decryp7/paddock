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
    /// Hears every line written before the file is first armed (the OS log: the only place those lines can go that is not the
    /// unverified data folder, and a service has no console), and says once when the file cannot be written.
    early: Option<EarlySink>,
    /// The failure to write the file has been reported to the sink.
    write_failure_reported: AtomicBool,
    /// Standard error (tests collect the lines).
    stderr: Box<dyn Fn(&str) + Send + Sync>,
}

/// Where the lines of a start go before the data folder is verified (besides standard error): the level and the line. The
/// service's OS log (`paddock_service::service_main::Context::early_sink`).
pub type EarlySink = paddock_service::service_main::EarlySink;

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
            write_failure_reported: AtomicBool::new(false),
            stderr: Box::new(|line| eprintln!("{line}")),
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

    /// Another standard error (tests).
    #[must_use]
    pub fn with_stderr(mut self, stderr: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.stderr = Box::new(stderr);
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
            if INSTALLED.set(gate.clone()).is_err() {
                log::warn!("Helper: the log gate was already registered; the first one stays");
            }
        }
        gate
    }

    fn report_failure(&self, what: &str, error: &std::io::Error) {
        if self.write_failure_reported.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(sink) = &self.early {
            sink(
                Level::Warn,
                &format!(
                    "{} {what} ({error}); lines the file cannot take go to standard error.",
                    self.path
                        .file_name()
                        .map_or_else(|| "the log file".into(), |n| n.to_string_lossy().into_owned())
                ),
            );
        }
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
            // Rotation is housekeeping: a failure is told once and the line is still appended.
            if let Err(e) = std::fs::rename(&self.path, old) {
                self.report_failure("could not be rotated", &e);
            }
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
        if record.key_values().visit(&mut pairs).is_err() {
            pairs.0.push_str(" (some fields could not be read)");
        }
        let line = format_line((self.now)(), record.level(), &record.args().to_string(), &pairs.0);
        if !self.state.armed.load(Ordering::SeqCst) {
            // Before the data folder is trusted a line is written once: to the OS log when there is one (it falls back to
            // standard error itself, and under journald standard error is the journal), else to standard error.
            match self.early.as_ref().filter(|_| !self.state.ever_armed.load(Ordering::SeqCst)) {
                Some(sink) => sink(record.level(), &line),
                None => (self.stderr)(&line),
            }
            return;
        }
        // A log that cannot be written must not take the daemon down: the line goes to standard error and the first failure
        // is told to the OS log, so the reason the file is empty can be found.
        if let Err(e) = self.append(&line) {
            (self.stderr)(&line);
            self.report_failure("could not be written", &e);
        }
    }

    fn flush(&self) {}
}

/// The last `n` lines of the log at `path`, oldest first, each cut at 400 characters. A missing log is no lines. When the log was
/// rotated a moment ago and holds fewer than `n` lines, the end of the rotated file (`<path>.1`) comes first, so a report taken
/// right after a rotation still shows what led up to it.
pub fn tail(path: &Path, n: usize) -> Vec<String> {
    let mut lines = tail_of_file(path, n);
    if lines.len() < n {
        let mut rotated = path.as_os_str().to_owned();
        rotated.push(".1");
        let mut older = tail_of_file(Path::new(&rotated), n - lines.len());
        older.append(&mut lines);
        return older;
    }
    lines
}

fn tail_of_file(path: &Path, n: usize) -> Vec<String> {
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
    fn a_log_file_that_cannot_be_written_is_reported_once_to_the_os_log_and_the_line_goes_to_standard_error() {
        let dir = tempfile::tempdir().unwrap();
        // The folder of the log does not exist: every append fails.
        let path = dir.path().join("missing").join("helper.log");
        let heard = Arc::new(Mutex::new(Vec::<(Level, String)>::new()));
        let h = heard.clone();
        let log = HelperLog::new(path)
            .with(1000, || 7)
            .with_early_sink(Arc::new(move |level, line| h.lock().unwrap().push((level, line.to_string()))));
        let write = |text: &str| log.log(&Record::builder().args(format_args!("{text}")).level(Level::Info).build());
        write("first");
        write("second");
        let heard = heard.lock().unwrap();
        assert_eq!(heard.len(), 1, "once, not per line: {heard:?}");
        assert_eq!(heard[0].0, Level::Warn);
        assert!(
            heard[0].1.contains("helper.log could not be written") && heard[0].1.contains("standard error"),
            "{heard:?}"
        );
    }

    #[test]
    fn a_line_before_the_data_folder_is_trusted_is_written_once_to_the_os_log_or_else_to_standard_error() {
        let dir = tempfile::tempdir().unwrap();
        let on_stderr = Arc::new(Mutex::new(Vec::<String>::new()));
        let heard = Arc::new(Mutex::new(Vec::<String>::new()));
        let (e, h) = (on_stderr.clone(), heard.clone());
        let with_sink = HelperLog::new(dir.path().join("helper.log"))
            .with(1000, || 7)
            .deferred()
            .with_stderr(move |line| e.lock().unwrap().push(line.to_string()))
            .with_early_sink(Arc::new(move |_, line| h.lock().unwrap().push(line.to_string())));
        let write = |log: &HelperLog, text: &str| log.log(&Record::builder().args(format_args!("{text}")).level(Level::Info).build());
        write(&with_sink, "pre-trust");
        assert_eq!(heard.lock().unwrap().len(), 1);
        assert!(
            on_stderr.lock().unwrap().is_empty(),
            "the OS log is where it goes: {:?}",
            on_stderr.lock().unwrap()
        );
        let e = on_stderr.clone();
        let without_sink = HelperLog::new(dir.path().join("helper2.log"))
            .with(1000, || 7)
            .deferred()
            .with_stderr(move |line| e.lock().unwrap().push(line.to_string()));
        write(&without_sink, "pre-trust, no OS log");
        assert_eq!(on_stderr.lock().unwrap().len(), 1);
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
    fn a_fresh_log_after_a_rotation_continues_from_the_rotated_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("helper.log");
        std::fs::write(dir.path().join("helper.log.1"), "old-1\nold-2\nold-3\n").unwrap();
        std::fs::write(&path, "new-1\nnew-2\n").unwrap();
        assert_eq!(tail(&path, 4), ["old-2", "old-3", "new-1", "new-2"]);
        assert_eq!(
            tail(&path, 2),
            ["new-1", "new-2"],
            "no look at the rotated file when the log has enough"
        );
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            tail(&path, 2),
            ["old-2", "old-3"],
            "a log not yet recreated still shows the rotated one"
        );
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
