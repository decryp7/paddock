//! The Pitcrew app: tray icon and window over the Pitcrew service (ADR 0001). It holds no runner state of its own:
//! everything comes from the service through [`service_client::ServiceClient`].
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod commands;
pub mod dto;
pub mod service_client;
pub mod tray;
pub mod updater;

use commands::SharedClient;
use std::sync::Arc;

/// Starts the app.
pub fn run() {
    let client: SharedClient = Arc::new(service_client::LocalServiceClient);
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(updater::plugin())
        .manage(client)
        .invoke_handler(tauri::generate_handler![commands::runner_status, commands::app_update_status])
        .setup(|app| {
            tray::create(app.handle())?;
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
