//! [`Checklist`]: the steps, the user's answers kept in a [`VersionedFile`] of `paddock-env`, and the actions run on a
//! worker. Pure of Tauri: the adapter ([`super::tauri`]) gives it an emit function and a thread.

use super::model::{
    Check, ItemStatus, ItemView, SetupCommandError, SetupErrorCode, SetupItem, SetupState,
};
use paddock_env::VersionedFile;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Longest item id.
pub const MAX_ID_LEN: usize = 64;
/// Most steps one checklist holds (the shell's four and a few of the app's own).
pub const MAX_ITEMS: usize = 16;
/// The current format of the file of answers.
const FORMAT: u32 = 1;

/// Where the engine tells every window the checklist changed (the payload is a [`SetupState`]).
pub type Emit = Arc<dyn Fn(&SetupState) + Send + Sync>;
/// Runs a closure on a worker thread (the adapter's thread; tests run it at once). An error means no thread could be
/// started and the closure did not run.
pub type Runner = Arc<dyn Fn(Box<dyn FnOnce() + Send>) -> Result<(), String> + Send + Sync>;

/// The user's answers, as kept on disk (`setup.json`). Unknown fields are dropped, missing ones default.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct SetupFile {
    /// Steps the user skipped.
    pub skipped: BTreeSet<String>,
    /// The checklist was finished or dismissed.
    pub finished: bool,
}

/// Why a checklist could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListError {
    /// An id is empty, too long or has characters other than lowercase letters, digits, `.`, `-`, `_`.
    BadId(String),
    /// Two steps share an id.
    Duplicate(String),
    /// More than [`MAX_ITEMS`] steps.
    TooMany(usize),
}

impl std::fmt::Display for ListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ListError::BadId(id) => write!(
                f,
                "the set-up step id {id:?} is not allowed (lowercase letters, digits, '.', '-' and '_', up to {MAX_ID_LEN})"
            ),
            ListError::Duplicate(id) => write!(f, "two set-up steps are called {id:?}"),
            ListError::TooMany(n) => {
                write!(f, "{n} set-up steps; the most is {MAX_ITEMS}")
            }
        }
    }
}

impl std::error::Error for ListError {}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_')
        })
}

impl SetupCommandError {
    fn new(code: SetupErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        SetupCommandError {
            code,
            message: message.into(),
            retryable,
        }
    }

    /// The app has no checklist.
    pub fn not_set_up() -> Self {
        Self::new(
            SetupErrorCode::NotSetUp,
            "This app has no set-up checklist.",
            false,
        )
    }

    /// A step or a worker ended abnormally.
    pub fn failed(why: &str) -> Self {
        Self::new(
            SetupErrorCode::Failed,
            format!("The set-up step stopped unexpectedly: {why}"),
            true,
        )
    }

    fn unknown(id: &str) -> Self {
        Self::new(
            SetupErrorCode::Unknown,
            format!("There is no set-up step {id:?}."),
            false,
        )
    }

    fn busy(title: &str) -> Self {
        Self::new(
            SetupErrorCode::Busy,
            format!("{title} is running already."),
            true,
        )
    }
}

/// What an item is asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    /// The primary action.
    Act,
    /// The repair.
    Repair,
}

#[derive(Default)]
struct Live {
    working: BTreeSet<String>,
    failures: BTreeMap<String, String>,
}

/// The first-run checklist of one app.
pub struct Checklist {
    items: Vec<Arc<dyn SetupItem>>,
    file: Option<VersionedFile<SetupFile>>,
    saved: Mutex<SetupFile>,
    live: Mutex<Live>,
    emit: Emit,
    run: Runner,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An item's call, with a panic in the item turned into a message (one bad step never takes the checklist down).
fn guarded<T>(what: &str, f: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f))
        .map_err(|_| format!("{what} stopped unexpectedly. Try again, or skip it for now."))
}

impl Checklist {
    /// A checklist of `items`, in order. The answers live in `file` (`None`: in memory, for demos and tests); a missing or
    /// damaged file starts fresh (`paddock-env` keeps the damaged copy). `emit` is told after every change and `run`
    /// puts actions on a worker (rule 10).
    pub fn new(
        items: Vec<Arc<dyn SetupItem>>,
        file: Option<PathBuf>,
        emit: Emit,
        run: Runner,
    ) -> Result<Arc<Checklist>, ListError> {
        if items.len() > MAX_ITEMS {
            return Err(ListError::TooMany(items.len()));
        }
        let mut seen = BTreeSet::new();
        for item in &items {
            let id = item.id();
            if !valid_id(id) {
                return Err(ListError::BadId(id.to_string()));
            }
            if !seen.insert(id.to_string()) {
                return Err(ListError::Duplicate(id.to_string()));
            }
        }
        let file = file.map(|p| VersionedFile::<SetupFile>::new(p, FORMAT));
        let mut saved = SetupFile::default();
        if let Some(f) = &file {
            let loaded = f.load();
            if let Some(problem) = &loaded.problem {
                log::warn!("the set-up answers start fresh: {problem}");
            }
            saved = loaded.value;
            // Answers about steps that no longer exist are dropped, so the file stays bounded by the list.
            saved.skipped.retain(|id| seen.contains(id));
        }
        Ok(Arc::new(Checklist {
            items,
            file,
            saved: Mutex::new(saved),
            live: Mutex::new(Live::default()),
            emit,
            run,
        }))
    }

