//! Long operations as jobs (ER-17, ER-18, ER-19), on the family's `paddock-jobs`: a request that starts one answers at
//! once, the work runs on a small worker pool, each job reports phases with their own counts, Cancel is checked while it
//! works, and every change of a job's status is pushed to the connected apps (`Message::Job`), which the shell relays as
//! `paddock://jobs` events, so the shell's Jobs view shows them. A job builds its result on the side and hands it over only
//! when it ends well, so a cancelled or failed job changes nothing. Observe-only like everything else here: no job changes
//! the machine.

use crate::agent::{lock, now, Shared};
use dyno_core::issues::ReadControl;
use dyno_core::jobs::{JobKind, JobOutput, JobSpec, KEPT_FINISHED};
use dyno_core::protocol::{AgentError, ErrorCode};
use paddock_jobs::wire::WireSnapshot;
use paddock_jobs::{EventSink, JobContext, JobError, JobId, JobPool, JobRegistry, JobSnapshot, Phase, Workers};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, Weak};

#[derive(Default)]
struct Book {
    /// Jobs that have not ended, with what they were asked to do (a second start of the same work answers the first).
    active: Vec<(u64, JobSpec)>,
    /// The last jobs that ended, oldest first.
    finished: VecDeque<WireSnapshot>,
    /// What the done ones among them produced.
    outputs: HashMap<u64, JobOutput>,
}

/// Remembers how jobs end and hands every change to the connected apps. Called by the reporter with its lock held, so it only
/// touches the book and the apps' slots and never calls back into the job.
struct Journal {
    book: Arc<Mutex<Book>>,
    shared: Weak<Shared>,
}

impl EventSink for Journal {
    fn emit(&self, snapshot: &JobSnapshot) {
        let wire = WireSnapshot::from(snapshot);
        if wire.is_terminal() {
            let mut book = lock(&self.book);
            book.active.retain(|(id, _)| *id != wire.id);
            book.finished.push_back(wire.clone());
            while book.finished.len() > KEPT_FINISHED {
                if let Some(old) = book.finished.pop_front() {
                    book.outputs.remove(&old.id);
                }
            }
        }
        if let Some(shared) = self.shared.upgrade() {
            shared.broadcast_job(&wire);
        }
    }
}

/// The running and the recently finished jobs.
pub struct JobCentre {
    book: Arc<Mutex<Book>>,
    registry: JobRegistry,
    pool: JobPool,
}

impl JobCentre {
    /// A centre with no jobs, pushing their statuses to the apps connected to `shared`. `workers` jobs run at once (the Setting
    /// `jobs.workers`, read at start); the rest wait their turn as `queued`.
    pub fn new(shared: Weak<Shared>, workers: usize) -> JobCentre {
        let book = Arc::new(Mutex::new(Book::default()));
        JobCentre {
            registry: JobRegistry::new(Arc::new(Journal {
                book: book.clone(),
                shared,
            })),
            pool: JobPool::new(Workers::new(workers)),
            book,
        }
    }

    /// Starts `spec` in the background and answers its status at once. Work that is already running answers the running
    /// job; a search, log read or backup check while another of its kind runs is refused (`Busy`), because the person has to
    /// choose which to keep.
    pub fn start(&self, shared: &Arc<Shared>, spec: JobSpec) -> Result<WireSnapshot, AgentError> {
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
            if spec.kind() != JobKind::FileHash && book.active.iter().any(|(_, s)| s.kind() == spec.kind()) {
                return Err(AgentError::new(
                    ErrorCode::Busy,
                    format!("{} is already running; wait for it or cancel it.", spec.kind().title()),
                ));
            }
            let ctx = self
                .registry
                .register(spec.kind().slug(), spec.kind().title(), subject(&spec).as_deref());
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
    pub fn statuses(&self) -> Vec<WireSnapshot> {
        let mut all: Vec<WireSnapshot> = self
            .registry
            .snapshots()
            .iter()
            .filter(|s| !s.state.is_terminal())
            .map(WireSnapshot::from)
            .collect();
        all.extend(lock(&self.book).finished.iter().cloned());
        all.sort_by_key(|s| s.id);
        all.dedup_by_key(|s| s.id);
        all
    }

    fn status(&self, id: u64) -> Option<WireSnapshot> {
        self.statuses().into_iter().find(|s| s.id == id)
    }

    /// Asks a job to stop and returns at once; false when it is unknown or already over. A job that is stopping no longer
    /// counts as running work: the same work, or another search, may start while it winds down.
    pub fn cancel(&self, id: u64) -> bool {
        let found = self.registry.cancel(JobId(id));
        if found {
            lock(&self.book).active.retain(|(i, _)| *i != id);
        }
        found
    }

    /// What a done job produced.
    pub fn output(&self, id: u64) -> Result<JobOutput, AgentError> {
        let book = lock(&self.book);
        let done = book
            .finished
            .iter()
            .any(|s| s.id == id && matches!(s.state, paddock_jobs::wire::WireState::Done));
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
        JobSpec::FileHash {
            source: dyno_core::weblinks::HashSource::Path(p),
        } => Some(p.clone()),
        _ => None,
    }
}

/// A job's token and counter as the control of a log read.
pub struct JobControl<'a>(pub &'a JobContext);

