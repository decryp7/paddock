//! The plugin's generated files (checked on every run; `PADDOCK_WRITE_TS=1` rewrites them): the command list of the
//! build step, the permission sets per window class, and the TypeScript of the command names and wire types.

use crate::{COMMANDS, permissions};
use paddock_commands::testing::{WriteMode, sync_generated};
use std::path::{Path, PathBuf};

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-shell --lib generated";

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The `shell` entry point of `@decryp7/paddock-ui`, where the plugin's command names are used.
fn ui_dir() -> PathBuf {
    crate_dir().join("../../ui/projects/paddock-ui/shell/generated")
}

fn shell_ts() -> String {
    use ts_rs::TS;
    let decls = [crate::AppIdentity::decl(), crate::VersionReply::decl()];
    let mut out = format!(
        "// Generated from the Rust wire types of paddock-shell (ts-rs). Do not edit: {REGENERATE}.\n\n\
         /** The plugin's name: its commands are `plugin:{name}|<command>`. */\n\
         export const SHELL_PLUGIN = '{name}';\n\n\
         /** The shell's version: the plugin's `shell_version` must answer it (one version across the boundary). */\n\
         export const SHELL_VERSION = '{version}';\n\n",
        name = crate::PLUGIN_NAME,
        version = crate::VERSION
    );
    for d in decls {
        out.push_str(&format!("export {d}\n\n"));
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

fn generated() -> Vec<(PathBuf, String)> {
    vec![
        (
            crate_dir().join("src/command_names.rs"),
            permissions::command_names_rs(&COMMANDS, REGENERATE),
        ),
        (
            crate_dir().join("permissions").join(permissions::FILE),
            permissions::window_classes_toml(&COMMANDS, REGENERATE),
        ),
        (
            ui_dir().join("shell-commands.ts"),
            paddock_commands::ts::names_module(
                &COMMANDS,
                "SHELL_COMMANDS",
                "ShellCommand",
                REGENERATE,
            )
            .unwrap(),
        ),
        (ui_dir().join("shell.ts"), shell_ts()),
    ]
}

#[test]
fn generated_files_are_current() {
    let mode = WriteMode::from_env();
    let stale: Vec<String> = generated()
        .into_iter()
        .filter_map(|(path, text)| {
            sync_generated(&path, &text, mode)
                .err()
                .map(|e| e.to_string())
        })
        .collect();
    assert!(stale.is_empty(), "{}", stale.join("\n"));
}

#[test]
fn the_build_step_lists_every_command_of_the_table() {
    let mut names: Vec<&str> = COMMANDS.specs().iter().map(|s| s.name()).collect();
    names.sort_unstable();
    assert_eq!(crate::command_names::COMMAND_NAMES, names.as_slice());
}

#[test]
fn every_command_is_async() {
    let roots: Vec<PathBuf> = [
        "paddock-shell",
        "paddock-startup",
        "paddock-health",
        "paddock-dialogs",
        "paddock-diag",
        "paddock-files",
        "paddock-storage",
        "paddock-service",
        "paddock-packs",
        "paddock-channels",
        "paddock-mcp",
        "paddock-jobs",
        "paddock-settings",
        "paddock-updater",
        "paddock-window",
        "paddock-menu",
    ]
    .iter()
    .map(|c| crate_dir().join("..").join(c).join("src"))
    .collect();
    let refs: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
    paddock_commands::testing::assert_async(&COMMANDS, &refs);
}

/// The shell's own dialog contracts (`pd.confirm`) as TypeScript, in the `shell/confirm` entry point (ADR 0026 §3, §5).
#[test]
fn generated_dialog_contracts_are_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/confirm/generated/dialog-contracts.ts");
    let text = paddock_dialogs::ts::contracts_module("paddock-shell", &crate::DIALOGS, REGENERATE);
    sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}
