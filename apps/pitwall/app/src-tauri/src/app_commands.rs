//! The app's own commands (everything that is not a management method): the service's registration, start at log-in
//! and updates.

use crate::app_state::{self, AppState, FirstRun};
use crate::commands::{forward, SharedClient};
use crate::jobs::{JobContext, JobEvent, JobSink, JobSpec, Jobs};
use crate::service_install::{self, ServiceInstall};
use crate::updater::{self, AppUpdateStatus, UpdateInfo};
use pitwall_core::mgmt::{CommandError, ErrorCode};
use serde_json::Value;
use std::sync::Arc;
use tauri::ipc::Channel;
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

/// Repairs the background service (macOS): removes the registration launchd refuses, waits until launchd dropped it and
/// registers again. Runs in the background; the state it returns shows the progress. This is the app's own business and
/// does not need the server.
#[tauri::command]
pub async fn repair_service(client: State<'_, SharedClient>) -> Result<ServiceInstall, CommandError> {
    let c = client.inner().clone();
    let answers = move || tauri::async_runtime::block_on(forward(c.as_ref(), "overview", Value::Null)).is_ok();
    tauri::async_runtime::spawn_blocking(move || service_install::request_repair(answers))
        .await
        .map_err(|e| internal(format!("repairing the service stopped unexpectedly: {e}")))??;
    Ok(compose_state(client.inner()).await)
}

/// Opens System Settings > General > Login Items (macOS); the user switches the item, Pitwall never does.
#[tauri::command]
pub async fn open_login_items() -> Result<(), CommandError> {
    tauri::async_runtime::spawn_blocking(service_install::open_login_items)
        .await
        .map_err(|e| internal(format!("opening Login Items stopped unexpectedly: {e}")))?
}

/// Runs blocking work (file or OS calls) off the command's thread; a panic or cancellation becomes an error.
async fn blocking<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> Result<T, CommandError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| internal(format!("{what} stopped unexpectedly: {e}")))
}

/// The user said no to the service; do not ask again.
#[tauri::command]
pub async fn decline_service() -> Result<(), CommandError> {
    blocking("remembering the answer", mark_offered).await
}

/// Whether Pitwall starts at log-in.
#[tauri::command]
pub async fn get_autostart(app: AppHandle) -> Result<bool, CommandError> {
    blocking("reading start at log-in", move || {
        app.autolaunch()
            .is_enabled()
            .map_err(|e| internal(format!("start at log-in could not be read: {e}")))
    })
    .await?
}

/// Turns start at log-in (in the tray, without a window) on or off. Off until the user turns it on.
#[tauri::command]
pub async fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, CommandError> {
    blocking("changing start at log-in", move || {
        let launch = app.autolaunch();
        let done = if enabled { launch.enable() } else { launch.disable() };
        done.map_err(|e| internal(format!("start at log-in could not be changed: {e}")))?;
        launch
            .is_enabled()
            .map_err(|e| internal(format!("start at log-in could not be read: {e}")))
    })
    .await?
}

