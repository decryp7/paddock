//! The app's own commands (everything that is not a management method): the service's registration, start at log-in
//! and updates.

use crate::app_state::{self, AppState, FirstRun};
use crate::commands::{forward, SharedClient};
use crate::service_install::{self, ServiceInstall};
use crate::updater::{self, AppUpdateStatus, UpdateInfo};
use pitwall_core::mgmt::{CommandError, ErrorCode};
use serde_json::Value;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

fn internal(m: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::Internal, m)
}

async fn compose_state(client: &SharedClient) -> ServiceInstall {
    let os = service_install::os_state();
    let running = forward(client.as_ref(), "overview", Value::Null).await.is_ok();
    let state = app_state::data_dir().map(|d| AppState::load(&d)).unwrap_or_default();
    let ask = app_state::first_run(&state, os.installed, running) == FirstRun::Ask;
    service_install::compose(os, running, ask)
}

fn mark_offered() {
    if let Some(dir) = app_state::data_dir() {
        let mut s = AppState::load(&dir);
        s.service_offered = true;
        if let Err(e) = s.save(&dir) {
            eprintln!("Pitwall: could not remember the answer: {e}");
        }
    }
}

/// The service's state, including whether to ask the user about it now.
#[tauri::command]
pub async fn service_state(client: State<'_, SharedClient>) -> Result<ServiceInstall, CommandError> {
    Ok(compose_state(client.inner()).await)
}

/// Registers and starts the service (the user said yes).
#[tauri::command]
pub async fn install_service(client: State<'_, SharedClient>) -> Result<ServiceInstall, CommandError> {
    tauri::async_runtime::spawn_blocking(service_install::install)
        .await
        .map_err(|e| internal(format!("installing the service stopped unexpectedly: {e}")))??;
    mark_offered();
    Ok(compose_state(client.inner()).await)
}

/// Stops and removes the service.
#[tauri::command]
pub async fn uninstall_service(client: State<'_, SharedClient>) -> Result<ServiceInstall, CommandError> {
    tauri::async_runtime::spawn_blocking(service_install::uninstall)
        .await
        .map_err(|e| internal(format!("removing the service stopped unexpectedly: {e}")))??;
    Ok(compose_state(client.inner()).await)
}

/// The user said no to the service; do not ask again.
#[tauri::command]
pub fn decline_service() {
    mark_offered();
}

/// Whether Pitwall starts at log-in.
#[tauri::command]
pub fn get_autostart(app: AppHandle) -> Result<bool, CommandError> {
    app.autolaunch()
        .is_enabled()
        .map_err(|e| internal(format!("start at log-in could not be read: {e}")))
}

/// Turns start at log-in (in the tray, without a window) on or off. Off until the user turns it on.
#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, CommandError> {
    let launch = app.autolaunch();
    let done = if enabled { launch.enable() } else { launch.disable() };
    done.map_err(|e| internal(format!("start at log-in could not be changed: {e}")))?;
    launch
        .is_enabled()
        .map_err(|e| internal(format!("start at log-in could not be read: {e}")))
}

/// Whether this build can update itself (`AppUpdateStatus` in `api.types.ts`).
#[tauri::command]
pub fn app_update_status(app: AppHandle) -> AppUpdateStatus {
    updater::status(&app)
}

/// Checks for a release; only when the user asks.
#[tauri::command]
pub async fn check_app_update(app: AppHandle) -> Result<Option<UpdateInfo>, CommandError> {
    updater::check(&app).await
}

/// Installs the update found by the last check, then restarts the app.
#[tauri::command]
pub async fn install_app_update(app: AppHandle) -> Result<(), CommandError> {
    updater::install(&app).await
}
