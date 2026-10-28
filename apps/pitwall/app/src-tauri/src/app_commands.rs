//! The app's own commands (everything that is not a management method): the service's registration, start at log-in
//! and updates.

use crate::app_state::{self, AppState, FirstRun};
use crate::commands::{forward, SharedClient};
use crate::jobs::{self, JobEvent, JobSink, JobSpec, Jobs, Outcome};
use crate::service_install::{self, ServiceInstall};
use crate::updater::{self, AppUpdateStatus, UpdateInfo};
use paddock_jobs::{Phase, Unit};
use pitwall_core::mgmt::{CommandError, ErrorCode};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
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
    updater::install(&app, |_| {}).await
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
        own_progress: false,
    }
}

/// How long the server gets to stop the work after `cancel_op` was acknowledged. Not a limit on the operation (that has
/// none): only on the server honouring a cancel. Past it the job says Cancel did not take effect.
const CANCEL_GRACE: Duration = Duration::from_secs(15);
/// What the job shows once Cancel did not take effect.
const CANCEL_FAILED: &str = "The server did not stop this. It is still working and cannot be cancelled again from here.";

/// A random per-run token for operation ids. Job ids restart at 1 on every launch of the app while the server keeps
/// running, so an id made of the job number alone could meet a stale cancel stored for an earlier run's job.
fn run_nonce() -> &'static str {
    static NONCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NONCE.get_or_init(|| {
        let mut b = [0u8; 8];
        if getrandom::fill(&mut b).is_err() {
            // No system randomness: the clock and the process id still differ between runs.
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos() as u64);
            b = (t ^ (u64::from(std::process::id()) << 32)).to_le_bytes();
        }
        b.iter().map(|x| format!("{x:02x}")).collect()
    })
}

/// The id the server knows an operation by: method, this run of the app and the job.
fn op_id(nonce: &str, method: &str, job: u64) -> String {
    format!("{method}-{nonce}-{job}")
}

/// Cancel did not take effect: say so, and stop offering Cancel (it cannot be repeated). Held until the job ends.
fn cancel_failed(ctx: &paddock_jobs::JobContext) -> paddock_jobs::UncancellableGuard {
    ctx.phase(Phase::discovering("cancel-failed", "Could not cancel"));
    ctx.uncancellable(CANCEL_FAILED)
}

