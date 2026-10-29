//! The start's error lands in the app's log (ADR 0029): the shell's log installer into a temporary app folder (never the
//! real log folder, ER-14) and a start that fails on the mock runtime. Its own binary: it installs the process-wide log,
//! and other tests that log the same error or change the global level would race it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_shell::logging::install_in;
use paddock_shell::{AppIdentity, Builder};
use paddock_window::launch::LaunchArgs;
use paddock_window::tauri::{HeadlessTray, Tray, WindowConfig, Windows};
use paddock_window::tray::TrayView;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use tauri::test::mock_builder;

/// The process's log through the shell's own installer, once, into a temporary app folder (never the real log folder,
/// ER-14); every test of this binary may log into it. Returns the log folder.
fn log_dir() -> &'static PathBuf {
    static DIR: OnceLock<(tempfile::TempDir, PathBuf)> = OnceLock::new();
    &DIR.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let id = paddock_env::AppId::new("demo", "Shell Demo").unwrap();
        let dirs = paddock_env::AppDirs::resolve(
            &id,
            paddock_env::Os::current(),
            &paddock_env::MapVars::new().with(&id.home_var(), dir.path()),
            None,
            true,
        )
        .unwrap();
        let installed = install_in(
            &AppIdentity::new("demo", "Shell Demo", "1.2.3"),
            &dirs,
            false,
        );
        assert!(
            installed.is_some(),
            "the log installs into the app's folders"
        );
        let logs = dirs.logs().to_path_buf();
        (dir, logs)
    })
    .1
}

fn logged() -> String {
    std::fs::read_dir(log_dir())
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// A start that cannot open its window ends the app (code 1) and writes an ERROR line with the reason as its `Error`
/// field (redaction and dedupe work on fields).
#[test]
fn a_start_that_cannot_open_its_window_logs_an_error_and_exits() {
    log_dir();
    let exits = Arc::new(Mutex::new(Vec::new()));
    let codes = exits.clone();
    let started = paddock_shell::testing::start(move || {
        Builder::new(AppIdentity::new("demo", "Shell Demo", "1.2.3"))
            .in_memory()
            .native_menu(false)
            .windows(move |_| {
                Windows::builder(WindowConfig::new("Shell Demo"))
                    .exit_app(move |_, code| codes.lock().unwrap().push(code))
                    .build()
            })
            .tray(|_| {
                // Makes the start fail: this thread now counts as inside a plugin hook, so the main window is refused.
                std::mem::forget(paddock_window::tauri::plugin_hook_scope());
                Tray::new(HeadlessTray, |_| {
                    TrayView::new("Shell Demo", vec![], "normal")
                })
            })
            .launch(LaunchArgs::default())
            .add_to(mock_builder())
    });
    assert_eq!(
        started,
        Ok(paddock_shell::testing::Started {
            main_window: false,
            tray: true
        })
    );
    assert_eq!(*exits.lock().unwrap(), [1]);
    let log = logged();
    let line = log
        .lines()
        .find(|l| l.contains("could not open its window"))
        .unwrap_or_else(|| panic!("no start error in the log: {log}"));
    let event: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(event["@l"], "Error", "{line}");
    assert!(
        event["Error"]
            .as_str()
            .is_some_and(|e| e.contains("plugin")),
        "the reason is a field: {line}"
    );
}
