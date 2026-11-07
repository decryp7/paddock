use crate::cancel::CancelToken;
use crate::error::{JobError, panic_text};
use crate::model::{JobId, JobKind, JobSnapshot, JobState, Phase};
use crate::reporter::{Counter, DEFAULT_MIN_INTERVAL_MS, EventSink, Reporter, UncancellableGuard};
use crate::time::{Clock, SystemClock};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// What a running job body holds: its reporter and cancel token. Cheap to clone, `Send + Sync`.
#[derive(Clone)]
pub struct JobContext {
    reporter: Reporter,
    cancel: CancelToken,
}

impl JobContext {
    pub fn id(&self) -> JobId {
        self.reporter.id()
    }

    pub fn reporter(&self) -> &Reporter {
        &self.reporter
    }

    /// The token to hand to helpers ([`par_walk`](crate::par_walk), [`copy_hashed`](crate::copy_hashed), ...).
    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// `Err` once the job was cancelled or its deadline passed.
    pub fn check(&self) -> Result<(), JobError> {
        self.cancel.check().map_err(JobError::from)
    }

    /// Starts a new phase on its own scale.
    pub fn phase(&self, phase: Phase) {
        self.reporter.begin_phase(phase);
    }

    pub fn add(&self, n: u64) {
        self.reporter.add(n);
    }

    pub fn set(&self, done: u64) {
        self.reporter.set(done);
    }

    pub fn set_total(&self, total: u64) {
        self.reporter.set_total(total);
    }

    /// Declares a step that cannot be interrupted; see [`Reporter::uncancellable`].
    pub fn uncancellable(&self, reason: &str) -> UncancellableGuard {
        self.reporter.uncancellable(reason)
    }

    /// Runs the job body: starts the job, contains panics, maps the outcome to the terminal state (done, failed, cancelled) and
    /// returns the body's result. Exactly one terminal snapshot is emitted.
    pub fn run<T>(
        &self,
        body: impl FnOnce(&JobContext) -> Result<T, JobError>,
    ) -> Result<T, JobError> {
        self.reporter.start();
        let result = catch_unwind(AssertUnwindSafe(|| body(self)))
            .unwrap_or_else(|p| Err(JobError::Panicked(panic_text(&*p))));
        self.reporter.flush();
        let end = match &result {
            Ok(_) => JobState::Done,
            Err(JobError::Cancelled(reason)) => JobState::Cancelled { reason: *reason },
            Err(e) => JobState::Failed {
                message: e.to_string(),
                limit: match e {
                    JobError::Limit(l) => Some(*l),
                    _ => None,
                },
            },
        };
        self.reporter.finish(end);
        result
    }
}

impl Counter for JobContext {
    fn add(&self, n: u64) {
        self.reporter.add(n);
    }
}

/// Owns the ids, cancel tokens and latest snapshots of an app's jobs. Queueing and concurrency limits stay with the app
/// (Debrief keeps its own queue and calls [`JobContext::run`] when a slot frees up).
pub struct JobRegistry {
    next: AtomicU64,
    sink: Arc<dyn EventSink>,
    clock: Arc<dyn Clock>,
    min_interval_ms: u64,
    jobs: Mutex<HashMap<JobId, JobContext>>,
}

impl JobRegistry {
    pub fn new(sink: Arc<dyn EventSink>) -> Self {
        Self::with_clock(sink, Arc::new(SystemClock), DEFAULT_MIN_INTERVAL_MS)
    }

    pub fn with_clock(
        sink: Arc<dyn EventSink>,
        clock: Arc<dyn Clock>,
        min_interval_ms: u64,
    ) -> Self {
        JobRegistry {
            next: AtomicU64::new(1),
            sink,
            clock,
            min_interval_ms,
            jobs: Mutex::new(HashMap::new()),
        }
    }

    fn jobs(&self) -> MutexGuard<'_, HashMap<JobId, JobContext>> {
        self.jobs.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Registers a queued job and emits its first snapshot.
    pub fn register(&self, kind: &str, title: &str, subject: Option<&str>) -> JobContext {
        self.register_with(kind, title, subject, CancelToken::new())
    }

    /// As [`register`](Self::register) with a deadline: the token cancels itself with reason `Deadline` after `limit` on
    /// the registry's clock (a [`FakeClock`](crate::time::FakeClock) in tests runs it out without waiting).
    pub fn register_with_deadline(
        &self,
        kind: &str,
        title: &str,
        subject: Option<&str>,
        limit: Duration,
    ) -> JobContext {
        self.register_with(
            kind,
            title,
            subject,
            CancelToken::with_deadline_on(self.clock.clone(), limit),
        )
    }

    fn register_with(
        &self,
        kind: &str,
        title: &str,
        subject: Option<&str>,
        cancel: CancelToken,
    ) -> JobContext {
        let id = JobId(self.next.fetch_add(1, Ordering::Relaxed));
        let reporter = Reporter::with_clock(
            id,
            JobKind::new(kind),
            title,
            subject,
            self.sink.clone(),
            self.clock.clone(),
            self.min_interval_ms,
        );
        let ctx = JobContext { reporter, cancel };
        self.jobs().insert(id, ctx.clone());
        ctx
    }

    /// Asks a job to stop. A queued job is cancelled at once. False when the id is unknown or the job already ended.
    pub fn cancel(&self, id: JobId) -> bool {
        let Some(ctx) = self.jobs().get(&id).cloned() else {
            return false;
        };
        if ctx.reporter.snapshot().state.is_terminal() {
            return false;
        }
        ctx.cancel.cancel();
        if matches!(ctx.reporter.snapshot().state, JobState::Queued) {
            ctx.reporter.finish(JobState::Cancelled {
                reason: crate::cancel::CancelReason::User,
            });
        }
        true
    }

    /// Latest snapshot of every known job, ordered by id (for a freshly opened Jobs pane).
    pub fn snapshots(&self) -> Vec<JobSnapshot> {
        let mut all: Vec<JobSnapshot> = self
            .jobs()
            .values()
            .map(|c| c.reporter.snapshot())
            .collect();
        all.sort_by_key(|s| s.id);
        all
    }

    /// Forgets jobs that ended; returns how many.
    pub fn forget_finished(&self) -> usize {
        let mut jobs = self.jobs();
        let before = jobs.len();
        jobs.retain(|_, c| !c.reporter.snapshot().state.is_terminal());
        before - jobs.len()
    }
}
