//! The shell plugin with Spotter's parts, on Tauri's mock runtime: nothing is drawn, nothing outside the temp folder
//! is touched (rule 7).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_commands::testing::{sync_generated, WriteMode};
use spotter_app_lib::health::Hub;
use spotter_app_lib::{shell, Place, COMMANDS, MANIFEST};

fn builder() -> paddock_shell::Builder<tauri::test::MockRuntime> {
    let dir = tempfile::tempdir().unwrap();
    shell(
        Place::Test {
            dir: dir.path().to_path_buf(),
        },
        &Hub::default(),
    )
}

/// The contract file, the manifest, Spotter's table and the shell's table agree (a dialog contract naming a missing or
/// closed command, a command both tables use, a service or settings section the UI does not know).
#[test]
fn the_dialog_contracts_the_manifest_and_the_tables_agree() {
    let problems = builder().problems(Some(MANIFEST));
    assert!(problems.is_empty(), "{problems:#?}");
}

/// Spotter's services are the ones the UI registers, in the status bar's order.
#[test]
fn the_services_are_the_ones_the_ui_lists() {
    assert_eq!(builder().service_ids(), vec!["sp.scheduler", "sp.sources", "sp.renderer"]);
}

/// Spotter's table never shadows a shell command.
#[test]
fn no_command_is_in_both_tables() {
    for spec in COMMANDS.specs() {
        assert!(paddock_shell::COMMANDS.spec(spec.name()).is_none(), "{}", spec.name());
    }
}

const REGENERATE: &str = "cd app/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test shell";

/// The UI's command names are generated from the table, so a typo in a dialog contract is a compile error.
#[test]
fn the_uis_command_names_are_generated_from_the_table() {
    let text = paddock_commands::ts::names_module(&COMMANDS, "APP_COMMANDS", "AppCommand", REGENERATE).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/app/generated/app-commands.ts");
    sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}

/// Every command runs on the async runtime and returns at once (rule 10).
#[test]
fn every_command_is_async() {
    paddock_commands::testing::assert_async(&COMMANDS, &[&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")]);
}
