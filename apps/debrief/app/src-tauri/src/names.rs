//! Human-name matching in the shell (ADR 0053): each window holds the [`Names`] of its workspace, rebuilt when the
//! entity catalogue is built (in a cancellable job with phases, never on a request), and persisted (user corrections and old AI aliases) in the configuration folder.
//!
//! Thin by design: grouping, matching, ranking and questions are the engine's (`debrief_core::names`); this module
//! owns the files and the commands. No model is asked here: names are never sent to a model unless the user starts
//! it (rule 46).
//!
//! Commands (all validated here, the engine validates again): `names_accept`, `names_reject`, `names_undo`,
//! `names_remove_alias`; `find_values` (in `search_everything`) answers through [`WindowNames::search`].

use crate::error::{CmdError, ErrorCode};
use crate::jobs::queue::JobEnd;
use crate::jobs::{JobCtx, JobKind, JobSpec, Jobs, PhaseScale};
use crate::windows::{CurrentWindow, WindowState, Windows};
use crate::CmdResult;
use debrief_core::catalogue::CatalogueStatusRequest;
use debrief_core::find::Resolved;
use debrief_core::names::book::{NameBook, UserFact, MAX_TERM_CHARS};
use debrief_core::names::{Names, RebuildControl, RebuildEnd, RebuildProgress};
use debrief_core::ports::Store;
use debrief_core::search_all::SearchSource;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

/// File of the name book below the configuration folder.
pub const BOOK_FILE: &str = "names/book.json";
/// Longest group id accepted from the webview.
const MAX_ID_CHARS: usize = 256;
/// The longest one wait on the store's "catalogue built" signal takes; a closed window is noticed between waits.
const WATCH_SLICE: Duration = Duration::from_millis(1_000);
/// How long a rebuild waits for the catalogue in all before it gives up (the next search asks again).
const WATCH_MAX: Duration = Duration::from_secs(10 * 60);

/// What the table was last built from: the store's catalogue generation (it changes with every import, replacement,
/// removal, refresh and tail append). A table is current while the store is still at this generation.
type Version = u64;

/// Whether the name table matches the workspace. Asking never rebuilds anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Built from the workspace as it is.
    Current,
    /// The workspace changed (or no table was built yet): a rebuild job is due.
    Stale,
    /// The store cannot say: the table stays as it is.
    Unknown,
}

/// How a rebuild ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebuildOutcome {
    /// The table was replaced; it has this many groups.
    Built(usize),
    /// The catalogue was not built (the job is queued only once it is, so this is a race with a new import).
    NotBuilt,
    Cancelled,
    Failed(String),
}

#[derive(Default)]
struct Inner {
    names: Arc<Names>,
    file: Option<PathBuf>,
    /// The workspace version of the table now in use.
    built_for: Mutex<Option<Version>>,
    /// Rebuild jobs of this window that started, for the tests that count them (single flight is the job key's).
    #[cfg(test)]
    runs: std::sync::atomic::AtomicUsize,
}

/// A window's name layer. Cheap to clone.
#[derive(Clone, Default)]
pub struct WindowNames(Arc<Inner>);

fn version_of(store: &dyn Store) -> Option<Version> {
    store
        .catalogue_status(&CatalogueStatusRequest::default())
        .ok()
        .map(|s| s.generation)
}

/// A job's progress as the Jobs pane shows it: its own label and scale (a growing count when the total is not known).
fn job_phase(p: RebuildProgress) -> (&'static str, PhaseScale, u64, u64) {
    let scale = if p.total == 0 {
        PhaseScale::Indeterminate
    } else {
        PhaseScale::Count
    };
    (p.phase.label(), scale, p.done, p.total)
}

impl WindowNames {
    /// The layer with its book read from `file` (a missing or damaged file gives an empty book and a log line).
    pub fn open(file: Option<PathBuf>) -> WindowNames {
        let names = Arc::new(Names::default());
        if let Some(f) = &file {
            let (book, note) = NameBook::load(f);
            if let Some(n) = note {
                log::warn!("{n}");
            }
            names.set_book(book);
        }
        WindowNames(Arc::new(Inner {
            names,
            file,
            ..Inner::default()
        }))
    }

    pub fn names(&self) -> &Arc<Names> {
        &self.0.names
    }

