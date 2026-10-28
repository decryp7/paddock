//! The generated TypeScript of the checklist, checked on every test run (a stale file fails CI): the wire types (ts-rs),
//! the event name and the command names, in the `shell/setup` entry point of `@decryp7/paddock-ui`.

use super::model::{ItemStatus, ItemView, SetupCommandError, SetupErrorCode, SetupState};
use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-shell --lib setup::generated";

fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/projects/paddock-ui/shell/setup/generated")
}

fn generated_files() -> Vec<(&'static str, String)> {
    let header = format!(
        "// Generated from the Rust wire types of paddock-shell's set-up checklist (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        ItemStatus::decl(),
        ItemView::decl(),
        SetupState::decl(),
        SetupErrorCode::decl(),
        SetupCommandError::decl(),
    ];
    let types: Vec<String> = decls.into_iter().map(|d| format!("export {d}\n")).collect();
    let consts = format!(
        "\n/** The event every change of the checklist is sent on, to every window (a `SetupState`). */\nexport const SETUP_EVENT = '{}';\n\n\
/** The id of the Set-up settings page. */\nexport const SETUP_PAGE_ID = 'pd.setup';\n",
        super::EVENT
    );
    vec![
        ("setup.ts", format!("{header}{}{consts}", types.join("\n"))),
        (
            "setup-commands.ts",
            paddock_commands::ts::names_module(
                &super::COMMANDS,
                "SETUP_COMMANDS",
                "SetupCommand",
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
            testing::sync_generated(&generated_dir().join(name), &text, mode)
                .err()
                .map(|e| e.to_string())
        })
        .collect();
    assert!(stale.is_empty(), "{}", stale.join("\n"));
}

/// The checklist's dialog contract as TypeScript (ADR 0026 §3).
#[test]
fn generated_dialog_contracts_are_current() {
    let path = generated_dir().join("dialog-contracts.ts");
    let text = paddock_dialogs::ts::contracts_module(
        "paddock-shell (set-up)",
        &super::DIALOGS,
        REGENERATE,
    );
    testing::sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}
