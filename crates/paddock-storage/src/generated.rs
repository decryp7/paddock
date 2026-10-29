//! The generated TypeScript of storage, checked on every test run (a stale file fails CI): the wire types (ts-rs) and the command
//! names, in the `shell/storage` entry point of `@decryp7/paddock-ui`. Needs the `tauri` feature.

use crate::tauri::StorageEvent;
use crate::{
    CategoryClearResult, ClearChoice, ClearRequest, ItemView, Kind, SizeNode, Skipped,
    StorageCategory, StorageClearResult, StorageTree, StorageUsage,
};
use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str =
    "PADDOCK_WRITE_TS=1 cargo test -p paddock-storage --all-features --lib generated";

fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/storage/generated")
}

fn generated_files() -> Vec<(&'static str, String)> {
    let header = format!(
        "// Generated from the Rust wire types of paddock-storage (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        Kind::decl(),
        ItemView::decl(),
        StorageCategory::decl(),
        StorageUsage::decl(),
        Skipped::decl(),
        CategoryClearResult::decl(),
        ClearChoice::decl(),
        ClearRequest::decl(),
        StorageClearResult::decl(),
        SizeNode::decl(),
        StorageTree::decl(),
        StorageEvent::decl(),
    ];
    let types: Vec<String> = decls.into_iter().map(|d| format!("export {d}\n")).collect();
    let consts = format!(
        "\n/** The event the outcome of a scan or a clear is sent on, to every window (a `StorageEvent`). */\nexport const STORAGE_EVENT = '{}';\n",
        crate::EVENT_NAME
    );
    vec![
        (
            "storage.ts",
            format!("{header}{}{consts}", types.join("\n")),
        ),
        (
            "storage-commands.ts",
            paddock_commands::ts::names_module(
                &crate::COMMANDS,
                "STORAGE_COMMANDS",
                "StorageCommand",
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
fn the_table_has_the_storage_commands_for_main_windows_and_dialogs() {
    let names: Vec<&str> = crate::COMMANDS.specs().iter().map(|s| s.name()).collect();
    assert_eq!(
        names,
        [
            "storage_last",
            "storage_scan",
            "storage_clear",
            "storage_tree",
            "storage_cancel",
            "storage_reveal"
        ]
    );
    assert!(
        crate::COMMANDS.opens_to_dialogs("storage_clear"),
        "the Settings dialog clears"
    );
    assert_eq!(
        crate::COMMANDS.kind_of("storage_last"),
        paddock_commands::Kind::Query
    );
}

/// The dialog contracts of this crate as TypeScript (ADR 0026 §3), beside its other generated files.
#[test]
fn generated_dialog_contracts_are_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/storage/generated/dialog-contracts.ts");
    let text =
        paddock_dialogs::ts::contracts_module("paddock-storage", &crate::DIALOGS, REGENERATE);
    testing::sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}
