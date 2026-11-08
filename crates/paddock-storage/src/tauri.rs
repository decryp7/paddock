//! The Tauri adapter of `paddock-storage` (built with the `tauri` feature; plan X32).
//!
//! - [`install`] manages the [`StorageState`]. `paddock-shell` calls it from `Builder::storage`. Set up
//!   `paddock_jobs::tauri::manage` first (a scan and a clear are jobs) and, for "Show in folder", `paddock_files::tauri::install`.
//! - [`COMMANDS`](crate::COMMANDS): `storage_scan` and `storage_clear` start a job and return at once (live, phase-labelled progress
//!   from the first moment, Cancel in every phase), `storage_cancel`, `storage_last` (the last measured usage, for a window that
//!   opens later) and `storage_reveal` (a category by id; the webview never supplies a path). The outcome of a job arrives as a
//!   [`StorageEvent`] on [`EVENT`] to every window.
//! - One job at a time: a second request is refused as busy, so two windows cannot clear against each other.

use crate::{
    ClearRequest, EVENT_NAME, Run, Storage, StorageClearResult, StorageError, StorageTree,
    StorageUsage,
};
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Emitter, Manager, Runtime, State};
use paddock_commands::CommandError;
use paddock_jobs::tauri::Jobs;
use paddock_jobs::{CancelReason, JobContext, JobError, JobId, Phase};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, MutexGuard};

/// The event the outcome of a scan or a clear is sent on.
pub const EVENT: &str = EVENT_NAME;
/// The job kind of a scan.
pub const SCAN_KIND: &str = "storage-scan";
/// The job kind of a size tree.
pub const TREE_KIND: &str = "storage-tree";
/// The job kind of a clear.
pub const CLEAR_KIND: &str = "storage-clear";

/// How a job ended, sent on [`EVENT`] (camelCase, tagged by `state`).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum StorageEvent {
    /// A scan finished.
    Usage {
        /// What the app keeps.
        usage: StorageUsage,
    },
    /// A clear finished (or was stopped part way: `result.cancelled`).
    Cleared {
        /// What it did.
        result: StorageClearResult,
    },
    /// A size tree finished.
    Tree {
        /// The category's tree.
        tree: StorageTree,
    },
    /// The user cancelled a scan; nothing changed.
    Cancelled,
    /// It failed; the message says what to change.
    Failed {
        /// Words for the user.
        message: String,
    },
}

#[derive(Default)]
struct Active {
    job: Option<JobId>,
}

/// The managed state of storage.
pub struct StorageState {
    storage: Arc<Storage>,
    active: Mutex<Active>,
    last: Mutex<Option<StorageUsage>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl StorageState {
    /// The storage service.
    pub fn storage(&self) -> &Arc<Storage> {
        &self.storage
    }
}

/// Manages the storage service (once; a second call returns the first).
pub fn install<R: Runtime>(app: &AppHandle<R>, storage: Storage) -> Arc<StorageState> {
    if let Some(existing) = app.try_state::<Arc<StorageState>>() {
        return existing.inner().clone();
    }
    let state = Arc::new(StorageState {
        storage: Arc::new(storage),
        active: Mutex::new(Active::default()),
        last: Mutex::new(None),
    });
    app.manage(state.clone());
    state
}

/// A job as the core sees it.
struct JobRun<'a>(&'a JobContext);

impl Run for JobRun<'_> {
    fn phase(&self, id: &str, label: &str, total: Option<u64>) {
        self.0.phase(match total {
            Some(n) => Phase::items(id, label, n),
            None => Phase::discovering(id, label),
        });
    }

    fn add(&self, n: u64) {
        self.0.add(n);
    }

    fn cancelled(&self) -> bool {
        self.0.cancel_token().is_cancelled()
    }
}

type Managed<'a> = State<'a, Arc<StorageState>>;

/// Which job to start.
enum Work {
    Scan,
    Clear(ClearRequest),
    Tree(String),
}

fn start<R: Runtime>(
    app: &AppHandle<R>,
    state: &Arc<StorageState>,
    work: Work,
) -> Result<(), CommandError> {
    let jobs = app
        .try_state::<Jobs>()
        .ok_or_else(|| CommandError::unavailable("Jobs are not set up in this app"))?
        .registry()
        .clone();
    let (kind, title) = match work {
        Work::Scan => (SCAN_KIND, "Measuring what the app keeps"),
        Work::Clear(_) => (CLEAR_KIND, "Clearing storage"),
        Work::Tree(_) => (TREE_KIND, "Measuring a folder"),
    };
    // A refusal reaches the user at once, before any job exists.
    match &work {
        Work::Clear(request) => state
            .storage
            .validate(request)
            .map_err(CommandError::from)?,
        Work::Tree(id) => state
            .storage
            .validate_tree(id)
            .map_err(CommandError::from)?,
        Work::Scan => {}
    }
    let ctx = {
        let mut active = lock(&state.active);
        if active.job.is_some() {
            return Err(CommandError::busy(
                "Storage is being measured or cleared already",
            ));
        }
        // The job exists before the thread starts, so a Cancel that follows at once finds it (no phase without a job).
        let ctx = jobs.register(kind, title, None);
        active.job = Some(ctx.id());
        ctx
    };
    let (app, state) = (app.clone(), state.clone());
    // Walking and deleting block, so they run on the blocking pool, never on the IPC or main thread (rule 10).
    ::tauri::async_runtime::spawn_blocking(move || {
        let event = run_work(&ctx, &state, work);
        lock(&state.active).job = None;
        // A failed emit means the app is closing; nothing is left to tell.
        let _ = app.emit(EVENT, &event);
    });
    Ok(())
}

