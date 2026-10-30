//! Pitwall on the shell plugin, on Tauri's mock runtime: nothing is drawn, nothing outside the test's folder is touched,
//! no keychain is read (ER-14). The start as `run` composes it, without the OS plugins (single instance, autostart,
//! updater).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_shell::testing::{start, Started};
use paddock_window::launch::LaunchArgs;
use pitwall_app_lib::{app, shell, Place, COMMANDS};

/// Pitwall as `run` composes it, its state and live feed included (its files in `dir`, a server that is not there).
fn composed(dir: &std::path::Path, launch: LaunchArgs) -> tauri::Builder<tauri::test::MockRuntime> {
    let shell = shell(Place::Test { dir: dir.to_path_buf() });
    app(tauri::test::mock_builder(), shell, launch, paddock_window::tauri::HeadlessTray)
}

/// The start (rule 43, ADR 0029): the tray and the main window open, after every plugin is set up (a window built inside
/// a plugin hook deadlocks the start).
#[test]
fn the_start_opens_the_tray_and_the_main_window() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_path_buf();
    assert_eq!(
        start(move || composed(&data, LaunchArgs::default())),
        Ok(Started {
            main_window: true,
            tray: true
        })
    );
}

/// Started at log-in (`--hidden`) with a tray: Pitwall stays in the tray without a window.
#[test]
fn a_hidden_start_stays_in_the_tray() {
    let launch = LaunchArgs {
        hidden: true,
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_path_buf();
    assert_eq!(
        start(move || composed(&data, launch)),
        Ok(Started {
            main_window: false,
            tray: true
        })
    );
}

/// Pitwall's table and the shell's agree (no command in both, every name valid), with no UI manifest yet.
#[test]
fn the_tables_agree() {
    let dir = tempfile::tempdir().unwrap();
    let problems = shell::<tauri::test::MockRuntime>(Place::Test {
        dir: dir.path().to_path_buf(),
    })
    .problems(None);
    assert!(problems.is_empty(), "{problems:#?}");
}

/// Every command the UI calls is in the table, the chat has room for its largest request, and the tray popover may
/// reach only the few commands it shows.
#[test]
fn the_command_table_covers_the_ui_and_narrows_the_tray() {
    let names: Vec<&str> = COMMANDS.specs().iter().map(|s| s.name()).collect();
    let backend = include_str!("../../src/app/core/tauri-backend.ts");
    for name in [
        "overview",
        "chat",
        "quit_app",
        "start_app_update",
        "app_job_cancel",
        "save_settings",
    ] {
        assert!(names.contains(&name), "{name}");
        assert!(backend.contains(&format!("'{name}'")), "the UI calls {name}");
    }
    let chat = COMMANDS.spec("chat").unwrap();
    assert!(
        chat.limit() >= pitwall_app_lib::commands::MAX_CHAT_BYTES,
        "a chat of the largest accepted size fits"
    );
    let tray: Vec<&str> = COMMANDS
        .specs()
        .iter()
        .filter(|s| s.windows().contains(paddock_commands::WindowClass::Tray))
        .map(|s| s.name())
        .collect();
    assert!(tray.contains(&"overview") && tray.contains(&"set_serving"), "{tray:?}");
    assert!(!tray.contains(&"delete_model") && !tray.contains(&"save_settings"), "{tray:?}");
}
