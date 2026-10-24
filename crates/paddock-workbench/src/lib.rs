//! The Rust side of the Paddock workbench (ADR 0013 §3; plan W2a, W7).
//!
//! - [`layout`]: the dock's saved arrangements (per window and perspective, named layouts) as one versioned, bounded state
//!   file of `paddock-env` in the app's config folder. The UI owns the content and parses it at its edge.
//! - `tauri` (feature): the managed [`layout::LayoutStore`] and its commands, `layout_get` and `layout_set` (file I/O on
//!   the blocking pool, never on the IPC thread: rule 10).
//!
//! The pop-out panels (`paddock-panels`) and the assistant (`paddock-assistant`) keep their own tables; the workbench
//! plugin folds them with [`COMMANDS`] behind one guard.

pub mod layout;

pub use layout::{LayoutError, LayoutStore, Layouts};

paddock_commands::command_table! {
    /// The layout commands (the dock's persistence).
    pub static COMMANDS;
    /// The value saved under a key (`{ key }` -> object or null). Pop-outs read the layout of their host's perspective.
    Query [MAIN | POPOUT] => tauri::layout_get,
    /// Saves (or with `null` deletes) the value under a key; at most 1 MiB of layout plus the request's envelope.
    Quiet [MAIN] limit 1088 * paddock_commands::KIB => tauri::layout_set,
}

#[cfg(feature = "tauri")]
pub mod tauri;
