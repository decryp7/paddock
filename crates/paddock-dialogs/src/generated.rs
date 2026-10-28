//! The generated TypeScript of this crate's wire types (`ts-rs`), checked on every test run: a stale file fails, and
//! `PADDOCK_WRITE_TS=1` rewrites it.

use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-dialogs --lib generated";

/// Where the shell's generated TypeScript goes: the `shell` entry point of `@decryp7/paddock-ui`.
fn path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/generated/dialogs.ts")
}

fn text() -> String {
    let header = format!(
        "// Generated from the Rust wire types of paddock-dialogs (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        crate::Kind::decl(),
        crate::Modality::decl(),
        crate::Size::decl(),
        crate::Measure::decl(),
        crate::Remembered::decl(),
        crate::DialogRequest::decl(),
        crate::DialogInfo::decl(),
        crate::DialogStart::decl(),
        crate::DialogResult::decl(),
        crate::DialogClosed::decl(),
        crate::DialogErrorCode::decl(),
        crate::DialogError::decl(),
    ];
    let types: String = decls
        .into_iter()
        .map(|d| format!("export {d}\n\n"))
        .collect();
    let base = crate::BASE_COMMANDS
        .iter()
        .map(|c| format!("'{c}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{header}{types}\
/** The events of the dialog layer: to the opener (`result`, `closed`) and to the dialog (`closeRequested`). */
export const DIALOG_EVENTS = {{
  result: '{}',
  closed: '{}',
  closeRequested: '{}',
}} as const;

/** The commands every dialog window may call whatever its contract says. */
export const DIALOG_BASE_COMMANDS = [{base}] as const;
",
        crate::RESULT_EVENT,
        crate::CLOSED_EVENT,
        crate::CLOSE_REQUESTED_EVENT,
    )
}

/// The kind table every app shares (ADR 0026 §3).
fn kinds_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/generated/dialog-kinds.ts")
}

#[test]
fn generated_kind_table_is_current() {
    let synced = testing::sync_generated(
        &kinds_path(),
        &crate::ts::kinds_module(REGENERATE),
        WriteMode::from_env(),
    );
    synced.unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn generated_typescript_is_current() {
    let synced = testing::sync_generated(&path(), &text(), WriteMode::from_env());
    synced.unwrap_or_else(|e| panic!("{e}"));
}
