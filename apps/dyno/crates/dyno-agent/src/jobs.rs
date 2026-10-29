//! Long operations as jobs (ER-17, ER-18, ER-19), on the family's `paddock-jobs`: a request that starts one answers at
//! once, the work runs on a small worker pool, each job reports phases with their own counts, Cancel is checked while it
//! works, and the app polls [`Request::Jobs`](dyno_core::protocol::Request::Jobs) the way it polls the benchmarks. A job
//! builds its result on the side and hands it over only when it ends well, so a cancelled or failed job changes nothing.
//! Observe-only like everything else here: no job changes the machine.

use crate::agent::{lock, now, Shared};
use dyno_core::jobs::{JobKind, JobOutput, JobPhase, JobSpec, JobState, JobStatus, JobUnit, KEPT_FINISHED};
use dyno_core::protocol::{AgentError, ErrorCode};
use paddock_jobs::{EventSink, JobContext, JobError, JobId, JobPool, JobRegistry, JobSnapshot, JobState as RunState, Unit, Workers};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// Jobs that run at once; the rest wait their turn as `queued`.
const WORKERS: usize = 4;

#[derive(Default)]
struct Book {
    /// Jobs that have not ended, with what they were asked to do (a second start of the same work answers the first).
    active: Vec<(u64, JobSpec)>,
    /// The last jobs that ended, oldest first.
    finished: VecDeque<JobStatus>,
    /// What the done ones among them produced.
    outputs: HashMap<u64, JobOutput>,
}

/// Remembers how jobs end. Called by the reporter with its lock held, so it only touches the book.
struct Journal(Arc<Mutex<Book>>);

impl EventSink for Journal {
    fn emit(&self, snapshot: &JobSnapshot) {
        if !snapshot.state.is_terminal() {
            return;
        }
        let mut book = lock(&self.0);
        book.active.retain(|(id, _)| *id != snapshot.id.0);
        book.finished.push_back(status_of(snapshot));
        while book.finished.len() > KEPT_FINISHED {
            if let Some(old) = book.finished.pop_front() {
                book.outputs.remove(&old.id);
            }
        }
    }
}

/// The running and the recently finished jobs.
pub struct JobCentre {
    book: Arc<Mutex<Book>>,
    registry: JobRegistry,
    pool: JobPool,
}

impl Default for JobCentre {
    fn default() -> Self {
        JobCentre::new()
    }
}

impl JobCentre {
    /// A centre with no jobs.
    pub fn new() -> JobCentre {
        let book = Arc::new(Mutex::new(Book::default()));
        JobCentre {
            registry: JobRegistry::new(Arc::new(Journal(book.clone()))),
            pool: JobPool::new(Workers::new(WORKERS)),
            book,
        }
    }

    /// Starts `spec` in the background and answers its status at once. Work that is already running answers the running
    /// job; a search while another one runs is refused (`Busy`), because the person has to choose which to keep.
    pub fn start(&self, shared: &Arc<Shared>, spec: JobSpec) -> Result<JobStatus, AgentError> {
        let spec = validated(spec)?;
        let ctx = {
            let mut book = lock(&self.book);
            if let Some((id, _)) = book.active.iter().find(|(_, s)| *s == spec) {
                let id = *id;
                drop(book);
                return self
                    .status(id)
                    .ok_or_else(|| AgentError::new(ErrorCode::Internal, "The job vanished."));
            }
            if matches!(spec.kind(), JobKind::FindFiles | JobKind::RescanIssues | JobKind::RefreshBackup)
                && book.active.iter().any(|(_, s)| s.kind() == spec.kind())
            {
                return Err(AgentError::new(
                    ErrorCode::Busy,
                    format!("{} is already running; wait for it or cancel it.", spec.kind().title()),
                ));
            }
            let ctx = self
                .registry
                .register(slug(spec.kind()), spec.kind().title(), subject(&spec).as_deref());
            book.active.push((ctx.id().0, spec.clone()));
            ctx
        };
        // Everything that ended is in the book; the registry need not keep it.
        self.registry.forget_finished();
        let id = ctx.id().0;
        let shared2 = shared.clone();
        let book = self.book.clone();
        let spawned = self.pool.spawn_job(ctx, move |ctx| {
            let output = execute(&shared2, ctx, spec)?;
            lock(&book).outputs.insert(id, output);
            Ok(())
        });
        if spawned.is_err() {
            return Err(AgentError::new(ErrorCode::Internal, "Dyno is shutting down."));
        }
        self.status(id)
            .ok_or_else(|| AgentError::new(ErrorCode::Internal, "The job vanished."))
    }

