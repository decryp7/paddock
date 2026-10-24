//! The Tauri adapter of `paddock-jobs` (built with the `tauri` feature; ADR 0008 open question 2, ADR 0012 §3, plan S9).
//!
//! - [`Emit`] is the [`EventSink`] that sends every [`JobSnapshot`] to the app's webviews as the event [`EVENT`]
//!   (`paddock://jobs`); the UI keeps the highest `seq` per job id.
//! - [`Jobs`] is the managed state the commands read: the app's [`JobRegistry`]. [`manage`] builds a registry whose sink
//!   is [`Emit`] and puts it in the app's state.
//! - [`COMMANDS`] is the adapter's command table (`paddock-commands`): `jobs_snapshot` (every known job, for a freshly
//!   opened Jobs list) and `job_cancel` (asks one job to stop; a queued job ends at once). Both are async and return at
//!   once: they read or flag the registry and never wait for a job (rule 10). [`handler`] is their guarded invoke
//!   handler; [`route`] puts it in front of the app's own handler, so the app keeps one `invoke_handler`.
//!
//! Starting work stays with the app (its command registers a job, submits it to a [`JobPool`](crate::JobPool) and returns
//! the id); progress, phases and the terminal state reach the UI through the events. Nothing here has a timeout: a
//! deadline belongs to the job (`JobRegistry::register_with_deadline`), never to the UI (ADR 0008 §4).
//!
//! ```ignore
//! tauri::Builder::default()
//!     .setup(|app| {
//!         let jobs = paddock_jobs::tauri::manage(app.handle());
//!         // ... keep `jobs` (an `Arc<JobRegistry>`) for the app's commands that start work.
//!         Ok(())
//!     })
//!     .invoke_handler(paddock_jobs::tauri::route(
//!         paddock_jobs::tauri::handler(),
//!         paddock_commands::tauri::Guard::new(&APP_COMMANDS).wrap(app_handler!()),
//!     ))
//! ```

use crate::job::JobRegistry;
use crate::model::{JobId, JobSnapshot};
use crate::reporter::EventSink;
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Emitter, Manager, Runtime};
use paddock_commands::DialogGrant;
use paddock_commands::tauri::Guard;
use serde::Serialize;
use std::fmt;
use std::sync::Arc;

/// The event every job snapshot is emitted on (the crate's [`EVENT_NAME`](crate::EVENT_NAME)).
pub const EVENT: &str = crate::EVENT_NAME;

/// Sends every snapshot to all webviews of the app as [`EVENT`]. A failed emit (the app is shutting down) is dropped:
/// snapshots are idempotent and `jobs_snapshot` gives the current state to a window that missed some.
pub struct Emit<R: Runtime>(AppHandle<R>);

impl<R: Runtime> Emit<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Emit(app)
    }
}

impl<R: Runtime> EventSink for Emit<R> {
    fn emit(&self, snapshot: &JobSnapshot) {
        let _ = self.0.emit(EVENT, snapshot);
    }
}

/// The app's jobs as the commands see them: managed Tauri state.
pub struct Jobs(Arc<JobRegistry>);

impl Jobs {
    pub fn new(registry: Arc<JobRegistry>) -> Self {
        Jobs(registry)
    }

    pub fn registry(&self) -> &Arc<JobRegistry> {
        &self.0
    }
}

/// Builds the app's [`JobRegistry`] with the [`Emit`] sink, manages it as [`Jobs`] and returns it (for the app's own
/// commands that start work). Called once, in `setup`; a second call keeps the first registry and returns it.
pub fn manage<R: Runtime>(app: &AppHandle<R>) -> Arc<JobRegistry> {
    if let Some(jobs) = app.try_state::<Jobs>() {
        return jobs.registry().clone();
    }
    let registry = Arc::new(JobRegistry::new(Arc::new(Emit::new(app.clone()))));
    app.manage(Jobs::new(registry.clone()));
    registry
}

/// Why a jobs command could not answer. On the wire `{ code, message, retryable }`, like a command refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandError {
    /// The app did not set up the jobs state ([`manage`] or `app.manage(Jobs::new(..))`): a bug in the app.
    NotSetUp,
}

impl CommandError {
    /// The stable code on the wire.
    pub fn code(self) -> &'static str {
        match self {
            CommandError::NotSetUp => "notSetUp",
        }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::NotSetUp => f.write_str("The jobs service is not set up in this app"),
        }
    }
}

impl std::error::Error for CommandError {}

impl Serialize for CommandError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("CommandError", 3)?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        st.serialize_field("retryable", &false)?;
        st.end()
    }
}

fn registry_of<R: Runtime>(app: &AppHandle<R>) -> Result<Arc<JobRegistry>, CommandError> {
    app.try_state::<Jobs>()
        .map(|jobs| jobs.registry().clone())
        .ok_or(CommandError::NotSetUp)
}

/// The latest snapshot of every known job, ordered by id (a Jobs list that opens, or a window that missed events).
#[::tauri::command]
pub async fn jobs_snapshot<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<JobSnapshot>, CommandError> {
    Ok(registry_of(&app)?.snapshots())
}

/// Asks job `id` to stop and returns at once: a queued job ends cancelled now, a running one at its next cancel check
/// (its events say when). False when the id is unknown or the job already ended.
#[::tauri::command]
pub async fn job_cancel<R: Runtime>(app: AppHandle<R>, id: JobId) -> Result<bool, CommandError> {
    Ok(registry_of(&app)?.cancel(id))
}

paddock_commands::command_table! {
    /// The commands of the jobs adapter: callable from every window class (a dialog only when its contract names them).
    pub static COMMANDS, macro generated_handler;
    Query [WINDOW] => jobs_snapshot,
    Mutation [WINDOW] => job_cancel,
}

/// The invoke handler of [`COMMANDS`] without a guard: for a guard of the app's or the shell plugin's own (which folds
/// [`COMMANDS`] into its table).
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    generated_handler!()
}

/// The guarded invoke handler of [`COMMANDS`]. Dialog windows may call nothing of it; use [`handler_with_grants`] to let
/// the dialogs whose contracts name a jobs command call it.
pub fn handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    Guard::new(&COMMANDS).wrap(generated_handler!())
}

/// As [`handler`], with the per-dialog contracts (ADR 0009): `lookup(app, label)` gives the grant of dialog `label`.
pub fn handler_with_grants<R: Runtime, G: DialogGrant + 'static>(
    lookup: impl Fn(&AppHandle<R>, &str) -> Option<G> + Send + Sync + 'static,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    Guard::new(&COMMANDS)
        .dialog_grants(lookup)
        .wrap(generated_handler!())
}

/// One invoke handler of two: the commands of [`COMMANDS`] go to `jobs`, everything else to `other` (the app's).
pub fn route<R: Runtime>(
    jobs: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
    other: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke: Invoke<R>| {
        if COMMANDS.spec(invoke.message.command()).is_some() {
            jobs(invoke)
        } else {
            other(invoke)
        }
    }
}
