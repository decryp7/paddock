//! The Tauri adapter of `paddock-diag` (built with the `tauri` feature; plan X2).
//!
//! - [`Setup`] says what the app contributes (its sections, its redaction rules, the log to report and to tune); [`install`]
//!   manages the [`Diagnostics`] state. `paddock-shell` calls it from `Builder::diagnostics` and registers the
//!   `pd.logging` settings section on the log handle.
//! - [`COMMANDS`](crate::COMMANDS): `diagnostics_preview` (collect and redact on the blocking pool, answer with the preview
//!   and a token), `diagnostics_save` (returns at once; a native save dialog asks **where**, then the report is written as a
//!   job with progress and Cancel; the outcome arrives as [`EVENT`](crate::EVENT_NAME)), `diagnostics_cancel`, and
//!   `log_from_webview` (the webview's lines, with their `Window`, into the log).
//! - The path is chosen **only** by the user: the webview never supplies one, the dialog's answer is validated again
//!   ([`validate_destination`](crate::validate_destination)), and nothing is written before the zip is complete.

use crate::{
    AppInfo, Collected, Preview, Redactor, Saved, Section, builtin, suggested_name,
    validate_destination,
};
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Emitter, Manager, Runtime, State, Window};
use paddock_env::AppDirs;
use paddock_jobs::tauri::Jobs;
use paddock_jobs::{JobError, JobId, Phase};
use paddock_log::{Clock, LogHandle, SystemClock};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

#[cfg(feature = "dialog")]
mod native;
#[cfg(feature = "dialog")]
pub use native::NativePicker;

/// The event the outcome of a save is sent on.
pub const EVENT: &str = crate::EVENT_NAME;
/// The job kind of a save.
pub const JOB_KIND: &str = "diagnostics";

/// Asks the user where to save. Blocking: called on a worker thread, never on the IPC or main thread.
pub trait SavePicker: Send + Sync {
    /// The file the user chose, or `None` when they closed the dialog. `window` is the label of the window that owns it.
    fn pick(&self, window: &str, suggested_name: &str) -> Option<PathBuf>;
}

/// What the app contributes to its reports.
pub struct Setup {
    app: AppInfo,
    dirs: AppDirs,
    sections: Vec<Arc<dyn Section>>,
    redactor: Redactor,
    log: Option<LogHandle>,
    picker: Option<Arc<dyn SavePicker>>,
    clock: Arc<dyn Clock>,
}

impl Setup {
    /// A setup with the built-in sections ([`builtin::all`]) and the default redactor for this machine.
    pub fn new(app: AppInfo, dirs: AppDirs) -> Setup {
        Setup {
            app,
            dirs,
            sections: builtin::all(),
            redactor: Redactor::for_machine(&paddock_env::ProcessVars),
            log: None,
            picker: None,
            clock: Arc::new(SystemClock),
        }
    }

    /// Reports the settings file `name` in the config folder instead of `settings.json` (the shell passes
    /// `shell-settings.json`).
    pub fn settings_file(mut self, name: &str) -> Setup {
        if let Some(slot) = self.sections.iter_mut().find(|s| s.id() == "settings") {
            *slot = Arc::new(builtin::Settings::file(name));
        }
        self
    }

    /// Adds an app section after the built-in ones.
    pub fn section(mut self, section: Arc<dyn Section>) -> Setup {
        self.sections.push(section);
        self
    }

    /// Replaces the redactor (the app's patterns and field rules).
    pub fn redactor(mut self, redactor: Redactor) -> Setup {
        self.redactor = redactor;
        self
    }

    /// The running log: pending "repeated n times" lines are written before a report is collected, and the shell's Logging
    /// section tunes its level.
    pub fn log(mut self, handle: LogHandle) -> Setup {
        self.log = Some(handle);
        self
    }

    /// Replaces how the user is asked where to save (tests; the default is the native dialog with the `dialog` feature).
    pub fn picker(mut self, picker: Arc<dyn SavePicker>) -> Setup {
        self.picker = Some(picker);
        self
    }

    /// Replaces the clock (tests).
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Setup {
        self.clock = clock;
        self
    }
}

/// What a command could not do. On the wire `{ code, message, retryable }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct DiagCommandError {
    /// What kind of refusal.
    pub code: DiagErrorCode,
    /// Words for the user.
    pub message: String,
    /// Whether trying again can help.
    pub retryable: bool,
}

/// The kinds of refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum DiagErrorCode {
    /// The app did not set up reports.
    NotSetUp,
    /// The preview a save refers to is gone (a newer preview replaced it); preview again.
    Expired,
    /// A save is already running.
    Busy,
    /// There is no way to ask where to save in this build.
    NoPicker,
    /// The collection failed unexpectedly.
    Failed,
}

fn refuse(code: DiagErrorCode, message: &str, retryable: bool) -> DiagCommandError {
    DiagCommandError {
        code,
        message: message.into(),
        retryable,
    }
}

