//! The start-up contract of the Paddock apps: the pure watchdog and its ticker, `window_ready`, boot-error forwarding,
//! the UI checkpoint, the liveness heartbeat and the native ask dialog.
//!
//! ADR 0012 §3; plan S7 and §11. Taken from Debrief's `startup_watchdog.rs` (Debrief ADR 0058), without Debrief's
//! window list, Safe Mode restart or dialog layer (those are builder options of the adapter).
//!
//! Every window reports `window_ready` once it has painted its frame. A window that does not within
//! [`Config::ready_timeout`] is reloaded once; if the reload does not help either, the user is asked natively: Reload /
//! Restart in Safe Mode / Keep Waiting. Once ready, a visible window answers a ping (`paddock://ping` → `window_pong`)
//! every [`Config::ping_interval`]; after [`Config::missed_pings`] missed in a row it is marked unresponsive, logged,
//! its dialog may be closed by the OS close, and the user is asked: Reload / Keep Waiting. A gap between ticks longer
//! than [`Config::pause_gap`] means the app process itself was paused (a CPU limiter, a suspend): the clocks restart
//! instead of firing. A window whose web content process terminated is reloaded (at most three times a minute) and its
//! reloaded page hears why, with its last UI checkpoint.
//!
//! - [`Watchdog`]: the decisions, pure (the caller passes the time). [`Checkpoints`] and [`ready_reply`]: the UI state a
//!   recovered page gets back. [`clean_error`]: one bounded log line. [`Prompt`]: the native question's texts.
//! - `tauri` (feature `tauri`): the builder, `on_page_load`, the ticker, the commands and their table; feature
//!   `dialog` adds the native question through `tauri-plugin-dialog`.
//!
//! ```
//! use paddock_startup::{Action, Watchdog};
//! use std::time::{Duration, Instant};
//!
//! let t0 = Instant::now();
//! let mut dog = Watchdog::default();
//! dog.page_started("main", t0);
//! assert!(dog.due(t0 + Duration::from_millis(9_500)).is_empty());
//! assert_eq!(dog.due(t0 + Duration::from_secs(10)), vec![Action::Reload("main".into())]);
//! ```

pub mod ask;
pub mod boot_error;
pub mod checkpoint;
pub mod config;
pub mod lifecycle;
pub mod run_marker;
pub mod safe_mode;
pub mod session;
pub mod watchdog;

pub use ask::{Choice, Prompt, Question};
pub use boot_error::{BootErrorKind, clean_error};
pub use checkpoint::{Checkpoints, ReadyReply, ready_reply};
pub use config::{Config, ConfigError};
pub use lifecycle::{
    CrashInfo, Decision, InstanceScope, Lifecycle, Offers, PreviousRun, RestorePlan, Start,
    StartReport, UncleanShutdown,
};
pub use safe_mode::{Feature, Mode, SAFE_MODE_FLAG};
pub use session::{Autosaver, SessionError, SessionInfo, SessionStore};
pub use watchdog::{Action, Pong, Recovery, Watchdog};

/// The event a ready window gets as a liveness ping; the page answers with `window_pong` and the same `seq`.
pub const PING_EVENT: &str = "paddock://ping";

paddock_commands::command_table! {
    /// The start-up contract's commands (`paddock_startup::tauri::handler` serves them). An app without the shell plugin
    /// lists the same entries in its own table, or chains [`COMMANDS`] with `tauri::handler` behind its own guard.
    pub static COMMANDS;
    /// The page painted its frame; the reply says whether the shell reloaded it, and what to restore.
    Quiet [WINDOW] => tauri::window_ready,
    /// An uncaught error before ready: one bounded log line.
    Quiet [WINDOW] limit 16 * paddock_commands::KIB => tauri::window_boot_error,
    /// The window's UI state, kept in memory for a recovery reload (the main window's state can be large).
    Quiet [MAIN] limit 16 * paddock_commands::MIB => tauri::ui_checkpoint,
    /// How this run started: safe mode, and after an unclean shutdown what the notice offers until the user decides.
    Query [MAIN] => tauri::recovery_status,
    /// Restore: the previous session, with the calling window's own UI state.
    Mutation [MAIN] => tauri::recovery_restore,
    /// Start fresh: forget the previous session.
    Mutation [MAIN] => tauri::recovery_start_fresh,
    /// Safe mode: the app restarts with extensions, plugins and the last session off.
    Mutation [MAIN] => tauri::recovery_safe_mode,
    /// Restart the whole app keeping its session (a language change): the checkpoints are written, then the app's relaunch runs.
    Mutation [WINDOW] => tauri::app_relaunch,
    /// The answer to a liveness ping.
    Quiet [WINDOW] => tauri::window_pong,
}

#[cfg(feature = "tauri")]
pub mod tauri;

#[cfg(test)]
mod generated;
