//! The generated TypeScript of reports, checked on every test run (a stale file fails CI): the wire types (ts-rs) and the
//! command names, in the `shell/diagnostics` entry point of `@decryp7/paddock-ui`. Needs the `tauri` feature (the command
//! replies live in the adapter); CI runs the crate with `--all-features`.

#![cfg(feature = "tauri")]

use crate::tauri::{DiagCommandError, DiagErrorCode, PreviewReply, SaveOutcome};
use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str =
    "PADDOCK_WRITE_TS=1 cargo test -p paddock-diag --all-features --lib generated";

fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/diagnostics/generated")
}

fn generated_files() -> Vec<(&'static str, String)> {
    let header = format!(
        "// Generated from the Rust wire types of paddock-diag (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        crate::FileKind::decl(),
        crate::AppInfo::decl(),
        crate::RedactionCounts::decl(),
        crate::PreviewFile::decl(),
        crate::Preview::decl(),
        crate::Saved::decl(),
        PreviewReply::decl(),
        SaveOutcome::decl(),
        DiagErrorCode::decl(),
        DiagCommandError::decl(),
    ];
    let types: Vec<String> = decls.into_iter().map(|d| format!("export {d}\n")).collect();
    let consts = format!(
        "\n/** The event the outcome of a save is sent on, to every window. */\nexport const DIAGNOSTICS_EVENT = '{}';\n",
        crate::EVENT_NAME
    );
    // The Logging section as `settings_overview` answers it, written by the settings service from the Rust schema: the shell demo's
    // browser build shows it without a Rust engine.
    let settings = paddock_settings::Settings::in_memory();
    settings
        .register(std::sync::Arc::new(crate::logging::schema()))
        .unwrap();
    let section = serde_json::to_string_pretty(&settings.overview().sections[0]).unwrap();
    let fixtures = format!(
        "// Generated from paddock-diag's own code by its tests (Rust round trips). Do not edit: {REGENERATE}.\n\n\
import type {{ SectionView }} from '@decryp7/paddock-ui/shell/settings';\n\n\
/** The \"Logging\" settings section at its defaults, as `settings_overview` answers it. */\n\
export const LOGGING_SECTION_FIXTURE: SectionView = {section};\n"
    );
    vec![
        ("diagnostics-fixtures.ts", fixtures),
        (
            "diagnostics.ts",
            format!("{header}{}{consts}", types.join("\n")),
        ),
        (
            "diagnostics-commands.ts",
            paddock_commands::ts::names_module(
                &crate::COMMANDS,
                "DIAGNOSTICS_COMMANDS",
                "DiagnosticsCommand",
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
fn the_table_has_the_report_commands() {
    let names: Vec<&str> = crate::COMMANDS.specs().iter().map(|s| s.name()).collect();
    assert_eq!(
        names,
        [
            "diagnostics_preview",
            "diagnostics_save",
            "diagnostics_cancel",
            "log_from_webview"
        ]
    );
    assert!(
        crate::COMMANDS.opens_to_dialogs("diagnostics_save"),
        "the Report a Problem dialog saves"
    );
    assert!(
        crate::COMMANDS.opens_to_dialogs("log_from_webview"),
        "any window may forward its log lines"
    );
    assert_eq!(
        crate::COMMANDS.kind_of("log_from_webview"),
        paddock_commands::Kind::Quiet
    );
}

/// The dialog contracts of this crate as TypeScript (ADR 0026 §3), beside its other generated files.
#[test]
fn generated_dialog_contracts_are_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/diagnostics/generated/dialog-contracts.ts");
    let text = paddock_dialogs::ts::contracts_module("paddock-diag", &crate::DIALOGS, REGENERATE);
    testing::sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}
