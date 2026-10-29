//! The start test every app on the shell has (rule 43): the app's own composition (`Builder::add_to` with its tray and
//! launch) on Tauri's mock runtime, Tauri's set-up run once, and what opened. A fake tray backend
//! (`paddock_window::tauri::HeadlessTray`) stands in for the icon; the OS plugins (single instance, autostart, updater)
//! stay out (rule 7).

use tauri::Manager;
use tauri::test::{MockRuntime, mock_context, noop_assets};

/// How long the start may take before the test calls it a deadlock: a hang guard only a deadlocked start reaches (the
/// mock runtime starts in milliseconds), never a measure of a working one.
pub const DEADLOCKED_AFTER: std::time::Duration = std::time::Duration::from_secs(30);

/// What the start opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Started {
    /// The main window exists.
    pub main_window: bool,
    /// The tray icon exists.
    pub tray: bool,
}

/// Builds the app `compose` returns on its own thread, runs Tauri's set-up once and says what opened. `Err` names why
/// the start failed; a start that never returns (a window built inside a plugin hook) fails the test with that.
pub fn start(
    compose: impl FnOnce() -> tauri::Builder<MockRuntime> + Send + 'static,
) -> Result<Started, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let started = compose()
            .build(mock_context(noop_assets()))
            .map_err(|e| e.to_string())
            .map(|mut app| {
                // One iteration runs Tauri's set-up; the mock runtime has no event loop to run instead.
                #[allow(deprecated)]
                app.run_iteration(|_, _| {});
                Started {
                    main_window: app.get_webview_window("main").is_some(),
                    tray: paddock_window::tauri::tray_available(app.handle()),
                }
            });
        let _ = tx.send(started);
    });
    rx.recv_timeout(DEADLOCKED_AFTER).unwrap_or_else(|_| {
        Err("the start deadlocked: a window was built while Tauri held its plugin store".into())
    })
}