    /// Writes the book (user corrections and old AI aliases). A failure is logged: the session keeps working.
    pub fn save(&self) {
        let Some(file) = &self.0.file else { return };
        if let Some(dir) = file.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log::warn!("the name book folder cannot be created: {e}");
                return;
            }
        }
        if let Err(e) = self.0.names.read(|b, _| b.save(file)) {
            log::warn!("the name book cannot be saved: {e}");
        }
    }

    /// Whether the table matches the workspace (one cheap summary call; nothing is rebuilt).
    pub fn freshness(&self, store: &dyn Store) -> Freshness {
        let Some(version) = version_of(store) else {
            return Freshness::Unknown;
        };
        if *crate::sync::lock(&self.0.built_for) == Some(version) && self.0.names.is_built() {
            Freshness::Current
        } else {
            Freshness::Stale
        }
    }

    /// The layer for the assistant: the last table, at once.
    pub fn for_assistant(&self) -> Arc<Names> {
        self.0.names.clone()
    }

    /// The values and entities the words refer to (see [`Names::search`]), from the last table, with whether it is
    /// still current. A caller that sees `Stale` queues a rebuild ([`queue_if_stale`]); a request never rebuilds.
    pub fn search(&self, store: &dyn Store, text: &str, limit: usize) -> CmdResult<(Resolved, Freshness)> {
        let fresh = self.freshness(store);
        let found = self
            .0
            .names
            .search(store, text, limit)
            .map_err(|e| CmdError::new(ErrorCode::Internal, format!("the values cannot be looked up: {e}")))?;
        Ok((found, fresh))
    }

    /// How many rebuild jobs started their work.
    #[cfg(test)]
    pub(crate) fn rebuild_runs(&self) -> usize {
        self.0.runs.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Rebuilds the table now, on the calling thread, which must be a job's (never a request's): reports its phases
    /// to `ctx` and stops at the next step when `ctx` is cancelled. A catalogue that is not built ends the rebuild
    /// without reading any names ([`schedule_rebuild`] queues the job only once it is built).
    pub fn run_rebuild(&self, store: &dyn Store, ctx: &JobCtx) -> RebuildOutcome {
        let version = version_of(store);
        let ctl = RebuildControl {
            cancelled: &|| ctx.is_cancelled(),
            progress: &|p| {
                let (label, scale, done, total) = job_phase(p);
                ctx.phase(label, scale, done, total);
            },
        };
        match self.0.names.rebuild(store, &ctl) {
            Ok(RebuildEnd::Built { groups }) => {
                *crate::sync::lock(&self.0.built_for) = version;
                RebuildOutcome::Built(groups)
            }
            Ok(RebuildEnd::Cancelled) => RebuildOutcome::Cancelled,
            Ok(RebuildEnd::NotBuilt) => RebuildOutcome::NotBuilt,
            Err(e) => RebuildOutcome::Failed(format!("the names cannot be read: {e}")),
        }
    }
}

/// How a wait for the catalogue ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    /// Every source is catalogued.
    Built,
    /// The wait was cancelled or the window went away, the store cannot say or cannot wait, or the time ran out.
    GaveUp,
}

/// Blocks until the store's catalogue is built, woken by the store when its last source is done (each call to
/// `catalogue_status` waits up to `slice` inside the store; nothing here sleeps). Reads no names. `alive` is asked
/// before each wait (it reports the wait as a phase and says whether to go on). A store that answers "not built" at once, without waiting, cannot signal: the wait gives up.
fn wait_for_catalogue(source: &(impl SearchSource + ?Sized), mut alive: impl FnMut() -> bool, slice: Duration, longest: Duration) -> Wait {
    let started = std::time::Instant::now();
    let mut after = None;
    while started.elapsed() < longest && alive() {
        let asked = std::time::Instant::now();
        let status = match source.catalogue_status(&CatalogueStatusRequest {
            after,
            wait_ms: slice.as_millis() as u64,
        }) {
            Ok(s) => s,
            Err(_) => return Wait::GaveUp,
        };
        if status.built {
            return Wait::Built;
        }
        // An answer that came at once with nothing new was not a wait: the store cannot signal, and asking again would spin.
        if after == Some(status.generation) && asked.elapsed() < slice / 2 {
            return Wait::GaveUp;
        }
        after = Some(status.generation);
    }
    Wait::GaveUp
}

