//! Human-name matching in the shell (ADR 0053): each window holds the [`Names`] of its workspace, rebuilt when the
//! entity catalogue is built (in a cancellable job with phases, never on a request), and persisted (user corrections and old AI aliases) in the configuration folder.
//!
//! Thin by design: grouping, matching, ranking and questions are the engine's (`debrief_core::names`); this module
//! owns the files and the commands. No model is asked here: names are never sent to a model unless the user starts
//! it (rule 46).
//!
//! Commands (all validated here, the engine validates again): `names_accept`, `names_reject`, `names_undo`,
//! `names_remove_alias`; `find_values` (in `search_everything`) answers through [`WindowNames::search`].

use crate::engine::StoreId;
use crate::error::{CmdError, ErrorCode};
use crate::jobs::queue::JobEnd;
use crate::jobs::{JobCtx, JobKind, JobSpec, Jobs, PhaseScale};
use crate::windows::{CurrentWindow, WindowState, Windows};
use crate::CmdResult;
use debrief_core::catalogue::CatalogueStatusRequest;
use debrief_core::find::Resolved;
use debrief_core::names::book::{NameBook, UserFact, MAX_TERM_CHARS};
use debrief_core::names::{Names, RebuildControl, RebuildEnd, RebuildPhase, RebuildProgress};
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
/// The longest one wait on the store's "catalogue built" signal takes. The store wakes the wait as soon as its catalogue
/// changes; the slice only bounds how long a cancel, a closed window or a replaced store goes unnoticed. Long, because
/// each end of a slice is a statement on an otherwise idle store.
const WATCH_SLICE: Duration = Duration::from_secs(10);
/// How long the catalogue may go without any change before a rebuild gives up waiting for it (the next search asks
/// again). A catalogue that keeps moving is waited for however long it takes.
const WATCH_STALL: Duration = Duration::from_secs(10 * 60);

/// What the table was last built from: the window's store and its catalogue generation (the generation changes with
/// every import, replacement, removal, refresh and tail append, but two stores can both be at generation 3). A table is
/// current while the window still holds that store and it is still at that generation.
type Version = (StoreId, u64);

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
    /// The table was already built from the store as it is now: nothing was read (a trigger that arrived while the
    /// last rebuild ran, with nothing changed since).
    UpToDate,
    Cancelled,
    Failed(String),
}

#[derive(Default)]
struct Inner {
    names: Arc<Names>,
    file: Option<PathBuf>,
    /// The workspace version of the table now in use.
    built_for: Mutex<Option<Version>>,
    /// Requests that found a rebuild queued and left it to that job, since the last job ended (the job's log line says).
    joined: std::sync::atomic::AtomicU64,
    /// Rebuilds of this window that read the catalogue, for the tests that count them (single flight is the job key's).
    #[cfg(test)]
    runs: std::sync::atomic::AtomicUsize,
}

/// A window's name layer. Cheap to clone.
#[derive(Clone, Default)]
pub struct WindowNames(Arc<Inner>);

