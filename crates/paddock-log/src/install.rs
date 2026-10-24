//! Setting the log up: [`LogConfig`] says where and how much, [`Logging`] builds the pieces, [`Logging::install`] makes them
//! the process's log (once), and the [`LogHandle`] it returns changes the level at run time and reads the breadcrumbs.

use crate::breadcrumbs::{Breadcrumb, Breadcrumbs, ErrorDedup};
use crate::bridge::TracingBridge;
use crate::clef::Enrichment;
use crate::clock::{Clock, SystemClock};
use crate::core::{ClefLayer, Core};
use crate::error::{LogError, Result, lock};
use crate::files::{DailyFiles, LineSink, StdoutSink};
use crate::panic::{CrashMarker, PanicReport, install_hook};
use crate::redact::{Redact, Redactor};
use crate::settings::{LogLevel, LogSettings};
use paddock_env::{AppDirs, AppId};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::layer::{Layered, SubscriberExt};
use tracing_subscriber::{Registry, reload};

type Subscriber = Layered<ClefLayer, Layered<reload::Layer<LevelFilter, Registry>, Registry>>;

/// How a process logs. Build with [`LogConfig::new`] and the `with_*` methods.
pub struct LogConfig {
    application: String,
    process: String,
    file_prefix: String,
    version: String,
    commit: String,
    session_id: String,
    log_dir: Option<PathBuf>,
    crash_marker: Option<PathBuf>,
    settings: LogSettings,
    stdout: bool,
    panic_hook: bool,
    redact: Arc<dyn Redact>,
    clock: Arc<dyn Clock>,
}

impl LogConfig {
    /// A config for `process` of `app` (`app` for the main process, `server`, `agent`, ... for the others; each process
    /// writes its own file, `<id>.<day>.log` for `app`, `<id>-<process>.<day>.log` otherwise). Writes to standard output
    /// only until [`LogConfig::with_dirs`] or [`LogConfig::with_log_dir`] names a folder.
    pub fn new(app: &AppId, process: &str, version: &str) -> Self {
        let file_prefix = if process == "app" {
            app.id().to_string()
        } else {
            format!("{}-{process}", app.id())
        };
        LogConfig {
            application: app.name().to_string(),
            process: process.to_string(),
            file_prefix,
            version: version.to_string(),
            commit: String::new(),
            session_id: new_session_id(),
            log_dir: None,
            crash_marker: None,
            settings: LogSettings::default(),
            stdout: cfg!(debug_assertions),
            panic_hook: true,
            redact: Arc::new(Redactor::new()),
            clock: Arc::new(SystemClock),
        }
    }

    /// The app's folders: the log folder from [`AppDirs::logs`] and the crash marker in [`AppDirs::data`].
    pub fn with_dirs(self, dirs: &AppDirs) -> Self {
        let marker = CrashMarker::path_for(dirs.data(), &self.process);
        self.with_log_dir(dirs.logs()).with_crash_marker(&marker)
    }

    /// Writes the files into `dir`.
    pub fn with_log_dir(mut self, dir: &Path) -> Self {
        self.log_dir = Some(dir.to_path_buf());
        self
    }

    /// Where the panic hook leaves its marker (see [`CrashMarker`]); none by default.
    pub fn with_crash_marker(mut self, path: &Path) -> Self {
        self.crash_marker = Some(path.to_path_buf());
        self
    }

    /// The level and limits (clamped to their ranges).
    pub fn with_settings(mut self, settings: LogSettings) -> Self {
        self.settings = settings.clamped();
        self
    }

    /// The build commit written on every event.
    pub fn with_commit(mut self, commit: &str) -> Self {
        self.commit = commit.to_string();
        self
    }

    /// Also write to standard output (on by default in debug builds only).
    pub fn with_stdout(mut self, on: bool) -> Self {
        self.stdout = on;
        self
    }

    /// Whether [`Logging::install`] sets the panic hook (on by default).
    pub fn with_panic_hook(mut self, on: bool) -> Self {
        self.panic_hook = on;
        self
    }

    /// Replaces the redaction rules (the default [`Redactor`] never writes file contents).
    pub fn with_redactor(mut self, redact: impl Redact) -> Self {
        self.redact = Arc::new(redact);
        self
    }

    /// Replaces the clock (tests).
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }
}

fn new_session_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{:08x}{nanos:08x}", std::process::id())
}

struct Shared {
    core: Arc<Core>,
    files: Option<Arc<DailyFiles>>,
    filter: reload::Handle<LevelFilter, Registry>,
    level: Mutex<LogLevel>,
}

/// Changes the log while it runs. Cheap to clone; everything it does is safe from any thread.
#[derive(Clone)]
pub struct LogHandle(Arc<Shared>);

