//! The app's own commands (everything that is not a management method): the service's registration, start at log-in
//! and updates.

use crate::app_state::{self, AppState, FirstRun};
use crate::commands::{forward, SharedClient};
use crate::jobs::{self, JobEvent, JobSink, JobSpec, Jobs, Outcome};
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

// Long operations as jobs (ER-17..19) on the shared executor. Each `start_*` returns the job id at once and reports on
// the channel; `job_cancel` stops one. The blocking commands above stay until the UI has moved over.

fn channel_sink(channel: Channel<JobEvent>) -> JobSink {
    Arc::new(move |event| {
        let _ = channel.send(event);
    })
}

fn spec(kind: &'static str, title: &str, phase: &str) -> JobSpec {
    JobSpec {
        kind,
        title: title.into(),
        phase: phase.into(),
        not_cancellable: None,
    }
}

/// Runs one server method as a cancellable job. The call carries an `opId`; on Cancel the app sends `cancel_op` with it
/// and the server stops the work and undoes it (a half-loaded model is not left behind, a benchmark stores nothing), then
/// answers `{"cancelled": true}`, which ends the job as cancelled. If the server answers something else first, the work
/// had finished and the job reports that.
fn server_job(jobs: &Arc<Jobs>, client: SharedClient, sink: JobSink, spec: JobSpec, method: &'static str, mut args: Value) -> u64 {
    jobs::start(jobs, spec, sink, move |ctx| async move {
        // Unique per job and per run of the app; the server forgets it when the work ends.
        let op_id = format!("{method}-{}", ctx.id().0);
        args["opId"] = Value::String(op_id.clone());
        let work = forward(client.as_ref(), method, args);
        tokio::pin!(work);
        let cancel = jobs::cancelled(ctx.cancel_token());
        tokio::pin!(cancel);
        let mut asked = false;
        let reply = loop {
            tokio::select! {
                r = &mut work => break r,
                () = &mut cancel, if !asked => {
                    asked = true;
                    // Best effort: when the server cannot be reached the work call fails too, which ends the job.
                    let _ = forward(client.as_ref(), "cancel_op", serde_json::json!({ "opId": op_id })).await;
                }
            }
        }?;
        Ok(if reply.get("cancelled") == Some(&Value::Bool(true)) {
            Outcome::Cancelled
        } else {
            Outcome::Done(reply)
        })
    })
}

/// Loads a model in the background (`load_model` as a job). Cancel aborts the load and leaves no model behind.
#[tauri::command]
pub async fn start_load_model(
    client: State<'_, SharedClient>,
    jobs: State<'_, Arc<Jobs>>,
    id: String,
    on_event: Channel<JobEvent>,
) -> Result<u64, CommandError> {
    crate::commands::Validate::validate(&id, "id")?;
    let spec = spec("load-model", "Loading the model", "Starting the engine and loading the weights");
    Ok(server_job(
        jobs.inner(),
        client.inner().clone(),
        channel_sink(on_event),
        spec,
        "load_model",
        serde_json::json!({ "id": id }),
    ))
}

/// Runs a benchmark in the background (`run_benchmark` as a job). Cancel stops it and stores no result.
#[tauri::command]
pub async fn start_benchmark(
    client: State<'_, SharedClient>,
    jobs: State<'_, Arc<Jobs>>,
    model_id: String,
    on_event: Channel<JobEvent>,
) -> Result<u64, CommandError> {
    crate::commands::Validate::validate(&model_id, "modelId")?;
    let spec = spec("benchmark", "Running the benchmark", "Benchmarking");
    Ok(server_job(
        jobs.inner(),
        client.inner().clone(),
        channel_sink(on_event),
        spec,
        "run_benchmark",
        serde_json::json!({ "modelId": model_id }),
    ))
}

/// The spec of a step the OS owns: Cancel is off for its whole run (`cancellable: false` with the reason in the events).
fn os_prompt_spec(kind: &'static str, title: &str, phase: &str) -> JobSpec {
    JobSpec {
        not_cancellable: Some("Answer the system prompt, or wait for it to finish; it cannot be cancelled from here.".into()),
        ..spec(kind, title, phase)
    }
}

