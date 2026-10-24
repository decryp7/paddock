//! The Rust side of the Paddock workbench (ADR 0013 §3; plan W2a, W7).
//!
//! - [`layout`]: the dock's saved arrangements (per window and perspective, named layouts) as one versioned, bounded state
//!   file of `paddock-env` in the app's config folder. The UI owns the content and parses it at its edge.
//! - `tauri` (feature): the managed [`layout::LayoutStore`] and its commands, `layout_get` and `layout_set` (file I/O on
//!   the blocking pool, never on the IPC thread: rule 10).
//!
//! - `COMMANDS` (feature `tauri`): the workbench plugin's table, the layout commands folded with the pop-out panels'
//!   (`paddock-panels`) and the assistant's (`paddock-assistant`) behind one guard.
//! - `Builder` (feature `tauri`): the plugin `paddock-workbench`, each part optional; the commands are
//!   `plugin:paddock-workbench|<command>`, the permissions `paddock-workbench:<set>` per window class
//!   (`default` for `main`, `popout`, `dialog`; [`permissions`]).
//!
//! ```ignore
//! let workbench = paddock_workbench::Builder::new()
//!     .layouts(config_dir)                                   // <config>/workbench-layout.json
//!     .panels(Panels::builder().route("index.html#/popout").bounds_file(data.join("popout-places.json")).build())
//!     .assistant(Arc::new(assistant))
//!     .build();
//! tauri::Builder::default().plugin(shell).plugin(workbench) // the shell first: the dialogs' grants are its state
//! ```
//!
//! The app's capabilities grant `paddock-workbench:default` to `main` and `w-*`, `paddock-workbench:popout` to `p-*`
//! and `paddock-workbench:dialog` to `d-*`. Tauri's ACL is the coarse boundary; the guard is the fine one (a dialog
//! reaches only what its contract declares). The drag ghost's window label (`paddock_panels::ghost::GHOST_LABEL`)
//! matches none of them: the ghost is a page without commands.

pub mod layout;
pub mod permissions;

pub use layout::{LayoutError, LayoutStore, Layouts};

paddock_commands::command_table! {
    /// The layout commands (the dock's persistence).
    pub static LAYOUT;
    /// The value saved under a key (`{ key }` -> object or null). Pop-outs read the layout of their host's perspective.
    Query [MAIN | POPOUT] => tauri::layout_get,
    /// Saves (or with `null` deletes) the value under a key; at most 1 MiB of layout plus the request's envelope.
    Quiet [MAIN] limit 1088 * paddock_commands::KIB => tauri::layout_set,
}

#[cfg(feature = "tauri")]
pub mod tauri;

#[cfg(feature = "tauri")]
mod builder;
#[cfg(feature = "tauri")]
pub use builder::Builder;

/// The plugin's name: commands are `plugin:paddock-workbench|<command>`, permissions `paddock-workbench:<set>`.
pub const PLUGIN_NAME: &str = "paddock-workbench";

#[cfg(feature = "tauri")]
paddock_commands::command_table! {
    /// Every command of the workbench plugin: the layout commands, the pop-out panels' and the assistant's folded into
    /// one table (`command_table!` composition; a command two of them declare does not compile).
    pub static COMMANDS = [LAYOUT, paddock_panels::COMMANDS, paddock_assistant::tauri::COMMANDS];
}

/// The command names, for the plugin's build step (generated from `COMMANDS`; a test fails when it is stale).
pub mod command_names {
    include!("command_names.rs");
}

#[cfg(all(test, feature = "tauri"))]
mod tests;
