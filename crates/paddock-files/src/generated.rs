//! The generated TypeScript of user files, checked on every test run (a stale file fails CI): the wire types (ts-rs) and the
//! command names, in the `shell/files` entry point of `@decryp7/paddock-ui`. Needs the `tauri` feature.

use crate::tauri::DroppedFiles;
use crate::{AppFolder, PickFilter, PickKind, PickRequest, RecentEntry, RecentList, RecentLists};
use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str =
    "PADDOCK_WRITE_TS=1 cargo test -p paddock-files --all-features --lib generated";

fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/projects/paddock-ui/shell/files/generated")
}

fn generated_files() -> Vec<(&'static str, String)> {
    let header = format!(
        "// Generated from the Rust wire types of paddock-files (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        PickKind::decl(),
        PickFilter::decl(),
        PickRequest::decl(),
        AppFolder::decl(),
        RecentEntry::decl(),
        RecentList::decl(),
        RecentLists::decl(),
        DroppedFiles::decl(),
    ];
    let types: Vec<String> = decls.into_iter().map(|d| format!("export {d}\n")).collect();
    let consts = format!(
        "\n/** Sent to every window when a recent list changed (a `RecentLists`). */\nexport const FILES_RECENT_EVENT = '{}';\n\n/** Sent to a window when files dropped on it were granted (a `DroppedFiles`). */\nexport const FILES_DROPPED_EVENT = '{}';\n",
        crate::RECENT_EVENT,
        crate::DROPPED_EVENT
    );
    vec![
        ("files.ts", format!("{header}{}{consts}", types.join("\n"))),
        (
            "files-commands.ts",
            paddock_commands::ts::names_module(
                &crate::COMMANDS,
                "FILES_COMMANDS",
                "FilesCommand",
                REGENERATE,
            )
            .unwrap_or_default(),
        ),
    ]
}

#[test]
fn generated_typescript_is_current() {
    let mode = WriteMode::from_env();
    let stale: Vec<String> = generated_files()
        .into_iter()
        .filter_map(|(name, text)| {
            testing::sync_generated(&generated_dir().join(name), &text, mode).err()
        })
        .map(|e| e.to_string())
        .collect();
    assert!(stale.is_empty(), "{}", stale.join("\n"));
}

#[test]
fn the_table_has_the_file_commands_and_the_pickers_are_for_windows_that_open() {
    let names: Vec<&str> = crate::COMMANDS.specs().iter().map(|s| s.name()).collect();
    assert_eq!(
        names,
        [
            "files_pick",
            "files_recent_list",
            "files_recent_remove",
            "files_recent_clear",
            "files_recent_grant",
            "files_reveal",
            "files_open",
            "files_open_app_folder",
            "files_open_url"
        ]
    );
    assert!(crate::COMMANDS.opens_to_dialogs("files_pick"));
    assert!(
        !crate::COMMANDS.opens_to_dialogs("files_recent_grant"),
        "only a main window or the tray re-grants"
    );
}