/// The window's rebuild job: a job of its own kind ([`JobKind::Names`], one per window), so while it waits for the
/// catalogue it holds no Maintenance slot, and keyed by the window, so however many requests and imports ask, one
/// rebuild is queued or running (ADR 0024 section 3: `submit` returns the job with the same key).
pub(crate) fn rebuild_spec(label: &str) -> JobSpec {
    JobSpec::new(JobKind::Names, "Updating names")
        .window(label)
        .quiet()
        .keyed(format!("names\u{1f}{label}"))
}

/// Queues the window's rebuild job unless one is queued or running already.
pub fn schedule_rebuild<R: Runtime>(app: &AppHandle<R>, label: &str) {
    let Some(jobs) = app.try_state::<Jobs>() else { return };
    let (app, label_owned) = (app.clone(), label.to_string());
    let queued = jobs.submit_work(rebuild_spec(label), crate::jobs::Controls::default(), move |ctx| {
        // Nobody waits for it: background priority (ADR 0024 section 1). The job has a thread of its own.
        debrief_core::threads::lower_priority();
        let end = match app.try_state::<Windows>().and_then(|w| w.get(&label_owned)) {
            Some(win) => {
                #[cfg(test)]
                win.names.0.runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                match win.engine.store() {
                    Ok(store) => {
                        let mut waits = 0u64;
                        let alive = || {
                            waits += 1;
                            ctx.phase("Waiting for the catalogue", PhaseScale::Indeterminate, waits, 0);
                            !ctx.is_cancelled() && app.try_state::<Windows>().is_some_and(|w| w.get(&label_owned).is_some())
                        };
                        match wait_for_catalogue(store.as_ref(), alive, WATCH_SLICE, WATCH_MAX) {
                            Wait::Built => win.names.run_rebuild(store.as_ref(), ctx),
                            Wait::GaveUp if ctx.is_cancelled() => RebuildOutcome::Cancelled,
                            Wait::GaveUp => RebuildOutcome::NotBuilt,
                        }
                    }
                    Err(e) => RebuildOutcome::Failed(e.to_string()),
                }
            }
            None => RebuildOutcome::Cancelled,
        };
        match end {
            RebuildOutcome::Built(n) => {
                ctx.message(&format!("{n} names"));
                JobEnd::Done(None)
            }
            RebuildOutcome::NotBuilt => {
                ctx.message("The catalogue is not built yet");
                JobEnd::Done(None)
            }
            RebuildOutcome::Cancelled => JobEnd::Cancelled,
            RebuildOutcome::Failed(e) => JobEnd::Failed(e),
        }
    });
    if let Err(e) = queued {
        log::warn!("the name table cannot be rebuilt now: {e}");
    }
}

/// A request found the window's table out of date: queue the rebuild job (at most one) and carry on with the last
/// table. Reads the workspace version, nothing else; call it off the UI thread.
pub fn queue_if_stale<R: Runtime>(app: &AppHandle<R>, win: &WindowState) -> bool {
    let Ok(store) = win.engine.store() else { return false };
    if win.names.freshness(store.as_ref()) != Freshness::Stale {
        return false;
    }
    schedule_rebuild(app, win.label());
    true
}

/// An import finished in `label`: its name table is rebuilt, in a job, once the catalogue is built.
pub fn after_import<R: Runtime>(app: &AppHandle<R>, label: &str) {
    schedule_rebuild(app, label);
}

// Commands ---------------------------------------------------------------------------------------------------------

/// What the user said about a match: the words they typed and the entity (`group` of a `ValueMatch`) they mean or not.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameFeedback {
    pub text: String,
    pub group: String,
    /// Remember it for every workspace, not only this one.
    #[serde(default)]
    pub global: bool,
}

/// An AI alias to remove from an entity.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveAlias {
    pub group: String,
    pub alias: String,
}

/// The fact that was recorded (its id undoes it), or none when the book already said so.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FactReply {
    pub fact: Option<u64>,
}

/// What `names_undo` took back.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoReply {
    pub undone: Option<UserFact>,
}

/// What `names_remove_alias` did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedReply {
    pub removed: bool,
}

fn invalid(message: impl Into<String>) -> CmdError {
    CmdError::new(ErrorCode::Invalid, message)
}

/// A phrase from the webview: non-empty, bounded, no control characters.
fn phrase(what: &str, s: &str) -> CmdResult<String> {
    let t = s.trim();
    if t.is_empty() {
        return Err(invalid(format!("{what} is empty")));
    }
    if t.chars().count() > MAX_TERM_CHARS {
        return Err(invalid(format!("{what} is longer than {MAX_TERM_CHARS} characters")));
    }
    if t.chars().any(char::is_control) {
        return Err(invalid(format!("{what} holds control characters")));
    }
    Ok(t.to_string())
}

