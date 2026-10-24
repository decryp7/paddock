//! The Tauri adapter of the activity journal (built with the `tauri` feature; plan X44), so apps do not each write it.
//!
//! - [`manage`] puts the app's [`Journal`] in Tauri state as [`Activity`] and emits every recorded or changed entry to all
//!   webviews as [`EVENT`] (`paddock://activity`); the UI's `TauriActivityBackend` listens there.
//! - [`COMMANDS`] is the command table: `activity_list` (the newest entries, newest first), `activity_undo` (runs the app's
//!   registered undo handler for an entry that can be undone, then marks it undone) and `activity_mark` (moves an entry to
//!   undone or expired without running anything, for an app that undid it some other way or found it can no longer be).
//!   All are async and run the app's handler off the IPC thread (rule 10).
//! - Undoing stays the app's work: [`Activity::on_undo`] registers `Fn(&Entry) -> Result<(), String>`; the `Err` text is
//!   what the user reads, and the entry stays available.
//!
//! ```ignore
//! let activity = paddock_jobs::tauri_activity::manage(app.handle(), journal);
//! activity.on_undo(|entry| restore_from_quarantine(entry).map_err(|e| e.to_string()));
//! tauri::Builder::default().invoke_handler(paddock_jobs::tauri_activity::route(
//!     paddock_jobs::tauri_activity::handler(),
//!     app_handler,
//! ))
//! ```

use crate::journal::{Entry, Journal, JournalError, UndoState};
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Emitter, Manager, Runtime};
use paddock_commands::tauri::Guard;
use serde::Serialize;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

/// The event every recorded or changed entry is emitted on.
pub const EVENT: &str = "paddock://activity";

/// The most entries `activity_list` returns, whatever the window asks for.
pub const MAX_LIST: usize = 1000;
/// What `activity_list` returns when the window names no limit.
pub const DEFAULT_LIST: usize = 200;

/// The app's undo of one entry: it undoes the action and returns the reason for the user when it cannot.
pub type UndoHandler = Arc<dyn Fn(&Entry) -> Result<(), String> + Send + Sync>;

/// The journal and the registered undo handler, as managed Tauri state.
pub struct Activity {
    journal: Arc<Journal>,
    handler: Mutex<Option<UndoHandler>>,
}

impl Activity {
    /// The journal (the app records into it).
    pub fn journal(&self) -> &Arc<Journal> {
        &self.journal
    }

