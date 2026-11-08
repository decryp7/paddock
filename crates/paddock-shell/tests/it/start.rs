//! The start the shell opens (ADR 0029), through `paddock_shell::testing::start` on the mock runtime: what fails it
//! fails fast and is named (the start's log line is tested in `paddock_shell::logging`, a binary without another log).
#![cfg(feature = "mock-runtime")]

use paddock_shell::testing::start;
use paddock_shell::{AppIdentity, Builder};
use paddock_window::launch::LaunchArgs;
use paddock_window::tauri::{HeadlessTray, Tray};
use paddock_window::tray::TrayView;
use tauri::test::mock_builder;

fn identity() -> AppIdentity {
    AppIdentity::new("demo", "Shell Demo", "1.2.3")
}

/// The tray and the launch open only through `add_to`: a shell built with `build()` that has them fails the start, so
/// the app cannot start without its window by accident.
#[test]
fn a_tray_or_launch_without_add_to_fails_the_start() {
    for b in [
        Builder::new(identity()).launch(LaunchArgs::default()),
        Builder::new(identity())
            .tray(|_| Tray::new(HeadlessTray, |_| TrayView::new("x", vec![], "normal"))),
    ] {
        let built = mock_builder()
            .plugin(b.in_memory().native_menu(false).build())
            .build(tauri::test::mock_context(tauri::test::noop_assets()));
        let e = built.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("tray/launch need Builder::add_to"), "{e}");
    }
}

/// A start that panics is reported as a panic, not as a deadlock.
#[test]
fn a_start_that_panics_is_named_as_one() {
    let e =
        start(|| -> tauri::Builder<tauri::test::MockRuntime> { panic!("the composition broke") })
            .unwrap_err();
    assert!(e.contains("panicked"), "{e}");
}

fn failed_start(b: Builder<tauri::test::MockRuntime>) -> String {
    mock_builder()
        .plugin(b.in_memory().native_menu(false).build())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default()
}

/// A part whose set-up closure fails fails the app's start, and the message names that part (so a user's log shows
/// which one), for every optional part.
#[test]
fn a_part_that_fails_to_set_up_is_named() {
    let cases: Vec<(&str, Builder<tauri::test::MockRuntime>)> = vec![
        (
            "Report a problem",
            Builder::new(identity()).diagnostics(|_| Err("no log folder".into())),
        ),
        (
            "user files",
            Builder::new(identity()).files(|_| Err("no folder".into())),
        ),
        (
            "storage",
            Builder::new(identity()).storage(|_| Err("no categories".into())),
        ),
        (
            "the knowledge packs",
            Builder::new(identity()).packs(|_| Err("no packs".into())),
        ),
        (
            "the notification channels",
            Builder::new(identity()).channels(|_, _| Err("no store".into())),
        ),
        (
            "the AI access settings",
            Builder::new(identity()).mcp(|_, _| Err("no keys".into())),
        ),
    ];
    for (part, b) in cases {
        let e = failed_start(b);
        assert!(
            e.contains("The shell could not set up") && e.contains(part),
            "{part}: {e}"
        );
    }
}

/// The composition test of an app asks whether diagnostics were set up.
#[test]
fn diagnostics_installed_follows_the_diagnostics_call() {
    assert!(!Builder::<tauri::test::MockRuntime>::new(identity()).diagnostics_installed());
    assert!(
        Builder::<tauri::test::MockRuntime>::new(identity())
            .diagnostics(|_| Err("x".into()))
            .diagnostics_installed()
    );
}