fn version_of(id: StoreId, store: &(impl SearchSource + ?Sized)) -> Option<Version> {
    store
        .catalogue_status(&CatalogueStatusRequest::default())
        .ok()
        .map(|s| (id, s.generation))
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
    pub fn freshness(&self, id: StoreId, store: &(impl SearchSource + ?Sized)) -> Freshness {
        let Some(version) = version_of(id, store) else {
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
    pub fn search(&self, id: StoreId, store: &dyn Store, text: &str, limit: usize) -> CmdResult<(Resolved, Freshness)> {
        let fresh = self.freshness(id, store);
        let found = self
            .0
            .names
            .search(store, text, limit)
            .map_err(|e| CmdError::new(ErrorCode::Internal, format!("the values cannot be looked up: {e}")))?;
        Ok((found, fresh))
    }

    /// How many rebuilds read the catalogue (a job that finds the table current reads nothing and is not counted).
    #[cfg(test)]
    pub(crate) fn rebuild_runs(&self) -> usize {
        self.0.runs.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Rebuilds the table now, on the calling thread, which must be a job's (never a request's): reports its phases
    /// to `ctx` and stops at the next step when `ctx` is cancelled. A catalogue that is not built ends the rebuild
    /// without reading any names ([`schedule_rebuild`] queues the job only once it is built). `id` is the window's store
    /// that `store` is; the table is marked as built from that store.
    #[cfg(test)]
    pub fn run_rebuild(&self, id: StoreId, store: &(impl SearchSource + Sync + ?Sized), ctx: &JobCtx) -> RebuildOutcome {
        self.rebuild_timed(id, store, ctx, &|| false).0
    }

    /// As [`run_rebuild`](Self::run_rebuild), also stopping (as cancelled) when `stop` says so, and returning how long
    /// each phase took. `stop` is asked at the rebuild's steps, the last one just before the table is replaced.
    fn rebuild_timed(
        &self,
        id: StoreId,
        store: &(impl SearchSource + Sync + ?Sized),
        ctx: &JobCtx,
        stop: &dyn Fn() -> bool,
    ) -> (RebuildOutcome, PhaseTimes) {
        #[cfg(test)]
        self.0.runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // Read before the names, so a change during the rebuild leaves the table stale.
        let version = version_of(id, store);
        let times = std::cell::RefCell::new(PhaseTimes::default());
        let ctl = RebuildControl {
            cancelled: &|| ctx.is_cancelled() || stop(),
            progress: &|p| {
                times.borrow_mut().enter(p.phase);
                let (label, scale, done, total) = job_phase(p);
                ctx.phase(label, scale, done, total);
            },
        };
        let outcome = match self.0.names.rebuild(store, &ctl) {
            Ok(RebuildEnd::Built { groups }) => {
                *crate::sync::lock(&self.0.built_for) = version;
                RebuildOutcome::Built(groups)
            }
            Ok(RebuildEnd::Cancelled) => RebuildOutcome::Cancelled,
            Ok(RebuildEnd::NotBuilt) => RebuildOutcome::NotBuilt,
            Err(e) => RebuildOutcome::Failed(format!("the names cannot be read: {e}")),
        };
        (outcome, times.into_inner().finish())
    }
}

/// How long each phase of a rebuild took (the phases run in order, each once).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PhaseTimes {
    done: Vec<(RebuildPhase, Duration)>,
    current: Option<(RebuildPhase, std::time::Instant)>,
}

impl PhaseTimes {
    /// A progress report of `phase`: the phase before it, if another, ends now.
    fn enter(&mut self, phase: RebuildPhase) {
        if self.current.is_some_and(|(p, _)| p == phase) {
            return;
        }
        self.close();
        self.current = Some((phase, std::time::Instant::now()));
    }

    fn close(&mut self) {
        if let Some((p, since)) = self.current.take() {
            self.done.push((p, since.elapsed()));
        }
    }

    fn finish(mut self) -> PhaseTimes {
        self.close();
        self
    }

    /// Milliseconds of `phase` (0 when it did not run).
    fn ms(&self, phase: RebuildPhase) -> u64 {
        self.done
            .iter()
            .filter(|(p, _)| *p == phase)
            .map(|(_, d)| d.as_millis() as u64)
            .sum()
    }
}

/// Why a rebuild job stopped waiting for the catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WaitEnd {
    /// Every source is done (catalogued, or given up on).
    Built,
    /// The job was cancelled.
    Cancelled,
    /// The window was closed.
    WindowGone,
    /// The window holds another store now (a project was opened): this store's names are not wanted.
    StoreChanged,
    /// The store answered "not built" at once, without waiting: it cannot signal, and asking again would spin.
    CannotSignal,
    /// The catalogue did not change for [`WATCH_STALL`].
    Stalled,
    /// The store could not be reached or could not say; the text is its error.
    Unavailable(String),
}

impl WaitEnd {
    fn as_str(&self) -> &'static str {
        match self {
            WaitEnd::Built => "built",
            WaitEnd::Cancelled => "cancelled",
            WaitEnd::WindowGone => "window gone",
            WaitEnd::StoreChanged => "store changed",
            WaitEnd::CannotSignal => "store cannot signal",
            WaitEnd::Stalled => "stalled",
            WaitEnd::Unavailable(_) => "store unavailable",
        }
    }
}

/// Why a job is asked to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Cancelled,
    WindowGone,
}

/// The wait for the catalogue: how it ended and what it saw.
struct Waited<S: ?Sized> {
    end: WaitEnd,
    /// The window's store and its identity when the wait began (none: it could not be reached).
    store: Option<(StoreId, Arc<S>)>,
    /// Waits made (each up to one slice).
    slices: u64,
    /// Sources a build gave up on, as last reported (they count as done).
    failed: u64,
    waited: Duration,
}

