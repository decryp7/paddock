//! The plugin's generated files (checked on every run; `PADDOCK_WRITE_TS=1` rewrites them): the command list of the
//! build step, the permission sets per window class and the TypeScript of the command names.

use crate::{COMMANDS, PLUGIN_NAME, permissions};
use paddock_commands::testing::{WriteMode, sync_generated};
use std::path::{Path, PathBuf};

const REGENERATE: &str =
    "PADDOCK_WRITE_TS=1 cargo test -p paddock-workbench --all-features --lib generated";

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The `workbench` entry point of `@decryp7/paddock-ui`, where the plugin's command names are used.
fn ui_dir() -> PathBuf {
    crate_dir().join("../../ui/projects/paddock-ui/workbench/generated")
}

fn plugin_ts() -> String {
    format!(
        "// Generated from paddock-workbench. Do not edit: {REGENERATE}.\n\n\
         /** The plugin's name: its commands are `plugin:{PLUGIN_NAME}|<command>`. */\n\
         export const WORKBENCH_PLUGIN = '{PLUGIN_NAME}';\n"
    )
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
            ui_dir().join("workbench-commands.ts"),
            paddock_commands::ts::names_module(
                &COMMANDS,
                "WORKBENCH_COMMANDS",
                "WorkbenchCommand",
                REGENERATE,
            )
            .unwrap(),
        ),
        (ui_dir().join("workbench.ts"), plugin_ts()),
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
fn the_composed_table_holds_every_part_once() {
    let n = |t: &paddock_commands::Table| t.specs().len();
    assert_eq!(
        n(&COMMANDS),
        n(&crate::LAYOUT) + n(&paddock_panels::COMMANDS) + n(&paddock_assistant::tauri::COMMANDS)
    );
}

#[test]
fn every_command_is_async() {
    let roots: Vec<PathBuf> = ["paddock-workbench", "paddock-panels", "paddock-assistant"]
        .iter()
        .map(|c| crate_dir().join("..").join(c).join("src"))
        .collect();
    let refs: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
    paddock_commands::testing::assert_async(&COMMANDS, &refs);
}

/// The plugin hands every window's events to the panel layer: the host going away closes its pop-outs.
#[test]
fn the_window_hook_closes_the_pop_outs_of_a_host_that_goes_away() {
    use paddock_panels::tauri::Panels;
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

    let app = mock_builder().build(mock_context(noop_assets())).unwrap();
    app.manage(Panels::builder().build());
    let window = |label: &str| {
        WebviewWindowBuilder::new(&app, label, WebviewUrl::default())
            .build()
            .unwrap()
    };
    let host = window("main");
    let info = paddock_panels::tauri::open(
        app.handle(),
        "main",
        serde_json::from_value(serde_json::json!({ "panel": "pn.code" })).unwrap(),
    )
    .unwrap();
    assert_eq!(info.label, "p-main-1");
    crate::builder::on_window_event(&host.as_ref().window(), &WindowEvent::Destroyed);
    assert!(app.state::<Panels>().popouts().of_host("main").is_empty());
}

/// The dock's dialog contracts as TypeScript (ADR 0026 §3).
#[test]
fn generated_dialog_contracts_are_current() {
    let path = crate_dir()
        .join("../../ui/projects/paddock-ui/workbench/dock/generated/dialog-contracts.ts");
    let text =
        paddock_dialogs::ts::contracts_module("paddock-workbench", &crate::DIALOGS, REGENERATE);
    sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}
