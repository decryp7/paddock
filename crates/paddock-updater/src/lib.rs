//! Updates for the Paddock apps, and About and What's New: the update state machine, the release information
//! (`latest.json`) and its minisign signature check over `paddock-download`, the install hand-off, the services' hooks
//! across updates (rule 24), the running version and legal texts for About, and the running version's release notes.
//!
//! ADR 0012 §3; plan S11. Taken from Debrief's `updater/`, `legal.rs` and What's New; replaces the apps' own
//! `updater.rs` copies.
//!
//! - [`Updater`]: the app-wide updater. A check and a download are job bodies ([`Updater::run_check`],
//!   [`Updater::run_download`]) the adapter runs on a job pool, so commands return at once and progress arrives as job
//!   phases (rule 10, 11); nothing has a UI timeout (rule 12). Every change is an [`UpdateStatus`] on [`STATUS_EVENT`].
//! - Downloads go through `paddock-download` (segmented, resumable, size-pinned when the release says, free disk space
//!   checked first), then [`signature::verify_file`] checks the minisign signature against the key built into the app
//!   and requires it to be made for the announced version. A refused signature deletes the file; nothing unsigned is
//!   installed. A build without a key is [`UpdatePhase::Unavailable`] ("Updates are not available in this build").
//! - Installing happens when the app exits ([`Updater::install_at_exit`]) through an [`Installer`] (the adapter's is
//!   `tauri-plugin-updater`, which verifies again), after [`ServiceHooks::before_install`]; at the first start of a new
//!   version [`Updater::on_start`] runs [`ServiceHooks::on_version_change`] once (re-register, repair).
//! - Every build says how it installs: `UpdatesBuilder::installer(..)` (the `PluginInstaller` of feature `install`) or
//!   `UpdatesBuilder::no_install()` for one that only notifies; `tauri::install` refuses a builder with neither
//!   (`InstallError::InstallerUndeclared`), so an update that is downloaded and can never be installed cannot ship (conformance
//!   check C59 is the static half). [`UpdateStatus::can_install`] tells the Updates dialog: a build that cannot install offers
//!   no Download, "Restart to update" or "Install when the app quits", and [`Updater::prepare_restart`] and
//!   [`Updater::install_on_quit`] refuse at the request.
//! - [`About`]: the running version, build facts, legal documents served on request, and What's New for the running
//!   version only ([`notes`]: never another release's notes).
//!
//! The core is pure Rust with no Tauri dependency; the adapter is `paddock_updater::tauri` behind the `tauri` feature, the installer
//! over `tauri-plugin-updater` behind `install`.
//!
//! ```
//! use paddock_updater::{Event, UpdatePhase};
//!
//! let idle = UpdatePhase::Idle;
//! assert!(matches!(idle.next(Event::Check), Ok(UpdatePhase::Checking { .. })));
//! assert!(idle.next(Event::Install).is_err(), "nothing to install");
//! ```

pub mod about;
pub mod error;
pub mod manifest;
pub mod notes;
pub mod record;
pub mod signature;
pub mod state;
pub mod updater;

pub use about::{About, AboutError, AboutInfo, LegalDocInfo};
pub use error::{DEFAULT_MAX_DOWNLOAD_BYTES, UpdateError, UpdateErrorDto, UpdateErrorKind};
pub use manifest::{Manifest, Platform, Target};
pub use notes::{NotesGroup, ReleaseNotes, WhatsNew, WhatsNewDecision};
pub use record::{Record, RecordStore};
pub use signature::UpdateKey;
pub use state::{Event, UpdateInfo, UpdatePhase};
pub use updater::{
    CHECK_PHASE, CancelOutcome, Channel, ChannelInfo, ExitAction, Installer, PreferenceStore,
    Preferences, SIGNATURE_PHASE, STATUS_EVENT, ServiceHooks, ServicesOutcome, Ticket,
    UpdateStatus, Updater, UpdaterConfig,
};

paddock_commands::command_table! {
    /// The updater's and About's commands (`paddock_updater::tauri::handler` serves them). Queries may come from any
    /// window; the steps of an update from the main window and dialogs (the Updates dialog).
    pub static COMMANDS;
    /// The updater's status; changes follow on `paddock://updates`.
    Query [WINDOW] => tauri::updates_status,
    /// Starts a check (a job).
    Mutation [MAIN | DIALOG] => tauri::updates_check,
    /// Starts the download and signature check of the update found (a job).
    Mutation [MAIN | DIALOG] => tauri::updates_download,
    /// Cancels the check or download, or drops the verified download.
    Mutation [MAIN | DIALOG] => tauri::updates_cancel,
    /// Installs the verified update when the app quits (or not).
    Mutation [MAIN | DIALOG] => tauri::updates_install_on_quit,
    /// Quits, installs the verified update and starts the new version.
    Mutation [MAIN | DIALOG] => tauri::updates_restart,
    /// Automatic checks and the release channel.
    Mutation [MAIN | DIALOG] => tauri::updates_set_preferences,
    /// The running version, build facts and the legal documents' titles.
    Query [WINDOW] => tauri::about_info,
    /// One legal document's text.
    Query [WINDOW] => tauri::about_legal,
    /// The running version's release notes.
    Query [WINDOW] => tauri::about_whats_new,
    /// Once per start: whether to open What's New.
    Mutation [MAIN] => tauri::about_whats_new_start,
}

paddock_dialogs::dialogs! {
    /// About, What's new and Updates (ADR 0026 §3).
    pub static DIALOGS;
    // The releases link opens in the system browser through the files adapter (a webview window opens no links itself).
    "pd.about" => Viewer { commands: &[dialog_callable::about_info, dialog_callable::about_legal, paddock_files::dialog_callable::files_open_url] },
    "pd.whats-new" => Viewer { commands: &[dialog_callable::about_whats_new] },
    "pd.updates" => Form(Modeless) { commands: &[
        dialog_callable::updates_status,
        dialog_callable::updates_check,
        dialog_callable::updates_download,
        dialog_callable::updates_cancel,
        dialog_callable::updates_install_on_quit,
        dialog_callable::updates_restart,
        dialog_callable::updates_set_preferences,
        // A build that cannot install points to the releases page, which opens in the system browser.
        paddock_files::dialog_callable::files_open_url,
    ] },
}

#[cfg(feature = "tauri")]
pub mod tauri;

#[cfg(test)]
mod generated;
