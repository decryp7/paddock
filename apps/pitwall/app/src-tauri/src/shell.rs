//! Tauri glue for the window lifecycle: carries out the decisions of `window.rs`. Closing the window destroys it; this
//! module builds it again from the configuration (so a reopened window is a fresh webview, never a blank hidden one).

use crate::native_feel;
use crate::tray::NoTray;
use crate::window::{react, Lifecycle, Reaction, MAIN};
use tauri::{AppHandle, Manager, Runtime};

/// Whether the app has a tray icon.
pub fn has_tray<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<NoTray>().is_none()
}

/// Applies `event`'s reaction (except preventing an exit, which the caller does with the exit API) and returns it.
pub fn handle<R: Runtime>(app: &AppHandle<R>, event: Lifecycle) -> Reaction {
    let window = app.get_webview_window(MAIN);
    let reaction = react(event, window.is_some(), has_tray(app));
    match reaction {
        Reaction::FocusWindow => {
            if let Some(w) = window {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
        Reaction::CreateWindow => create_main(app),
        Reaction::DestroyWindow => {
            if let Some(w) = window {
                let _ = w.destroy();
            }
        }
        Reaction::KeepRunning | Reaction::Exit | Reaction::Nothing => {}
    }
    reaction
}

/// Builds the main window from `tauri.conf.json`.
pub fn create_main<R: Runtime>(app: &AppHandle<R>) {
    let Some(cfg) = app.config().app.windows.iter().find(|w| w.label == MAIN).cloned() else {
        eprintln!("Pitwall: the main window is not in the configuration");
        return;
    };
    let built = tauri::WebviewWindowBuilder::from_config(app, &cfg).and_then(native_feel::build);
    match built {
        Ok(w) => {
            let _ = w.set_focus();
        }
        Err(e) => eprintln!("Pitwall could not open its window: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mock_app_has_a_tray_until_marked_otherwise() {
        let app = tauri::test::mock_app();
        assert!(has_tray(app.handle()));
        app.manage(NoTray);
        assert!(!has_tray(app.handle()));
    }

    #[test]
    fn closing_without_a_tray_exits_and_with_one_stays() {
        let app = tauri::test::mock_app();
        let ev = Lifecycle::ExitRequested { code: None };
        assert_eq!(handle(app.handle(), ev), Reaction::KeepRunning);
        app.manage(NoTray);
        assert_eq!(handle(app.handle(), ev), Reaction::Exit);
    }

    #[test]
    fn open_without_a_window_asks_for_one_to_be_built() {
        let app = tauri::test::mock_app();
        // No `main` window in the mock app's configuration: building is attempted and reported, never a panic.
        assert_eq!(handle(app.handle(), Lifecycle::TrayOpen), Reaction::CreateWindow);
        assert_eq!(handle(app.handle(), Lifecycle::DockReopen), Reaction::CreateWindow);
    }
}
