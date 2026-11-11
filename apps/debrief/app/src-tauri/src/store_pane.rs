//! The Store pane' commands (ADR 0030): read-only reports about the window's store (overview, segments, fields,
//! full-text index, memory), its maintenance as confirmed jobs, and the read-only SQL console. Everything is
//! computed by the store (in the store worker); the shell adds what only it knows: the window's running jobs and
//! its notes. The names and payloads are documented in app/src/app/core/api.types.ts.

use crate::engine::with_store;
use crate::jobs::{Controls, Done, JobKind, JobSpec, PhaseScale};
use crate::query_cancel::QueryStreamEvent;
use crate::windows::CurrentWindow;
use crate::{CmdError, CmdResult};
use debrief_core::ports::{
    MaintenanceAction, RunningJob, SegmentsPage, SegmentsRequest, SqlExportRequest, SqlIds, SqlIdsRequest, SqlNote, SqlPage,
    SqlPageRequest, SqlRequest, SqlResult, SqlSchema, StoreError, StoreFields, StoreIndex, StoreMemory, StoreOverview, MAX_SQL_LEN,
    SQL_MAX_NOTES, SQL_NOTE_CHARS,
};
use debrief_core::ql::{QlError, QueryControl};
use debrief_notes::{Anchor, Note};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, Runtime};

#[tauri::command]
pub(crate) async fn store_overview(state: CurrentWindow) -> CmdResult<StoreOverview> {
    with_store(&state, |s| s.store_overview()).await
}

#[tauri::command]
pub(crate) async fn store_segments(state: CurrentWindow, request: SegmentsRequest) -> CmdResult<SegmentsPage> {
    with_store(&state, move |s| s.store_segments(&request)).await
}

#[tauri::command]
pub(crate) async fn store_fields(state: CurrentWindow) -> CmdResult<StoreFields> {
    with_store(&state, |s| s.store_fields()).await
}

#[tauri::command]
pub(crate) async fn store_index(state: CurrentWindow) -> CmdResult<StoreIndex> {
    with_store(&state, |s| s.store_index()).await
}

/// The window's queued and running jobs (and the app-wide ones), as the Memory section lists them.
fn running_jobs<R: Runtime>(app: &AppHandle<R>, window: &str) -> Vec<RunningJob> {
    let Some(jobs) = app.try_state::<crate::jobs::Jobs>() else {
        return Vec::new();
    };
    jobs.find_active(|j| j.window().is_none_or(|w| w == window))
        .into_iter()
        .filter_map(|id| jobs.get(id))
        .map(|j| RunningJob {
            kind: j.kind.as_str().to_string(),
            title: j.title,
        })
        .collect()
}

/// The store's memory against its budgets, with the worker process and the window's running jobs.
#[tauri::command]
pub(crate) async fn store_memory<R: Runtime>(app: AppHandle<R>, state: CurrentWindow) -> CmdResult<StoreMemory> {
    let mut m = with_store(&state, |s| s.store_memory()).await?;
    m.running_jobs = running_jobs(&app, state.label());
    // The budgets the user chose in Settings ▸ Performance (ADR 0031), not the defaults the store knows.
    if let Some(app_state) = app.try_state::<crate::app_state::AppState>() {
        let s = app_state.performance.settings();
        m.import_budget_bytes = u64::from(s.import_budget_mb) << 20;
        m.query_budget_bytes = u64::from(s.query_budget_mb) << 20;
    }
    Ok(m)
}

/// Runs a maintenance action as a job of the window (the UI asks to confirm first) and returns its id at once;
/// the job's message says what it did. Refused (busy) while an import runs.
#[tauri::command]
pub(crate) async fn store_maintenance<R: Runtime>(app: AppHandle<R>, state: CurrentWindow, action: MaintenanceAction) -> CmdResult<u64> {
    start_maintenance(&app, &state, action)
}

/// The percentage of `p` (0 to 100), never below the highest reported so far (`best`); `None` before an estimate.
fn monotonic_percent(best: &std::sync::atomic::AtomicU64, p: debrief_core::ql::QueryProgress) -> Option<u64> {
    (p.total > 0).then(|| {
        let now = p.done.min(p.total).saturating_mul(100) / p.total;
        best.fetch_max(now, std::sync::atomic::Ordering::SeqCst).max(now)
    })
}

