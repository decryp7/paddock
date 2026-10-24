//! The plugin's build step: Tauri's `allow-<command>`/`deny-<command>` permissions for every workbench command, and the
//! permission sets per window class (`permissions/window-classes.toml`, written by this crate's tests from the table).
//! The command list is generated from the composed table and checked by a test (`src/command_names.rs`).

include!("src/command_names.rs");

fn main() {
    tauri_plugin::Builder::new(COMMAND_NAMES).build();
}
