//! The plugin's permission sets, one per window class, generated from the table (`permissions/window-classes.toml`).
//!
//! Tauri's build step makes `allow-<command>` for every command; these sets group them by the classes the table opens
//! each command to, so an app's capability names one set per window class and can never grant a command to a class
//! the guard would refuse anyway. `default` is the main window's set.

use paddock_commands::{Table, WindowClass};

/// The set of each window class: its identifier and what it is for.
pub const SETS: [(WindowClass, &str, &str); 4] = [
    (
        WindowClass::Main,
        "main",
        "Every shell command a top-level window (`main`, `w-<n>`) may call.",
    ),
    (
        WindowClass::Popout,
        "popout",
        "Every shell command a panel popped out of a window (`p-<host>-<n>`) may call.",
    ),
    (
        WindowClass::Dialog,
        "dialog",
        "Every shell command a dialog window (`d-<host>-<n>`) may call; the guard narrows it to the dialog's own contract.",
    ),
    (
        WindowClass::Tray,
        "tray",
        "Every shell command a tray popover (`tray`, `tray-<item>`) may call.",
    ),
];

/// The file name of the generated sets in the crate's `permissions` folder.
pub const FILE: &str = "window-classes.toml";

/// Tauri's permission identifier for allowing `command` (`allow-health-snapshot`).
pub fn allow(command: &str) -> String {
    format!("allow-{}", command.replace('_', "-"))
}

/// The commands of `table` open to `class`, sorted.
pub fn commands_for(table: &Table, class: WindowClass) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = table
        .specs()
        .iter()
        .filter(|s| s.windows().contains(class))
        .map(|s| s.name())
        .collect();
    names.sort_unstable();
    names
}

/// The text of `permissions/window-classes.toml` for `table`. `regenerate` tells a reader how to rewrite it.
pub fn window_classes_toml(table: &Table, regenerate: &str) -> String {
    let mut out = format!(
        "# Generated from the command table of paddock-shell. Do not edit: {regenerate}.\n\n\
         [default]\n\
         description = \"The main window's set: every shell command a top-level window may call.\"\n\
         permissions = [\"main\"]\n"
    );
    for (class, id, description) in SETS {
        out.push_str(&format!(
            "\n[[set]]\nidentifier = \"{id}\"\ndescription = \"{description}\"\npermissions = [\n"
        ));
        for name in commands_for(table, class) {
            out.push_str(&format!("  \"{}\",\n", allow(name)));
        }
        out.push_str("]\n");
    }
    out
}

/// The text of `src/command_names.rs` (what the build step generates permissions for).
pub fn command_names_rs(table: &Table, regenerate: &str) -> String {
    let mut names: Vec<&str> = table.specs().iter().map(|s| s.name()).collect();
    names.sort_unstable();
    let mut out = format!(
        "// Generated from `paddock_shell::COMMANDS` by its tests. Do not edit: {regenerate}.\npub const COMMAND_NAMES: &[&str] = &[\n"
    );
    for n in names {
        out.push_str(&format!("    \"{n}\",\n"));
    }
    out.push_str("];\n");
    out
}