/// After an open job: a new ordering episode, and the ordering job when the store's order is incomplete.
pub(crate) fn order_events_after_open<R: Runtime>(app: &AppHandle<R>, state: &CurrentWindow) {
    let _ = ordering_event(app, state, crate::ordering::Event::Opened);
    order_events_if_incomplete(app, state);
}

/// The ordering job when the window's store says its order is incomplete (a live refresh, a worker restart, a failed
/// clock correction), at most once per episode (`crate::ordering`).
pub(crate) fn order_events_if_incomplete<R: Runtime>(app: &AppHandle<R>, state: &CurrentWindow) {
    if let Ok(store) = state.engine.store() {
        order_events_if_needed(app, state, store.as_ref());
    }
}

/// [`order_events_if_incomplete`] with the store given: `Positions::order_complete` (two counts and a flag) decides;
/// the store does not order inside the open call, which has a time limit.
pub(crate) fn order_events_if_needed<R: Runtime>(app: &AppHandle<R>, state: &CurrentWindow, store: &dyn debrief_core::ports::Positions) {
    if matches!(store.order_complete(), Ok(false)) {
        // A start that fails is logged and left to the user (the state machine's LeftToUser).
        let _ = ordering_event(app, state, crate::ordering::Event::Incomplete);
    }
}

/// The cause of a window's message that its events are not in time order (resolved when an ordering succeeds there).
fn ordering_cause(label: &str) -> String {
    format!("ordering:incomplete:{label}")
}

/// Feeds `event` to the window's ordering state machine and does what it says; the id of the ordering job it started,
/// if any. An error: the job could not start, or (the user asked) one runs already.
pub(crate) fn ordering_event<R: Runtime>(
    app: &AppHandle<R>,
    state: &CurrentWindow,
    event: crate::ordering::Event,
) -> CmdResult<Option<u64>> {
    use crate::ordering::{Action, Event, State};
    let action = {
        let mut s = crate::sync::lock(&state.engine.ordering);
        let (next, action) = crate::ordering::step(*s, event);
        *s = next;
        action
    };
    match action {
        Action::Nothing => Ok(None),
        Action::StartJob { auto } => {
            if auto {
                log::warn!("The events' time order is incomplete; ordering them again");
            }
            // Not under the state's lock: the job's end comes back through `ordering_event`.
            match spawn_maintenance(app, state, MaintenanceAction::OrderEvents) {
                Ok(id) => {
                    ordering_event(app, state, Event::Started { job: id })?;
                    Ok(Some(id))
                }
                Err(e) => {
                    log::warn!("the ordering job could not start: {}", e.message);
                    *crate::sync::lock(&state.engine.ordering) = if auto { State::LeftToUser } else { State::Ready };
                    Err(e)
                }
            }
        }
        Action::AlreadyRunning => Err(crate::error::CmdError::invalid(
            "The events are being put in time order already; the Jobs pane shows how far it got.",
        )),
        Action::AskUser { job } => {
            use crate::notification_centre::centre::{NewNotice, NoticeAction, NoticeScope, NoticeSeverity, NoticeSource};
            let notice = NewNotice::new(
                ordering_cause(state.label()),
                NoticeSource::Workspace,
                NoticeSeverity::Warning,
                "The events are not fully in time order",
                NoticeScope::Project,
            )
            .body(
                "Putting them in time order was cancelled or did not finish. New events are listed at the end until it is \
                 done; Debrief does not start it again by itself.",
            )
            .primary(NoticeAction::RetryJob { job_id: job });
            crate::notification_centre::post(app, Some(state.label()), notice, None);
            Ok(None)
        }
        Action::Resolve => {
            crate::notification_centre::resolve_cause(app, &ordering_cause(state.label()));
            Ok(None)
        }
    }
}

