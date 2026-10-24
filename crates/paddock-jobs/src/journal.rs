//! The activity journal (plan X44): a bounded, persisted log of what the app did on the user's behalf (a rule acted, a
//! cleanup ran, a setting changed), each entry saying whether it can be taken back. The journal only records and tracks
//! the undo *state*; carrying an undo out stays in the app (its closures or system actions), which then calls
//! [`Journal::mark`]. The UI half is `@decryp7/paddock-ui/shell/undo` (the undo stack for UI edits and the activity page).
//!
//! - **Bounded**: at most `capacity` entries (the oldest are dropped), titles and details are cut at a character boundary,
//!   and the file is size-limited, so a runaway rule cannot fill the disk.
//! - **Persisted** through `paddock-env`'s versioned state file: a missing or damaged file starts empty (the damaged one is
//!   kept), a newer one is never overwritten. A failed save never fails the action that was recorded; it is kept for the
//!   app to show ([`Journal::save_error`]).
//! - **Private**: entries hold titles the app wrote, never file contents; the file is readable by the user only.
//! - Time comes from the injected [`Clock`] (tests use a [`crate::FakeClock`]).

use crate::time::Clock;
use paddock_env::{Problem, VersionedFile};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::UNIX_EPOCH;

/// The default number of entries kept.
pub const DEFAULT_CAPACITY: usize = 1000;
/// The most entries a journal keeps, whatever the app asks for.
pub const MAX_CAPACITY: usize = 5000;
/// Longest entry title, in characters.
pub const MAX_TITLE_CHARS: usize = 200;
/// Longest entry detail, in characters.
pub const MAX_DETAIL_CHARS: usize = 2000;
/// Longest entry kind, in characters.
pub const MAX_KIND_CHARS: usize = 40;
/// The journal file's format version.
const FORMAT: u32 = 1;
/// The journal file is damaged beyond this size (a full journal is well below it).
const MAX_FILE_BYTES: u64 = 32 << 20;

/// Whether an entry can be undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UndoState {
    /// Nothing to undo.
    NotUndoable,
    /// Can be undone.
    Available,
    /// Was undone.
    Undone,
    /// No longer possible (the thing it touched is gone, the quarantine expired).
    Expired,
}

/// One logged action, as the UI shows it (camelCase on the wire).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Unique and increasing; never reused.
    pub id: u64,
    /// When it was recorded, in milliseconds since the Unix epoch.
    pub at_ms: i64,
    /// The app's own tag for the kind of action (`rule`, `cleanup`, ...), for filters and icons.
    pub kind: String,
    /// One line, e.g. "Lowered Chrome's priority".
    pub title: String,
    /// Why or what exactly.
    pub detail: String,
    /// Whether it can be undone.
    pub undo: UndoState,
    /// Bytes freed or moved, for actions that have a size.
    pub bytes: Option<u64>,
}

/// What an app records: the journal adds the id and the time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEntry {
    kind: String,
    title: String,
    detail: String,
    undoable: bool,
    bytes: Option<u64>,
}

impl NewEntry {
    /// An action of `kind` with a one-line `title`; not undoable until [`undoable`](Self::undoable).
    pub fn new(kind: impl Into<String>, title: impl Into<String>) -> NewEntry {
        NewEntry {
            kind: kind.into(),
            title: title.into(),
            detail: String::new(),
            undoable: false,
            bytes: None,
        }
    }

    /// Why or what exactly.
    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> NewEntry {
        self.detail = detail.into();
        self
    }

    /// The action can be undone (the app keeps what it needs to do so).
    #[must_use]
    pub fn undoable(mut self) -> NewEntry {
        self.undoable = true;
        self
    }

    /// A size, for actions that free or move bytes.
    #[must_use]
    pub fn bytes(mut self, bytes: u64) -> NewEntry {
        self.bytes = Some(bytes);
        self
    }
}

/// Why [`Journal::mark`] refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalError {
    /// No entry has this id (it was dropped as the oldest, or never existed).
    NotFound(u64),
    /// The entry cannot change from its current state (already undone, expired, or never undoable).
    NotAvailable {
        /// The entry.
        id: u64,
        /// Its state.
        state: UndoState,
    },
    /// The journal only moves an entry to [`UndoState::Undone`] or [`UndoState::Expired`].
    BadTarget(UndoState),
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JournalError::NotFound(id) => write!(f, "activity entry {id} is no longer in the log"),
            JournalError::NotAvailable { id, state } => {
                write!(f, "activity entry {id} cannot be changed ({state:?})")
            }
            JournalError::BadTarget(state) => write!(f, "an entry cannot be moved to {state:?}"),
        }
    }
}

impl std::error::Error for JournalError {}

/// The persisted content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct State {
    next_id: u64,
    entries: Vec<Entry>,
}

impl Default for State {
    fn default() -> State {
        State {
            next_id: 1,
            entries: Vec::new(),
        }
    }
}

/// Called with an entry after it was recorded or changed, outside the journal's lock.
pub type Listener = Arc<dyn Fn(&Entry) + Send + Sync>;

struct Inner {
    state: State,
    save_error: Option<String>,
}

/// The activity journal. Cheap to share behind an `Arc`; all methods take `&self`.
pub struct Journal {
    inner: Mutex<Inner>,
    file: Option<VersionedFile<State>>,
    capacity: usize,
    clock: Arc<dyn Clock>,
    listeners: Mutex<Vec<Listener>>,
}

