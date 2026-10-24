//! The autosaved UI session: each window's latest UI state, kept on disk in a versioned file so an unclean shutdown can
//! offer to restore it.
//!
//! Debrief's session store (`lifecycle.rs`, format 2), on [`paddock_env::VersionedFile`]: one file for this run
//! (`current.json`) and one for the previous run (`recovered.json`). At start [`SessionStore::rotate`] moves what the
//! previous run left in `current.json` to `recovered.json` and starts the current run empty, so an autosave of the new run
//! never overwrites what a restore would offer. An empty previous session leaves `recovered.json` alone (a crash loop
//! must not wipe the session that was worth restoring).
//!
//! - Writes are atomic with a backup, so a kill in the middle of a checkpoint leaves the old file or the new one, never
//!   half of one; a torn or damaged file falls back to the last good copy, or to "no session" with a notice
//!   ([`SessionInfo::problem`]); the damaged file is kept for the report.
//! - A file written by a newer version of the app is left untouched and not overwritten.
//! - [`Autosaver`] writes on its own thread (rule 10), newest state per window wins, and [`Autosaver::flush`] waits until
//!   everything submitted is on disk (the clean exit uses it; so do the tests, with no sleeping).
//! - Bounded: window labels are `[A-Za-z0-9_-]{1,64}`, at most [`MAX_WINDOWS`] windows of at most [`MAX_WINDOW_BYTES`] each.

use paddock_env::{Problem, VersionedFile};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

/// Format version of the session files.
pub const SESSION_VERSION: u32 = 1;
/// Windows kept in one session; more are refused.
pub const MAX_WINDOWS: usize = 8;
/// Largest UI state of one window kept on disk (serialized); larger is refused (the in-memory checkpoint is unaffected).
pub const MAX_WINDOW_BYTES: usize = 2 << 20;
/// Longest window label.
pub const MAX_LABEL_LEN: usize = 64;
const FILE_LIMIT: u64 = (MAX_WINDOWS * MAX_WINDOW_BYTES) as u64 + (1 << 20);

/// The content of a session file: each window's UI state by label.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDoc {
    /// UI state by window label (an object each; what is in it is the app's).
    #[serde(default)]
    pub windows: BTreeMap<String, Value>,
}

/// What the start found out about the previous run's session (told to the UI).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    /// There is a session to restore.
    pub available: bool,
    /// How many windows it has.
    pub windows: u32,
    /// Why the session could not be used as it was (damaged, from a newer version), in a sentence for the notice.
    pub problem: Option<String>,
}

/// Why a window's state was not kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    /// The label is empty, too long or has characters other than letters, digits, `-` and `_`.
    InvalidLabel,
    /// The state is not a JSON object.
    NotAnObject,
    /// The state is larger than [`MAX_WINDOW_BYTES`].
    TooLarge {
        /// Its serialized size.
        bytes: usize,
    },
    /// The session already has [`MAX_WINDOWS`] windows.
    TooManyWindows,
    /// The file could not be written.
    Write(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLabel => f.write_str("the window label is not valid for a session"),
            Self::NotAnObject => f.write_str("the UI state must be an object"),
            Self::TooLarge { bytes } => write!(
                f,
                "the UI state is {bytes} bytes, more than the {MAX_WINDOW_BYTES} a session keeps"
            ),
            Self::TooManyWindows => write!(f, "a session keeps at most {MAX_WINDOWS} windows"),
            Self::Write(e) => write!(f, "the session could not be saved: {e}"),
        }
    }
}

impl std::error::Error for SessionError {}

/// Whether `label` may name a window in a session.
pub fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL_LEN
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn file(path: std::path::PathBuf) -> VersionedFile<SessionDoc> {
    VersionedFile::new(path, SESSION_VERSION)
        .max_bytes(FILE_LIMIT)
        .private()
}

/// The session files of one instance: this run's and the previous run's.
pub struct SessionStore {
    current: VersionedFile<SessionDoc>,
    recovered: VersionedFile<SessionDoc>,
    /// This run's windows (the file's content, kept so a checkpoint is one write).
    doc: Mutex<SessionDoc>,
    /// The previous run's session as it was when this run started, until discarded.
    previous: Mutex<(SessionDoc, Option<String>)>,
}

impl SessionStore {
    /// A store in `dir` (`current.json`, `recovered.json`); nothing is read or written yet.
    pub fn new(dir: &Path) -> Self {
        SessionStore {
            current: file(dir.join("current.json")),
            recovered: file(dir.join("recovered.json")),
            doc: Mutex::new(SessionDoc::default()),
            previous: Mutex::new((SessionDoc::default(), None)),
        }
    }

