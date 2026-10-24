//! Report a problem for the Paddock apps (plan X2, decision D9): a diagnostics zip the user reviews file by file and saves
//! where they choose. **Nothing is ever sent**: the crate has no network dependency (`cargo deny` bans the HTTP stacks), and
//! the only way out is a path the user picked in a native save dialog.
//!
//! ```no_run
//! use paddock_diag::{AppInfo, Collected, Redactor, builtin};
//! use paddock_env::{AppDirs, AppId, ProcessVars};
//! use paddock_jobs::CancelToken;
//!
//! let id = AppId::new("spotter", "Spotter")?;
//! let dirs = AppDirs::for_app(&id)?;
//! let app = AppInfo::new("spotter", "Spotter", "1.0.0", "");
//! let redactor = Redactor::for_machine(&ProcessVars);
//! let collected = Collected::collect(&app, &dirs, &builtin::all(), &redactor, chrono::Utc::now());
//! let report = collected.assemble("The timeline stays empty after a reload");
//! let preview = report.preview();                                   // what the dialog lists, with sizes
//! report.save(std::path::Path::new("/Users/me/spotter-diagnostics.zip"), &CancelToken::new(), &())?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! - [`Redactor`]: the union of the three designs the apps had, as one tested layer stack: secrets ([`redact_secrets`], from
//!   Pitcrew and Debrief), personal data ([`Personal`], from Dyno: user names, home folders, machine name, e-mail and IP
//!   addresses) and `paddock-log`'s field rules (secret keys, file contents). Log lines are parsed as CLEF JSON, so a
//!   replacement never breaks a line.
//! - [`Section`]s: what goes in. The built-ins ([`builtin`]) are the system facts, the settings file, the crash marker and
//!   the newest part of the log (a seek from the end, so a 1 GB log costs the same as a small one: [`window`]); an app adds
//!   its own. A section returns [`Content`], never bytes, so every file is redacted; a section that panics costs only itself.
//! - [`Collected`] → [`Report`] → [`Preview`] / zip: the preview and the zip are built from the same list of files, so the
//!   preview equals the zip. The zip replaces the destination atomically at the end (cancel leaves no partial file).
//! - [`logging`]: the "Diagnostics > Logging" settings section over `paddock-log`'s `LogSettings`.
//! - `tauri` (feature): the adapter the shell plugin installs: state, the preview/save/cancel commands, the save as a job
//!   with a native picker, the `paddock://diagnostics` outcome event, and the webview's log command.
//!
//! The core is pure Rust with no Tauri dependency, so services and command-line tools can write the same report.

mod error;
pub mod logging;
mod manifest;
mod personal;
mod redact;
mod report;
mod secrets;
mod section;
pub mod window;

pub use error::{DiagError, Result};
pub use manifest::{
    AppInfo, FileKind, MANIFEST_KIND, MANIFEST_NAME, Manifest, ManifestError, ManifestFile,
    SCHEMA_VERSION,
};
pub use personal::{Personal, RedactionCounts};
pub use redact::{MAX_PATTERN_BYTES, MAX_PATTERN_LEN, MAX_PATTERNS, PatternError, Redactor};
pub use report::{
    Collected, DESCRIPTION_NAME, Limits, MAX_DESCRIPTION, MAX_FILE_BYTES, MAX_FILES,
    MAX_NAME_CHARS, MAX_NOTE, MAX_TOTAL_BYTES, Preview, PreviewFile, Report, ReportFile, Saved,
    TRUNCATION_NOTE, cap_description, suggested_name, valid_entry_name, validate_destination,
};
pub use secrets::{REDACTED, luhn_ok, redact_secrets};
pub use section::{Content, Context, Section, SectionFile, SectionOutput, builtin};

/// The event the outcome of a save is sent on, to every window (a `SaveOutcome` of the adapter).
pub const EVENT_NAME: &str = "paddock://diagnostics";

paddock_commands::command_table! {
    /// The commands of the report adapter (`paddock_diag::tauri::invoke_handler` serves them; the shell plugin folds them
    /// into its table). The three report commands are for the Report a Problem dialog; any window may forward log lines.
    pub static COMMANDS;
    /// Collects and redacts the report and answers with the preview and a token for saving.
    Mutation [DIALOG] => tauri::diagnostics_preview,
    /// Asks where to save (a native dialog) and writes the previewed report as a job; returns at once.
    Mutation [DIALOG] => tauri::diagnostics_save,
    /// Cancels the save in progress.
    Mutation [DIALOG] => tauri::diagnostics_cancel,
    /// A line the webview logged (an error of the page, a warning); it carries the window it came from.
    Quiet [WINDOW | TRAY] limit 64 * paddock_commands::KIB => tauri::log_from_webview,
}

#[cfg(feature = "tauri")]
pub mod tauri;

#[cfg(test)]
mod generated;