/// Registers and starts the service in the background (`install_service` as a job). The result is the service state.
#[tauri::command]
pub async fn start_install_service(
    client: State<'_, SharedClient>,
    jobs: State<'_, Arc<Jobs>>,
    on_event: Channel<JobEvent>,
) -> Result<u64, CommandError> {
    let client = client.inner().clone();
    let spec = os_prompt_spec(
        "install-service",
        "Installing the background service",
        "Waiting for the system to register it",
    );
    Ok(jobs::start(jobs.inner(), spec, channel_sink(on_event), move |_| async move {
        blocking_install(service_install::install, "installing the service").await?;
        mark_offered();
        Ok(Outcome::Done(state_json(&client).await?))
    }))
}

/// Stops and removes the service in the background (`uninstall_service` as a job).
#[tauri::command]
pub async fn start_uninstall_service(
    client: State<'_, SharedClient>,
    jobs: State<'_, Arc<Jobs>>,
    on_event: Channel<JobEvent>,
) -> Result<u64, CommandError> {
    let client = client.inner().clone();
    let spec = os_prompt_spec(
        "uninstall-service",
        "Removing the background service",
        "Waiting for the system to remove it",
    );
    Ok(jobs::start(jobs.inner(), spec, channel_sink(on_event), move |_| async move {
        blocking_install(service_install::uninstall, "removing the service").await?;
        Ok(Outcome::Done(state_json(&client).await?))
    }))
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
/// Cancel drops the download, which leaves the installed version untouched.
#[tauri::command]
pub async fn start_app_update(app: AppHandle, jobs: State<'_, Arc<Jobs>>, on_event: Channel<JobEvent>) -> Result<u64, CommandError> {
    let spec = spec("app-update", "Updating Pitwall", "Downloading and installing the update");
    Ok(jobs::start(jobs.inner(), spec, channel_sink(on_event), move |ctx| async move {
        tokio::select! {
            r = updater::install(&app) => r.map(|()| Outcome::Done(Value::Null)),
            () = jobs::cancelled(ctx.cancel_token()) => Ok(Outcome::Cancelled),
        }
    }))
}

/// Cancels a job. `true` when the request was made; `false` for an unknown or finished job, or one in a step that cannot
/// be interrupted (its events say `cancellable: false`).
#[tauri::command]
pub async fn job_cancel(jobs: State<'_, Arc<Jobs>>, id: u64) -> Result<bool, CommandError> {
    Ok(jobs.cancel(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server_client::{BoxFuture, FrameStream, ServerClient};
    use pitwall_core::mgmt::ErrorCode;
    use std::sync::Mutex;
    use std::time::Duration;
    use tokio::sync::Notify;

    /// A server whose long methods run until `cancel_op` arrives, then answer as the real one does: `{"cancelled": true}`.
    #[derive(Default)]
    struct CancelAware {
        calls: Mutex<Vec<(String, Value)>>,
        cancel: Notify,
        /// Finish the long methods with this instead of waiting for a cancel.
        finish_with: Mutex<Option<Value>>,
    }

    impl ServerClient for CancelAware {
        fn call<'a>(&'a self, method: &'a str, args: Value, _timeout: Duration) -> BoxFuture<'a, Result<Value, CommandError>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push((method.to_string(), args));
                if method == "cancel_op" {
                    self.cancel.notify_one();
                    return Ok(Value::Null);
                }
                if let Some(v) = self.finish_with.lock().unwrap().clone() {
                    return Ok(v);
                }
                self.cancel.notified().await;
                Ok(serde_json::json!({ "cancelled": true }))
            })
        }

        fn open_stream<'a>(
            &'a self,
            _: &'a str,
            _: Option<Value>,
            _: Duration,
        ) -> BoxFuture<'a, Result<Box<dyn FrameStream>, CommandError>> {
            Box::pin(async { Err(crate::server_client::unavailable()) })
        }
    }

    fn collector() -> (JobSink, Arc<Mutex<Vec<JobEvent>>>) {
        let events: Arc<Mutex<Vec<JobEvent>>> = Arc::default();
        let e2 = events.clone();
        (Arc::new(move |e| e2.lock().unwrap().push(e)), events)
    }

    async fn settle() {
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
    }

    fn jobs() -> Arc<Jobs> {
        Arc::new(Jobs::with_clock(Arc::new(paddock_jobs::FakeClock::new()), 0))
    }

    /// Starts the job, cancels it and checks what the app told the server and the UI.
    async fn cancelling(method: &'static str, args: Value, kind: &'static str) {
        let server = Arc::new(CancelAware::default());
        let client: SharedClient = server.clone();
        let (sink, events) = collector();
        let jobs = jobs();
        let id = server_job(&jobs, client, sink, spec(kind, "t", "p"), method, args);
        settle().await;
        assert_eq!(jobs.running(), 1, "still working, no timeout");
        assert!(jobs.cancel(id));
        settle().await;
        let calls = server.calls.lock().unwrap().clone();
        let work_op = calls[0].1["opId"].as_str().unwrap().to_string();
        assert_eq!(calls[0].0, method);
        assert_eq!(
            calls[1],
            ("cancel_op".to_string(), serde_json::json!({ "opId": work_op })),
            "the server is told to undo it"
        );
        assert_eq!(jobs.running(), 0);
        let ev = events.lock().unwrap();
        assert_eq!(ev.last(), Some(&JobEvent::Cancelled { job_id: id }));
        assert_eq!(ev.iter().filter(|e| matches!(e, JobEvent::Cancelled { .. })).count(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_a_model_load_tells_the_server_to_abort_it_and_ends_cancelled() {
        cancelling("load_model", serde_json::json!({ "id": "m" }), "load-model").await;
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_a_benchmark_tells_the_server_to_stop_it_and_ends_cancelled() {
        cancelling("run_benchmark", serde_json::json!({ "modelId": "m" }), "benchmark").await;
    }

    #[tokio::test(start_paused = true)]
    async fn work_that_finished_before_the_cancel_arrived_reports_its_result() {
        let server = Arc::new(CancelAware::default());
        *server.finish_with.lock().unwrap() = Some(serde_json::json!({ "genTps": 5.0 }));
        let client: SharedClient = server.clone();
        let (sink, events) = collector();
        let jobs = jobs();
        let id = server_job(
            &jobs,
            client,
            sink,
            spec("benchmark", "t", "p"),
            "run_benchmark",
            serde_json::json!({ "modelId": "m" }),
        );
        settle().await;
        assert_eq!(
            events.lock().unwrap().last(),
            Some(&JobEvent::Done {
                job_id: id,
                result: serde_json::json!({ "genTps": 5.0 })
            })
        );
        assert!(!jobs.cancel(id), "nothing left to cancel");
    }

    #[tokio::test(start_paused = true)]
    async fn a_server_error_fails_the_job_with_its_message() {
        let fake = Arc::new(crate::server_client::fake::FakeServer::new());
        fake.reply(
            "load_model",
            crate::server_client::fake::Reply::Err(CommandError::new(ErrorCode::InsufficientMemory, "not enough memory")),
        );
        let client: SharedClient = fake;
        let (sink, events) = collector();
        let jobs = jobs();
        server_job(
            &jobs,
            client,
            sink,
            spec("load-model", "t", "p"),
            "load_model",
            serde_json::json!({ "id": "m" }),
        );
        settle().await;
        assert!(matches!(
            events.lock().unwrap().last(),
            Some(JobEvent::Failed { error, .. }) if error.code == ErrorCode::InsufficientMemory
        ));
    }

    #[test]
    fn the_install_and_uninstall_specs_are_not_cancellable_with_a_reason() {
        let s = os_prompt_spec("install-service", "t", "p");
        assert!(s.not_cancellable.as_deref().is_some_and(|r| !r.is_empty()));
        assert!(spec("load-model", "t", "p").not_cancellable.is_none());
    }
}