/// A group id the book knows.
fn known_group(names: &Names, id: &str) -> CmdResult<String> {
    if id.is_empty() || id.chars().count() > MAX_ID_CHARS {
        return Err(invalid("the name is not valid; search again"));
    }
    if names.read(|b, _| b.group(id).is_none()) {
        return Err(invalid("that name is no longer in the logs; search again"));
    }
    Ok(id.to_string())
}

fn feedback(
    state: &WindowState,
    request: &NameFeedback,
    apply: impl FnOnce(&mut NameBook, &str, &str, bool) -> Option<u64>,
) -> CmdResult<FactReply> {
    let text = phrase("the words", &request.text)?;
    let group = known_group(state.names.names(), &request.group)?;
    let fact = state.names.names().write(|b| apply(b, &text, &group, request.global));
    state.names.save();
    Ok(FactReply { fact })
}

/// "Yes, that is what I meant": the words now mean the entity (outranks everything, including the AI's aliases).
#[tauri::command]
pub async fn names_accept(state: CurrentWindow, request: NameFeedback) -> CmdResult<FactReply> {
    feedback(&state, &request, NameBook::accept)
}

/// "No, not that": the words never mean the entity again.
#[tauri::command]
pub async fn names_reject(state: CurrentWindow, request: NameFeedback) -> CmdResult<FactReply> {
    feedback(&state, &request, NameBook::reject)
}

/// Takes back the user's latest correction.
#[tauri::command]
pub async fn names_undo(state: CurrentWindow) -> CmdResult<UndoReply> {
    let undone = state.names.names().write(NameBook::undo);
    state.names.save();
    Ok(UndoReply { undone })
}

/// Removes an AI-suggested alias (it is not suggested again by the same model).
#[tauri::command]
pub async fn names_remove_alias(state: CurrentWindow, request: RemoveAlias) -> CmdResult<RemovedReply> {
    let alias = phrase("the alias", &request.alias)?;
    let group = known_group(state.names.names(), &request.group)?;
    let removed = state.names.names().write(|b| b.remove_ai_alias(&group, &alias));
    if removed {
        state.names.save();
    }
    Ok(RemovedReply { removed })
}

#[cfg(test)]
mod tests {
    use super::*;
    use debrief_core::names::group::NameGroup;
    use std::sync::atomic::Ordering;

    fn temp(name: &str) -> PathBuf {
        let d = crate::test_support::scratch_dir(&format!("it-names-{name}"));
        d
    }

    fn with_group(win: &WindowNames, spelling: &str, count: u64) {
        use debrief_core::catalogue::{Candidate, Scope};
        use debrief_core::entities::EntityKind;
        use debrief_core::names::group::group;
        let c = Candidate {
            scope: Scope::Entity(EntityKind::Executable),
            name: spelling.into(),
            count,
            first_ts: None,
            last_ts: None,
            seen_in: vec![],
        };
        let groups: Vec<NameGroup> = group(&[c], &debrief_core::names::Vocabulary::builtin());
        win.names().write(|b| b.rebuild(groups));
    }

    #[test]
    fn the_book_survives_a_restart_and_tolerates_bad_files() {
        let dir = temp("persist");
        let file = dir.join(BOOK_FILE);
        let win = WindowNames::open(Some(file.clone()));
        with_group(&win, "graphicbuilder.exe", 9);
        assert!(win
            .names()
            .write(|b| b.accept("gfx", "executable|graphic builder", false))
            .is_some());
        win.save();
        let again = WindowNames::open(Some(file.clone()));
        assert_eq!(again.names().read(|b, _| b.facts().len()), 1);
        // A damaged file gives an empty book, not a failure.
        std::fs::write(&file, b"{ not json").unwrap();
        assert_eq!(WindowNames::open(Some(file.clone())).names().read(|b, _| b.facts().len()), 0);
        // A missing file and no file at all too.
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(WindowNames::open(Some(file)).names().read(|b, _| b.groups().len()), 0);
        WindowNames::open(None).save();
    }

