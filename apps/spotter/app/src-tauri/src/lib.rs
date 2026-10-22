//! The Spotter app: tray icon, popup and window over the engine (ADR 0003). The shell holds no watch state of its own:
//! the engine owns it, the commands map the engine to the UI's `SpotterBackend`, and changes are pushed on
//! `spotter://state`. Closing the window keeps Spotter in the tray; only Quit exits.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod applog;
pub mod commands;
pub mod credentials;
pub mod dto;
pub mod lifecycle;
pub mod native_feel;
pub mod notifier;
pub mod state;
pub mod tray;
pub mod updater;

use tauri::Manager;

/// Starts the app.
pub fn run() {
    let result = tauri::Builder::default()
        // First: a second launch shows the running app's window instead of starting another engine.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app, None)))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(updater::plugin())
        .manage(updater::Pending::default())
        .manage(tray::PopupGuard::default())
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::list_watches,
            commands::get_watch,
            commands::source_kinds,
            commands::test_source,
            commands::save_watch,
            commands::delete_watch,
            commands::set_paused,
            commands::check_now,
            commands::acknowledge,
            commands::acknowledge_product,
            commands::suggest_pattern,
            commands::test_pattern,
            commands::version_schemes,
            commands::reset_acknowledgement,
            commands::set_installed,
            commands::open_release,
            commands::download_release,
            commands::dashboard,
            commands::events,
            commands::get_settings,
            commands::save_settings,
            commands::pause_notifications,
            commands::get_autostart,
            commands::set_autostart,
            commands::open,
            commands::choose_folder,
            commands::choose_program,
            commands::presets,
            commands::discover_feeds,
            commands::renderer_status,
            commands::install_renderer,
            commands::import_list,
            commands::app_update_status,
            commands::check_app_update,
            commands::install_app_update,
            commands::show_main_window,
            commands::hide_tray,
        ])
        .setup(|app| {
            // The engine opens before the event loop runs, so no command can arrive before it exists; nothing here
            // waits for the UI.
            let state = lifecycle::init(app.handle()).map_err(|e| {
                applog::write(&format!("Spotter could not start: {e}"));
                e
            })?;
            let watches = state.engine.list_watches();
            let paused = lifecycle::notifications_paused(&state.engine);
            app.manage(state);
            if let Err(e) = tray::create(app.handle(), &watches, paused) {
                // Linux without a status-notifier host or libayatana-appindicator: the window is then the only way
                // in, so closing it must exit (see `tray::keeps_running`).
                if cfg!(target_os = "linux") {
                    applog::write(&format!("no tray icon ({e}); closing the window will quit the app"));
                    app.manage(tray::NoTray);
                } else {
                    return Err(e.into());
                }
            }
            if let Some(w) = app.get_webview_window(tray::MAIN) {
                native_feel::apply(&w);
            }
            // Started at log-in: stay in the tray, without a webview (only when there is a tray to come back from).
            if std::env::args().any(|a| a == "--hidden") && tray::has_tray(app.handle()) {
                if let Some(w) = app.get_webview_window(tray::MAIN) {
                    let _ = w.destroy();
                }
            }
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match result {
        Ok(app) => app,
        Err(e) => {
            applog::write(&format!("Spotter could not start: {e}"));
            std::process::exit(1);
        }
    };
    app.run(|app, event| match event {
        tauri::RunEvent::ExitRequested { code, api, .. } if tray::keeps_running(code, tray::has_tray(app)) => api.prevent_exit(),
        tauri::RunEvent::Exit => lifecycle::stop(app),
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => tray::show_main(app, None),
        _ => {}
    });
}

#[cfg(test)]
mod capability_tests {
    fn caps() -> serde_json::Value {
        serde_json::from_str(include_str!("../capabilities/default.json")).unwrap()
    }

    /// The window has an overlay title bar on macOS: the page's drag regions only move it when the webview may start
    /// a window drag. Without the permission dragging fails silently.
    #[test]
    fn the_main_window_can_be_dragged_by_its_drag_regions() {
        let c = caps();
        let granted: Vec<&str> = c["permissions"].as_array().unwrap().iter().filter_map(|p| p.as_str()).collect();
        assert!(granted.contains(&"core:window:allow-start-dragging"), "{granted:?}");
        assert!(granted.contains(&"core:window:allow-internal-toggle-maximize"), "{granted:?}");
        assert!(c["windows"].as_array().unwrap().iter().any(|w| w == "main"));
        assert!(
            c["windows"].as_array().unwrap().iter().any(|w| w == "tray"),
            "the popup needs events too"
        );
    }

    /// The webview reaches the OS only through Spotter's own commands: no plugin permission, no wildcard.
    #[test]
    fn the_capability_is_least_privilege() {
        let c = caps();
        for p in c["permissions"].as_array().unwrap().iter().filter_map(|p| p.as_str()) {
            assert!(
                p.starts_with("core:window:allow-") || p == "core:event:default",
                "unexpected permission {p}"
            );
            assert!(!p.contains('*'), "{p}");
        }
        assert!(c.get("remote").is_none(), "no remote content may use IPC");
    }

    /// No `unsafe-eval`, no remote origin, nothing but the app's own scheme and IPC.
    #[test]
    fn the_csp_has_no_unsafe_eval_and_no_remote_origin() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let csp = conf["app"]["security"]["csp"].as_str().unwrap();
        assert!(!csp.contains("unsafe-eval"), "{csp}");
        assert!(!csp.contains("http://") || csp.contains("http://ipc.localhost"), "{csp}");
        assert!(!csp.contains("https:"), "{csp}");
        assert!(!csp.contains('*'), "{csp}");
        assert!(csp.contains("object-src 'none'") && csp.contains("frame-ancestors 'none'"));
    }

    /// The bundle step checks the built index.html first (a CSP nonce would break Angular's styles).
    #[test]
    fn the_build_checks_the_index_before_bundling() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert!(conf["build"]["beforeBuildCommand"].as_str().unwrap().contains("check-index.mjs"));
    }
}
