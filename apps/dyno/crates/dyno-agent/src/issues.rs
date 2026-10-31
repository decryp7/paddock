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
use paddock_jobs::{JobContext, JobError, Phase};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};

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

/// The mark of a read in progress: dropping it brings the flag down and wakes the reads that wait for it.
struct RunningRead<'a> {
    state: &'a Mutex<State>,
    ended: &'a Condvar,
}

impl Drop for RunningRead<'_> {
    fn drop(&mut self) {
        lock(self.state).scanning = false;
        self.ended.notify_all();
    }
}

/// The sources, how they were last read, and the schedule.
pub struct IssueCentre {
    sources: Mutex<Vec<Box<dyn IssueSource>>>,
    state: Arc<Mutex<State>>,
    /// Signalled when a read ends (a read the person asked for waits for the scheduled one on it) and when a waiter is cancelled.
    ended: Arc<Condvar>,
}

impl IssueCentre {
    /// A centre over `sources`.
    pub fn new(sources: Vec<Box<dyn IssueSource>>) -> IssueCentre {
        let statuses = sources.iter().map(|s| SourceStatus::pending(s.id())).collect();
        IssueCentre {
            sources: Mutex::new(sources),
            state: Arc::new(Mutex::new(State {
                statuses,
                last_scan: None,
                scanning: false,
            })),
            ended: Arc::new(Condvar::new()),
        }
    }

    /// Whether the sources have never been read.
    pub fn never_scanned(&self) -> bool {
        lock(&self.state).last_scan.is_none()
    }

    /// Reads every source now, stores what is new and raises alarms. With a `job`, reports each source as a phase with a growing
    /// count and stops when it is cancelled: what was found is then dropped, the sources' last statuses and the time of the last
    /// read stay as they were, and nothing is stored. A read that is already running (the scheduled one) is waited for, not
    /// repeated and not answered with the old state: this returns `false` once that read has ended, `true` after its own read.
    /// Without a job and with a read running it returns `false` at once.
    pub fn scan(&self, shared: &Shared, job: Option<&JobContext>) -> Result<bool, JobError> {
        self.scan_with(shared, job, &|| {})
    }

    /// [`scan`](Self::scan) with `between` run after the first look at whether a read is running and before the read is claimed:
    /// the moment a scheduled read can slip in (tests start one there).
    #[doc(hidden)]
    pub fn scan_with(&self, shared: &Shared, job: Option<&JobContext>, between: &dyn Fn()) -> Result<bool, JobError> {
        self.scan_hooked(shared, job, between, &|| {})
    }

    /// [`scan_with`](Self::scan_with) and `after_read`, run once the sources have been read and before anything is stored: the
    /// moment a read has its results but a store does not yet (tests hold the read there).
    #[doc(hidden)]
    pub fn scan_hooked(
        &self,
        shared: &Shared,
        job: Option<&JobContext>,
        between: &dyn Fn(),
        after_read: &dyn Fn(),
    ) -> Result<bool, JobError> {
        let mut first = true;
        let (since, running) = loop {
            // A read is already running: wait for it (a job) or leave it (the schedule).
            if let Some(j) = job {
                if lock(&self.state).scanning {
                    j.phase(Phase::discovering("waiting", "Waiting for the system log read that is running"));
                    // The waker goes in BEFORE the lock is taken: a token that is already cancelled runs it on the spot, and it
                    // takes this same lock. Cancel wakes the wait and so does the end of the running read; no polling.
                    let (state, ended) = (self.state.clone(), self.ended.clone());
                    let _wake = j.cancel_token().on_cancel(move || {
                        let _held = lock(&state);
                        ended.notify_all();
                    });
                    let mut st = lock(&self.state);
                    while st.scanning {
                        j.check()?;
                        st = self.ended.wait(st).unwrap_or_else(std::sync::PoisonError::into_inner);
                    }
                    j.check()?;
                    return Ok(false);
                }
            }
            if std::mem::take(&mut first) {
                between();
            }
            let mut st = lock(&self.state);
            if st.scanning {
                if job.is_some() {
                    // A scheduled read started since the look above: wait for it too, like the check above does.
                    drop(st);
                    continue;
                }
                return Ok(false);
            }
            st.scanning = true;
            // The guard is made in the same critical section that sets the flag, so there is no moment in which the flag is up and
            // nothing will bring it down. It only holds references; it takes the lock when it is dropped, after `st` is gone.
            let running = RunningRead {
                state: &self.state,
                ended: &self.ended,
            };
            let t = now();
            break (st.last_scan.map_or(t.minus(FIRST_DAYS * DAY), |l| l.minus(OVERLAP_MS)), running);
        };
        let until = now();
        // The read stays "running" until its results are stored: the flag comes down (and the waiters are woken) only when `running`
        // is dropped, and on every way out, a cancel and a panic included. A waiter that left earlier answered from a store that did not
        // have them yet.
        let (statuses, found) = self.read_all(job, since, until)?;
        after_read();
        let found = collapse_across_sources(found);
        record(shared, &found, until);
        let mut st = lock(&self.state);
        st.statuses = statuses;
        st.last_scan = Some(until);
        // `st` first: dropping `running` takes this same lock to bring the flag down, and the lock is not re-entrant.
        drop(st);
        drop(running);
        Ok(true)
    }

    /// Reads the sources one after the other into a result of its own: nothing is shared until the caller keeps it.
    fn read_all(&self, job: Option<&JobContext>, since: Timestamp, until: Timestamp) -> Result<(Vec<SourceStatus>, Vec<Issue>), JobError> {
        let sources = lock(&self.sources);
        let mut statuses = Vec::new();
        let mut found: Vec<Issue> = Vec::new();
        for source in sources.iter() {
            if let Some(j) = job {
                j.check()?;
            }
            let id = source.id();
            if let Some(j) = job {
                j.phase(Phase::discovering(
                    &format!("source-{id:?}"),
                    &format!("Reading {}", id.label().to_lowercase()),
                ));
            }
            // A source that streams its tool reports a growing count and stops at Cancel; one that does not is a single step
            // that cannot be interrupted and says so. One misbehaving reader must not take the others (or the agent) down.
            let controllable = source.controllable();
            let _step = job
                .filter(|_| !controllable)
                .map(|j| j.uncancellable("A system log is being read; Cancel takes effect when it has been read."));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match job {
                Some(j) if controllable => source.read_controlled(since, until, &crate::jobs::JobControl(j)),
                _ => source.read(since, until),
            }))
            .unwrap_or_else(|_| {
                Err(dyno_core::ports::PlatformError::Failed(
                    "This log reader stopped unexpectedly.".into(),
                ))
            });
            drop(_step);
            if let Some(j) = job {
                // A cancelled read drops what the tool had given so far.
                j.check()?;
                if !controllable {
                    j.set(result.as_ref().map_or(0, |l| l.len() as u64));
                }
            }
            statuses.push(SourceStatus::after(id, until, &result));
            if let Ok(list) = result {
                found.extend(list);
            }
        }
        Ok((statuses, found))
    }

    pub(crate) fn snapshot(&self) -> (Vec<SourceStatus>, Option<Timestamp>) {
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
    Ok(build_overview(t, q.days, core.zone.as_ref(), &issues, sources, scanned_at))
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
        // Reading the OS logs is heavier work: only inside the maintenance windows, when the user set any.
        if crate::extras::maintenance_open(shared) {
            // Nobody can cancel the scheduled read, so it cannot end early.
            let _ = shared.issues.scan(shared, None);
            wait(SCAN_EVERY);
        } else {
            wait(std::time::Duration::from_secs(5 * 60));
        }
    }
}
