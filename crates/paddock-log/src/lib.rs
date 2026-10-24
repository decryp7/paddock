//! The app and service log of the Paddock apps (plan X1, decision D1 and D2).
//!
//! ```no_run
//! use paddock_env::{AppDirs, AppId};
//! use paddock_log::{LogConfig, Logging};
//!
//! let app = AppId::new("spotter", "Spotter")?;
//! let dirs = AppDirs::for_app(&app)?;
//! let log = Logging::new(LogConfig::new(&app, "app", env!("CARGO_PKG_VERSION")).with_dirs(&dirs)).install()?;
//! log::info!("Spotter started");            // the `log` facade and `tracing` land in the same file
//! // later, from the Settings section: log.set_level(paddock_log::LogLevel::Debug);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! - One CLEF JSON-lines file per process per day in the OS log folder ([`paddock_env::AppDirs::logs`]):
//!   [`DailyFiles`], `<id>.<day>.log` for the app, `<id>-<process>.<day>.log` for a service or sidecar. [`LogSettings`] (the
//!   "Diagnostics > Logging" section) holds the level, the day's size budget (after it only errors are written) and how many
//!   older days are kept.
//! - `tracing` and `log` in one stream ([`TracingBridge`]); [`ipc_span`] gives commands `Window`, `OperationId` and
//!   `Command`; [`log_from_webview`] carries the webview's lines with their `Window`.
//! - Privacy: **never file contents**. The default [`Redactor`] masks secrets, replaces content-like properties by a size and
//!   bounds every value; apps add [`Redactor::with_hook`] rules. [`fingerprint`] shows equality without writing the value.
//! - Repeated identical errors are written once per minute and summarised; every error carries the latest [`Breadcrumb`]s
//!   of its window.
//! - [`CrashMarker`] and the panic hook: a panic on any thread lands in the file and leaves the marker for the next start.
//! - [`spawn_child_capture`] puts a child's standard error into the log, bounded.
//!
//! The crate has no Tauri dependency: services and sidecars use it as is; the app's shell forwards the webview's log
//! command to [`log_from_webview`]. Tests use temporary folders and [`FakeClock`], never the real log folder.

mod breadcrumbs;
mod bridge;
mod child;
mod clef;
mod clock;
mod core;
mod error;
mod files;
mod install;
mod panic;
mod redact;
mod settings;
mod webview;

pub use breadcrumbs::{
    BREADCRUMBS_PER_ERROR, BREADCRUMBS_PER_WINDOW, Breadcrumb, DEDUP_WINDOW, MAX_BREADCRUMB_JSON,
};
pub use bridge::TracingBridge;
pub use child::{MAX_CHILD_LINE, pump_child_lines, spawn_child_capture};
pub use clef::{Enrichment, format_event, key_values};
pub use clock::{Clock, FakeClock, SystemClock, now_utc};
pub use error::{LogError, Result};
pub use files::{DailyFiles, LineSink, StdoutSink};
pub use install::{LogConfig, LogHandle, Logging};
pub use panic::{CrashMarker, MAX_BACKTRACE, bounded_lines, payload_message};
pub use redact::{
    Hook, MAX_MESSAGE, MAX_TRUSTED_VALUE, MAX_VALUE, Redact, Redactor, SECRET, fingerprint,
    is_content_key, is_secret_key,
};
pub use settings::{KEEP_FILES_RANGE, LogLevel, LogSettings, MAX_FILE_MB_RANGE};
pub use webview::{ipc_span, log_from_webview};
