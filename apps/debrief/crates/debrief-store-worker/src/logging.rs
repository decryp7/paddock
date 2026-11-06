//! The worker's own log: one dated paddock-log file, `debrief-store.<day>.log`, in the app's log folder (the folder the
//! app's `debrief.<day>.log` is in, which the Report-a-problem bundle reads), with the panic hook and a crash marker. The
//! worker's standard error stays the second channel (native aborts, the allocator, start failures before this log exists).
//! The folder, the level and the limits come from the app on the command line ([`LogArgs`]): the worker's environment is
//! cleared, and the app's values are the single source. Names, codes, counts and milliseconds only: no record content, no
//! query text, no user file names (the logging rules).

use debrief_store_client::protocol::{LogArgs, LogLevelArg, StoreTuning};
use paddock_env::AppId;
use paddock_log::{CrashMarker, LogConfig, LogHandle, LogLevel, LogSettings, Logging};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// The app's id (the log names) and display name.
const APP_ID: &str = "debrief";
const APP_NAME: &str = "Debrief";
/// The process name in the file name and the crash marker: `debrief-store.<day>.log`, `crash-store.json`.
pub const PROCESS: &str = "store";

static STARTED: OnceLock<Instant> = OnceLock::new();
static HANDLE: OnceLock<LogHandle> = OnceLock::new();
static EXITED: AtomicBool = AtomicBool::new(false);