impl ReadControl for JobControl<'_> {
    fn cancelled(&self) -> bool {
        self.0.cancel_token().is_cancelled()
    }

    fn parsed(&self, records: u64) {
        self.0.set(records);
    }

    fn on_cancel(&self, stop: Box<dyn Fn() + Send + Sync>) -> Box<dyn Send> {
        Box::new(self.0.cancel_token().on_cancel(stop))
    }
}

/// One read of an OS tool that cannot be interrupted: a phase that says what is being read (the card shows the running time),
/// Cancel declared as taking effect when the tool has answered, and the answer dropped when the job was cancelled meanwhile.
pub fn tool_step<T>(ctx: &JobContext, id: &str, label: &str, read: impl FnOnce() -> T) -> Result<T, JobError> {
    ctx.phase(Phase::discovering(id, label));
    ctx.check()?;
    let step = ctx.uncancellable("A system tool is being run; Cancel takes effect when it has answered.");
    let out = read();
    drop(step);
    ctx.check()?;
    Ok(out)
}

/// Runs the work of `spec`; the output is stored only if this returns `Ok`.
fn execute(shared: &Arc<Shared>, ctx: &JobContext, spec: JobSpec) -> Result<JobOutput, JobError> {
    match spec {
        JobSpec::FindFiles { query } => {
            let id = shared.extras.next_scan_id(now());
            // The Settings, read when the search starts: the limits it runs with, and how many threads read the folder (Auto is
            // what the kind of volume allows).
            let settings = lock(&shared.core).settings.disk;
            let root = std::path::Path::new(&query.folder);
            let (location, rotational, cores) = shared.disk.storage_of(root);
            let threads = settings.read_threads(dyno_core::extras::files::auto_finder_threads(&location, rotational, cores));
            crate::extras::finder::run(
                ctx,
                &query,
                &id,
                settings.finder_limits(),
                Workers::new(threads),
                crate::extras::finder::Linger::of(shared.disk.host_threads(root), threads),
            )
            .map(|r| JobOutput::Finder(Box::new(r)))
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
        JobSpec::SleepBlockers => tool_step(ctx, "sleep", "Asking the system what keeps it awake", || {
            let x = &shared.extras;
            dyno_platform::extras::sleep_report(x.tools().as_ref(), x.os())
        })
        .map(|r| JobOutput::Sleep(Box::new(r))),
        JobSpec::BootAnalysis => tool_step(ctx, "boot", "Reading the boot history", || crate::extras::boot_report(shared))?
            .map(|r| JobOutput::Boot(Box::new(r)))
            .map_err(|e| JobError::Failed(e.message)),
        JobSpec::AppUsage => tool_step(ctx, "usage", "Reading network and energy use", || crate::extras::app_usage(shared))
            .map(|r| JobOutput::AppUsage(Box::new(r))),
        JobSpec::SoftwareInventory => tool_step(ctx, "software", "Listing installed software", || crate::extras::software(shared))
            .map(|r| JobOutput::Software(Box::new(r))),
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