/// Whether this build can update itself (`AppUpdateStatus` in `api.types.ts`).
#[tauri::command]
pub async fn app_update_status(app: AppHandle) -> Result<AppUpdateStatus, CommandError> {
    blocking("reading the update status", move || updater::status(&app)).await
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

// Long operations as jobs (ER-17..19). Each `start_*` returns the job id at once and reports on the channel; `job_cancel`
// stops one. The blocking commands above stay until the UI has moved over.

fn channel_sink(channel: Channel<JobEvent>) -> JobSink {
    Arc::new(move |event| {
        let _ = channel.send(event);
    })
}

fn spec(title: &str, phase: &str, cancellable: bool) -> JobSpec {
    JobSpec {
        title: title.into(),
        phase: phase.into(),
        cancellable,
    }
}

/// Runs one server method as a job. Cancel stops waiting for it (the server keeps what it was doing).
fn server_job(
    jobs: &Arc<Jobs>,
    client: SharedClient,
    on_event: Channel<JobEvent>,
    spec: JobSpec,
    method: &'static str,
    args: Value,
) -> u64 {
    let phase = spec.phase.clone();
    jobs.start(spec, channel_sink(on_event), move |ctx: JobContext| async move {
        ctx.phase(&phase);
        forward(client.as_ref(), method, args).await
    })
}

/// Loads a model in the background (`load_model` as a job).
#[tauri::command]
pub async fn start_load_model(
    client: State<'_, SharedClient>,
    jobs: State<'_, Arc<Jobs>>,
    id: String,
    on_event: Channel<JobEvent>,
) -> Result<u64, CommandError> {
    crate::commands::Validate::validate(&id, "id")?;
    let spec = spec("Loading the model", "Starting the engine and loading the weights", true);
    Ok(server_job(
        jobs.inner(),
        client.inner().clone(),
        on_event,
        spec,
        "load_model",
        serde_json::json!({ "id": id }),
    ))
}

/// Runs a benchmark in the background (`run_benchmark` as a job).
#[tauri::command]
pub async fn start_benchmark(
    client: State<'_, SharedClient>,
    jobs: State<'_, Arc<Jobs>>,
    model_id: String,
    on_event: Channel<JobEvent>,
) -> Result<u64, CommandError> {
    crate::commands::Validate::validate(&model_id, "modelId")?;
    let spec = spec("Running the benchmark", "Benchmarking", true);
    Ok(server_job(
        jobs.inner(),
        client.inner().clone(),
        on_event,
        spec,
        "run_benchmark",
        serde_json::json!({ "modelId": model_id }),
    ))
}

/// Registers and starts the service in the background (`install_service` as a job). The result is the service state.
#[tauri::command]
pub async fn start_install_service(
    client: State<'_, SharedClient>,
    jobs: State<'_, Arc<Jobs>>,
    on_event: Channel<JobEvent>,
) -> Result<u64, CommandError> {
    let client = client.inner().clone();
    // Not cancellable: the OS prompt (UAC, Login Items) is the user's to answer, and the registration is not atomic.
    Ok(jobs.start(
        spec("Installing the background service", "Waiting for the system to register it", false),
        channel_sink(on_event),
        move |_| async move {
            blocking_install(service_install::install, "installing the service").await?;
            mark_offered();
            state_json(&client).await
        },
    ))
}

/// Stops and removes the service in the background (`uninstall_service` as a job).
#[tauri::command]
pub async fn start_uninstall_service(
    client: State<'_, SharedClient>,
    jobs: State<'_, Arc<Jobs>>,
    on_event: Channel<JobEvent>,
) -> Result<u64, CommandError> {
    let client = client.inner().clone();
    Ok(jobs.start(
        spec("Removing the background service", "Waiting for the system to remove it", false),
        channel_sink(on_event),
        move |_| async move {
            blocking_install(service_install::uninstall, "removing the service").await?;
            state_json(&client).await
        },
    ))
}

async fn blocking_install(f: fn() -> Result<(), CommandError>, what: &str) -> Result<(), CommandError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| internal(format!("{what} stopped unexpectedly: {e}")))?
}

async fn state_json(client: &SharedClient) -> Result<Value, CommandError> {
    serde_json::to_value(compose_state(client).await).map_err(|e| internal(format!("the service state could not be read: {e}")))
}

/// Downloads and installs the update in the background (`install_app_update` as a job); the app restarts when it is done.
/// Cancel is possible until the install begins, as dropping the download leaves the old version untouched.
#[tauri::command]
pub async fn start_app_update(app: AppHandle, jobs: State<'_, Arc<Jobs>>, on_event: Channel<JobEvent>) -> Result<u64, CommandError> {
    Ok(jobs.start(
        spec("Updating Pitwall", "Downloading and installing the update", true),
        channel_sink(on_event),
        move |_| async move { updater::install(&app).await.map(|()| Value::Null) },
    ))
}

/// Cancels a job; an unknown or finished id is fine.
#[tauri::command]
pub async fn job_cancel(jobs: State<'_, Arc<Jobs>>, id: u64) -> Result<(), CommandError> {
    jobs.cancel(id)
}
