//! Windows for the Paddock apps: top-level windows in the shell's label scheme, remembered bounds on the right monitor,
//! single instance and deep links, the bounded quit, native feel, the tray icon and the tray popover window.
//!
//! ADR 0006, ADR 0012 §3; plan S12. Taken from Debrief's `window_manager.rs`, `windows.rs`, `instance.rs`,
//! `lifecycle.rs` (the exit watchdog), `native_feel.rs` and `tray.rs`, Pitwall's `window.rs` and `tray.rs`, and Dyno's
//! `popover.rs`, without any app's names, settings or menu items.
//!
//! - [`close`]: what closing a window and an exit request do (quit, keep running in the tray, ask), as pure decisions.
//! - [`launch`]: the command line (flags, paths, deep links of the app's schemes), single instance, and the plan for
//!   files and links from outside.
//! - [`bounds`]: remembered bounds, restored on the monitor they overlap most, scaled and kept inside its work area.
//! - [`exit`]: bounded teardown steps and the exit watchdog: a stuck shutdown never hangs Quit.
//! - [`feel`]: the webview settings of an app window (no page zoom, no browser keys in release builds).
//! - [`tray`]: the tray menu model and what a click does; [`popover`]: the tray popover's geometry, labels, material
//!   and boot script.
//! - [`wire`]: the wire types and events.
//!
//! Labels are the shell's (`paddock_commands::labels`): `main` and `w-<n>` are top-level windows, `d-`/`p-` labels
//! belong to a host window, and `tray`/`tray-<item>` are tray popovers (window class `Tray`, which calls only the
//! commands that name `TRAY`). The adapter (`tauri` feature) applies the decisions to Tauri windows; the native tray
//! icon needs the `tray` feature.
//!
//! ```
//! use paddock_window::close::{decide_close, CloseDecision, OnClose};
//! use paddock_window::launch::{parse_args, plan, OpenFilesIn, OpenPlan, OpenRequest, Target};
//!
//! // Closing the last window of a tray app keeps it running there; without a tray it quits.
//! assert_eq!(decide_close(OnClose::Tray, 0, true, false), CloseDecision::Background);
//! assert_eq!(decide_close(OnClose::Tray, 0, false, false), CloseDecision::Quit);
//!
//! // A second launch with a deep link: the link goes to the window in front.
//! let args = parse_args(["pitwall://models".to_string()], None, &["pitwall"]);
//! let request = OpenRequest { paths: vec![], links: args.links };
//! let target = Some(Target { label: "main".into(), pristine: false });
//! assert!(matches!(plan(request, false, OpenFilesIn::Ask, target, false), OpenPlan::OpenIn(l, _) if l == "main"));
//! ```

pub mod bounds;
pub mod close;
pub mod exit;
pub mod feel;
pub mod launch;
pub mod popover;
pub mod tray;
pub mod wire;

pub use close::{Background, CloseChoice, CloseDecision, OnClose, OpenChoice};
pub use feel::WebviewFeel;
pub use launch::{DeepLink, LaunchArgs, OpenFilesIn, OpenRequest};
pub use wire::{
    CLOSE_REQUESTED_EVENT, OPEN_ASK_EVENT, OPEN_EVENT, OpenAsk, WINDOWS_EVENT, WindowError,
    WindowErrorCode, WindowInfo,
};

paddock_commands::command_table! {
    /// The window layer's commands (`paddock_window::tauri::handler` serves them). An app without the shell plugin lists
    /// the same entries in its own table, or chains [`COMMANDS`] with `tauri::handler` behind its own guard.
    pub static COMMANDS;
    /// Opens a new, empty top-level window (multi-window apps).
    Mutation [MAIN | TRAY] => tauri::window_new,
    /// Closes the calling window as its close button does (the close setting applies).
    Mutation [MAIN] => tauri::window_close,
    /// The top-level windows, `main` first.
    Query [WINDOW | TRAY] => tauri::windows_list,
    /// Shows, restores and focuses a top-level window (`main` is rebuilt when a tray app destroyed it).
    Mutation [WINDOW | TRAY] => tauri::window_focus,
    /// The calling window's title (one line, bounded).
    Quiet [MAIN] limit 4 * paddock_commands::KIB => tauri::window_set_title,
    /// The answer to `paddock://window-close-requested`.
    Mutation [MAIN] => tauri::window_resolve_close,
    /// The files and links waiting for the calling window (taken: a second call gets nothing).
    Mutation [MAIN] => tauri::window_take_pending,
    /// The answer to `paddock://window-open-ask`.
    Mutation [MAIN] => tauri::window_resolve_open,
    /// Quits the app: every window closes at once (the UI asks about unsaved work first).
    Mutation [WINDOW | TRAY] => tauri::app_quit,
    /// The tray popover's measured content height (logical px).
    Quiet [TRAY] => tauri::tray_fit_height,
    /// Closes the calling tray popover (Escape, a choice made in it).
    Mutation [TRAY] => tauri::tray_close,
}

#[cfg(feature = "tauri")]
pub mod tauri;

#[cfg(test)]
mod generated;