/// Runs one server method as a cancellable job with no deadline. The call carries an `opId`; on Cancel the job shows
/// "Cancelling" and the app sends `cancel_op` with it, and the server stops the work and undoes it (a half-loaded model is
/// not left behind, a benchmark stores nothing), then answers `{"cancelled": true}`, which ends the job as cancelled. If
/// the server answers something else first, the work had finished and the job reports that. If the server refuses the
/// cancel or does not stop within [`CANCEL_GRACE`], the job says so and goes on.
///
/// No liveness polling (ADR 0024 rule 2): the call is one connection, and a server that dies closes it, which fails the
/// call and so the job at once.
fn server_job(
    jobs: &Arc<Jobs>,
    client: SharedClient,
    sink: JobSink,
    spec: JobSpec,
    nonce: &str,
    method: &'static str,
    mut args: Value,
) -> u64 {
    let nonce = nonce.to_string();
    jobs::start(jobs, spec, sink, move |ctx| async move {
        let op_id = op_id(&nonce, method, ctx.id().0);
        args["opId"] = Value::String(op_id.clone());
        let work = client.call(method, args, None);
        tokio::pin!(work);
        let cancel = jobs::cancelled(ctx.cancel_token());
        tokio::pin!(cancel);
        let mut asked = false;
        let mut grace: Option<std::pin::Pin<Box<tokio::time::Sleep>>> = None;
        let mut failed_guard = None;
        let reply = loop {
            tokio::select! {
                r = &mut work => break r,
                () = &mut cancel, if !asked => {
                    asked = true;
                    ctx.phase(Phase::discovering("cancel", "Cancelling"));
                    match forward(client.as_ref(), "cancel_op", serde_json::json!({ "opId": op_id })).await {
                        Ok(_) => grace = Some(Box::pin(tokio::time::sleep(CANCEL_GRACE))),
                        // Refused or unreachable (an unreachable server also fails the work call, which ends the job).
                        Err(_) => failed_guard = Some(cancel_failed(&ctx)),
                    }
                }
                () = async {
                    match grace.as_mut() {
                        Some(g) => g.await,
                        None => std::future::pending().await,
                    }
                }, if grace.is_some() => {
                    grace = None;
                    failed_guard = Some(cancel_failed(&ctx));
                }
            }
        }?;
        drop(failed_guard);
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
        run_nonce(),
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
        run_nonce(),
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
/// The download reports bytes. Cancel drops the download, which leaves the installed version untouched and the found update
/// in place for another try.
#[tauri::command]
pub async fn start_app_update(app: AppHandle, jobs: State<'_, Arc<Jobs>>, on_event: Channel<JobEvent>) -> Result<u64, CommandError> {
    let mut spec = spec("app-update", "Updating Pitwall", "Downloading the update");
    spec.own_progress = true;
    Ok(jobs::start(jobs.inner(), spec, channel_sink(on_event), move |ctx| async move {
        ctx.phase(Phase {
            id: "download".into(),
            label: "Downloading the update".into(),
            unit: Unit::Bytes,
            total: None,
        });
        let progress = ctx.clone();
        let report = move |p| apply_install_progress(&progress, p);
        tokio::select! {
            r = updater::install(&app, report) => r.map(|()| Outcome::Done(Value::Null)),
            () = jobs::cancelled(ctx.cancel_token()) => Ok(Outcome::Cancelled),
        }
    }))
}

/// Shows an install's progress on its job: bytes (and the size when known) while downloading, then the install phase.
fn apply_install_progress(ctx: &paddock_jobs::JobContext, p: updater::InstallProgress) {
    match p {
        updater::InstallProgress::Downloading { done, total } => {
            if let Some(t) = total {
                ctx.set_total(t);
            }
            ctx.set(done);
        }
        updater::InstallProgress::Installing => ctx.phase(Phase::discovering("install", "Installing the update")),
    }
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
    use tokio::sync::{watch, Notify};

    /// Only a bound on how long a broken test waits, in virtual time; a passing test never reaches it.
    const GUARD: Duration = Duration::from_secs(24 * 3600);
    const NONCE: &str = "00aa11bb22cc33dd";

    /// The events a job sent, with a way to wait for a condition on them instead of yielding a fixed number of times.
    struct Events {
        log: Mutex<Vec<JobEvent>>,
        changed: watch::Sender<usize>,
    }

    impl Events {
        fn all(&self) -> Vec<JobEvent> {
            self.log.lock().unwrap().clone()
        }
        fn last(&self) -> Option<JobEvent> {
            self.log.lock().unwrap().last().cloned()
        }
        async fn until(&self, what: &str, pred: impl Fn(&[JobEvent]) -> bool) {
            let mut rx = self.changed.subscribe();
            let waited = tokio::time::timeout(GUARD, async {
                loop {
                    if pred(&self.log.lock().unwrap()) {
                        return;
                    }
                    rx.changed().await.unwrap();
                }
            })
            .await;
            assert!(waited.is_ok(), "never happened: {what}; events: {:?}", self.all());
        }
        async fn until_terminal(&self) {
            self.until("the job ended", |e| {
                e.iter()
                    .any(|e| matches!(e, JobEvent::Done { .. } | JobEvent::Failed { .. } | JobEvent::Cancelled { .. }))
            })
            .await;
        }
    }

    fn collector() -> (JobSink, Arc<Events>) {
        let (changed, _) = watch::channel(0);
        let events = Arc::new(Events {
            log: Mutex::default(),
            changed,
        });
        let e2 = events.clone();
        (
            Arc::new(move |e| {
                e2.log.lock().unwrap().push(e);
                e2.changed.send_modify(|n| *n += 1);
            }),
            events,
        )
    }

    fn progress_phase(e: &JobEvent) -> Option<&str> {
        match e {
            JobEvent::Progress { phase, .. } => Some(phase),
            _ => None,
        }
    }

    /// A server whose long methods run until `cancel_op` arrives (and is honoured), then answer as the real one does:
    /// `{"cancelled": true}`.
    struct CancelAware {
        calls: Mutex<Vec<(String, Value)>>,
        called: watch::Sender<usize>,
        cancel: Notify,
        /// Finish the long methods with this instead of waiting for a cancel.
        finish_with: Mutex<Option<Value>>,
        /// What `cancel_op` answers.
        cancel_reply: Mutex<Result<Value, CommandError>>,
        /// Whether `cancel_op` really stops the work.
        honours_cancel: bool,
        /// Lets an ignored-cancel work finish.
        release: Notify,
    }

    impl CancelAware {
        fn new() -> CancelAware {
            CancelAware {
                calls: Mutex::default(),
                called: watch::channel(0).0,
                cancel: Notify::new(),
                finish_with: Mutex::default(),
                cancel_reply: Mutex::new(Ok(Value::Null)),
                honours_cancel: true,
                release: Notify::new(),
            }
        }
        fn calls(&self) -> Vec<(String, Value)> {
            self.calls.lock().unwrap().clone()
        }
        /// Resolves once the server has received `n` calls.
        async fn received(&self, n: usize) {
            let mut rx = self.called.subscribe();
            tokio::time::timeout(GUARD, rx.wait_for(|c| *c >= n))
                .await
                .expect("the call arrives")
                .unwrap();
        }
    }

    impl ServerClient for CancelAware {
        fn call<'a>(&'a self, method: &'a str, args: Value, _timeout: Option<Duration>) -> BoxFuture<'a, Result<Value, CommandError>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push((method.to_string(), args));
                self.called.send_modify(|n| *n += 1);
                if method == "cancel_op" {
                    if self.honours_cancel {
                        self.cancel.notify_one();
                    }
                    return self.cancel_reply.lock().unwrap().clone();
                }
                if let Some(v) = self.finish_with.lock().unwrap().clone() {
                    return Ok(v);
                }
                if self.honours_cancel {
                    self.cancel.notified().await;
                    Ok(serde_json::json!({ "cancelled": true }))
                } else {
                    self.release.notified().await;
                    Ok(serde_json::json!({ "loaded": true }))
                }
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

    fn jobs() -> Arc<Jobs> {
        Arc::new(Jobs::with_clock(Arc::new(paddock_jobs::FakeClock::new()), 0))
    }

    /// Starts the job, cancels it and checks what the app told the server and the UI.
    async fn cancelling(method: &'static str, args: Value, kind: &'static str) {
        let server = Arc::new(CancelAware::new());
        let client: SharedClient = server.clone();
        let (sink, events) = collector();
        let jobs = jobs();
        let id = server_job(&jobs, client, sink, spec(kind, "t", "p"), NONCE, method, args);
        server.received(1).await;
        assert_eq!(jobs.running(), 1, "still working, no timeout");
        assert!(jobs.cancel(id));
        events.until_terminal().await;
        let calls = server.calls();
        let work_op = calls[0].1["opId"].as_str().unwrap().to_string();
        assert_eq!(calls[0].0, method);
        assert_eq!(
            calls[1],
            ("cancel_op".to_string(), serde_json::json!({ "opId": work_op })),
            "the server is told to undo it"
        );
        assert_eq!(jobs.running(), 0);
        let ev = events.all();
        assert_eq!(ev.last(), Some(&JobEvent::Cancelled { job_id: id }));
        assert_eq!(ev.iter().filter(|e| matches!(e, JobEvent::Cancelled { .. })).count(), 1);
        assert!(
            ev.iter().any(|e| progress_phase(e) == Some("Cancelling")),
            "the job shows it is cancelling"
        );
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
        let server = Arc::new(CancelAware::new());
        *server.finish_with.lock().unwrap() = Some(serde_json::json!({ "genTps": 5.0 }));
        let client: SharedClient = server.clone();
        let (sink, events) = collector();
        let jobs = jobs();
        let id = server_job(
            &jobs,
            client,
            sink,
            spec("benchmark", "t", "p"),
            NONCE,
            "run_benchmark",
            serde_json::json!({ "modelId": "m" }),
        );
        events.until_terminal().await;
        assert_eq!(
            events.last(),
            Some(JobEvent::Done {
                job_id: id,
                result: serde_json::json!({ "genTps": 5.0 })
            })
        );
        assert!(!jobs.cancel(id), "nothing left to cancel");
    }

    #[tokio::test(start_paused = true)]
    async fn a_server_that_ignores_the_cancel_makes_the_job_say_so_instead_of_leaving_cancel_on() {
        let mut server = CancelAware::new();
        server.honours_cancel = false;
        let server = Arc::new(server);
        let client: SharedClient = server.clone();
        let (sink, events) = collector();
        let jobs = jobs();
        let id = server_job(
            &jobs,
            client,
            sink,
            spec("load-model", "t", "p"),
            NONCE,
            "load_model",
            serde_json::json!({ "id": "m" }),
        );
        server.received(1).await;
        assert!(jobs.cancel(id));
        events
            .until("it shows Cancelling", |e| e.iter().any(|e| progress_phase(e) == Some("Cancelling")))
            .await;
        // The server acknowledged but never stops: after the grace the job says Cancel did not take effect.
        events
            .until("it says the cancel failed", |e| {
                matches!(e.last(), Some(JobEvent::Progress { phase, cancellable: false, not_cancellable_reason: Some(_), .. }) if phase == "Could not cancel")
            })
            .await;
        assert_eq!(jobs.running(), 1, "the work goes on and is still reported");
        assert!(!jobs.cancel(id), "Cancel is off, not silently enabled");
        // The work finishing later still ends the job with its result.
        server.release.notify_one();
        events.until_terminal().await;
        assert!(matches!(events.last(), Some(JobEvent::Done { .. })), "{:?}", events.last());
    }

    #[tokio::test(start_paused = true)]
    async fn a_server_that_refuses_the_cancel_makes_the_job_say_so_at_once() {
        let mut server = CancelAware::new();
        server.honours_cancel = false;
        *server.cancel_reply.lock().unwrap() = Err(CommandError::new(ErrorCode::NotFound, "no such method"));
        let server = Arc::new(server);
        let client: SharedClient = server.clone();
        let (sink, events) = collector();
        let jobs = jobs();
        let id = server_job(
            &jobs,
            client,
            sink,
            spec("load-model", "t", "p"),
            NONCE,
            "load_model",
            serde_json::json!({ "id": "m" }),
        );
        server.received(1).await;
        assert!(jobs.cancel(id));
        events
            .until(
                "it says the cancel failed",
                |e| matches!(e.last(), Some(JobEvent::Progress { phase, cancellable: false, .. }) if phase == "Could not cancel"),
            )
            .await;
        assert_eq!(jobs.running(), 1);
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
            NONCE,
            "load_model",
            serde_json::json!({ "id": "m" }),
        );
        events.until_terminal().await;
        assert!(matches!(
            events.last(),
            Some(JobEvent::Failed { error, .. }) if error.code == ErrorCode::InsufficientMemory
        ));
    }

    #[test]
    fn the_install_and_uninstall_specs_are_not_cancellable_with_a_reason() {
        let s = os_prompt_spec("install-service", "t", "p");
        assert!(s.not_cancellable.as_deref().is_some_and(|r| !r.is_empty()));
        assert!(spec("load-model", "t", "p").not_cancellable.is_none());
    }

    /// Two runs of the app both start their first job as number 1 while the server keeps running: the operation ids must
    /// differ, or a cancel stored for the earlier run's job would stop the later run's.
    #[tokio::test(start_paused = true)]
    async fn operation_ids_differ_between_runs_of_the_app_even_when_the_job_numbers_repeat() {
        async fn first_op_id(nonce: &str) -> String {
            let server = Arc::new(CancelAware::new());
            let client: SharedClient = server.clone();
            let (sink, _events) = collector();
            let jobs = jobs();
            let id = server_job(
                &jobs,
                client,
                sink,
                spec("load-model", "t", "p"),
                nonce,
                "load_model",
                serde_json::json!({ "id": "m" }),
            );
            assert_eq!(id, 1, "job numbers restart with each run");
            server.received(1).await;
            server.calls()[0].1["opId"].as_str().unwrap().to_string()
        }
        let (a, b) = (first_op_id("1111111111111111").await, first_op_id("2222222222222222").await);
        assert_ne!(a, b);
        assert!(a.starts_with("load_model-1111111111111111-"), "{a}");
        // This run's own token is random, stable within the run and made of hex digits only.
        let n = run_nonce();
        assert_eq!(n, run_nonce());
        assert_eq!(n.len(), 16);
        assert!(n.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    /// A server that works for `work_takes` on the (paused) clock, or never answers the work.
    struct Timed {
        work_takes: Option<Duration>,
        calls: Mutex<Vec<(String, Option<Duration>)>>,
    }

    impl ServerClient for Timed {
        fn call<'a>(&'a self, method: &'a str, _args: Value, timeout: Option<Duration>) -> BoxFuture<'a, Result<Value, CommandError>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push((method.to_string(), timeout));
                match self.work_takes {
                    Some(d) => {
                        tokio::time::sleep(d).await;
                        Ok(serde_json::json!({ "loaded": true }))
                    }
                    None => std::future::pending().await,
                }
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

    fn seconds_counted(e: &[JobEvent]) -> u64 {
        e.iter()
            .filter_map(|e| match e {
                // One tick a second while the total is unknown.
                JobEvent::Progress { done, .. } => Some(*done),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    #[tokio::test(start_paused = true)]
    async fn a_load_that_takes_longer_than_the_old_600_second_limit_still_completes_with_no_deadline_and_no_polling() {
        let server = Arc::new(Timed {
            work_takes: Some(Duration::from_secs(1500)),
            calls: Mutex::default(),
        });
        let client: SharedClient = server.clone();
        let (sink, events) = collector();
        let jobs = jobs();
        let id = server_job(
            &jobs,
            client,
            sink,
            spec("load-model", "t", "p"),
            NONCE,
            "load_model",
            serde_json::json!({ "id": "m" }),
        );
        events.until("past 700 s", |e| seconds_counted(e) >= 700).await;
        assert_eq!(jobs.running(), 1, "past 600 s and still working");
        events.until_terminal().await;
        assert_eq!(
            events.last(),
            Some(JobEvent::Done {
                job_id: id,
                result: serde_json::json!({ "loaded": true })
            })
        );
        // One call, no deadline, and nothing else asked of the server meanwhile (ADR 0024: no polling).
        assert_eq!(*server.calls.lock().unwrap(), vec![("load_model".to_string(), None)]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_job_waiting_for_a_silent_server_for_hours_makes_no_other_call() {
        let server = Arc::new(Timed {
            work_takes: None,
            calls: Mutex::default(),
        });
        let client: SharedClient = server.clone();
        let (sink, events) = collector();
        let jobs = jobs();
        server_job(
            &jobs,
            client,
            sink,
            spec("load-model", "t", "p"),
            NONCE,
            "load_model",
            serde_json::json!({ "id": "m" }),
        );
        events.until("three hours", |e| seconds_counted(e) >= 3 * 3600).await;
        assert_eq!(jobs.running(), 1, "still working, never timed out");
        assert_eq!(server.calls.lock().unwrap().len(), 1, "no overview or ping");
    }

    #[tokio::test(start_paused = true)]
    async fn a_connection_that_closes_mid_load_fails_the_job_at_once() {
        let fake = Arc::new(crate::server_client::fake::FakeServer::new());
        fake.reply(
            "load_model",
            crate::server_client::fake::Reply::Err(crate::server_client::unavailable()),
        );
        let client: SharedClient = fake;
        let (sink, events) = collector();
        let jobs = jobs();
        server_job(
            &jobs,
            client,
            sink,
            spec("load-model", "t", "p"),
            NONCE,
            "load_model",
            serde_json::json!({ "id": "m" }),
        );
        events.until_terminal().await;
        assert!(matches!(events.last(), Some(JobEvent::Failed { error, .. }) if error.code == ErrorCode::ServerUnavailable));
    }

    #[test]
    fn an_install_reports_bytes_then_the_install_phase() {
        let jobs = jobs();
        let (sink, events) = collector();
        let spec = {
            let mut s = spec("app-update", "t", "Downloading the update");
            s.own_progress = true;
            s
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap();
        rt.block_on(async {
            jobs::start(&jobs, spec, sink, |ctx| async move {
                ctx.phase(Phase {
                    id: "download".into(),
                    label: "Downloading the update".into(),
                    unit: Unit::Bytes,
                    total: None,
                });
                apply_install_progress(&ctx, updater::InstallProgress::Downloading { done: 100, total: None });
                apply_install_progress(
                    &ctx,
                    updater::InstallProgress::Downloading {
                        done: 400,
                        total: Some(1000),
                    },
                );
                apply_install_progress(&ctx, updater::InstallProgress::Installing);
                Ok(Outcome::Done(Value::Null))
            });
            events.until_terminal().await;
        });
        let all = events.all();
        let bytes: Vec<(u64, Option<u64>)> = all
            .iter()
            .filter_map(|e| match e {
                JobEvent::Progress { phase, done, total, .. } if phase == "Downloading the update" => Some((*done, *total)),
                _ => None,
            })
            .collect();
        assert!(bytes.contains(&(100, None)) && bytes.contains(&(400, Some(1000))), "{bytes:?}");
        assert!(bytes.windows(2).all(|w| w[0].0 <= w[1].0), "never backwards: {bytes:?}");
        assert!(all.iter().any(|e| progress_phase(e) == Some("Installing the update")));
    }
}