impl Journal {
    /// A journal that lives in memory only (tests, and apps that do not persist).
    pub fn in_memory(capacity: usize, clock: Arc<dyn Clock>) -> Journal {
        Journal::build(None, State::default(), capacity, clock)
    }

    /// A journal persisted at `path`. A missing, damaged or newer file starts empty and says why in the returned
    /// [`Problem`] (the damaged file is kept beside it; a newer one is never overwritten).
    pub fn open(
        path: impl AsRef<Path>,
        capacity: usize,
        clock: Arc<dyn Clock>,
    ) -> (Journal, Option<Problem>) {
        let file = VersionedFile::<State>::new(path.as_ref(), FORMAT)
            .max_bytes(MAX_FILE_BYTES)
            .private();
        let loaded = file.load();
        let mut state = loaded.value;
        let capacity = capacity.clamp(1, MAX_CAPACITY);
        let excess = state.entries.len().saturating_sub(capacity);
        state.entries.drain(..excess);
        // Never reuse an id, even when the file was edited or trimmed.
        let top = state.entries.iter().map(|e| e.id).max().unwrap_or(0);
        state.next_id = state.next_id.max(top + 1);
        (
            Journal::build(Some(file), state, capacity, clock),
            loaded.problem,
        )
    }

    fn build(
        file: Option<VersionedFile<State>>,
        state: State,
        capacity: usize,
        clock: Arc<dyn Clock>,
    ) -> Journal {
        Journal {
            inner: Mutex::new(Inner {
                state,
                save_error: None,
            }),
            file,
            capacity: capacity.clamp(1, MAX_CAPACITY),
            clock,
            listeners: Mutex::new(Vec::new()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Calls `listener` after every recorded or changed entry (the app pushes it to its windows).
    pub fn subscribe(&self, listener: Listener) {
        self.listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(listener);
    }

    fn notify(&self, entry: &Entry) {
        let listeners: Vec<Listener> = self
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        for l in listeners {
            // A listener that panics must not break the action that was recorded.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| l(entry)));
        }
    }

    fn persist(&self, inner: &mut Inner) {
        if let Some(file) = &self.file {
            inner.save_error = file.save(&inner.state).err().map(|e| e.to_string());
        }
    }

    /// Records an action and returns the entry (with its id and time).
    pub fn record(&self, new: NewEntry) -> Entry {
        let at_ms = self
            .clock
            .wall()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        let entry = {
            let mut inner = self.lock();
            let entry = Entry {
                id: inner.state.next_id,
                at_ms,
                kind: cut(&new.kind, MAX_KIND_CHARS),
                title: cut(&new.title, MAX_TITLE_CHARS),
                detail: cut(&new.detail, MAX_DETAIL_CHARS),
                undo: if new.undoable {
                    UndoState::Available
                } else {
                    UndoState::NotUndoable
                },
                bytes: new.bytes,
            };
            inner.state.next_id += 1;
            inner.state.entries.push(entry.clone());
            let excess = inner.state.entries.len().saturating_sub(self.capacity);
            inner.state.entries.drain(..excess);
            self.persist(&mut inner);
            entry
        };
        self.notify(&entry);
        entry
    }

    /// Moves an available entry to [`UndoState::Undone`] (the app carried the undo out) or [`UndoState::Expired`] (it
    /// can no longer be). Any other change is refused.
    pub fn mark(&self, id: u64, to: UndoState) -> Result<Entry, JournalError> {
        if !matches!(to, UndoState::Undone | UndoState::Expired) {
            return Err(JournalError::BadTarget(to));
        }
        let entry = {
            let mut inner = self.lock();
            let entry = inner
                .state
                .entries
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or(JournalError::NotFound(id))?;
            if entry.undo != UndoState::Available {
                return Err(JournalError::NotAvailable {
                    id,
                    state: entry.undo,
                });
            }
            entry.undo = to;
            let entry = entry.clone();
            self.persist(&mut inner);
            entry
        };
        self.notify(&entry);
        Ok(entry)
    }

    /// Expires every still-available entry recorded before `cutoff_ms` (a quarantine that ran out); returns them.
    pub fn expire_before(&self, cutoff_ms: i64) -> Vec<Entry> {
        let changed: Vec<Entry> = {
            let mut inner = self.lock();
            let changed: Vec<Entry> = inner
                .state
                .entries
                .iter_mut()
                .filter(|e| e.undo == UndoState::Available && e.at_ms < cutoff_ms)
                .map(|e| {
                    e.undo = UndoState::Expired;
                    e.clone()
                })
                .collect();
            if !changed.is_empty() {
                self.persist(&mut inner);
            }
            changed
        };
        changed.iter().for_each(|e| self.notify(e));
        changed
    }

    /// The newest `limit` entries, newest first.
    pub fn entries(&self, limit: usize) -> Vec<Entry> {
        self.lock()
            .state
            .entries
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    /// One entry by id.
    pub fn get(&self, id: u64) -> Option<Entry> {
        self.lock()
            .state
            .entries
            .iter()
            .find(|e| e.id == id)
            .cloned()
    }

    /// Forgets every entry (ids keep counting).
    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.state.entries.clear();
        self.persist(&mut inner);
    }

    /// Why the last save failed, if it did (the app shows it once; the log keeps working in memory).
    pub fn save_error(&self) -> Option<String> {
        self.lock().save_error.clone()
    }
}

/// `text` cut to `max` characters at a character boundary, with an ellipsis when it was cut.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