/// Blocks until the catalogue of the window's store is built, woken by the store when it changes (each call to
/// `catalogue_status` waits up to `slice` inside the store). Reads no names. Before each wait it asks `gate` whether to go
/// on and `current` which store the window holds: a store other than the one the wait began with ends it
/// ([`WaitEnd::StoreChanged`]). It gives up when the catalogue's generation has not moved for `stall` (`now` is the clock),
/// never because a fixed time passed: a catalogue that keeps changing is waited for. A store that answers "not built"
/// at once, without waiting, cannot signal: the wait gives up.
fn wait_for_catalogue<S: SearchSource + ?Sized>(
    current: impl Fn() -> Result<(StoreId, Arc<S>), String>,
    mut gate: impl FnMut() -> Option<Stop>,
    slice: Duration,
    stall: Duration,
    now: impl Fn() -> Duration,
) -> Waited<S> {
    let started = now();
    let mut slices = 0;
    let done = |end, store, slices, failed, at: Duration| Waited {
        end,
        store,
        slices,
        failed,
        waited: at.saturating_sub(started),
    };
    let (id, store) = match current() {
        Ok(c) => c,
        Err(e) => return done(WaitEnd::Unavailable(e), None, 0, 0, now()),
    };
    let held = || Some((id, store.clone()));
    let (mut after, mut moved_at, mut failed) = (None, now(), 0);
    loop {
        match gate() {
            Some(Stop::Cancelled) => return done(WaitEnd::Cancelled, held(), slices, failed, now()),
            Some(Stop::WindowGone) => return done(WaitEnd::WindowGone, held(), slices, failed, now()),
            None => {}
        }
        match current() {
            Ok((now_id, _)) if now_id != id => return done(WaitEnd::StoreChanged, held(), slices, failed, now()),
            Err(e) => return done(WaitEnd::Unavailable(e), held(), slices, failed, now()),
            Ok(_) => {}
        }
        let asked = std::time::Instant::now();
        slices += 1;
        let status = match store.catalogue_status(&CatalogueStatusRequest {
            after,
            wait_ms: slice.as_millis() as u64,
        }) {
            Ok(s) => s,
            Err(e) => return done(WaitEnd::Unavailable(e.to_string()), held(), slices, failed, now()),
        };
        failed = status.failed;
        if status.built {
            return done(WaitEnd::Built, held(), slices, failed, now());
        }
        if after == Some(status.generation) {
            // An answer that came at once with nothing new was not a wait: the store cannot signal.
            if asked.elapsed() < slice / 2 {
                return done(WaitEnd::CannotSignal, held(), slices, failed, now());
            }
            if now().saturating_sub(moved_at) >= stall {
                return done(WaitEnd::Stalled, held(), slices, failed, now());
            }
        } else {
            moved_at = now();
        }
        after = Some(status.generation);
    }
}

/// What one rebuild job did, for its log line and its tests.
struct RebuildReport {
    outcome: RebuildOutcome,
    wait: WaitEnd,
    slices: u64,
    waited: Duration,
    failed_sources: u64,
    phases: PhaseTimes,
    /// Requests that left it to this job (or to the one queued before it) since the last job ended.
    joined: u64,
}

impl RebuildReport {
    /// One structured line per ended job: the outcome, why the wait ended, how long it waited and each phase took, how
    /// many groups were built, which sources failed to be catalogued and how many requests joined. No names.
    fn log(&self, job: u64) {
        let outcome = match &self.outcome {
            RebuildOutcome::Built(_) => "built",
            RebuildOutcome::NotBuilt => "not built",
            RebuildOutcome::UpToDate => "up to date",
            RebuildOutcome::Cancelled => "cancelled",
            RebuildOutcome::Failed(_) => "failed",
        };
        let groups = if let RebuildOutcome::Built(n) = self.outcome { n as u64 } else { 0 };
        let ph = |p| self.phases.ms(p);
        log::log!(
            if matches!(self.outcome, RebuildOutcome::Failed(_)) { log::Level::Warn } else { log::Level::Info },
            Job = job,
            Outcome = outcome,
            WaitEnd = self.wait.as_str(),
            WaitMs = self.waited.as_millis() as u64,
            Waits = self.slices,
            Groups = groups,
            FailedSources = self.failed_sources,
            JoinedRequests = self.joined,
            CatalogueMs = ph(RebuildPhase::Catalogue),
            GroupingMs = ph(RebuildPhase::Grouping),
            EventsMs = ph(RebuildPhase::Events),
            JoiningMs = ph(RebuildPhase::Joining),
            PublishingMs = ph(RebuildPhase::Publishing);
            "Name table rebuild {outcome}{}",
            match (&self.outcome, &self.wait) {
                (RebuildOutcome::Failed(e), _) => format!(": {e}"),
                (_, WaitEnd::Unavailable(e)) => format!(": the store could not be asked: {e}"),
                _ => String::new(),
            }
        );
    }
}