/// An ordering job of a window ended (the jobs service's finished event, also for a job cancelled before it started).
pub(crate) fn ordering_job_finished<R: Runtime>(app: &AppHandle<R>, job: &crate::jobs::model::Job) {
    use crate::jobs::JobState;
    use crate::ordering::Outcome;
    if job.kind != JobKind::Maintenance || job.title != MaintenanceAction::OrderEvents.title() {
        return;
    }
    let Some(label) = job.window() else { return };
    let outcome = match job.state() {
        JobState::Done => Outcome::Done,
        JobState::Cancelled => Outcome::Cancelled,
        JobState::Failed | JobState::Interrupted => Outcome::Failed,
        JobState::Queued | JobState::Running => return,
    };
    if let Ok(win) = CurrentWindow::of(app, label) {
        let _ = ordering_event(app, &win, crate::ordering::Event::Finished { outcome, job: job.id.0 });
    }
}

/// Retry of a maintenance job (the message's "Put events in time order", the Jobs pane): the user starts it again.
pub(crate) fn retry_maintenance<R: Runtime>(
    app: &AppHandle<R>,
    job: &crate::jobs::model::Job,
) -> Result<Option<crate::jobs::JobId>, String> {
    let action: MaintenanceAction = job
        .request
        .as_ref()
        .and_then(|r| r.get("action"))
        .and_then(|a| serde_json::from_value(a.clone()).ok())
        .ok_or("this job cannot be run again")?;
    let label = job.window().ok_or("this job has no window")?;
    let win = CurrentWindow::of(app, label).map_err(|e| e.message)?;
    start_maintenance(app, &win, action)
        .map(|id| Some(crate::jobs::JobId(id)))
        .map_err(|e| e.message)
}

/// [`store_maintenance`]: the user starts a maintenance job (for the ordering, the state machine hears so); its id.
pub(crate) fn start_maintenance<R: Runtime>(app: &AppHandle<R>, state: &CurrentWindow, action: MaintenanceAction) -> CmdResult<u64> {
    if action == MaintenanceAction::OrderEvents {
        // Through the state machine: never a second ordering job beside a running one.
        return ordering_event(app, state, crate::ordering::Event::UserStarted)?
            .ok_or_else(|| crate::error::CmdError::invalid("The events are being put in time order already."));
    }
    spawn_maintenance(app, state, action)
}

/// Spawns a maintenance job of the window and returns its id; the ordering can be retried (the message's action).
fn spawn_maintenance<R: Runtime>(app: &AppHandle<R>, state: &CurrentWindow, action: MaintenanceAction) -> CmdResult<u64> {
    let mut spec = JobSpec::new(JobKind::Maintenance, action.title()).window(state.label());
    if action == MaintenanceAction::OrderEvents {
        spec = spec.retry_with(serde_json::json!({ "action": action }));
    }
    let win = state.arc().clone();
    let (controls, control) = crate::jobs::store_cancel(Controls::default());
    // The ordering is one long statement: DuckDB's estimate of it, relayed to the control, is the job's percentage,
    // which never goes back (the estimate restarts with each statement and a retry).
    let controls = if action == MaintenanceAction::OrderEvents {
        let (c, best) = (control.clone(), std::sync::atomic::AtomicU64::new(0));
        controls.with_probe(move || {
            monotonic_percent(&best, c.progress())
                .map(|pct| crate::jobs::JobProgress::in_phase(MaintenanceAction::OrderEvents.phase(), PhaseScale::Count, pct, 100))
        })
    } else {
        controls
    };
    let id = crate::jobs::spawn(app, spec, controls, move |ctx| -> CmdResult<Done<()>> {
        let store = win.engine.store()?;
        ctx.phase(action.phase(), PhaseScale::Indeterminate, 0, 0);
        let report = match control.scope(|| store.maintain(action)) {
            Ok(r) => r,
            // Cancel interrupted the store's statements: nothing was half applied (compaction swaps in one transaction).
            Err(_) if ctx.is_cancelled() => return Ok(Done::new(()).cancelled_if(true)),
            Err(e) => return Err(CmdError::from(e)),
        };
        if action == MaintenanceAction::RebuildIndex {
            // The store returns at once and indexes in the background: the job lasts until the index covers the segments.
            let cancelled = follow_indexing(&*store, ctx);
            let msg = if cancelled {
                "Stopped waiting; the full-text index keeps catching up in the background."
            } else {
                "The full-text index is rebuilt."
            };
            ctx.message(msg);
            log::info!(Action = action.title(); "Store maintenance done");
            return Ok(if cancelled {
                Done::new(()).cancelled_if(true)
            } else {
                Done::new(())
            });
        }
        log::info!(Action = action.title(); "Store maintenance done");
        ctx.message(&report.message);
        Ok(Done::new(()))
    });
    crate::error::lift(id.map(|id| id.0))
}