    fn find(&self, id: &str) -> Result<&Arc<dyn SetupItem>, SetupCommandError> {
        self.items
            .iter()
            .find(|i| i.id() == id)
            .ok_or_else(|| SetupCommandError::unknown(id))
    }

    /// Where every step stands now. Blocking (each step asks the OS, bounded): call it off the IPC thread.
    pub fn state(&self) -> SetupState {
        let saved = lock(&self.saved).clone();
        let (working, failures) = {
            let live = lock(&self.live);
            (live.working.clone(), live.failures.clone())
        };
        let items: Vec<ItemView> = self
            .items
            .iter()
            .filter_map(|item| {
                let check = guarded(&item.title(), || item.check()).unwrap_or_else(Check::Problem);
                let id = item.id().to_string();
                let failure = failures.get(&id).cloned();
                let (status, message) = match check {
                    Check::NotApplicable => return None,
                    Check::Done => (ItemStatus::Done, None),
                    Check::Todo => match failure {
                        Some(m) => (ItemStatus::Problem, Some(m)),
                        None => (ItemStatus::Todo, None),
                    },
                    Check::Problem(m) => (ItemStatus::Problem, Some(failure.unwrap_or(m))),
                };
                Some(ItemView {
                    skipped: saved.skipped.contains(&id),
                    working: working.contains(&id),
                    hint: guarded(&item.title(), || item.hint()).unwrap_or(None),
                    needs_ai: guarded(&item.title(), || item.needs_ai()).unwrap_or(None),
                    action: item.action_label(),
                    title: item.title(),
                    detail: item.detail(),
                    id,
                    status,
                    message,
                })
            })
            .collect();
        let open = items
            .iter()
            .any(|i| i.status != ItemStatus::Done && !i.skipped);
        SetupState {
            items,
            finished: saved.finished,
            show_on_start: !saved.finished && open,
        }
    }

    /// Runs the step's action (or its repair) on a worker and returns at once; the outcome arrives through `emit`, first
    /// with the step marked as working. Refused while that step already runs.
    pub fn request(self: &Arc<Self>, id: &str, what: Do) -> Result<(), SetupCommandError> {
        let item = self.find(id)?.clone();
        if !lock(&self.live).working.insert(id.to_string()) {
            return Err(SetupCommandError::busy(&item.title()));
        }
        lock(&self.live).failures.remove(id);
        let this = Arc::clone(self);
        let id = id.to_string();
        let failed_id = id.clone();
        let started = (self.run)(Box::new(move || {
            (this.emit)(&this.state());
            let title = item.title();
            let result = guarded(&title, || match what {
                Do::Act => item.act(),
                Do::Repair => item.repair(),
            })
            .and_then(|r| r);
            {
                let mut live = lock(&this.live);
                live.working.remove(&id);
                if let Err(message) = result {
                    live.failures.insert(id, message);
                }
            }
            (this.emit)(&this.state());
        }));
        started.map_err(|why| {
            lock(&self.live).working.remove(&failed_id);
            SetupCommandError::new(
                SetupErrorCode::Failed,
                format!("The step could not start: {why}"),
                true,
            )
        })
    }

    fn update(&self, change: impl FnOnce(&mut SetupFile)) -> Result<SetupState, SetupCommandError> {
        let snapshot = {
            let mut saved = lock(&self.saved);
            change(&mut saved);
            saved.clone()
        };
        let stored = match &self.file {
            Some(f) => f.save(&snapshot).map_err(|e| {
                SetupCommandError::new(
                    SetupErrorCode::NotSaved,
                    format!("Your choice could not be kept: {e}"),
                    true,
                )
            }),
            None => Ok(()),
        };
        let state = self.state();
        (self.emit)(&state);
        stored.map(|()| state)
    }

    /// Skips the step for now (`skipped`) or takes it up again. The step stays in Settings either way.
    pub fn skip(&self, id: &str, skipped: bool) -> Result<SetupState, SetupCommandError> {
        let id = self.find(id)?.id().to_string();
        self.update(|f| {
            if skipped {
                f.skipped.insert(id);
            } else {
                f.skipped.remove(&id);
            }
        })
    }

    /// Finishes (or dismisses) the checklist: it does not open by itself again.
    pub fn finish(&self) -> Result<SetupState, SetupCommandError> {
        self.update(|f| f.finished = true)
    }
}