/// The body of a rebuild job: waits for the catalogue of the window's store (see [`wait_for_catalogue`]), then rebuilds the
/// table from that store, which it stops as cancelled if the window gets another store meanwhile, so the table never
/// holds the names of a store the window has left.
fn rebuild_job<S: SearchSource + Sync + ?Sized>(
    names: &WindowNames,
    ctx: &JobCtx,
    current: impl Fn() -> Result<(StoreId, Arc<S>), String>,
    mut gate: impl FnMut() -> Option<Stop>,
    slice: Duration,
    stall: Duration,
) -> RebuildReport {
    let clock = std::time::Instant::now();
    let waited = wait_for_catalogue(&current, &mut gate, slice, stall, || clock.elapsed());
    let joined = names.0.joined.swap(0, std::sync::atomic::Ordering::SeqCst);
    let (outcome, phases) = match (&waited.end, &waited.store) {
        (WaitEnd::Built, Some((id, store))) if names.freshness(*id, store.as_ref()) == Freshness::Current => {
            (RebuildOutcome::UpToDate, PhaseTimes::default())
        }
        (WaitEnd::Built, Some((id, store))) => {
            let moved_on = || current().map_or(true, |(now, _)| now != *id);
            names.rebuild_timed(*id, store.as_ref(), ctx, &moved_on)
        }
        (WaitEnd::Cancelled | WaitEnd::WindowGone | WaitEnd::StoreChanged, _) => (RebuildOutcome::Cancelled, PhaseTimes::default()),
        (WaitEnd::CannotSignal | WaitEnd::Stalled, _) => (RebuildOutcome::NotBuilt, PhaseTimes::default()),
        (WaitEnd::Unavailable(e), _) => (RebuildOutcome::Failed(e.clone()), PhaseTimes::default()),
        (WaitEnd::Built, None) => (RebuildOutcome::Failed("the store went away".into()), PhaseTimes::default()),
    };
    RebuildReport {
        outcome,
        wait: waited.end,
        slices: waited.slices,
        waited: waited.waited,
        failed_sources: waited.failed,
        phases,
        joined,
    }
}

/// The window's rebuild job: a job of its own kind ([`JobKind::Names`], one per window), so while it waits for the
/// catalogue it holds no Maintenance slot. Keyed by the window, and joined only while it is still queued
/// ([`JobSpec::keyed_while_queued`]): however many requests and imports ask, at most one rebuild waits, but a trigger
/// that arrives while a rebuild runs queues the next one, since the running one may have read the catalogue before it
/// (ADR 0024 section 3).
pub(crate) fn rebuild_spec(label: &str) -> JobSpec {
    JobSpec::new(JobKind::Names, "Updating names")
        .window(label)
        .quiet()
        .keyed_while_queued(format!("names\u{1f}{label}"))
}

