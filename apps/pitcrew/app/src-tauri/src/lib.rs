//! The Pitcrew app: tray icon and window over the Pitcrew service (ADR 0001). It holds no runner state of its own:
//! everything comes from the service through [`service_client::ServiceClient`], pushed by `events`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod applog;
pub mod channels;
pub mod commands;
pub mod credentials;
pub mod diagnostics;
pub mod dto;
pub mod events;
pub mod gitea;
pub mod handoff;
pub mod native_feel;
pub mod ops;
pub mod service_client;
pub mod service_install;
pub mod service_repair;
pub mod tray;
pub mod updater;

use commands::SharedClient;
use std::sync::Arc;

/// Starts the app.
pub fn run() {
    let client: SharedClient = Arc::new(service_client::LocalServiceClient);
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(updater::plugin())
        .manage(client.clone())
        .manage(events::PendingNav::default())
        .manage(events::Latest::default())
        .manage(events::Actions::new(client))
        .manage(updater::Pending::default())
        .invoke_handler(tauri::generate_handler![
            commands::platform_name,
            commands::take_pending_navigation,
            commands::runner_status,
            commands::connection_state,
            commands::runner_action,
            commands::skip_version,
            commands::pin_version,
            commands::runner_logs,
            commands::search_logs,
            commands::runner_activity,
            commands::machine_metrics,
            commands::runner_disk,
            commands::clean_disk,
            commands::runner_config_get,
            commands::runner_config_preview,
            commands::runner_config_save,
            commands::runner_processes,
            commands::runner_process_stop,
            commands::runner_process_keep,
            commands::run_details,
            commands::get_settings,
            commands::save_settings,
            commands::add_runner,
            commands::remove_runner,
            commands::register_runner,
            commands::has_api_token,
            commands::set_api_token,
            commands::clear_api_token,
            commands::service_install_state,
            commands::install_service,
            commands::uninstall_service,
            commands::open_login_items,
            commands::app_update_status,
            commands::check_app_update,
            commands::install_app_update,
            commands::get_autostart,
            commands::set_autostart,
            commands::export_diagnostics,
            commands::open_url,
            commands::reveal_logs,
            ops::fleet_insights,
            ops::engine_report,
            ops::engine_prune_preview,
            ops::engine_prune,
            ops::cache_report,
            ops::cache_preview,
            ops::cache_clean,
            ops::debrief_status,
            ops::open_in_debrief,
            ops::channels_get,
            ops::channels_save,
            ops::channel_secret_set,
            ops::channel_secret_clear,
            ops::channel_secret_present,
            ops::channel_test,
        ])
        .setup(|app| {
            // After an update launchd may refuse the registered service (a rebuilt app has a new code identity on
            // macOS): register it again in the background, so the runners come back without the user's help.
            #[cfg(target_os = "macos")]
            std::thread::spawn(|| {
                // The service may only fail later (it restarts itself onto a new binary when idle), so keep checking
                // while it does not answer, a bounded number of times per run of the app.
                for attempt in 0..service_install::REPAIR_ATTEMPTS {
                    std::thread::sleep(service_install::repair_delay(attempt));
                    match service_install::repair_registration() {
                        Some(service_repair::RepairOutcome::Recovered | service_repair::RepairOutcome::NeedsApproval) => break,
                        // Failed: the next check tries again (bounded by REPAIR_ATTEMPTS); nothing to repair: keep watching.
                        Some(service_repair::RepairOutcome::Failed(_)) | None => {}
                    }
                }
            });
            if let Err(e) = tray::create(app.handle()) {
                // Linux without a status-notifier host or libayatana-appindicator: the window is then the only way
                // in, so closing it must exit (see `tray::keeps_running`).
                if cfg!(target_os = "linux") {
                    eprintln!("Pitcrew: no tray icon ({e}); closing the window will quit the app");
                    use tauri::Manager;
                    app.manage(tray::NoTray);
                } else {
                    return Err(e.into());
                }
            }
            events::start(app.handle().clone());
            {
                use tauri::Manager;
                if let Some(w) = app.get_webview_window(tray::MAIN) {
                    native_feel::apply(&w);
                }
            }
            // Started at log-in: stay in the tray, without a webview (only when there is a tray to come back from).
            if std::env::args().any(|a| a == "--hidden") && tray::has_tray(app.handle()) {
                use tauri::Manager;
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
            eprintln!("Pitcrew could not start: {e}");
            std::process::exit(1);
        }
    };
    // Closing the window destroys it and Pitcrew stays in the tray (a hidden webview could come back blank on macOS
    // and kept its memory); Open in the tray, or the app's Dock icon on macOS, creates it again. Quit exits.
    app.run(|app, event| match event {
        tauri::RunEvent::ExitRequested { code, api, .. } if tray::keeps_running(code, tray::has_tray(app)) => api.prevent_exit(),
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => tray::show_main(app),
        _ => {}
    });
}

#[cfg(test)]
mod capability_tests {
    /// The window has an overlay title bar on macOS (and no native one where decorations are off): the page's drag
    /// regions only move it when the webview may start a window drag. Without the permission dragging fails silently.
    #[test]
    fn the_main_window_can_be_dragged_by_its_drag_regions() {
        let caps: serde_json::Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let granted: Vec<&str> = caps["permissions"].as_array().unwrap().iter().filter_map(|p| p.as_str()).collect();
        assert!(granted.contains(&"core:window:allow-start-dragging"), "{granted:?}");
        assert!(granted.contains(&"core:window:allow-internal-toggle-maximize"), "{granted:?}");
        assert!(caps["windows"].as_array().unwrap().iter().any(|w| w == "main"));
    }
}
