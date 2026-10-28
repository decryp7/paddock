//! The generated TypeScript of the updater and About, checked on every test run (a stale file fails CI): the wire types
//! (ts-rs) and the command names, in the `shell/about` entry point of `@decryp7/paddock-ui`.

use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-updater --lib generated";

fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/projects/paddock-ui/shell/about/generated")
}

fn generated_files() -> Vec<(&'static str, String)> {
    let header = format!(
        "// Generated from the Rust wire types of paddock-updater (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        crate::UpdateErrorKind::decl(),
        crate::UpdateErrorDto::decl(),
        crate::UpdateInfo::decl(),
        crate::UpdatePhase::decl(),
        crate::ChannelInfo::decl(),
        crate::UpdateStatus::decl(),
        crate::Preferences::decl(),
        crate::LegalDocInfo::decl(),
        crate::AboutInfo::decl(),
        crate::NotesGroup::decl(),
        crate::ReleaseNotes::decl(),
        crate::WhatsNew::decl(),
        crate::WhatsNewDecision::decl(),
    ];
    let types: Vec<String> = decls.into_iter().map(|d| format!("export {d}\n")).collect();
    let consts = format!(
        "\n/** The event every `UpdateStatus` is sent on, to every window. */\nexport const UPDATES_EVENT = '{}';\n\n/** The job phase of the signature check (bytes). */\nexport const SIGNATURE_PHASE = '{}';\n",
        crate::STATUS_EVENT,
        crate::SIGNATURE_PHASE
    );
    vec![
        ("about.ts", format!("{header}{}{consts}", types.join("\n"))),
        (
            "about-commands.ts",
            paddock_commands::ts::names_module(
                &crate::COMMANDS,
                "ABOUT_COMMANDS",
                "AboutCommand",
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
fn the_table_has_the_updater_and_about_commands() {
    let names: Vec<&str> = crate::COMMANDS.specs().iter().map(|s| s.name()).collect();
    assert_eq!(
        names,
        [
            "updates_status",
            "updates_check",
            "updates_download",
            "updates_cancel",
            "updates_install_on_quit",
            "updates_restart",
            "updates_set_preferences",
            "about_info",
            "about_legal",
            "about_whats_new",
            "about_whats_new_start"
        ]
    );
    assert!(
        crate::COMMANDS.opens_to_dialogs("updates_download"),
        "the Updates dialog runs the steps"
    );
    assert!(
        !crate::COMMANDS.opens_to_dialogs("about_whats_new_start"),
        "decided once, by the main window"
    );
    assert_eq!(
        crate::COMMANDS.kind_of("about_legal"),
        paddock_commands::Kind::Query
    );
}

/// The dialog contracts of this crate as TypeScript (ADR 0026 §3), beside its other generated files.
#[test]
fn generated_dialog_contracts_are_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/about/generated/dialog-contracts.ts");
    let text =
        paddock_dialogs::ts::contracts_module("paddock-updater", &crate::DIALOGS, REGENERATE);
    testing::sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}