    /// At start: the previous run's session becomes the recovered one and this run starts empty. Never fails: a problem
    /// is kept in [`SessionStore::previous_info`]. Call once, before any checkpoint.
    pub fn rotate(&self) {
        let loaded = self.current.load();
        let mut problem = loaded.problem.as_ref().map(problem_text);
        let usable = loaded.value;
        let previous = if usable.windows.is_empty() {
            // Nothing from the last run: keep what an earlier run left, unless the last run's file was lost.
            let before = self.recovered.load();
            if problem.is_none() {
                problem = before.problem.as_ref().map(problem_text);
            }
            if loaded.problem.is_some() {
                SessionDoc::default()
            } else {
                before.value
            }
        } else {
            if let Err(e) = self.recovered.save(&usable) {
                log::warn!("Could not keep the previous session for recovery: {e}");
                problem.get_or_insert_with(|| e.to_string());
            }
            usable
        };
        if let Err(e) = self.current.save(&SessionDoc::default()) {
            // Newer-format files are refused on purpose; the run then has no autosave.
            log::warn!("Could not start a new session file: {e}");
            problem.get_or_insert_with(|| e.to_string());
        }
        *lock(&self.doc) = SessionDoc::default();
        *lock(&self.previous) = (previous, problem);
    }

    /// The previous run's session as a summary for the restore offer.
    pub fn previous_info(&self) -> SessionInfo {
        let p = lock(&self.previous);
        SessionInfo {
            available: !p.0.windows.is_empty(),
            windows: p.0.windows.len() as u32,
            problem: p.1.clone(),
        }
    }

    /// The previous run's session (empty when none or discarded).
    pub fn previous(&self) -> SessionDoc {
        lock(&self.previous).0.clone()
    }

    /// Forgets the previous run's session (Start fresh): the file is emptied too, so it is not offered again.
    pub fn discard_previous(&self) {
        let had = !lock(&self.previous).0.windows.is_empty();
        *lock(&self.previous) = (SessionDoc::default(), None);
        if had && let Err(e) = self.recovered.save(&SessionDoc::default()) {
            log::warn!("Could not empty the recovered session: {e}");
        }
    }

    /// Keeps `ui` as window `label`'s state in this run's session and writes the file.
    pub fn put(&self, label: &str, ui: &Value) -> Result<(), SessionError> {
        self.apply(&[(label.to_string(), Some(ui.clone()))])
    }

    /// Removes a window the user closed from this run's session.
    pub fn forget(&self, label: &str) -> Result<(), SessionError> {
        self.apply(&[(label.to_string(), None)])
    }

    /// This run's windows by label (what is on disk, as far as writing worked).
    pub fn current(&self) -> SessionDoc {
        lock(&self.doc).clone()
    }

    /// Applies a batch (`Some` keeps a window's state, `None` removes the window) and writes the file once. A batch with
    /// an invalid entry applies the valid ones and returns the first error.
    pub fn apply(&self, changes: &[(String, Option<Value>)]) -> Result<(), SessionError> {
        let mut doc = lock(&self.doc);
        let mut first_error = None;
        let mut changed = false;
        for (label, ui) in changes {
            match admit(&doc, label, ui.as_ref()) {
                Ok(()) => {
                    changed = true;
                    match ui {
                        Some(ui) => doc.windows.insert(label.clone(), ui.clone()),
                        None => doc.windows.remove(label),
                    };
                }
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }
        if changed && let Err(e) = self.current.save(&doc) {
            return Err(SessionError::Write(e.to_string()));
        }
        first_error.map_or(Ok(()), Err)
    }
}

fn admit(doc: &SessionDoc, label: &str, ui: Option<&Value>) -> Result<(), SessionError> {
    if !valid_label(label) {
        return Err(SessionError::InvalidLabel);
    }
    let Some(ui) = ui else { return Ok(()) };
    if !ui.is_object() {
        return Err(SessionError::NotAnObject);
    }
    let bytes = serde_json::to_vec(ui).map_or(usize::MAX, |b| b.len());
    if bytes > MAX_WINDOW_BYTES {
        return Err(SessionError::TooLarge { bytes });
    }
    if !doc.windows.contains_key(label) && doc.windows.len() >= MAX_WINDOWS {
        return Err(SessionError::TooManyWindows);
    }
    Ok(())
}

fn problem_text(p: &Problem) -> String {
    p.to_string()
}

#[derive(Default)]
struct Queue {
    pending: BTreeMap<String, Option<Value>>,
    writing: bool,
    stop: bool,
    /// The last write error logged, so one failure is one log line rather than one per checkpoint.
    last_error: Option<String>,
    /// Batches written (tests).
    batches: u64,
    /// The writer thread could not start: writes then happen on the caller's thread rather than not at all.
    inline: bool,
}

struct Shared {
    store: Arc<SessionStore>,
    queue: Mutex<Queue>,
    work: Condvar,
    idle: Condvar,
}

/// Writes checkpoints to a [`SessionStore`] on its own thread. The newest state of a window replaces an older one that
/// has not been written yet, so a burst of checkpoints is one write.
pub struct Autosaver {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Autosaver {
    /// Starts the writer thread for `store`.
    pub fn start(store: Arc<SessionStore>) -> Autosaver {
        Autosaver::start_with(store, |work| {
            std::thread::Builder::new()
                .name("paddock-session-autosave".into())
                .spawn(work)
        })
    }