/// What a preview answers: the files with their sizes and the token a save refers to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PreviewReply {
    /// Identifies the collected report; pass it back to `diagnostics_save` and to later previews of the same report.
    pub token: String,
    /// What the zip would hold.
    pub preview: Preview,
}

/// How a save ended, sent on [`EVENT`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum SaveOutcome {
    /// The zip is written.
    Saved(Saved),
    /// The user closed the save dialog; nothing was written.
    Dismissed,
    /// The user cancelled the job; nothing was written.
    Cancelled,
    /// It failed; the message says what to change. Nothing was written.
    Failed {
        /// Words for the user.
        message: String,
    },
}

struct Cached {
    token: String,
    collected: Collected,
}

#[derive(Default)]
struct Active {
    /// A save is between the click and its outcome.
    saving: bool,
    job: Option<JobId>,
}

/// The managed state of reports.
pub struct Diagnostics {
    app: AppInfo,
    dirs: AppDirs,
    sections: Vec<Arc<dyn Section>>,
    redactor: Redactor,
    log: Option<LogHandle>,
    picker: Option<Arc<dyn SavePicker>>,
    clock: Arc<dyn Clock>,
    next: AtomicU64,
    cache: Mutex<Option<Cached>>,
    active: Mutex<Active>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Diagnostics {
    fn new(setup: Setup, picker: Option<Arc<dyn SavePicker>>) -> Diagnostics {
        Diagnostics {
            app: setup.app,
            dirs: setup.dirs,
            sections: setup.sections,
            redactor: setup.redactor,
            log: setup.log,
            picker: setup.picker.or(picker),
            clock: setup.clock,
            next: AtomicU64::new(1),
            cache: Mutex::new(None),
            active: Mutex::new(Active::default()),
        }
    }

    /// The log handle the app gave, for the Logging settings section.
    pub fn log_handle(&self) -> Option<LogHandle> {
        self.log.clone()
    }

    /// Collects (or, for a known `token`, re-uses what was collected) and assembles the report with `description`.
    /// Blocking: call it on the blocking pool.
    pub fn preview(&self, token: Option<&str>, description: &str) -> PreviewReply {
        let mut cache = lock(&self.cache);
        let known = matches!((&*cache, token), (Some(c), Some(t)) if c.token == t);
        if !known {
            *cache = None;
        }
        let cached = cache.get_or_insert_with(|| {
            if let Some(log) = &self.log {
                log.flush_repeats();
            }
            let collected = Collected::collect(
                &self.app,
                &self.dirs,
                &self.sections,
                &self.redactor,
                paddock_log::now_utc(&*self.clock),
            );
            let token = format!("r{}", self.next.fetch_add(1, Ordering::Relaxed));
            Cached { token, collected }
        });
        PreviewReply {
            token: cached.token.clone(),
            preview: cached.collected.assemble(description).preview(),
        }
    }
}

/// Manages the reports' state. Set up `paddock_jobs::tauri::manage` first (a save is a job). `native` is the picker used
/// when the setup names none (the shell passes [`NativePicker`] with the `dialog` feature).
pub fn install<R: Runtime>(
    app: &AppHandle<R>,
    setup: Setup,
    native: Option<Arc<dyn SavePicker>>,
) -> Arc<Diagnostics> {
    if let Some(existing) = app.try_state::<Arc<Diagnostics>>() {
        return existing.inner().clone();
    }
    let state = Arc::new(Diagnostics::new(setup, native));
    app.manage(state.clone());
    state
}

type Managed<'a> = State<'a, Arc<Diagnostics>>;

fn not_set_up() -> DiagCommandError {
    refuse(
        DiagErrorCode::NotSetUp,
        "Reports are not set up in this app.",
        false,
    )
}

/// Collects and redacts the report and answers with the preview. Pass the `token` of the last reply to re-use what was
/// collected (the description changed); without it, or with an old one, the report is collected again.
#[::tauri::command]
pub async fn diagnostics_preview(
    state: Managed<'_>,
    token: Option<String>,
    description: String,
) -> Result<PreviewReply, DiagCommandError> {
    let state = state.inner().clone();
    ::tauri::async_runtime::spawn_blocking(move || state.preview(token.as_deref(), &description))
        .await
        .map_err(|e| {
            refuse(
                DiagErrorCode::Failed,
                &format!("The report could not be collected: {e}"),
                true,
            )
        })
}

/// Asks where to save and writes the report of `token` with `description` as a job. Returns at once; the outcome arrives as
/// an [`EVENT`]. Refused when the preview is out of date, a save is running, or this build cannot ask where to save.
#[::tauri::command]
pub async fn diagnostics_save<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    state: Managed<'_>,
    token: String,
    description: String,
) -> Result<(), DiagCommandError> {
    let state = state.inner().clone();
    let label = window.label().to_string();
    start_save(&app, &state, &label, &token, &description)
}