/// Queues the window's rebuild job unless one is queued already (one that runs does not count: it may have read the
/// catalogue before this request's change).
pub fn schedule_rebuild<R: Runtime>(app: &AppHandle<R>, label: &str) {
    let Some(jobs) = app.try_state::<Jobs>() else { return };
    let (job_app, label_owned) = (app.clone(), label.to_string());
    let queued = jobs.submit_keyed_work(rebuild_spec(label), crate::jobs::Controls::default(), move |ctx| {
        // Nobody waits for it: background priority (ADR 0024 section 1). The job has a thread of its own.
        debrief_core::threads::lower_priority();
        let Some(win) = job_app.try_state::<Windows>().and_then(|w| w.get(&label_owned)) else {
            log::info!(Job = ctx.id.0, Outcome = "cancelled", WaitEnd = "window gone"; "Name table rebuild cancelled");
            return JobEnd::Cancelled;
        };
        let mut waits = 0u64;
        let gate = || {
            waits += 1;
            ctx.phase("Waiting for the catalogue", PhaseScale::Indeterminate, waits, 0);
            if ctx.is_cancelled() {
                Some(Stop::Cancelled)
            } else if job_app.try_state::<Windows>().and_then(|w| w.get(&label_owned)).is_none() {
                Some(Stop::WindowGone)
            } else {
                None
            }
        };
        let current = || win.engine.tagged_store().map_err(|e| e.to_string());
        let report = rebuild_job(&win.names, ctx, current, gate, WATCH_SLICE, WATCH_STALL);
        report.log(ctx.id.0);
        match report.outcome {
            RebuildOutcome::Built(n) if report.failed_sources > 0 => {
                ctx.message(&format!("{n} names; {} sources could not be catalogued", report.failed_sources));
                JobEnd::Done(None)
            }
            RebuildOutcome::Built(n) => {
                ctx.message(&format!("{n} names"));
                JobEnd::Done(None)
            }
            RebuildOutcome::NotBuilt => {
                ctx.message("The catalogue is not built yet");
                JobEnd::Done(None)
            }
            RebuildOutcome::UpToDate => {
                ctx.message("The names are up to date");
                JobEnd::Done(None)
            }
            RebuildOutcome::Cancelled => JobEnd::Cancelled,
            RebuildOutcome::Failed(e) => JobEnd::Failed(e),
        }
    });
    match queued {
        Ok(sub) if !sub.created => {
            if let Some(win) = app.try_state::<Windows>().and_then(|w| w.get(label)) {
                win.names.0.joined.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        Ok(_) => {}
        Err(e) => log::warn!("the name table cannot be rebuilt now: {e}"),
    }
}

/// A request found the window's table out of date: queue the rebuild job (at most one waits) and carry on with the last
/// table. Reads the workspace version, nothing else; call it off the UI thread.
pub fn queue_if_stale<R: Runtime>(app: &AppHandle<R>, win: &WindowState) -> bool {
    let Ok((id, store)) = win.engine.tagged_store() else { return false };
    if win.names.freshness(id, store.as_ref()) != Freshness::Stale {
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
        let d = std::env::temp_dir().join(format!("debrief-it-names-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
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
    fn at_most_one_rebuild_job_waits_however_many_requests_ask_and_a_trigger_during_a_run_queues_the_next() {
        use std::sync::atomic::AtomicUsize;
        let jobs = Jobs::new(Arc::new(crate::jobs::SystemClock));
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let ran = Arc::new(AtomicUsize::new(0));
        let first = {
            let ran = ran.clone();
            jobs.submit_keyed_work(rebuild_spec("main"), crate::jobs::Controls::default(), move |_| {
                ran.fetch_add(1, Ordering::SeqCst);
                let _ = gate.lock().unwrap().recv();
                JobEnd::Done(None)
            })
            .unwrap()
        };
        assert!(first.created);
        while ran.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        // 64 requests on 8 threads arrive while the first rebuild runs: it may have read the catalogue before them, so
        // exactly one more rebuild is queued, and every request is served by it.
        let submissions = Mutex::new(Vec::new());
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    for _ in 0..8 {
                        let ran = ran.clone();
                        let sub = jobs
                            .submit_keyed_work(rebuild_spec("main"), crate::jobs::Controls::default(), move |_| {
                                ran.fetch_add(1, Ordering::SeqCst);
                                JobEnd::Done(None)
                            })
                            .unwrap();
                        submissions.lock().unwrap().push(sub);
                    }
                });
            }
        });
        let submissions = submissions.into_inner().unwrap();
        let second = submissions[0].id;
        assert_ne!(second, first.id, "a trigger during a run is not lost");
        assert!(
            submissions.iter().all(|s| s.id == second),
            "the requests share the one queued rebuild"
        );
        assert_eq!(submissions.iter().filter(|s| s.created).count(), 1);
        assert_eq!(ran.load(Ordering::SeqCst), 1, "the queued one waits for the running one");
        // Another window has its own.
        let other = jobs
            .submit_work(rebuild_spec("w-2"), crate::jobs::Controls::default(), |_| JobEnd::Done(None))
            .unwrap();
        assert!(other != first.id && other != second);
        release.send(()).unwrap();
        while ran.load(Ordering::SeqCst) < 2 {
            std::thread::yield_now();
        }
    }

    /// A store with a catalogue that is built when `release` is called. Its status call blocks up to the asked wait,
    /// woken by `release` (like the store's), or answers at once when `waits` is false (a store that cannot signal).
    struct Gate {
        built: Mutex<bool>,
        cv: std::sync::Condvar,
        waits: bool,
        status: std::sync::atomic::AtomicUsize,
        all: std::sync::atomic::AtomicUsize,
        /// The generation it reports; it moves with every status call when `moving` (sources being catalogued).
        generation: std::sync::atomic::AtomicU64,
        moving: bool,
        failed: u64,
        /// Every status call fails.
        broken: bool,
        /// The names of its catalogue (one event each).
        names: Vec<&'static str>,
        /// A clock it advances by this much at every status call (a store whose time the test controls).
        clock: Option<(Arc<std::sync::atomic::AtomicU64>, u64)>,
    }

    impl Gate {
        fn new(waits: bool) -> Gate {
            Gate {
                built: Mutex::new(false),
                cv: std::sync::Condvar::new(),
                waits,
                status: Default::default(),
                all: Default::default(),
                generation: std::sync::atomic::AtomicU64::new(1),
                moving: false,
                failed: 0,
                broken: false,
                names: Vec::new(),
                clock: None,
            }
        }

        /// Built already, with these names.
        fn built_with(names: &[&'static str]) -> Gate {
            let g = Gate {
                names: names.to_vec(),
                ..Gate::new(true)
            };
            g.release();
            g
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
            use debrief_core::catalogue::{Candidate, Scope};
            use debrief_core::entities::EntityKind;
            if req.all {
                self.all.fetch_add(1, Ordering::SeqCst);
            }
            let built = *self.built.lock().unwrap();
            let candidates = if built && req.all {
                self.names
                    .iter()
                    .map(|n| Candidate {
                        scope: Scope::Entity(EntityKind::Executable),
                        name: (*n).into(),
                        count: 5,
                        first_ts: None,
                        last_ts: None,
                        seen_in: vec![],
                    })
                    .collect()
            } else {
                Vec::new()
            };
            Ok(debrief_core::catalogue::CatalogueReply {
                built,
                pending: u64::from(!built),
                candidates,
            })
        }
        fn catalogue_status(
            &self,
            req: &CatalogueStatusRequest,
        ) -> debrief_core::ports::StoreResult<debrief_core::catalogue::CatalogueStatus> {
            self.status.fetch_add(1, Ordering::SeqCst);
            if self.broken {
                return Err(debrief_core::ports::StoreError::Invalid("the store cannot say".into()));
            }
            if let Some((clock, step)) = &self.clock {
                clock.fetch_add(*step, Ordering::SeqCst);
            }
            let mut built = self.built.lock().unwrap();
            if self.waits {
                built = self
                    .cv
                    .wait_timeout_while(built, Duration::from_millis(req.wait_ms), |b| !*b)
                    .unwrap()
                    .0;
            }
            let generation = if self.moving {
                self.generation.fetch_add(1, Ordering::SeqCst) + 1
            } else {
                self.generation.load(Ordering::SeqCst)
            };
            Ok(debrief_core::catalogue::CatalogueStatus {
                generation,
                built: *built,
                pending: u64::from(!*built),
                failed: if *built { self.failed } else { 0 },
            })
        }
    }

    const FAR: Duration = Duration::from_secs(3600);

    /// The window's store as a test holds it: the store and its identity, replaceable.
    struct Holder(Mutex<(StoreId, Arc<Gate>)>);

    impl Holder {
        fn new(id: u64, gate: Gate) -> Holder {
            Holder(Mutex::new((StoreId::for_test(id), Arc::new(gate))))
        }
        fn current(&self) -> Result<(StoreId, Arc<Gate>), String> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn replace(&self, id: u64, gate: Gate) {
            *self.0.lock().unwrap() = (StoreId::for_test(id), Arc::new(gate));
        }
        fn gate(&self) -> Arc<Gate> {
            self.0.lock().unwrap().1.clone()
        }
    }

    fn no_clock() -> Duration {
        Duration::ZERO
    }

    #[test]
    fn the_wait_for_the_catalogue_is_woken_by_the_store_and_never_reads_names() {
        let holder = Holder::new(1, Gate::new(true));
        // One wait of an hour: only the store's signal can end it in time, a timer or a poll loop could not.
        let waited = std::thread::scope(|s| {
            let waiter = s.spawn(|| wait_for_catalogue(|| holder.current(), || None, FAR, FAR * 2, no_clock));
            while holder.gate().status.load(Ordering::SeqCst) == 0 {
                std::thread::yield_now();
            }
            holder.gate().release();
            waiter.join().unwrap()
        });
        assert_eq!(waited.end, WaitEnd::Built);
        assert_eq!(waited.slices, 1, "one wait, woken once");
        assert_eq!(
            holder.gate().all.load(Ordering::SeqCst),
            0,
            "the every-name aggregate never ran while it was not built"
        );
    }

    #[test]
    fn the_wait_gives_up_when_cancelled_when_the_window_is_gone_or_when_the_store_cannot_signal() {
        let holder = Holder::new(1, Gate::new(true));
        let waited = wait_for_catalogue(|| holder.current(), || Some(Stop::Cancelled), FAR, FAR, no_clock);
        assert_eq!(waited.end, WaitEnd::Cancelled, "cancelled: not even asked");
        let waited = wait_for_catalogue(|| holder.current(), || Some(Stop::WindowGone), FAR, FAR, no_clock);
        assert_eq!(waited.end, WaitEnd::WindowGone);
        assert_eq!(holder.gate().status.load(Ordering::SeqCst), 0);
        // A store that answers "not built" at once never signals: waiting would spin.
        let mute = Holder::new(1, Gate::new(false));
        let waited = wait_for_catalogue(|| mute.current(), || None, FAR, FAR, no_clock);
        assert_eq!(waited.end, WaitEnd::CannotSignal);
        assert_eq!(
            mute.gate().status.load(Ordering::SeqCst),
            2,
            "the second answer was at once and new for nothing"
        );
        assert_eq!(mute.gate().all.load(Ordering::SeqCst), 0);
        // Built already: answered at once, with the sources that failed counted as done and reported.
        let failing = Gate {
            failed: 2,
            ..Gate::new(false)
        };
        failing.release();
        let failing = Holder::new(1, failing);
        let waited = wait_for_catalogue(|| failing.current(), || None, FAR, FAR, no_clock);
        assert_eq!((waited.end, waited.failed), (WaitEnd::Built, 2));
        // The store cannot be reached.
        let waited = wait_for_catalogue::<Gate>(|| Err("no store".into()), || None, FAR, FAR, no_clock);
        assert_eq!(waited.end, WaitEnd::Unavailable("no store".into()));
    }

    #[test]
    fn the_wait_gives_up_after_the_catalogue_stood_still_but_not_while_it_moves() {
        let ms = |n: u64| Duration::from_millis(n);
        let slice = ms(2);
        let stall = Duration::from_secs(10 * 60);
        // Every status call is a minute on the clock the test controls.
        let tick = || Arc::new(std::sync::atomic::AtomicU64::new(0));
        let minute = 60_000;
        let at = |t: &Arc<std::sync::atomic::AtomicU64>| ms(t.load(Ordering::SeqCst));
        // A catalogue that never changes: given up after 10 minutes without change.
        let t = tick();
        let stuck = Holder::new(
            1,
            Gate {
                clock: Some((t.clone(), minute)),
                ..Gate::new(true)
            },
        );
        let waited = wait_for_catalogue(|| stuck.current(), || None, slice, stall, || at(&t));
        assert_eq!(waited.end, WaitEnd::Stalled);
        assert_eq!(
            waited.slices, 11,
            "the first answer moved it, ten more minutes passed without a change"
        );
        // One that keeps moving is waited for past the 10 minutes, and ends when built.
        let t = tick();
        let moving = Holder::new(
            1,
            Gate {
                moving: true,
                clock: Some((t.clone(), minute)),
                ..Gate::new(true)
            },
        );
        let calls = std::sync::atomic::AtomicU64::new(0);
        let waited = wait_for_catalogue(
            || moving.current(),
            || {
                if calls.fetch_add(1, Ordering::SeqCst) == 40 {
                    moving.gate().release();
                }
                None
            },
            slice,
            stall,
            || at(&t),
        );
        assert_eq!(waited.end, WaitEnd::Built);
        assert!(
            waited.waited > stall,
            "waited {:?}: longer than the stall time, since the catalogue kept changing",
            waited.waited
        );
    }

    /// The table the user sees comes from the store the window holds. A project opened while a rebuild waits for the old
    /// store's catalogue must not publish the old store's names into the window, and two stores at the same small catalogue
    /// generation are not the same version.
    #[test]
    fn a_rebuild_waiting_for_a_store_the_window_has_left_publishes_nothing_and_the_table_comes_from_the_new_store() {
        let win = WindowNames::open(None);
        let holder = Holder::new(1, Gate::new(true));
        let ctx = JobCtx::detached();
        let report = std::thread::scope(|s| {
            let job = s.spawn(|| rebuild_job(&win, &ctx, || holder.current(), || None, Duration::from_millis(5), FAR));
            while holder.gate().status.load(Ordering::SeqCst) == 0 {
                std::thread::yield_now();
            }
            // The project opens: another store, whose catalogue is built and at the same generation (1).
            holder.replace(2, Gate::built_with(&["newproject.exe"]));
            job.join().unwrap()
        });
        assert_eq!(report.wait, WaitEnd::StoreChanged);
        assert_eq!(report.outcome, RebuildOutcome::Cancelled);
        assert!(!win.names().is_built(), "nothing of the old store was published");
        assert_eq!(win.names().read(|b, _| b.groups().len()), 0);
        // The same generation number, another store: stale, not current.
        let new = holder.gate();
        assert_eq!(win.freshness(StoreId::for_test(2), new.as_ref()), Freshness::Stale);
        // The next rebuild reads the new store.
        let report = rebuild_job(&win, &ctx, || holder.current(), || None, Duration::from_millis(5), FAR);
        assert_eq!(report.outcome, RebuildOutcome::Built(1));
        assert_eq!(win.names().read(|b, _| b.groups().len()), 1);
        assert_eq!(win.freshness(StoreId::for_test(2), new.as_ref()), Freshness::Current);
        let old = Gate::new(true);
        assert_eq!(
            win.freshness(StoreId::for_test(1), &old),
            Freshness::Stale,
            "a table built from store 2 is not current for store 1, whatever the generation"
        );
    }

    /// The store is replaced after the wait ended, while the names are being read: the rebuild stops before it publishes.
    #[test]
    fn a_rebuild_stops_before_publishing_when_the_window_gets_another_store_meanwhile() {
        let win = WindowNames::open(None);
        let holder = Holder::new(1, Gate::built_with(&["oldproject.exe"]));
        let ctx = JobCtx::detached();
        // The store changes when the rebuild asks for the names (the every-name aggregate).
        let first = Mutex::new(true);
        let current = || {
            let now = holder.current();
            if holder.gate().all.load(Ordering::SeqCst) > 0 && std::mem::take(&mut *first.lock().unwrap()) {
                holder.replace(2, Gate::built_with(&["newproject.exe"]));
            }
            now
        };
        let report = rebuild_job(&win, &ctx, current, || None, FAR, FAR);
        assert_eq!(report.wait, WaitEnd::Built);
        assert_eq!(report.outcome, RebuildOutcome::Cancelled, "the old store's names are not published");
        assert!(!win.names().is_built());
    }

    #[test]
    fn a_rebuild_reports_why_it_ended_and_the_sources_it_could_not_catalogue() {
        let win = WindowNames::open(None);
        let ctx = JobCtx::detached();
        let failing = Holder::new(
            1,
            Gate {
                failed: 3,
                names: vec!["graphicbuilder.exe"],
                ..Gate::new(true)
            },
        );
        failing.gate().release();
        let report = rebuild_job(&win, &ctx, || failing.current(), || None, FAR, FAR);
        assert_eq!(report.outcome, RebuildOutcome::Built(1));
        assert_eq!((report.wait.clone(), report.failed_sources, report.slices), (WaitEnd::Built, 3, 1));
        assert!(report.phases.ms(RebuildPhase::Publishing) < 60_000);
        assert!(
            report.phases.done.iter().any(|(p, _)| *p == RebuildPhase::Publishing),
            "every phase it went through is timed"
        );
        report.log(7);
        // A cancelled job, and one whose store could not be reached.
        let report = rebuild_job(&win, &ctx, || failing.current(), || Some(Stop::Cancelled), FAR, FAR);
        assert_eq!(
            (report.outcome.clone(), report.wait.clone()),
            (RebuildOutcome::Cancelled, WaitEnd::Cancelled)
        );
        report.log(8);
        let report = rebuild_job::<Gate>(&win, &ctx, || Err("no store".into()), || None, FAR, FAR);
        assert_eq!(report.outcome, RebuildOutcome::Failed("no store".into()));
        report.log(9);
        let mute = Holder::new(1, Gate::new(false));
        let report = rebuild_job(&win, &ctx, || mute.current(), || None, FAR, FAR);
        assert_eq!(
            (report.outcome.clone(), report.wait.clone()),
            (RebuildOutcome::NotBuilt, WaitEnd::CannotSignal)
        );
        report.log(10);
    }

    #[test]
    fn freshness_is_unknown_when_the_store_cannot_say_and_stale_until_the_table_is_built_from_it() {
        let win = WindowNames::open(None);
        let (id, ctx) = (StoreId::for_test(1), JobCtx::detached());
        let broken = Gate {
            broken: true,
            ..Gate::new(false)
        };
        assert_eq!(win.freshness(id, &broken), Freshness::Unknown, "the table stays as it is");
        assert_eq!(win.freshness(id, &Gate::new(false)), Freshness::Stale, "no table was built yet");
        let built = Gate::built_with(&["graphicbuilder.exe"]);
        assert_eq!(win.run_rebuild(id, &built, &ctx), RebuildOutcome::Built(1));
        assert_eq!(win.freshness(id, &built), Freshness::Current);
        // The next rebuild finds the catalogue not built: the table keeps its version but is no longer current.
        let unbuilt = Gate::new(false);
        assert_eq!(win.run_rebuild(id, &unbuilt, &ctx), RebuildOutcome::NotBuilt);
        assert_eq!(win.freshness(id, &unbuilt), Freshness::Stale);
        // A failed status read ends a rebuild as failed, not as built.
        assert!(matches!(win.run_rebuild(id, &broken, &ctx), RebuildOutcome::Failed(_)));
    }

    /// A trigger that arrives while a rebuild runs queues the next one; if nothing changed since, that one reads nothing.
    #[test]
    fn a_queued_rebuild_reads_nothing_when_the_table_is_already_current() {
        let win = WindowNames::open(None);
        let ctx = JobCtx::detached();
        let holder = Holder::new(1, Gate::built_with(&["graphicbuilder.exe"]));
        let first = rebuild_job(&win, &ctx, || holder.current(), || None, FAR, FAR);
        assert_eq!(first.outcome, RebuildOutcome::Built(1));
        let reads = holder.gate().all.load(Ordering::SeqCst);
        assert_eq!(reads, 1);
        let again = rebuild_job(&win, &ctx, || holder.current(), || None, FAR, FAR);
        assert_eq!(again.outcome, RebuildOutcome::UpToDate);
        assert_eq!(holder.gate().all.load(Ordering::SeqCst), reads, "no name was read");
        assert_eq!(win.rebuild_runs(), 1);
        // The catalogue changed since (its generation moved): rebuilt.
        holder.gate().generation.store(2, Ordering::SeqCst);
        let changed = rebuild_job(&win, &ctx, || holder.current(), || None, FAR, FAR);
        assert_eq!(changed.outcome, RebuildOutcome::Built(1));
        assert_eq!(win.rebuild_runs(), 2);
        // Another store at the same generation: rebuilt too.
        holder.replace(2, Gate::built_with(&["newproject.exe"]));
        assert_eq!(
            rebuild_job(&win, &ctx, || holder.current(), || None, FAR, FAR).outcome,
            RebuildOutcome::Built(1)
        );
    }

    #[test]
    fn phase_times_add_up_the_time_of_each_phase_once() {
        let mut t = PhaseTimes::default();
        t.enter(RebuildPhase::Catalogue);
        t.enter(RebuildPhase::Catalogue);
        t.enter(RebuildPhase::Grouping);
        let t = t.finish();
        let phases: Vec<_> = t.done.iter().map(|(p, _)| *p).collect();
        assert_eq!(phases, [RebuildPhase::Catalogue, RebuildPhase::Grouping]);
        assert_eq!(t.ms(RebuildPhase::Events), 0);
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
