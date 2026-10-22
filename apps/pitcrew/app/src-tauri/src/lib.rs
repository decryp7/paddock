//! The Pitcrew app: tray icon and window over the Pitcrew service (ADR 0001). It holds no runner state of its own:
//! everything comes from the service through [`service_client::ServiceClient`], pushed by `events`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod commands;
pub mod credentials;
pub mod diagnostics;
pub mod dto;
pub mod events;
pub mod gitea;
pub mod native_feel;
pub mod service_client;
pub mod service_install;
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
        .manage(events::Latest::default())
        .manage(events::Actions::new(client))
        .manage(updater::Pending::default())
        .invoke_handler(tauri::generate_handler![
            commands::platform_name,
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
            commands::app_update_status,
            commands::check_app_update,
            commands::install_app_update,
            commands::get_autostart,
            commands::set_autostart,
            commands::export_diagnostics,
            commands::open_url,
            commands::reveal_logs,
        ])
        .setup(|app| {
            tray::create(app.handle())?;
            events::start(app.handle().clone());
            {
                use tauri::Manager;
                if let Some(w) = app.get_webview_window("main") {
                    native_feel::apply(&w);
                }
            }
            // Started at log-in: stay in the tray.
            if std::env::args().any(|a| a == "--hidden") {
                use tauri::Manager;
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps Pitcrew in the tray (Quit is in the tray menu).
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("Pitcrew could not start: {e}");
        std::process::exit(1);
    }
}