fn run_work(ctx: &JobContext, state: &StorageState, work: Work) -> StorageEvent {
    let mut outcome: Option<StorageEvent> = None;
    let result = ctx.run(|ctx| {
        let run = JobRun(ctx);
        match &work {
            Work::Scan => {
                let usage = state.storage.scan(&run).map_err(job_error)?;
                *lock(&state.last) = Some(usage.clone());
                outcome = Some(StorageEvent::Usage { usage });
            }
            Work::Tree(id) => {
                let tree = state.storage.tree(id, &run).map_err(job_error)?;
                outcome = Some(StorageEvent::Tree { tree });
            }
            Work::Clear(request) => {
                let result = state.storage.clear(request, &run).map_err(job_error)?;
                *lock(&state.last) = None;
                let cancelled = result.cancelled;
                outcome = Some(StorageEvent::Cleared { result });
                if cancelled {
                    return Err(JobError::Cancelled(CancelReason::User));
                }
            }
        }
        Ok(())
    });
    match (outcome, result) {
        (Some(event), _) => event,
        (None, Err(JobError::Cancelled(_))) => StorageEvent::Cancelled,
        (None, Err(JobError::Failed(message))) => StorageEvent::Failed { message },
        // Not reached today (the body returns only Cancelled and Failed); kept so the match stays exhaustive, and a limit that
        // ever comes out of the body shows its person-facing text.
        (None, Err(e @ JobError::Limit { .. })) => StorageEvent::Failed {
            message: e.to_string(),
        },
        (None, Err(JobError::Panicked(_)) | Ok(())) => StorageEvent::Failed {
            message: "Storage could not be read because of an internal error.".into(),
        },
    }
}

fn job_error(e: StorageError) -> JobError {
    match e {
        StorageError::Cancelled => JobError::Cancelled(CancelReason::User),
        other => JobError::Failed(other.to_string()),
    }
}

/// The last measured usage, if the app measured since it started (a clear forgets it).
#[::tauri::command]
pub async fn storage_last(state: Managed<'_>) -> Result<Option<StorageUsage>, CommandError> {
    Ok(lock(&state.last).clone())
}

/// Measures what the app keeps as a job. Returns at once; the outcome is an event.
#[::tauri::command]
pub async fn storage_scan<R: Runtime>(
    app: AppHandle<R>,
    state: Managed<'_>,
) -> Result<(), CommandError> {
    start(&app, state.inner(), Work::Scan)
}

/// Clears the chosen categories as a job. Returns at once; the outcome is an event. Refused when there is no preview, a category is
/// unknown or listed twice, or another job runs.
#[::tauri::command]
pub async fn storage_clear<R: Runtime>(
    app: AppHandle<R>,
    state: Managed<'_>,
    request: ClearRequest,
) -> Result<(), CommandError> {
    start(&app, state.inner(), Work::Clear(request))
}

/// Walks one category's folder into a size tree as a job. Returns at once; the outcome is an event. Refused when the category is
/// unknown, its folder is missing or outside the app's folders, or another job runs.
#[::tauri::command]
pub async fn storage_tree<R: Runtime>(
    app: AppHandle<R>,
    state: Managed<'_>,
    id: String,
) -> Result<(), CommandError> {
    start(&app, state.inner(), Work::Tree(id))
}

/// Cancels the scan or clear in progress; whether there was one.
#[::tauri::command]
pub async fn storage_cancel<R: Runtime>(
    app: AppHandle<R>,
    state: Managed<'_>,
) -> Result<bool, CommandError> {
    let job = lock(&state.active).job;
    let Some(job) = job else { return Ok(false) };
    let jobs = app
        .try_state::<Jobs>()
        .ok_or_else(|| CommandError::unavailable("Jobs are not set up in this app"))?;
    Ok(jobs.registry().cancel(job))
}

/// Shows a category's folder in the file manager.
#[::tauri::command]
pub async fn storage_reveal<R: Runtime>(
    app: AppHandle<R>,
    state: Managed<'_>,
    id: String,
) -> Result<(), CommandError> {
    let folder = state.storage.folder(&id).map_err(CommandError::from)?;
    let files = paddock_files::tauri::files(&app)
        .ok_or_else(|| CommandError::unavailable("This app does not set up user files"))?;
    ::tauri::async_runtime::spawn_blocking(move || files.show_folder(&folder))
        .await
        .map_err(|e| CommandError::internal(format!("the folder could not be shown: {e}")))?
}

/// The invoke handler of [`COMMANDS`](crate::COMMANDS) (every command); wrap it in the shell's guard.
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![
        storage_last,
        storage_scan,
        storage_clear,
        storage_tree,
        storage_cancel,
        storage_reveal
    ]
}

#[cfg(test)]
mod tests;