/// Milliseconds since the worker started its work (the first call, made first thing in `main`).
pub fn elapsed_ms() -> u64 {
    u64::try_from(STARTED.get_or_init(Instant::now).elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// The paddock-log level of a command-line level.
pub fn level_of(level: LogLevelArg) -> LogLevel {
    match level {
        LogLevelArg::Error => LogLevel::Error,
        LogLevelArg::Warn => LogLevel::Warn,
        LogLevelArg::Info => LogLevel::Info,
        LogLevelArg::Debug => LogLevel::Debug,
        LogLevelArg::Trace => LogLevel::Trace,
    }
}

/// Where the crash marker is left: the app's data folder, `crash-store.json`; none when the app named no data folder.
pub fn marker_path(log: &LogArgs) -> Option<PathBuf> {
    log.data_dir.as_deref().map(|d| CrashMarker::path_for(d, PROCESS))
}

/// Installs the worker's log (the first thing `main` does after `--self-test`). `None` when a log is installed already or the
/// id is refused: the worker then runs without a file log and says so on standard error.
pub fn install(log: &LogArgs) -> Option<LogHandle> {
    let _ = elapsed_ms();
    let app = AppId::new(APP_ID, APP_NAME).ok()?;
    let mut config = LogConfig::new(&app, PROCESS, env!("CARGO_PKG_VERSION"))
        .with_log_dir(&log.dir)
        .with_stdout(false)
        .with_settings(LogSettings {
            level: level_of(log.level),
            max_file_mb: log.max_file_mb,
            keep_files: log.keep_files,
        });
    if let Some(marker) = marker_path(log) {
        config = config.with_crash_marker(&marker);
    }
    match Logging::new(config).install() {
        Ok(handle) => {
            let _ = HANDLE.set(handle.clone());
            Some(handle)
        }
        Err(e) => {
            eprintln!("debrief-store-worker: the log could not start ({e})");
            None
        }
    }
}

/// The tuning as words for the start line: numbers and switches only (never the spill folder's path).
pub fn tuning_summary(t: &StoreTuning) -> String {
    let mut parts = Vec::new();
    let mut add = |name: &str, value: Option<String>| {
        if let Some(v) = value {
            parts.push(format!("{name}={v}"));
        }
    };
    add("parseThreads", t.parse_threads.map(|v| v.to_string()));
    add("duckThreads", t.duck_threads.map(|v| v.to_string()));
    add("memoryLimitMb", t.memory_limit_mb.map(|v| v.to_string()));
    add("indexHeapMb", t.index_heap_mb.map(|v| v.to_string()));
    add("indexThreads", t.index_threads.map(|v| v.to_string()));
    add("spillDir", t.spill_dir.as_ref().map(|_| "set".to_string()));
    add("maxSpillMb", t.max_spill_mb.map(|v| v.to_string()));
    add("allocatorFlushMb", t.allocator_flush_mb.map(|v| v.to_string()));
    add("allocatorBackgroundThreads", t.allocator_background_threads.map(|v| v.to_string()));
    add("allocatorArenas", t.allocator_arenas.map(|v| v.to_string()));
    add("pluginMemoryMb", t.plugin_memory_mb.map(|v| v.to_string()));
    add("pluginCpuPercent", t.plugin_cpu_percent.map(|v| v.to_string()));
    if parts.is_empty() {
        "defaults".to_string()
    } else {
        parts.join(" ")
    }
}

/// The parent's process id (Debrief's, for the join with the app's child log), 0 where the OS does not say.
pub fn parent_pid() -> u32 {
    #[cfg(unix)]
    {
        std::os::unix::process::parent_id()
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// `store.worker.start`: what this run is, so a bundle's worker lines can be joined to the app's by pid and time.
pub fn log_start(low_memory: bool, tuning: &StoreTuning, level: LogLevelArg) {
    log::info!(
        EventId = "store.worker.start",
        Version = env!("CARGO_PKG_VERSION"),
        ParentPid = parent_pid(),
        MemoryMode = if low_memory { "low" } else { "normal" },
        Tuning = tuning_summary(tuning).as_str(),
        LogLevel = level.as_str(),
        Arch = std::env::consts::ARCH;
        "Store worker started"
    );
}

/// `store.worker.ready`: the connection to Debrief is up.
pub fn log_ready() {
    log::info!(EventId = "store.worker.ready", Ms = elapsed_ms(); "Store worker connected to Debrief");
}

/// `store.worker.previous_panic` when the last run left its crash marker (its place, never its message, which may name data);
/// then the marker is cleared. A contained panic leaves the marker too, so this says "panicked", not "crashed": the parent's
/// child log is the authority on whether the process died.
pub fn note_previous_panic(log: &LogArgs) {
    let Some(path) = marker_path(log) else { return };
    if !path.exists() {
        return;
    }
    match CrashMarker::read(&path) {
        Some(m) => log::warn!(
            EventId = "store.worker.previous_panic",
            At = m.time.as_str(),
            Pid = m.pid,
            Location = m.location.as_str(),
            Thread = m.thread.as_str();
            "The previous store worker run panicked"
        ),
        None => log::warn!(EventId = "store.worker.previous_panic"; "The previous store worker run left a damaged crash marker"),
    }
    if let Err(e) = CrashMarker::clear(&path) {
        log::warn!(EventId = "store.worker.marker_not_cleared", Why = e.kind().to_string().as_str(); "The crash marker could not be removed");
    }
}

/// Why the worker ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitWhy {
    /// Debrief closed the connection.
    PeerClosed,
    /// Debrief closed the worker's standard input or died.
    StdinClosed,
    /// The worker could not connect to Debrief.
    ConnectFailed,
    /// The connection broke with an error.
    Error,
}

impl ExitWhy {
    pub fn as_str(self) -> &'static str {
        match self {
            ExitWhy::PeerClosed => "peer-closed",
            ExitWhy::StdinClosed => "stdin-closed",
            ExitWhy::ConnectFailed => "connect-failed",
            ExitWhy::Error => "error",
        }
    }
}

/// `store.worker.exit`, once per process (the connection closing and the lifeline can both end it). Error level with the cause
/// chain on a failure.
pub fn log_exit(why: ExitWhy, code: i32, error: Option<&dyn std::error::Error>) {
    if EXITED.swap(true, Ordering::SeqCst) {
        return;
    }
    let uptime = elapsed_ms();
    match error {
        Some(e) => log::error!(
            EventId = "store.worker.exit",
            Why = why.as_str(),
            UptimeMs = uptime,
            Code = code,
            ErrorChain = chain(e).as_str();
            "Store worker ended with an error"
        ),
        None => log::info!(EventId = "store.worker.exit", Why = why.as_str(), UptimeMs = uptime, Code = code; "Store worker ended"),
    }
    if let Some(h) = HANDLE.get() {
        h.flush_repeats();
    }
}

/// An error and its sources as one line (`a: b: c`).
pub fn chain(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        text.push_str(": ");
        text.push_str(&s.to_string());
        source = s.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crash_marker_goes_to_the_data_folder_as_crash_store() {
        let log = LogArgs {
            dir: std::env::temp_dir().join("l"),
            data_dir: Some(std::env::temp_dir().join("d")),
            level: LogLevelArg::Info,
            max_file_mb: 10,
            keep_files: 5,
        };
        assert_eq!(marker_path(&log), Some(std::env::temp_dir().join("d").join("crash-store.json")));
        assert_eq!(marker_path(&LogArgs { data_dir: None, ..log }), None);
    }

    #[test]
    fn the_tuning_summary_has_numbers_and_never_a_path() {
        assert_eq!(tuning_summary(&StoreTuning::default()), "defaults");
        let t = StoreTuning {
            parse_threads: Some(6),
            spill_dir: Some(std::env::temp_dir().join("secret-folder")),
            plugin_cpu_percent: Some(40),
            ..StoreTuning::default()
        };
        let text = tuning_summary(&t);
        assert_eq!(text, "parseThreads=6 spillDir=set pluginCpuPercent=40");
        assert!(!text.contains("secret"));
    }

    #[test]
    fn the_milliseconds_since_the_start_grow() {
        let first = elapsed_ms();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while elapsed_ms() <= first + 1 {
            assert!(std::time::Instant::now() < deadline, "the clock does not move");
            std::thread::yield_now();
        }
    }

    #[test]
    fn the_levels_map_one_to_one() {
        assert_eq!(level_of(LogLevelArg::Error), LogLevel::Error);
        assert_eq!(level_of(LogLevelArg::Warn), LogLevel::Warn);
        assert_eq!(level_of(LogLevelArg::Info), LogLevel::Info);
        assert_eq!(level_of(LogLevelArg::Debug), LogLevel::Debug);
        assert_eq!(level_of(LogLevelArg::Trace), LogLevel::Trace);
    }

    #[test]
    fn an_error_chain_reads_cause_after_cause() {
        let inner = std::io::Error::other("disk full");
        let outer = std::io::Error::other(inner);
        assert!(chain(&outer).contains("disk full"));
    }
}
