//! Pop-out panel windows for the Paddock apps: a panel of a window's dock moved into an OS window of its own, the drag
//! ghost that shows where a dragged tab will open, and the UI-state forwarding between a host window and its pop-outs and
//! dialogs.
//!
//! ADR 0013 (workbench), plan X50 W3. Taken from Debrief's `panel_windows.rs`, `drag_ghost/` and the transport half of
//! `app/src/app/popout`, without Debrief's window manager, view ids, workspace or events.
//!
//! - [`panel`]: panel ids (`<view>` or `<view>#<key>`, the workbench core's rules), parsed once at the edge.
//! - [`place`]: where a pop-out opens, on any display, clamped to that display's work area (pure, mixed scales).
//! - [`registry`]: which pop-outs exist (at most 12 per host) and where each view's pop-out was last seen (a small
//!   versioned file).
//! - [`sync`]: who receives forwarded UI state (only the windows of the sender's host; dialogs get display state only),
//!   the size limit and the rate limit.
//! - [`ghost`]: the drag ghost's session state machine, geometry, request parsing and static page.
//! - [`wire`]: the wire types and the events (`paddock://panels`, `paddock://ui-sync`).
//!
//! Labels are the shell's (`paddock_commands::labels`): a pop-out is `p-<host>-<n>`, class `POPOUT`, and works for its
//! host (it never outlives it). The adapter (`tauri` feature) applies it all to Tauri windows.
//!
//! ```
//! use paddock_panels::panel::Panel;
//! use paddock_panels::sync::recipients;
//! use paddock_panels::wire::SyncScope;
//!
//! let panel = Panel::parse("pn.code#src/main.rs").unwrap();
//! assert_eq!((panel.view(), panel.key()), ("pn.code", Some("src/main.rs")));
//!
//! // A pop-out's state goes to its host and the host's other pop-outs; another host's windows never hear it.
//! let windows = ["main", "p-main-1", "p-main-2", "w-2", "p-w-2-3"];
//! assert_eq!(recipients("p-main-1", SyncScope::Full, windows), ["main", "p-main-2"]);
//! ```

pub mod ghost;
pub mod panel;
pub mod place;
pub mod registry;
pub mod sync;
pub mod wire;

pub use panel::{MAX_KEY, Panel, PanelProblem};
pub use registry::{MAX_POPOUTS, Memory, PopOut, PopOuts};
pub use wire::{
    GhostRequest, PANELS_EVENT, PanelError, PanelErrorCode, PanelsEvent, PopOutBounds, PopOutInfo,
    PopOutRequest, SyncPublish, SyncScope, UI_SYNC_EVENT,
};

paddock_commands::command_table! {
    /// The panel layer's commands (`paddock_panels::tauri::invoke_handler` serves them; the workbench plugin folds them
    /// into its table).
    pub static COMMANDS;
    /// Pops a panel of the calling window out into a window of its own (or focuses the one that shows it).
    Mutation [MAIN] limit 16 * paddock_commands::KIB => tauri::panel_pop_out,
    /// Docks a pop-out back: the calling pop-out closes itself, or a window closes a pop-out of its own group by panel.
    Mutation [MAIN | POPOUT] => tauri::panel_dock_back,
    /// The pop-outs of the calling window's group (the host and its pop-outs see the same list).
    Query [MAIN | POPOUT] => tauri::panels_list,
    /// Brings a popped-out panel's window to the front.
    Mutation [MAIN | POPOUT] => tauri::panel_focus,
    /// Keeps the calling pop-out above its host window (or not); the window is rebuilt in place.
    Mutation [POPOUT] => tauri::panel_keep_on_top,
    /// Forwards UI state to the other windows of the caller's host (bounded size, rate-limited).
    Quiet [MAIN | POPOUT | DIALOG] limit 640 * paddock_commands::KIB => tauri::ui_sync_publish,
    /// Starts the drag ghost for a tab dragged out of the calling window (false where the platform cannot place it).
    Mutation [MAIN] limit 3 * paddock_commands::MIB => tauri::drag_ghost_start,
    /// Shows the ghost under the pointer (the drag left the window) or hides it (it came back).
    Quiet [MAIN] => tauri::drag_ghost_show,
    /// Ends the drag: hides the ghost and answers where it was (the place to pop the panel out at), or null.
    Mutation [MAIN] => tauri::drag_ghost_end,
    /// Whether the primary mouse button is held anywhere on screen (null when the platform cannot tell).
    Query [MAIN | POPOUT] => tauri::pointer_button_down,
}

#[cfg(feature = "tauri")]
pub mod tauri;

#[cfg(test)]
mod generated;