    /// Registers the app's undo, replacing an earlier one.
    pub fn on_undo(&self, handler: impl Fn(&Entry) -> Result<(), String> + Send + Sync + 'static) {
        *self.handler.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(handler));
    }

    fn handler(&self) -> Option<UndoHandler> {
        self.handler
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Manages `journal` as [`Activity`] and emits its entries as [`EVENT`]; returns the state (to register the undo
/// handler). Called once, in `setup`; a second call keeps the first and returns it.
pub fn manage<R: Runtime>(app: &AppHandle<R>, journal: Arc<Journal>) -> Arc<Activity> {
    if let Some(a) = app.try_state::<Arc<Activity>>() {
        return a.inner().clone();
    }
    let handle = app.clone();
    journal.subscribe(Arc::new(move |entry: &Entry| {
        // A failed emit (the app is shutting down) is dropped: `activity_list` gives the state to a window that missed it.
        let _ = handle.emit(EVENT, entry);
    }));
    let activity = Arc::new(Activity {
        journal,
        handler: Mutex::new(None),
    });
    app.manage(activity.clone());
    activity
}

/// Why an activity command could not answer. On the wire `{ code, message, retryable }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// The app did not call [`manage`]: a bug in the app.
    NotSetUp,
    /// The journal refused (unknown id, not undoable any more, a bad target state).
    Journal(JournalError),
    /// The app registered no undo handler.
    NoUndoHandler,
    /// The app's undo said it could not (its reason, for the user); the entry stays as it was.
    UndoFailed(String),
}

impl CommandError {
    /// The stable code on the wire.
    pub fn code(&self) -> &'static str {
        match self {
            CommandError::NotSetUp => "notSetUp",
            CommandError::Journal(JournalError::NotFound(_)) => "notFound",
            CommandError::Journal(JournalError::NotAvailable { .. }) => "notAvailable",
            CommandError::Journal(JournalError::BadTarget(_)) => "badTarget",
            CommandError::NoUndoHandler => "noUndoHandler",
            CommandError::UndoFailed(_) => "undoFailed",
        }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::NotSetUp => f.write_str("The activity log is not set up in this app"),
            CommandError::Journal(e) => e.fmt(f),
            CommandError::NoUndoHandler => f.write_str("This app cannot undo actions"),
            CommandError::UndoFailed(reason) => f.write_str(reason),
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

impl From<JournalError> for CommandError {
    fn from(e: JournalError) -> Self {
        CommandError::Journal(e)
    }
}

fn activity_of<R: Runtime>(app: &AppHandle<R>) -> Result<Arc<Activity>, CommandError> {
    app.try_state::<Arc<Activity>>()
        .map(|a| a.inner().clone())
        .ok_or(CommandError::NotSetUp)
}

/// The newest entries, newest first (`limit` defaults to [`DEFAULT_LIST`], at most [`MAX_LIST`]).
#[::tauri::command]
pub async fn activity_list<R: Runtime>(
    app: AppHandle<R>,
    limit: Option<usize>,
) -> Result<Vec<Entry>, CommandError> {
    Ok(activity_of(&app)?
        .journal
        .entries(limit.unwrap_or(DEFAULT_LIST).min(MAX_LIST)))
}

/// Undoes entry `id` through the app's handler (off the IPC thread) and marks it undone; resolves with the entry.
#[::tauri::command]
pub async fn activity_undo<R: Runtime>(app: AppHandle<R>, id: u64) -> Result<Entry, CommandError> {
    let activity = activity_of(&app)?;
    let entry = activity.journal.get(id).ok_or(JournalError::NotFound(id))?;
    if entry.undo != UndoState::Available {
        return Err(JournalError::NotAvailable {
            id,
            state: entry.undo,
        }
        .into());
    }
    let handler = activity.handler().ok_or(CommandError::NoUndoHandler)?;
    let ran = ::tauri::async_runtime::spawn_blocking(move || {
        // A handler that panics is its app's bug, reported as a failure, never a crash of the IPC thread.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(&entry)))
            .unwrap_or_else(|_| Err("The undo failed unexpectedly".to_string()))
    })
    .await
    .unwrap_or_else(|_| Err("The undo failed unexpectedly".to_string()));
    ran.map_err(CommandError::UndoFailed)?;
    Ok(activity.journal.mark(id, UndoState::Undone)?)
}

/// Moves an available entry to `undone` or `expired` without running anything.
#[::tauri::command]
pub async fn activity_mark<R: Runtime>(
    app: AppHandle<R>,
    id: u64,
    to: UndoState,
) -> Result<Entry, CommandError> {
    Ok(activity_of(&app)?.journal.mark(id, to)?)
}

paddock_commands::command_table! {
    /// The commands of the activity adapter: callable from every window class (a dialog only when its contract names them).
    pub static COMMANDS, macro generated_handler;
    Query [WINDOW] => activity_list,
    Mutation [WINDOW] => activity_undo,
    Mutation [WINDOW] => activity_mark,
}

/// The invoke handler of [`COMMANDS`] without a guard, for a guard of the app's or the shell plugin's own.
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    generated_handler!()
}

/// The guarded invoke handler of [`COMMANDS`].
pub fn handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    Guard::new(&COMMANDS).wrap(generated_handler!())
}

/// One invoke handler of two: the commands of [`COMMANDS`] go to `activity`, everything else to `other` (the app's).
pub fn route<R: Runtime>(
    activity: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
    other: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke: Invoke<R>| {
        if COMMANDS.spec(invoke.message.command()).is_some() {
            activity(invoke)
        } else {
            other(invoke)
        }
    }
}
