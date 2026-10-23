//! The agent's issue centre: reads the OS problem logs (`IssueSource`s from the platform) off the sampling thread, keeps
//! what they say in the store, raises alarms for the serious ones, feeds crash loops to the alerts, and answers the
//! Issues page. Observe-only: nothing here changes the logs or the machine. A source that cannot be read is an
//! "unavailable: reason" entry, never an error for the page.

use crate::agent::{lock, now, Shared};
use dyno_core::alarms::feed::{alarms_on, from_issue};
use dyno_core::issues::{
    build_overview, collapse_across_sources, crash_loops, to_jsonl, validate_export_path, Issue, IssueSource, IssuesOverview, IssuesQuery,
    SavedIssues, SourceStatus, MAX_DAYS,
};
use dyno_core::protocol::{AgentError, ErrorCode};
use dyno_core::time::{Timestamp, DAY, HOUR};
use std::path::Path;
use std::sync::Mutex;

/// How often the sources are read in the background.
pub const SCAN_EVERY: std::time::Duration = std::time::Duration::from_secs(15 * 60);
/// The first read waits this long after the agent starts, so start-up is not slowed down.
pub const FIRST_SCAN_AFTER: std::time::Duration = std::time::Duration::from_secs(5);
/// How far back the first read goes; later reads go back to the previous read.
const FIRST_DAYS: i64 = 7;
/// Later reads start this far before the previous one (events are stored once, so overlap is free and covers log lag).
const OVERLAP_MS: i64 = 30 * 60_000;
/// Serious issues older than this raise no alarm (a first read of a week of history must not announce last Tuesday).
const ALARM_WITHIN_MS: i64 = DAY;
/// Most rows read from the store for a page.
const PAGE_ROWS: u32 = 20_000;

struct State {
    statuses: Vec<SourceStatus>,
    last_scan: Option<Timestamp>,
    scanning: bool,
}

/// The sources, how they were last read, and the schedule.
pub struct IssueCentre {
    sources: Mutex<Vec<Box<dyn IssueSource>>>,
    state: Mutex<State>,
}

impl IssueCentre {
    /// A centre over `sources`.
    pub fn new(sources: Vec<Box<dyn IssueSource>>) -> IssueCentre {
        let statuses = sources.iter().map(|s| SourceStatus::pending(s.id())).collect();
        IssueCentre {
            sources: Mutex::new(sources),
            state: Mutex::new(State {
                statuses,
                last_scan: None,
                scanning: false,
            }),
        }
    }

    /// Whether the sources have never been read.
    pub fn never_scanned(&self) -> bool {
        lock(&self.state).last_scan.is_none()
    }

    /// Reads every source now, stores what is new and raises alarms. Returns false when another read is already running.
    pub fn scan(&self, shared: &Shared) -> bool {
        let since = {
            let mut st = lock(&self.state);
            if st.scanning {
                return false;
            }
            st.scanning = true;
            let t = now();
            st.last_scan.map_or(t.minus(FIRST_DAYS * DAY), |l| l.minus(OVERLAP_MS))
        };
        let until = now();
        let mut statuses = Vec::new();
        let mut found: Vec<Issue> = Vec::new();
        for source in lock(&self.sources).iter() {
            let id = source.id();
            // One misbehaving reader must not take the others (or the agent) down.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| source.read(since, until))).unwrap_or_else(|_| {
                Err(dyno_core::ports::PlatformError::Failed(
                    "This log reader stopped unexpectedly.".into(),
                ))
            });
            statuses.push(SourceStatus::after(id, until, &result));
            if let Ok(list) = result {
                found.extend(list);
            }
        }
        let found = collapse_across_sources(found);
        record(shared, &found, until);
        let mut st = lock(&self.state);
        st.statuses = statuses;
        st.last_scan = Some(until);
        st.scanning = false;
        true
    }

    fn snapshot(&self) -> (Vec<SourceStatus>, Option<Timestamp>) {
        let st = lock(&self.state);
        (st.statuses.clone(), st.last_scan)
    }
}

/// Stores `found`, raises alarms for the serious new ones and refreshes the crash loops the alerts watch.
fn record(shared: &Shared, found: &[Issue], at: Timestamp) {
    let mut core = lock(&shared.core);
    let added = match core.store.put_issues(found) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("dyno-agent: could not store issues: {e}");
            Vec::new()
        }
    };
    for i in added.iter().filter(|i| alarms_on(i.kind) && at.since(i.at) <= ALARM_WITHIN_MS) {
        let core = &mut *core;
        let changes = core.alarms.observe(&core.store, from_issue(i), i.at);
        crate::sampler::push_alarm_changes(shared, changes);
    }
    let recent = core.store.issues(at.minus(2 * HOUR), at, 2_000).unwrap_or_default();
    core.crash_loops = crash_loops(&recent, at);
}

/// The Issues page for `query`.
pub fn overview(shared: &Shared, query: IssuesQuery) -> Result<IssuesOverview, AgentError> {
    let q = query.normalised();
    let t = now();
    let (sources, scanned_at) = shared.issues.snapshot();
    let core = lock(&shared.core);
    let from = t.minus(i64::from(q.days) * 2 * DAY + DAY);
    let issues = core
        .store
        .issues(from, t, PAGE_ROWS)
        .map_err(|e| AgentError::new(ErrorCode::Internal, e.to_string()))?;
    Ok(build_overview(
        t,
        q.days,
        core.settings.alarms.utc_offset_minutes,
        &issues,
        sources,
        scanned_at,
    ))
}

/// Writes the issues of the last `days` days as JSON lines to `path` (chosen by the user in the OS save dialog).
pub fn save(shared: &Shared, path: &str, days: u32) -> Result<SavedIssues, AgentError> {
    let dest = validate_export_path(path).map_err(|m| AgentError::new(ErrorCode::BadRequest, m))?;
    if !dest.parent().is_some_and(Path::is_dir) {
        return Err(AgentError::new(
            ErrorCode::BadRequest,
            "The folder to save the issues in does not exist.",
        ));
    }
    let days = days.clamp(1, MAX_DAYS);
    let t = now();
    let issues = lock(&shared.core)
        .store
        .issues(t.minus(i64::from(days) * DAY), t, PAGE_ROWS)
        .map_err(|e| AgentError::new(ErrorCode::Internal, e.to_string()))?;
    let text = to_jsonl(&issues);
    std::fs::write(&dest, text.as_bytes()).map_err(|e| {
        let code = if e.kind() == std::io::ErrorKind::PermissionDenied {
            ErrorCode::AccessDenied
        } else {
            ErrorCode::Internal
        };
        AgentError::new(code, format!("Dyno could not write the issues ({e}). Choose another folder."))
    })?;
    Ok(SavedIssues {
        path: dest.to_string_lossy().into_owned(),
        bytes: text.len() as u64,
        count: issues.len() as u32,
    })
}

/// The background reader: waits a little, reads, then reads again every [`SCAN_EVERY`] until the agent stops.
pub fn run(shared: &Shared) {
    let wait = |d: std::time::Duration| {
        let mut waited = std::time::Duration::ZERO;
        while waited < d && !shared.stopped() {
            std::thread::sleep(std::time::Duration::from_millis(250));
            waited += std::time::Duration::from_millis(250);
        }
    };
    wait(FIRST_SCAN_AFTER);
    while !shared.stopped() {
        shared.issues.scan(shared);
        wait(SCAN_EVERY);
    }
}