/// Waits for the full-text index to cover every segment, reporting how many are done; true when Cancel stopped the wait.
fn follow_indexing(store: &dyn debrief_core::ports::Store, ctx: &crate::jobs::JobCtx) -> bool {
    let mut total = 0u64;
    loop {
        if ctx.is_cancelled() {
            return true;
        }
        let Ok(index) = store.store_index() else { return false };
        total = total.max(index.pending_segments);
        if index.pending_segments == 0 || index.error.is_some() {
            return false;
        }
        ctx.phase("Indexing segments", PhaseScale::Count, total - index.pending_segments, total);
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

#[tauri::command]
pub(crate) async fn sql_schema(state: CurrentWindow) -> CmdResult<SqlSchema> {
    with_store(&state, |s| s.sql_schema()).await
}

#[tauri::command]
pub(crate) async fn sql_page(state: CurrentWindow, request: SqlPageRequest) -> CmdResult<SqlPage> {
    with_store(&state, move |s| s.sql_page(&request)).await
}

/// The event ids of a kept result: *Open in Events* filters the grid to them (`@id in (...)`).
#[tauri::command]
pub(crate) async fn sql_ids(state: CurrentWindow, request: SqlIdsRequest) -> CmdResult<SqlIds> {
    with_store(&state, move |s| s.sql_ids(&request)).await
}

/// The window's notes as `debrief.notes` sees them.
fn window_notes(state: &CurrentWindow) -> Vec<SqlNote> {
    state.notes.snapshot().notes().iter().take(SQL_MAX_NOTES).map(sql_note).collect()
}

/// Writes all rows of a console query as CSV to a file the user chose: an export job (Cancel stops it, the job's
/// message says how many rows were written and whether the limits cut it). Returns the job's id at once.
#[tauri::command]
pub(crate) async fn sql_export<R: Runtime>(
    app: AppHandle<R>,
    state: CurrentWindow,
    sql: String,
    path: String,
    overwrite: bool,
) -> CmdResult<u64> {
    if sql.len() > MAX_SQL_LEN {
        return Err(CmdError::from(format!("The SQL text is longer than {} KB.", MAX_SQL_LEN >> 10)));
    }
    let target = crate::paths::WindowGrants::of(&app, state.label())
        .write(&path, &[])?
        .path()
        .to_path_buf();
    let request = SqlExportRequest {
        sql,
        notes: window_notes(&state),
        path: target.to_string_lossy().into_owned(),
        overwrite,
    };
    // The same export already running serves this ask (before the overwrite question), another export of the file is refused.
    let spec = crate::jobs::export_spec_for(&state, "Export SQL result to", &target, &request);
    if let Some(id) = crate::jobs::preflight(&app, &spec)? {
        return Ok(id);
    }
    // Checked now, so the dialog can ask to replace the file; the store checks again when it writes.
    debrief_core::overwrite::check_overwrite(&target, overwrite)?;
    crate::jobs::export(&app, &state, spec, &target, move |store| {
        let r = store.sql_export(&request).map_err(CmdError::from)?;
        log::info!(Rows = r.rows, Bytes = r.bytes, Truncated = r.truncated; "SQL result exported");
        Ok(exported_rows(r.rows, r.truncated))
    })
}

/// The line the export job's message shows.
fn exported_rows(rows: u64, truncated: bool) -> String {
    match (rows, truncated) {
        (1, false) => "Exported 1 row.".into(),
        (_, false) => format!("Exported {rows} rows."),
        (_, true) => format!("Exported the first {rows} rows (the export's limit); narrow the query for the rest."),
    }
}

/// A console run's outcome as the UI expects it (`SqlOutcome`): `{ result }`, `{ error }` (the span to underline)
/// or `{ cancelled: true }`; a failure of the store (or a limit) stays an error of the command.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<SqlResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<QlError>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cancelled: bool,
}

fn outcome(out: Result<SqlResult, StoreError>) -> CmdResult<SqlOutcome> {
    let none = SqlOutcome {
        result: None,
        error: None,
        cancelled: false,
    };
    match out {
        Ok(result) => Ok(SqlOutcome {
            result: Some(result),
            ..none
        }),
        Err(StoreError::Query(error)) => Ok(SqlOutcome {
            error: Some(error),
            ..none
        }),
        Err(StoreError::Cancelled) => Ok(SqlOutcome { cancelled: true, ..none }),
        Err(e) => Err(e.into()),
    }
}

/// A note as `debrief.notes` sees it (text cut to [`SQL_NOTE_CHARS`]).
fn sql_note(n: &Note) -> SqlNote {
    let mut out = SqlNote {
        id: n.id.clone(),
        severity: serde_json::to_value(n.severity)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default(),
        tags: n.tags.clone(),
        text: n.text.chars().take(SQL_NOTE_CHARS).collect(),
        author: n.author.clone(),
        created: n.created_ms,
        updated: n.updated_ms,
        ..SqlNote::default()
    };
    let (anchor, from, to) = match &n.anchor {
        Anchor::Event { event_id, ts, .. } => {
            out.event_id = Some(*event_id);
            ("event", *ts, None)
        }
        Anchor::Instant { at } => ("instant", Some(*at), None),
        Anchor::Range { from, to } => ("range", Some(*from), Some(*to)),
        Anchor::Source { path } => {
            out.source_path = Some(path.clone());
            ("source", None, None)
        }
        Anchor::Marker { from, to, .. } => ("marker", Some(*from), *to),
        Anchor::ChartPoint { at, .. } => ("chartPoint", Some(*at), None),
        Anchor::Project => ("project", None, None),
        Anchor::Image { .. } => ("image", None, None),
    };
    out.anchor = anchor.to_string();
    out.from = from;
    out.to = to;
    out
}

/// Runs a console query as an analysis job (Cancel in the Jobs pane, or `jobs_cancel` with the id sent first on
/// `on_event`, interrupts it in the store). The window's notes go with it (`debrief.notes`).
#[tauri::command]
pub(crate) async fn sql_run<R: Runtime>(
    app: AppHandle<R>,
    state: CurrentWindow,
    sql: String,
    explain: bool,
    on_event: Channel<QueryStreamEvent>,
) -> CmdResult<SqlOutcome> {
    // The store checks the text again; this keeps an oversized request from going further.
    if sql.len() > MAX_SQL_LEN {
        return Ok(SqlOutcome {
            error: Some(QlError::new(format!("The SQL text is longer than {} KB.", MAX_SQL_LEN >> 10), 0, 0)),
            result: None,
            cancelled: false,
        });
    }
    let notes = window_notes(&state);
    let request = SqlRequest { sql, explain, notes };
    let control = QueryControl::new();
    let stop = control.clone();
    let controls = Controls::cancel(move || stop.cancel());
    let started = on_event.clone();
    let announce = move |id: crate::jobs::JobId| {
        let _ = started.send(QueryStreamEvent::Started { job_id: id.0 });
    };
    let out = crate::jobs::analysis_announced(&app, &state, "SQL query", "store", None, controls, announce, move |store, _| {
        Ok(store.sql_run(&request))
    })
    .await?;
    outcome(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Json as _;
    use crate::test_support::Now as _;
    use crate::test_support::{block, main_window, samples, test_app};
    use serde_json::Value;
    use std::sync::{Arc, Mutex};
    use tauri::ipc::InvokeResponseBody;

    fn opened() -> tauri::App<tauri::test::MockRuntime> {
        let app = test_app(None);
        main_window(&app).queue(crate::instance::existing(&[samples().join("serilog-clef")]));
        let pending = crate::workspace_commands::take_pending_open(main_window(&app), crate::paths::WindowGrants::of(app.handle(), "main"))
            .now()
            .unwrap();
        crate::test_support::finished(
            &app,
            block(crate::workspace_commands::open_paths(
                app.handle().clone(),
                main_window(&app),
                pending,
            )),
        )
        .unwrap()
        .json();
        app
    }

    fn recorder() -> (Channel<QueryStreamEvent>, Arc<Mutex<Vec<Value>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(text) = body {
                sink.lock().unwrap().push(serde_json::from_str(&text).unwrap());
            }
            Ok(())
        });
        (channel, seen)
    }

    /// Positions whose order is incomplete (an ordering failed).
    struct Incomplete;

    impl debrief_core::ports::Positions for Incomplete {
        fn ids_at(&self, _from: u64, _len: usize) -> debrief_core::ports::StoreResult<Vec<debrief_core::batch::EventId>> {
            Ok(Vec::new())
        }
        fn position_of(&self, _id: debrief_core::batch::EventId) -> debrief_core::ports::StoreResult<Option<u64>> {
            Ok(None)
        }
        fn positioned(&self) -> debrief_core::ports::StoreResult<u64> {
            Ok(0)
        }
        fn order_complete(&self) -> debrief_core::ports::StoreResult<bool> {
            Ok(false)
        }
    }

    #[test]
    fn the_ordering_jobs_percentage_never_goes_back() {
        use debrief_core::ql::{QueryProgress, QueryStage};
        let p = |done, total| QueryProgress {
            stage: QueryStage::Evaluating,
            done,
            total,
        };
        let best = std::sync::atomic::AtomicU64::new(0);
        assert_eq!(monotonic_percent(&best, p(0, 0)), None, "no estimate yet");
        assert_eq!(monotonic_percent(&best, p(50, 100)), Some(50));
        assert_eq!(
            monotonic_percent(&best, p(10, 200)),
            Some(50),
            "the next statement starts its estimate again"
        );
        assert_eq!(monotonic_percent(&best, p(180, 200)), Some(90));
        assert_eq!(monotonic_percent(&best, p(300, 200)), Some(100), "never above 100");
    }

    #[test]
    fn an_ordering_job_cancelled_during_live_follow_is_not_restarted_by_the_next_refresh() {
        use tauri::Manager;
        let app = opened();
        let jobs = app.state::<crate::jobs::Jobs>().inner().clone();
        let active = || jobs.find_active(|j| j.title == MaintenanceAction::OrderEvents.title());
        jobs.hold_starts(true);
        order_events_if_needed(app.handle(), &main_window(&app), &Incomplete);
        let started = active();
        assert_eq!(started.len(), 1, "the incomplete order starts the job");
        // The user cancels it.
        jobs.cancel(started[0]).unwrap();
        crate::jobs::wait_until("the cancelled ordering job to end", || active().is_empty());
        // The next live refresh still finds the order incomplete: the job is left to the user, not started again.
        order_events_if_needed(app.handle(), &main_window(&app), &Incomplete);
        order_events_if_needed(app.handle(), &main_window(&app), &Incomplete);
        assert!(active().is_empty(), "not restarted after the user's Cancel");
        jobs.hold_starts(false);
    }

    #[test]
    fn a_failed_ordering_is_left_to_the_user_with_a_message_resolved_when_an_ordering_succeeds() {
        use crate::jobs::model::Job;
        use crate::jobs::{JobId, JobState};
        use tauri::Manager;
        let app = opened();
        let jobs = app.state::<crate::jobs::Jobs>().inner().clone();
        let active = || jobs.find_active(|j| j.title == MaintenanceAction::OrderEvents.title());
        let message = || {
            app.state::<crate::notification_centre::NotificationCentre>()
                .snapshot("main")
                .notices
                .into_iter()
                .find(|n| n.cause == ordering_cause("main"))
        };
        let ended = |id: JobId, state| {
            let spec = JobSpec::new(JobKind::Maintenance, MaintenanceAction::OrderEvents.title()).window("main");
            let mut job = Job::finished_for_test(spec, state);
            job.id = id;
            job
        };
        jobs.hold_starts(true);
        order_events_if_needed(app.handle(), &main_window(&app), &Incomplete);
        let started = active();
        assert_eq!(started.len(), 1);
        // Another job's end is not this one's.
        ordering_job_finished(app.handle(), &ended(JobId(started[0].0 + 1000), JobState::Failed));
        assert!(message().is_none(), "another job's failure");
        // The ordering ran out of memory.
        ordering_job_finished(app.handle(), &ended(started[0], JobState::Failed));
        let m = message().expect("the user is told");
        assert!(
            matches!(m.primary, Some(crate::notification_centre::centre::NoticeAction::RetryJob { .. })),
            "{m:?}"
        );
        // Later refreshes and checks do not start it again (it would fail the same way).
        jobs.cancel(started[0]).unwrap();
        crate::jobs::wait_until("the queued ordering job to end", || active().is_empty());
        order_events_if_needed(app.handle(), &main_window(&app), &Incomplete);
        assert!(active().is_empty(), "not restarted after a failure");
        // The user runs it (the message's action, the Store pane), it succeeds: the message goes.
        let again = start_maintenance(app.handle(), &main_window(&app), MaintenanceAction::OrderEvents).unwrap();
        ordering_job_finished(app.handle(), &ended(JobId(again), JobState::Done));
        assert!(message().is_none(), "resolved once the events are in time order");
        jobs.cancel(JobId(again)).unwrap();
        jobs.hold_starts(false);
    }

    #[test]
    fn the_store_pane_never_starts_a_second_ordering_job() {
        use tauri::Manager;
        let app = opened();
        let jobs = app.state::<crate::jobs::Jobs>().inner().clone();
        let active = || jobs.find_active(|j| j.title == MaintenanceAction::OrderEvents.title());
        jobs.hold_starts(true);
        order_events_if_needed(app.handle(), &main_window(&app), &Incomplete);
        assert_eq!(active().len(), 1);
        let e = start_maintenance(app.handle(), &main_window(&app), MaintenanceAction::OrderEvents).unwrap_err();
        assert!(e.message.contains("already"), "{}", e.message);
        assert_eq!(active().len(), 1, "nothing was spawned");
        for id in active() {
            jobs.cancel(id).unwrap();
        }
        jobs.hold_starts(false);
    }

    #[test]
    fn the_ordering_message_is_per_window() {
        assert_ne!(ordering_cause("main"), ordering_cause("w-2"));
    }

    #[test]
    fn an_incomplete_order_starts_exactly_one_ordering_job() {
        use tauri::Manager;
        let app = opened();
        let jobs = app.state::<crate::jobs::Jobs>().inner().clone();
        let ordering = || jobs.find_active(|j| j.title == MaintenanceAction::OrderEvents.title()).len();
        // The open was complete: no ordering job.
        assert_eq!(ordering(), 0);
        // Held, so the job cannot finish between the two checks.
        jobs.hold_starts(true);
        order_events_if_needed(app.handle(), &main_window(&app), &Incomplete);
        order_events_if_needed(app.handle(), &main_window(&app), &Incomplete);
        assert_eq!(ordering(), 1, "one job, not one per check");
        jobs.hold_starts(false);
    }

    #[test]
    fn the_reports_and_the_console_answer_for_the_window() {
        let app = opened();
        let o = block(store_overview(main_window(&app))).unwrap();
        assert!(o.events > 0 && o.segments > 0);
        let page = block(store_segments(main_window(&app), SegmentsRequest::default())).unwrap();
        assert_eq!(page.total, o.segments);
        assert!(!block(store_fields(main_window(&app))).unwrap().fields.is_empty());
        assert_eq!(block(store_index(main_window(&app))).unwrap().events, o.events);
        let m = block(store_memory(app.handle().clone(), main_window(&app))).unwrap();
        assert!(m.query_budget_bytes > 0);
        assert!(block(sql_schema(main_window(&app))).unwrap().views.len() == 4);

        let (channel, seen) = recorder();
        let out = block(sql_run(
            app.handle().clone(),
            main_window(&app),
            "SELECT count(*) AS n FROM events".into(),
            false,
            channel,
        ))
        .unwrap();
        let result = out.result.expect("rows");
        assert_eq!(result.rows[0][0], o.events);
        let events = seen.lock().unwrap().clone();
        assert_eq!(events[0]["kind"], "started", "the run is a job: {events:?}");
        let more = block(sql_page(
            main_window(&app),
            SqlPageRequest {
                result: result.result,
                offset: 0,
            },
        ))
        .unwrap();
        assert_eq!(more.rows.len(), 1);

        let (channel, _) = recorder();
        let refused = block(sql_run(
            app.handle().clone(),
            main_window(&app),
            "DROP VIEW events".into(),
            false,
            channel,
        ))
        .unwrap();
        let e = refused.error.clone().expect("a refusal");
        assert!(e.message.contains("DROP"), "{e:?}");
        let (channel, _) = recorder();
        let long = block(sql_run(
            app.handle().clone(),
            main_window(&app),
            "x".repeat(MAX_SQL_LEN + 1),
            false,
            channel,
        ))
        .unwrap();
        assert!(long.error.unwrap().message.contains("longer"));
        let wire = serde_json::to_value(&refused).unwrap();
        assert!(wire.get("result").is_none() && wire.get("cancelled").is_none());
    }

    #[test]
    fn outcomes_and_notes_have_their_wire_shape() {
        assert!(outcome(Err(StoreError::Cancelled)).unwrap().cancelled);
        assert!(outcome(Err(StoreError::Busy("x".into()))).is_err());
        let note = |anchor: Anchor| Note {
            id: "n".into(),
            anchor,
            text: "é".repeat(SQL_NOTE_CHARS + 10),
            tags: vec!["t".into()],
            severity: debrief_notes::NoteSeverity::Problem,
            author: "me".into(),
            created_ms: 1,
            updated_ms: 2,
        };
        let e = sql_note(&note(Anchor::Event {
            event_id: 7,
            ts: Some(9),
            source: String::new(),
            excerpt: String::new(),
        }));
        assert_eq!(
            (e.anchor.as_str(), e.event_id, e.from, e.severity.as_str()),
            ("event", Some(7), Some(9), "problem")
        );
        assert_eq!(e.text.chars().count(), SQL_NOTE_CHARS);
        let s = sql_note(&note(Anchor::Source { path: "/x/a.log".into() }));
        assert_eq!((s.anchor.as_str(), s.source_path.as_deref()), ("source", Some("/x/a.log")));
        let r = sql_note(&note(Anchor::Range { from: 1, to: 5 }));
        assert_eq!((r.from, r.to), (Some(1), Some(5)));
        assert_eq!(sql_note(&note(Anchor::Instant { at: 3 })).from, Some(3));
        assert_eq!(sql_note(&note(Anchor::Project)).anchor, "project");
        let m = sql_note(&note(Anchor::Marker {
            name: "m".into(),
            from: 1,
            to: Some(2),
        }));
        assert_eq!((m.anchor.as_str(), m.to), ("marker", Some(2)));
        let c = sql_note(&note(Anchor::ChartPoint {
            chart: "c".into(),
            series: None,
            at: 4,
            value: None,
        }));
        assert_eq!((c.anchor.as_str(), c.from), ("chartPoint", Some(4)));
    }

    #[test]
    fn a_sql_export_asked_again_joins_the_running_one_even_though_its_file_exists() {
        let dir = crate::test_support::TempDir::new("sql-export-exists");
        let app = {
            let app = test_app(Some(dir.0.clone()));
            main_window(&app).queue(crate::instance::existing(&[samples().join("serilog-clef")]));
            app
        };
        let st = || main_window(&app);
        crate::paths::allow_under(app.handle(), "main", &dir.0);
        let jobs = app.state::<crate::jobs::Jobs>().inner().clone();
        let out = dir.0.join("rows.csv").to_string_lossy().into_owned();
        let ask = |sql: &str, overwrite: bool| block(sql_export(app.handle().clone(), st(), sql.into(), out.clone(), overwrite));
        jobs.hold_starts(true);
        let first = ask("SELECT 1", false).unwrap();
        std::fs::write(&out, "x").unwrap();
        assert_eq!(
            ask("SELECT 1", false).unwrap(),
            first,
            "the file's existence stopped a repeated export"
        );
        assert_eq!(
            ask("SELECT 1", true).unwrap(),
            first,
            "confirming the replacement started a second writer"
        );
        // Another query into the same file is refused while this one writes it.
        let refused = ask("SELECT 2", true).unwrap_err();
        assert!(
            refused.message.contains("Another export is already writing this file"),
            "{}",
            refused.message
        );
        jobs.hold_starts(false);
        crate::test_support::job_end(&app, first);
    }
}