impl LogHandle {
    /// Applies `level` to `log` and `tracing` at once (no restart).
    pub fn set_level(&self, level: LogLevel) {
        log::set_max_level(level.log_filter());
        let _ = self.0.filter.modify(|f| *f = level.tracing_filter());
        *lock(&self.0.level) = level;
    }

    /// The level now in force.
    pub fn level(&self) -> LogLevel {
        *lock(&self.0.level)
    }

    /// Writes the pending "repeated n times" summaries (call before exit and before a report is collected).
    pub fn flush_repeats(&self) {
        self.0.core.flush_repeats();
    }

    /// The file lines go to today, when files are used.
    pub fn active_file(&self) -> Option<PathBuf> {
        self.0.files.as_ref().map(|f| f.active_file())
    }

    /// Why the log file cannot be written, in words for the user (a health entry can show it); `None` while it works.
    pub fn file_problem(&self) -> Option<String> {
        self.0.files.as_ref().and_then(|f| f.problem())
    }

    /// The latest `n` breadcrumbs of `window` (every window's when `None` or unknown), oldest first.
    pub fn breadcrumbs(&self, window: Option<&str>, n: usize) -> Vec<Breadcrumb> {
        lock(&self.0.core.crumbs).recent_for(window, n)
    }
}

/// The built log, before it is made the process's log.
pub struct Logging {
    handle: LogHandle,
    subscriber: Subscriber,
    report: PanicReport,
    panic_hook: bool,
    level: LogLevel,
}

impl Logging {
    /// Builds the pieces. Nothing is installed and no file is created yet.
    pub fn new(config: LogConfig) -> Logging {
        let enrich = Enrichment {
            application: config.application,
            process: config.process.clone(),
            version: config.version,
            os: std::env::consts::OS,
            commit: config.commit,
            session_id: config.session_id,
            process_id: std::process::id(),
        };
        let files = config.log_dir.as_ref().map(|dir| {
            Arc::new(DailyFiles::new(
                dir,
                &config.file_prefix,
                config.settings.keep_files,
                config.settings.max_bytes(),
                config.clock.clone(),
                enrich.clone(),
            ))
        });
        let mut sinks: Vec<Box<dyn LineSink>> = Vec::new();
        if let Some(f) = &files {
            sinks.push(Box::new(SharedFiles(f.clone())));
        }
        if config.stdout || files.is_none() {
            sinks.push(Box::new(StdoutSink));
        }
        let core = Arc::new(Core {
            sinks,
            redact: config.redact,
            crumbs: Mutex::new(Breadcrumbs::default()),
            dedup: Mutex::new(ErrorDedup::default()),
            clock: config.clock.clone(),
            enrich,
        });
        let (filter_layer, filter) = reload::Layer::new(config.settings.level.tracing_filter());
        let subscriber = Registry::default()
            .with(filter_layer)
            .with(ClefLayer(core.clone()));
        let report = PanicReport {
            process: config.process,
            marker: config.crash_marker,
            log_file: files.as_ref().map(|f| f.active_file()),
            clock: config.clock,
        };
        let handle = LogHandle(Arc::new(Shared {
            core,
            files,
            filter,
            level: Mutex::new(config.settings.level),
        }));
        Logging {
            handle,
            subscriber,
            report,
            panic_hook: config.panic_hook,
            level: config.settings.level,
        }
    }

    /// The handle (also returned by [`Logging::install`]).
    pub fn handle(&self) -> LogHandle {
        self.handle.clone()
    }

    /// Makes this the process's log: the `tracing` subscriber, the `log` bridge (`log` records, the webview's lines and
    /// dependencies reach the same files) and the panic hook. Fails soft on files: without a usable log folder the log
    /// goes to standard output and [`LogHandle::file_problem`] says why. Fails when another subscriber or logger exists.
    pub fn install(self) -> Result<LogHandle> {
        if tracing::dispatcher::has_been_set() {
            return Err(LogError::SubscriberTaken);
        }
        log::set_boxed_logger(Box::new(TracingBridge)).map_err(|_| LogError::LoggerTaken)?;
        tracing::subscriber::set_global_default(self.subscriber)
            .map_err(|_| LogError::SubscriberTaken)?;
        self.handle.set_level(self.level);
        if self.panic_hook {
            install_hook(self.report);
        }
        Ok(self.handle)
    }

    /// Runs `f` with this as the log of the current thread only (no global state touched; for tests and tools).
    pub fn scoped<R>(self, f: impl FnOnce(&LogHandle) -> R) -> R {
        let handle = self.handle.clone();
        tracing::subscriber::with_default(self.subscriber, || f(&handle))
    }
}

/// Lets the handle and the core share one [`DailyFiles`].
struct SharedFiles(Arc<DailyFiles>);

impl LineSink for SharedFiles {
    fn write_line(&self, level: log::Level, line: &str) {
        self.0.write_line(level, line);
    }
}