    /// Every job that has not ended and the last few that did, oldest first.
    pub fn statuses(&self) -> Vec<JobStatus> {
        let mut all: Vec<JobStatus> = self
            .registry
            .snapshots()
            .iter()
            .filter(|s| !s.state.is_terminal())
            .map(status_of)
            .collect();
        all.extend(lock(&self.book).finished.iter().cloned());
        all.sort_by_key(|s| s.id);
        all.dedup_by_key(|s| s.id);
        all
    }

    fn status(&self, id: u64) -> Option<JobStatus> {
        self.statuses().into_iter().find(|s| s.id == id)
    }

    /// Asks a job to stop and returns at once. A job that is unknown or already over is left as it is. A job that is
    /// stopping no longer counts as running work: the same work, or another search, may start while it winds down.
    pub fn cancel(&self, id: u64) {
        if self.registry.cancel(JobId(id)) {
            lock(&self.book).active.retain(|(i, _)| *i != id);
        }
    }

    /// What a done job produced.
    pub fn output(&self, id: u64) -> Result<JobOutput, AgentError> {
        let book = lock(&self.book);
        let done = book.finished.iter().any(|s| s.id == id && s.state == JobState::Done);
        match book.outputs.get(&id) {
            Some(out) if done => Ok(out.clone()),
            _ => Err(AgentError::new(
                ErrorCode::NotFound,
                "That job has no result: it is still running, was cancelled or failed, or is too old to be kept.",
            )),
        }
    }
}

impl Drop for JobCentre {
    fn drop(&mut self) {
        // Stop what runs so the pool joins quickly when the agent quits.
        let ids: Vec<u64> = lock(&self.book).active.iter().map(|(id, _)| *id).collect();
        for id in ids {
            self.registry.cancel(JobId(id));
        }
    }
}

/// The slug each kind goes by inside the job registry.
const KINDS: [(JobKind, &str); 4] = [
    (JobKind::FindFiles, "find-files"),
    (JobKind::RescanIssues, "rescan-issues"),
    (JobKind::RefreshBackup, "refresh-backup"),
    (JobKind::FileHash, "file-hash"),
];

fn slug(kind: JobKind) -> &'static str {
    KINDS.iter().find(|(k, _)| *k == kind).map_or("find-files", |(_, s)| s)
}

/// The refusals that need no work: a request that cannot succeed is not worth a job.
fn validated(spec: JobSpec) -> Result<JobSpec, AgentError> {
    match spec {
        JobSpec::FindFiles { query } => query
            .validated()
            .map(|query| JobSpec::FindFiles { query })
            .map_err(|m| AgentError::new(ErrorCode::BadRequest, m)),
        other => Ok(other),
    }
}

fn subject(spec: &JobSpec) -> Option<String> {
    match spec {
        JobSpec::FindFiles { query } => Some(query.folder.clone()),
        JobSpec::RescanIssues { query } => Some(format!("last {} days", query.days)),
        JobSpec::RefreshBackup => None,
        JobSpec::FileHash { source } => match source {
            dyno_core::weblinks::HashSource::Path(p) => Some(p.clone()),
            dyno_core::weblinks::HashSource::Process(_) => None,
        },
    }
}

/// Runs the work of `spec`; the output is stored only if this returns `Ok`.
fn execute(shared: &Arc<Shared>, ctx: &JobContext, spec: JobSpec) -> Result<JobOutput, JobError> {
    match spec {
        JobSpec::FindFiles { query } => {
            let id = shared.extras.next_scan_id(now());
            crate::extras::finder::run(ctx, &query, &id, crate::extras::finder::Limits::default()).map(|r| JobOutput::Finder(Box::new(r)))
        }
        JobSpec::RescanIssues { query } => {
            shared.issues.scan(shared, Some(ctx))?;
            crate::issues::overview(shared, query)
                .map(|o| JobOutput::Issues(Box::new(o)))
                .map_err(|e| JobError::Failed(e.message))
        }
        JobSpec::RefreshBackup => {
            let report = crate::extras::backup_check::refresh(shared, ctx)?;
            Ok(JobOutput::Backup(Box::new(report)))
        }
        JobSpec::FileHash { source } => {
            let path = crate::handlers_search::hash_path(shared, source).map_err(|e| JobError::Failed(e.message))?;
            shared.hasher.hash_job(ctx, &path).map(JobOutput::FileHash).map_err(|e| match e {
                crate::filehash::HashError::Cancelled(c) => JobError::Cancelled(c),
                crate::filehash::HashError::Failed(m) => JobError::Failed(m),
            })
        }
    }
}

