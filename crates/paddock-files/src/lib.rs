//! User files for the Paddock apps (plan X31): the one place that decides which paths the app may touch, shows the native
//! pickers, keeps the recent lists and hands files to the OS. Built from Debrief's `paths.rs`, `recent.rs` and `opener.rs`.
//!
//! ```no_run
//! use paddock_files::{Access, Files, GrantSource, PickRequest, recent};
//! use paddock_env::{AppDirs, AppId};
//!
//! let dirs = AppDirs::for_app(&AppId::new("spotter", "Spotter")?)?;
//! let files = Files::for_app(&dirs, &[recent::FILES, recent::PROJECTS]);
//! // Only a path the user chose is granted; a command then asks for it back:
//! files.grants().grant("main", std::path::Path::new("/Users/me/a.log"), Access::Read, GrantSource::CommandLine)?;
//! let granted = files.grants().check("main", "/Users/me/a.log", Access::Read, &["log"])?;
//! files.opened("files", &granted, None); // recorded only after a granted open
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! - [`PathGrants`]: the rule "open only paths the user chose" (security rule of the family). Grants come from the native
//!   dialogs, drag and drop, the command line and files the app produced; they are per window (a dialog or pop-out shares its
//!   host's), bounded, expire, and are checked canonically so `..` and links cannot reach another file. [`GrantedPath`] is
//!   the proof a command takes.
//! - [`path`]: the validators every path from the webview goes through (`parse`, `canonical`, `output_file`, `child`,
//!   `confined`).
//! - [`PickRequest`] and the [`Picker`] port: bounded, validated dialog requests; `tauri::NativePicker` (feature `dialog`)
//!   is the real adapter.
//! - [`Recents`]: recent lists recorded only after a granted open, kept in a versioned file, granted again on use.
//! - [`open`]: reveal in the file manager, open with the default program, web addresses; command lines are pure functions per
//!   OS, nothing that runs on open is opened, only http, https and mailto addresses are.
//! - [`Files`] joins them, and `tauri` (feature) is the adapter the shell plugin installs: the commands below, the
//!   [`WindowGrants`](tauri::WindowGrants) command argument for the app's own commands, and the drag-and-drop grants.

// `CommandError` is the family's wire envelope (X28); it is returned only on the refusal path, never in a loop.
#![allow(clippy::result_large_err)]

mod files;
mod grants;
pub mod open;
pub mod path;
mod pick;
pub mod recent;

pub use files::{AppFolder, Files};
pub use grants::{Access, GRANT_TTL, GrantSource, GrantedPath, MAX_GRANTS, PathGrants};
pub use paddock_commands::CommandError;
pub use pick::{
    MAX_EXTENSION_LEN, MAX_EXTENSIONS, MAX_FILTERS, MAX_TITLE, PickFilter, PickKind, PickRequest,
    Picker,
};
pub use recent::{ListSpec, RecentEntry, RecentList, RecentLists, Recents};

/// The event every window gets when a recent list changed (a `RecentLists`).
pub const RECENT_EVENT: &str = "paddock://files-recent";
/// The event a window gets when files were dropped on it and granted (a `DroppedFiles`).
pub const DROPPED_EVENT: &str = "paddock://files-dropped";

paddock_commands::command_table! {
    /// The commands of the files adapter (`paddock_files::tauri::invoke_handler` serves them; the shell plugin folds them into
    /// its table). Paths come back from `files_pick` and the drop event only; the others take a path the window was granted.
    pub static COMMANDS;
    /// Shows a native open or save dialog for the calling window and grants what the user chose to it; returns the paths.
    Mutation [MAIN | DIALOG] => tauri::files_pick,
    /// The recent lists.
    Query [WINDOW | TRAY] => tauri::files_recent_list,
    /// Takes one path out of a recent list.
    Mutation [WINDOW | TRAY] => tauri::files_recent_remove,
    /// Empties a recent list.
    Mutation [WINDOW | TRAY] => tauri::files_recent_clear,
    /// Grants a path of a recent list to the calling window again, and returns it.
    Mutation [MAIN | TRAY] => tauri::files_recent_grant,
    /// Shows a granted path in the file manager.
    Mutation [WINDOW | TRAY] => tauri::files_reveal,
    /// Opens a granted path with the OS's program for it (never a file that would run).
    Mutation [WINDOW] => tauri::files_open,
    /// Shows one of the app's own folders (data, config, cache, logs) in the file manager.
    Mutation [WINDOW | TRAY] => tauri::files_open_app_folder,
    /// Opens a web address (http, https or mailto) in the browser.
    Mutation [WINDOW | TRAY] => tauri::files_open_url,
}

#[cfg(feature = "tauri")]
pub mod tauri;

#[cfg(all(test, feature = "tauri"))]
mod generated;
