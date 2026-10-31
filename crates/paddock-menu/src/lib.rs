//! Menus for the Paddock apps: the menu model, its validation and limits, the macOS native menu with the standard items,
//! diffing and routing to the focused window.
//!
//! ADR 0012 §3; plan S13. Taken from Debrief's `menu.rs` (Debrief ADR 0008: one menu model, native on macOS) without
//! Debrief's names, events or window manager.
//!
//! The UI owns the menus: `@decryp7/paddock-ui/shell/menu` builds one model from the app's contributions (commands, menu
//! items, submenus, key bindings with the user's overrides), draws it as the in-app menu bar on Windows and Linux, and on
//! macOS sends it with `menu_set` once the app is ready (later changes coalesced). This crate:
//!
//! - [`model`]: the wire DTOs ([`NativeMenuModel`]) and [`parse`], which validates them once (sizes, ids, labels) into an
//!   [`AppMenu`] and turns key bindings into native accelerators ([`accelerator()`]).
//! - [`native`]: [`native_spec`], the macOS menu bar of a model with the standard items (app menu with Services and Hide
//!   and the shell's Quit last, Edit with Undo..Select All, Window with Minimize and Zoom), [`diff`] (unchanged, states
//!   in place, or a rebuild) and [`outline`], the snapshot form.
//! - [`route`](mod@route): clicks go to the focused top-level window (a dialog or pop-out stands for its host).
//! - [`manifest`]: [`manifest::check`] over the app's `paddock-manifest.json`: every menu item routes, every key has an
//!   accelerator, a command is in one menu only.
//!
//! The core is pure Rust with no Tauri dependency; the adapter (`tauri`: managed state, the `menu_set` command, the
//! native menu bar, click routing as `paddock://menu`) lives behind the `tauri` feature.
//!
//! ```
//! use paddock_menu::{NativeMenuModel, Update, diff, native_spec, parse};
//!
//! let wire: NativeMenuModel = serde_json::from_str(r#"{ "menus": [
//!   { "id": "view", "label": "View", "items": [
//!     { "kind": "item", "id": "pw.view.sidebar", "label": "Sidebar", "keybinding": "Mod+B", "checked": true, "enabled": true }
//!   ] }
//! ] }"#).unwrap();
//! let model = parse(wire).unwrap();
//! let spec = native_spec(&model, "Pitwall");
//! assert_eq!(paddock_menu::item_ids(&spec), ["pw.view.sidebar"]);
//! assert_eq!(diff(Some(&spec), &spec), Update::Unchanged);
//! ```

pub mod accelerator;
pub mod manifest;
pub mod model;
pub mod native;
pub mod route;

pub use accelerator::accelerator;
pub use model::{
    AppMenu, Entry, Item, MAX_DEPTH, MAX_ENTRIES, MAX_ID, MAX_LABEL, MAX_MENUS, MENU_EVENT,
    MENU_SET_COMMAND, MenuApplied, MenuError, MenuRole, MenuSetError, MenuSetErrorCode,
    NativeMenuEntry, NativeMenuModel, NativeTopMenu, QUIT_COMMAND, TopMenu, parse, valid_id,
};
pub use native::{NativeNode, Predefined, Update, diff, item_ids, native_spec, outline};
pub use route::{Focus, applies, route, runs_without_window};

#[cfg(feature = "tauri")]
pub mod tauri;

paddock_commands::command_table! {
    /// The menu's command (`paddock_menu::tauri::invoke_handler` serves it; the shell plugin folds it into its table).
    pub static COMMANDS;
    /// The window's menu model: shown natively on macOS, validated elsewhere.
    Quiet [MAIN | POPOUT] limit paddock_commands::MIB => tauri::menu_set,
}

#[cfg(test)]
mod generated;