/// The wire status of a run-time snapshot.
fn status_of(s: &JobSnapshot) -> JobStatus {
    JobStatus {
        id: s.id.0,
        kind: KINDS
            .iter()
            .find(|(_, slug)| *slug == s.kind.0)
            .map_or(JobKind::FindFiles, |(k, _)| *k),
        title: s.title.clone(),
        subject: s.subject.clone(),
        state: match &s.state {
            RunState::Queued => JobState::Queued,
            RunState::Running { phase } => JobState::Running {
                phase: phase.as_ref().map(|p| JobPhase {
                    id: p.phase.id.clone(),
                    label: p.phase.label.clone(),
                    unit: match p.phase.unit {
                        Unit::Items => JobUnit::Items,
                        Unit::Bytes => JobUnit::Bytes,
                    },
                    done: p.done,
                    total: p.phase.total,
                }),
            },
            RunState::Done => JobState::Done,
            RunState::Failed { message } => JobState::Failed { message: message.clone() },
            RunState::Cancelled { .. } => JobState::Cancelled,
        },
        seq: s.seq,
        elapsed_ms: s.elapsed_ms,
        eta_ms: s.eta_ms,
        cancellable: s.cancellable,
        not_cancellable_reason: s.not_cancellable_reason.clone(),
    }
}

/// Test support: a job context over a recording sink, so a test can read back every phase and count a body reported and
/// cancel at a chosen point.
#[cfg(test)]
pub(crate) mod testing {
    use paddock_jobs::{CancelToken, Clock, EventSink, JobContext, JobError, JobRegistry, JobSnapshot, JobState, SystemClock};
    use std::sync::{Arc, Mutex};

    /// One phase as it was reported.
    pub struct PhaseLog {
        pub id: String,
        pub total: Option<u64>,
        /// The count at every report, in order.
        pub counts: Vec<u64>,
    }

    struct Rec {
        snaps: Mutex<Vec<JobSnapshot>>,
        cancel_at: Option<(&'static str, u64)>,
        token: Mutex<Option<CancelToken>>,
    }

    impl EventSink for Rec {
        fn emit(&self, s: &JobSnapshot) {
            self.snaps.lock().unwrap().push(s.clone());
            if let (Some((phase, at)), JobState::Running { phase: Some(p) }) = (self.cancel_at, &s.state) {
                if p.phase.id == phase && p.done >= at {
                    if let Some(t) = self.token.lock().unwrap().as_ref() {
                        t.cancel();
                    }
                }
            }
        }
    }

    /// One job over a recording sink; every update is reported (no throttling).
    pub struct Run {
        rec: Arc<Rec>,
        registry: JobRegistry,
    }

    impl Run {
        pub fn new() -> Run {
            Run::with_cancel_at(None)
        }

        fn with_cancel_at(cancel_at: Option<(&'static str, u64)>) -> Run {
            let rec = Arc::new(Rec {
                snaps: Mutex::new(Vec::new()),
                cancel_at,
                token: Mutex::new(None),
            });
            let clock: Arc<dyn Clock> = Arc::new(SystemClock);
            Run {
                registry: JobRegistry::with_clock(rec.clone(), clock, 0),
                rec,
            }
        }

        /// Cancels the job as soon as the phase `phase` has counted `n`.
        pub fn cancel_when(self, phase: &'static str, n: u64) -> Run {
            Run::with_cancel_at(Some((phase, n)))
        }

        fn context(&self) -> JobContext {
            let ctx = self.registry.register("find-files", "Test", None);
            *self.rec.token.lock().unwrap() = Some(ctx.cancel_token().clone());
            ctx
        }

        /// Runs `f` as the job's body and returns what it returned.
        pub fn body<T>(&self, f: impl FnOnce(&JobContext) -> T) -> T {
            let ctx = self.context();
            ctx.reporter().start();
            let out = f(&ctx);
            ctx.reporter().flush();
            out
        }

        /// Runs `f` through `JobContext::run`, so the job also ends in its terminal state.
        pub fn job<T>(&self, f: impl FnOnce(&JobContext) -> Result<T, JobError>) -> Result<T, JobError> {
            self.context().run(f)
        }

        /// Every snapshot reported so far.
        pub fn snapshots(&self) -> Vec<JobSnapshot> {
            self.rec.snaps.lock().unwrap().clone()
        }

        /// The phases in the order they ran, each with its counts.
        pub fn phases(&self) -> Vec<PhaseLog> {
            let mut out: Vec<PhaseLog> = Vec::new();
            for s in self.snapshots() {
                if let JobState::Running { phase: Some(p) } = s.state {
                    if out.last().is_none_or(|l| l.id != p.phase.id) {
                        out.push(PhaseLog {
                            id: p.phase.id.clone(),
                            total: p.phase.total,
                            counts: Vec::new(),
                        });
                    }
                    let last = out.last_mut().unwrap();
                    last.total = p.phase.total;
                    last.counts.push(p.done);
                }
            }
            out
        }

        /// The state of the last snapshot.
        pub fn end_state(&self) -> JobState {
            self.snapshots().last().unwrap().state.clone()
        }
    }
}