fn start_save<R: Runtime>(
    app: &AppHandle<R>,
    state: &Arc<Diagnostics>,
    label: &str,
    token: &str,
    description: &str,
) -> Result<(), DiagCommandError> {
    let picker = state.picker.clone().ok_or_else(|| {
        refuse(
            DiagErrorCode::NoPicker,
            "Saving a report is not available in this build.",
            false,
        )
    })?;
    let jobs = app
        .try_state::<Jobs>()
        .ok_or_else(not_set_up)?
        .registry()
        .clone();
    let report = {
        let cache = lock(&state.cache);
        let cached = cache.as_ref().filter(|c| c.token == token);
        let cached = cached.ok_or_else(|| {
            refuse(
                DiagErrorCode::Expired,
                "The preview is out of date. It was made again; look it over and save once more.",
                true,
            )
        })?;
        cached.collected.assemble(description)
    };
    let ctx = {
        let mut active = lock(&state.active);
        if active.saving {
            return Err(refuse(
                DiagErrorCode::Busy,
                "A report is being saved already.",
                true,
            ));
        }
        active.saving = true;
        // The job exists before the save thread starts, so a Cancel that follows at once finds it (no phase without a job).
        let ctx = jobs.register(JOB_KIND, "Saving the problem report", None);
        active.job = Some(ctx.id());
        ctx
    };
    let suggested = suggested_name(&state.app, paddock_log::now_utc(&*state.clock));
    let (app, state, label) = (app.clone(), state.clone(), label.to_string());
    // The save blocks on the native dialog, so it runs on the blocking pool, never on the IPC or main thread.
    ::tauri::async_runtime::spawn_blocking(move || {
        let outcome = run_save(ctx, picker.as_ref(), &label, &suggested, &report);
        {
            let mut active = lock(&state.active);
            active.saving = false;
            active.job = None;
        }
        // A failed emit means the app is closing; nothing is left to tell.
        let _ = app.emit(EVENT, &outcome);
    });
    Ok(())
}

/// The save, start to finish, as one job from the first moment: choosing where (a native dialog; Cancel works here too), then
/// packing with bytes progress. Never panics (the job contains its body's panic).
fn run_save(
    ctx: paddock_jobs::JobContext,
    picker: &dyn SavePicker,
    label: &str,
    suggested: &str,
    report: &crate::Report,
) -> SaveOutcome {
    let mut dismissed = false;
    let result = ctx.run(|ctx| {
        ctx.phase(Phase::discovering("choose", "Choosing where to save"));
        let Some(chosen) = picker.pick(label, suggested) else {
            dismissed = true;
            return Err(JobError::Cancelled(paddock_jobs::CancelReason::User));
        };
        // Cancel pressed while the dialog was open.
        ctx.check()?;
        // The picker's answer is checked again: it is a path, and the engine opens only paths it validated.
        let dest = validate_destination(&chosen).map_err(|e| JobError::Failed(e.to_string()))?;
        ctx.phase(Phase::bytes(
            "pack",
            "Packing the report",
            Some(report.total_bytes()),
        ));
        report
            .save(&dest, ctx.cancel_token(), ctx)
            .map_err(|e| match e {
                crate::DiagError::Cancelled => {
                    JobError::Cancelled(paddock_jobs::CancelReason::User)
                }
                other => JobError::Failed(other.to_string()),
            })
    });
    match result {
        Ok(saved) => SaveOutcome::Saved(saved),
        Err(JobError::Cancelled(_)) if dismissed => SaveOutcome::Dismissed,
        Err(JobError::Cancelled(_)) => SaveOutcome::Cancelled,
        Err(JobError::Failed(message)) => SaveOutcome::Failed { message },
        Err(JobError::Panicked(_)) => SaveOutcome::Failed {
            message: "The report could not be saved because of an internal error.".into(),
        },
    }
}

/// Cancels the save in progress (its job); a no-op when none runs. The report is not written at all.
#[::tauri::command]
pub async fn diagnostics_cancel<R: Runtime>(
    app: AppHandle<R>,
    state: Managed<'_>,
) -> Result<bool, DiagCommandError> {
    let job = lock(&state.active).job;
    let Some(job) = job else { return Ok(false) };
    let jobs = app.try_state::<Jobs>().ok_or_else(not_set_up)?;
    Ok(jobs.registry().cancel(job))
}

/// Logs a line the webview sent, with the window it came from. `level` is a name or the log plugin's number; the message is
/// bounded and redacted by the log like any other. Needs no state: the log is the process's.
#[::tauri::command]
pub async fn log_from_webview<R: Runtime>(
    window: Window<R>,
    level: String,
    message: String,
) -> Result<(), DiagCommandError> {
    paddock_log::log_from_webview(window.label(), &level, &message);
    Ok(())
}

/// The invoke handler of [`COMMANDS`](crate::COMMANDS) (every command); wrap it in the shell's guard.
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![
        diagnostics_preview,
        diagnostics_save,
        diagnostics_cancel,
        log_from_webview
    ]
}

#[cfg(test)]
mod tests;