    #[test]
    fn webview_arguments_are_validated() {
        assert!(phrase("x", "  ").is_err());
        assert!(phrase("x", &"a".repeat(MAX_TERM_CHARS + 1)).is_err());
        assert!(phrase("x", "a\u{0}b").is_err());
        assert_eq!(phrase("x", "  gfx ").unwrap(), "gfx");
        let win = WindowNames::open(None);
        with_group(&win, "graphicbuilder.exe", 9);
        assert!(known_group(win.names(), "executable|graphic builder").is_ok());
        for bad in ["", "nope", &"g".repeat(MAX_ID_CHARS + 1)] {
            assert_eq!(known_group(win.names(), bad).unwrap_err().code, ErrorCode::Invalid);
        }
    }

    #[test]
    fn accept_reject_undo_and_remove_alias_change_the_book_and_save_it() {
        let dir = temp("feedback");
        let file = dir.join(BOOK_FILE);
        let win = WindowNames::open(Some(file.clone()));
        with_group(&win, "graphicbuilder.exe", 9);
        let mut state = crate::windows::tests::window_state("names-fb", debrief_core::PluginRegistry::new());
        state.names = win.clone();
        let req = |text: &str, group: &str| NameFeedback {
            text: text.into(),
            group: group.into(),
            global: false,
        };
        let id = "executable|graphic builder";
        assert!(feedback(&state, &req("gfx", id), NameBook::accept).unwrap().fact.is_some());
        assert!(feedback(&state, &req("paint", id), NameBook::reject).unwrap().fact.is_some());
        assert_eq!(win.names().read(|b, _| b.facts().len()), 2);
        assert_eq!(WindowNames::open(Some(file)).names().read(|b, _| b.facts().len()), 2, "saved");
        assert_eq!(
            feedback(&state, &req("", id), NameBook::accept).unwrap_err().code,
            ErrorCode::Invalid
        );
        assert_eq!(
            feedback(&state, &req("gfx", "nope"), NameBook::accept).unwrap_err().code,
            ErrorCode::Invalid
        );
        assert!(win.names().write(NameBook::undo).is_some());
        assert_eq!(win.names().read(|b, _| b.facts().len()), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn at_most_one_rebuild_job_is_queued_however_many_requests_ask() {
        let jobs = Jobs::new(Arc::new(crate::jobs::SystemClock));
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let ran = std::sync::atomic::AtomicUsize::new(0);
        let ran = Arc::new(ran);
        let first = {
            let ran = ran.clone();
            jobs.submit_work(rebuild_spec("main"), crate::jobs::Controls::default(), move |_| {
                ran.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let _ = gate.lock().unwrap().recv();
                JobEnd::Done(None)
            })
            .unwrap()
        };
        // 64 requests on 8 threads, all finding the table stale while the first rebuild is queued or running.
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    for _ in 0..8 {
                        let ran = ran.clone();
                        let id = jobs
                            .submit_work(rebuild_spec("main"), crate::jobs::Controls::default(), move |_| {
                                ran.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                JobEnd::Done(None)
                            })
                            .unwrap();
                        assert_eq!(id, first, "the request joined the running rebuild");
                    }
                });
            }
        });
        // Another window has its own.
        let other = jobs
            .submit_work(rebuild_spec("w-2"), crate::jobs::Controls::default(), |_| JobEnd::Done(None))
            .unwrap();
        assert_ne!(other, first);
        release.send(()).unwrap();
        assert_eq!(ran.load(std::sync::atomic::Ordering::SeqCst), 1, "only the first one ran");
    }

    /// A catalogue that is built when `release` is called. Its status call blocks up to the asked wait, woken by
    /// `release` (like the store's), or answers at once when `waits` is false (a store that cannot signal).
    struct Gate {
        built: Mutex<bool>,
        cv: std::sync::Condvar,
        waits: bool,
        status: std::sync::atomic::AtomicUsize,
        all: std::sync::atomic::AtomicUsize,
    }

    impl Gate {
        fn new(waits: bool) -> Gate {
            Gate {
                built: Mutex::new(false),
                cv: std::sync::Condvar::new(),
                waits,
                status: Default::default(),
                all: Default::default(),
            }
        }

        fn release(&self) {
            *self.built.lock().unwrap() = true;
            self.cv.notify_all();
        }
    }

    impl SearchSource for Gate {
        fn summary(&self) -> debrief_core::ports::StoreResult<debrief_core::ports::SummaryReply> {
            Err(debrief_core::ports::StoreError::Invalid("not part of the fixture".into()))
        }
        fn page(&self, _: &debrief_core::ports::PageQuery) -> debrief_core::ports::StoreResult<debrief_core::ports::PageReply> {
            Err(debrief_core::ports::StoreError::Invalid("not part of the fixture".into()))
        }
        fn facet(&self, _: &debrief_core::api::FacetRequest) -> debrief_core::ports::StoreResult<debrief_core::api::Facet> {
            Err(debrief_core::ports::StoreError::Invalid("not part of the fixture".into()))
        }
        fn exception_groups(&self) -> debrief_core::ports::StoreResult<Vec<debrief_core::exceptions::ExceptionGroup>> {
            Err(debrief_core::ports::StoreError::Invalid("not part of the fixture".into()))
        }
        fn catalogue(
            &self,
            req: &debrief_core::catalogue::CatalogueRequest,
        ) -> debrief_core::ports::StoreResult<debrief_core::catalogue::CatalogueReply> {
            if req.all {
                self.all.fetch_add(1, Ordering::SeqCst);
            }
            Ok(Default::default())
        }
        fn catalogue_status(
            &self,
            req: &CatalogueStatusRequest,
        ) -> debrief_core::ports::StoreResult<debrief_core::catalogue::CatalogueStatus> {
            self.status.fetch_add(1, Ordering::SeqCst);
            let mut built = self.built.lock().unwrap();
            if self.waits {
                built = self
                    .cv
                    .wait_timeout_while(built, Duration::from_millis(req.wait_ms), |b| !*b)
                    .unwrap()
                    .0;
            }
            Ok(debrief_core::catalogue::CatalogueStatus {
                generation: 1,
                built: *built,
                pending: u64::from(!*built),
            })
        }
    }

    const FAR: Duration = Duration::from_secs(3600);

    #[test]
    fn the_wait_for_the_catalogue_is_woken_by_the_store_and_never_reads_names() {
        let gate = Gate::new(true);
        // One wait of an hour: only the store's signal can end it in time, a timer or a poll loop could not.
        let end = std::thread::scope(|s| {
            let waiter = s.spawn(|| wait_for_catalogue(&gate, || true, FAR, FAR * 2));
            while gate.status.load(Ordering::SeqCst) == 0 {
                std::thread::yield_now();
            }
            gate.release();
            waiter.join().unwrap()
        });
        assert_eq!(end, Wait::Built);
        assert_eq!(gate.status.load(Ordering::SeqCst), 1, "one wait, woken once");
        assert_eq!(
            gate.all.load(Ordering::SeqCst),
            0,
            "the every-name aggregate never ran while it was not built"
        );
    }

    #[test]
    fn the_wait_gives_up_when_cancelled_when_out_of_time_or_when_the_store_cannot_signal() {
        let gate = Gate::new(true);
        assert_eq!(
            wait_for_catalogue(&gate, || false, FAR, FAR),
            Wait::GaveUp,
            "cancelled: not even asked"
        );
        assert_eq!(
            wait_for_catalogue(&gate, || true, FAR, Duration::ZERO),
            Wait::GaveUp,
            "no time left"
        );
        assert_eq!(gate.status.load(Ordering::SeqCst), 0);
        // A store that answers "not built" at once never signals: waiting would spin.
        let mute = Gate::new(false);
        assert_eq!(wait_for_catalogue(&mute, || true, FAR, FAR), Wait::GaveUp);
        assert_eq!(
            mute.status.load(Ordering::SeqCst),
            2,
            "the second answer was at once and new for nothing"
        );
        assert_eq!(mute.all.load(Ordering::SeqCst), 0);
        // Built already: answered at once.
        mute.release();
        assert_eq!(wait_for_catalogue(&mute, || true, FAR, FAR), Wait::Built);
    }

    #[test]
    fn a_rebuild_shows_its_phases_on_their_own_scales() {
        use debrief_core::names::RebuildPhase;
        let known = job_phase(RebuildProgress {
            phase: RebuildPhase::Events,
            done: 3,
            total: 40,
        });
        assert_eq!(known, ("Comparing look-alike names", PhaseScale::Count, 3, 40));
        let unknown = job_phase(RebuildProgress {
            phase: RebuildPhase::Catalogue,
            done: 0,
            total: 0,
        });
        assert_eq!(
            (unknown.1, unknown.2),
            (PhaseScale::Indeterminate, 0),
            "a growing count, never a static bar"
        );
    }
}