    /// [`Autosaver::start`] with the way the writer thread is started given (a test refuses it).
    fn start_with(
        store: Arc<SessionStore>,
        spawn: impl FnOnce(Box<dyn FnOnce() + Send>) -> std::io::Result<JoinHandle<()>>,
    ) -> Autosaver {
        let shared = Arc::new(Shared {
            store,
            queue: Mutex::new(Queue::default()),
            work: Condvar::new(),
            idle: Condvar::new(),
        });
        let worker = Arc::clone(&shared);
        let spawned = spawn(Box::new(move || run(&worker)));
        let thread = match spawned {
            Ok(t) => Some(t),
            Err(e) => {
                log::error!(
                    "The session autosave could not start; saving on the caller's thread: {e}"
                );
                lock(&shared.queue).inline = true;
                None
            }
        };
        Autosaver {
            shared,
            thread: Mutex::new(thread),
        }
    }

    /// Queues window `label`'s state; returns at once. A state that could never be kept (see [`SessionError`]) is refused
    /// here, so the caller hears about it.
    pub fn submit(&self, label: &str, ui: Value) -> Result<(), SessionError> {
        let doc = SessionDoc::default();
        admit(&doc, label, Some(&ui))?;
        self.queue(label, Some(ui));
        Ok(())
    }

    /// Queues the removal of a closed window.
    pub fn forget(&self, label: &str) {
        if valid_label(label) {
            self.queue(label, None);
        }
    }

    fn queue(&self, label: &str, change: Option<Value>) {
        let mut q = lock(&self.shared.queue);
        if q.stop {
            return;
        }
        if q.inline {
            drop(q);
            if let Err(e) = self.shared.store.apply(&[(label.to_string(), change)]) {
                log::warn!("Session autosave: {e}");
            }
            return;
        }
        q.pending.insert(label.to_string(), change);
        self.shared.work.notify_one();
    }

    /// Waits until everything submitted so far is written (or failed).
    pub fn flush(&self) {
        let mut q = lock(&self.shared.queue);
        while (!q.pending.is_empty() || q.writing) && !q.stop {
            q = self
                .shared
                .idle
                .wait(q)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Flushes and stops the writer thread. Later submissions are ignored.
    pub fn stop(&self) {
        let thread = lock(&self.thread).take();
        if let Some(thread) = thread {
            self.flush();
            lock(&self.shared.queue).stop = true;
            self.shared.work.notify_all();
            let _ = thread.join();
        }
        lock(&self.shared.queue).stop = true;
        self.shared.idle.notify_all();
    }

    /// How many batches were written (tests).
    pub fn batches(&self) -> u64 {
        lock(&self.shared.queue).batches
    }
}

impl Drop for Autosaver {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(shared: &Shared) {
    loop {
        let batch: Vec<(String, Option<Value>)> = {
            let mut q = lock(&shared.queue);
            while q.pending.is_empty() && !q.stop {
                q = shared.work.wait(q).unwrap_or_else(PoisonError::into_inner);
            }
            if q.pending.is_empty() {
                return;
            }
            q.writing = true;
            std::mem::take(&mut q.pending).into_iter().collect()
        };
        // A bug in a write must not end the autosave for the rest of the run.
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| shared.store.apply(&batch)));
        let mut q = lock(&shared.queue);
        q.writing = false;
        q.batches += 1;
        let error = match result {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e.to_string()),
            Err(_) => Some("the session writer panicked".to_string()),
        };
        if error != q.last_error {
            if let Some(e) = &error {
                log::warn!("Session autosave: {e}");
            }
            q.last_error = error;
        }
        shared.idle.notify_all();
    }
}

#[cfg(test)]
mod tests;
