use crate::eta::EtaEstimator;
use crate::model::{JobId, JobKind, JobSnapshot, JobState, Phase, PhaseProgress};
use crate::time::{Clock, SystemClock};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Default floor between two events of one job: 10 per second.
pub const DEFAULT_MIN_INTERVAL_MS: u64 = 100;
const NEVER: u64 = u64::MAX;

/// Where snapshots go (Tauri: `app.emit(EVENT_NAME, snapshot)`). Called with the reporter's lock held so events arrive in `seq`
/// order: it must be quick and must not call back into the reporter. A panic in a sink is contained, the event dropped and the panic counted ([`Reporter::sink_panics`]).
pub trait EventSink: Send + Sync {
    fn emit(&self, snapshot: &JobSnapshot);
}

/// Something workers can add progress to from many threads; [`Reporter`] is one, an `AtomicU64` another, `()` ignores it.
pub trait Counter: Sync {
    fn add(&self, n: u64);
}

impl Counter for () {
    fn add(&self, _: u64) {}
}

impl Counter for AtomicU64 {
    fn add(&self, n: u64) {
        self.fetch_add(n, Ordering::Relaxed);
    }
}

struct Inner {
    state: JobState,
    seq: u64,
    eta: EtaEstimator,
    eta_ms: Option<u64>,
    cancellable: bool,
    reason: Option<String>,
    /// The count the last emitted snapshot carried; a later `done` is a pending update. Kept under the lock, so no atomic
    /// ordering decides whether an update is pending.
    published: u64,
}

struct Shared {
    id: JobId,
    kind: JobKind,
    title: String,
    subject: Option<String>,
    sink: Arc<dyn EventSink>,
    clock: Arc<dyn Clock>,
    min_interval_ms: u64,
    start_ms: u64,
    done: AtomicU64,
    last_emit_ms: AtomicU64,
    sink_panics: AtomicU64,
    quiet_on_success: AtomicBool,
    inner: Mutex<Inner>,
}

/// Publishes one job's progress: cheap to call from many threads, throttled to one event per interval with the newest value
/// winning (coalescing), while phase and state changes go out at once. Clones share the job.
#[derive(Clone)]
pub struct Reporter(Arc<Shared>);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Reporter {
    /// A queued job. Emits its first snapshot immediately.
    pub fn new(
        id: JobId,
        kind: JobKind,
        title: &str,
        subject: Option<&str>,
        sink: Arc<dyn EventSink>,
    ) -> Self {
        Self::with_clock(
            id,
            kind,
            title,
            subject,
            sink,
            Arc::new(SystemClock),
            DEFAULT_MIN_INTERVAL_MS,
        )
    }

    /// As [`new`](Self::new) with an injected clock and interval (tests, or a slower channel).
    pub fn with_clock(
        id: JobId,
        kind: JobKind,
        title: &str,
        subject: Option<&str>,
        sink: Arc<dyn EventSink>,
        clock: Arc<dyn Clock>,
        min_interval_ms: u64,
    ) -> Self {
        let start_ms = clock.now_ms();
        let r = Reporter(Arc::new(Shared {
            id,
            kind,
            title: title.to_string(),
            subject: subject.map(str::to_string),
            sink,
            clock,
            min_interval_ms,
            start_ms,
            done: AtomicU64::new(0),
            last_emit_ms: AtomicU64::new(NEVER),
            sink_panics: AtomicU64::new(0),
            quiet_on_success: AtomicBool::new(false),
            inner: Mutex::new(Inner {
                state: JobState::Queued,
                seq: 0,
                eta: EtaEstimator::new(start_ms),
                eta_ms: None,
                cancellable: true,
                reason: None,
                published: 0,
            }),
        }));
        let g = lock(&r.0.inner);
        r.emit_locked(g);
        r
    }

    /// How many times the [`EventSink`] panicked on an event of this job (each panic is contained and the event dropped). An
    /// owner of the job can surface a non-zero count instead of letting a broken sink fail silently.
    pub fn sink_panics(&self) -> u64 {
        self.0.sink_panics.load(Ordering::Relaxed)
    }

    pub fn id(&self) -> JobId {
        self.0.id
    }

    /// Marks the job as one whose success is not announced (see [`JobSnapshot::quiet_on_success`]); every later snapshot
    /// carries it, the terminal one included.
    pub fn set_quiet_on_success(&self) {
        self.0.quiet_on_success.store(true, Ordering::Relaxed);
    }

    /// The current snapshot (does not count as an emit).
    pub fn snapshot(&self) -> JobSnapshot {
        let g = lock(&self.0.inner);
        self.build(&g, self.0.done.load(Ordering::Relaxed))
    }

    fn build(&self, g: &Inner, done: u64) -> JobSnapshot {
        let mut state = g.state.clone();
        if let JobState::Running { phase: Some(p) } = &mut state {
            p.done = done;
        }
        JobSnapshot {
            id: self.0.id,
            kind: self.0.kind.clone(),
            title: self.0.title.clone(),
            subject: self.0.subject.clone(),
            state,
            seq: g.seq,
            elapsed_ms: self.0.clock.now_ms().saturating_sub(self.0.start_ms),
            eta_ms: g.eta_ms,
            cancellable: g.cancellable,
            not_cancellable_reason: g.reason.clone(),
            quiet_on_success: self.0.quiet_on_success.load(Ordering::Relaxed),
        }
    }

    fn emit_locked(&self, mut g: MutexGuard<'_, Inner>) {
        let now = self.0.clock.now_ms();
        // The count is read once, after the clock, and the snapshot and `published` both carry that value: an add that lands later
        // is above `published`, so `flush` sees it.
        let done = self.0.done.load(Ordering::Relaxed);
        if let JobState::Running { phase: Some(p) } = &g.state {
            let total = p.phase.total;
            g.eta_ms = g.eta.observe(now, done, total);
        }
        g.seq += 1;
        g.published = done;
        let snap = self.build(&g, done);
        self.0.last_emit_ms.store(now, Ordering::Relaxed);
        if catch_unwind(AssertUnwindSafe(|| self.0.sink.emit(&snap))).is_err() {
            self.0.sink_panics.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Progress after the end of the job is dropped: the terminal snapshot is the last event.
    fn emit_live(&self, g: MutexGuard<'_, Inner>) {
        // Nothing is published after the end.
        if !g.state.is_terminal() {
            self.emit_locked(g);
        }
    }

    /// Moves queued to running (no phase yet). No-op in any other state.
    pub fn start(&self) {
        let mut g = lock(&self.0.inner);
        if matches!(g.state, JobState::Queued) {
            g.state = JobState::Running { phase: None };
            self.emit_locked(g);
        }
    }

    /// Starts a new phase on its own scale: the count restarts at 0 and the event goes out at once. Starts the job if needed.
    ///
    /// The previous phase's pending count is published first. Calling [`add`](Self::add) or [`set`](Self::set) from another thread
    /// while a phase begins is a caller error: which phase the count belongs to is then undefined. End a phase's workers before
    /// starting the next.
    pub fn begin_phase(&self, phase: Phase) {
        // The previous phase's last throttled count goes out first: it is that phase's final value and the reset below drops it.
        self.flush();
        let mut g = lock(&self.0.inner);
        if g.state.is_terminal() {
            return;
        }
        self.0.done.store(0, Ordering::Relaxed);
        g.eta = EtaEstimator::new(self.0.clock.now_ms());
        g.eta_ms = None;
        g.state = JobState::Running {
            phase: Some(PhaseProgress { phase, done: 0 }),
        };
        self.emit_locked(g);
    }

    /// Adds to the current phase's count; throttled.
    pub fn add(&self, n: u64) {
        self.0.done.fetch_add(n, Ordering::Relaxed);
        self.throttled();
    }

    /// Sets the current phase's count (for sources that report absolute values); throttled.
    pub fn set(&self, done: u64) {
        self.0.done.store(done, Ordering::Relaxed);
        self.throttled();
    }

    /// The total became known (or changed) during the phase; the event goes out at once so the bar switches from indeterminate.
    pub fn set_total(&self, total: u64) {
        let mut g = lock(&self.0.inner);
        if let JobState::Running { phase: Some(p) } = &mut g.state
            && p.phase.total != Some(total)
        {
            p.phase.total = Some(total);
            self.emit_locked(g);
        }
    }

    fn throttled(&self) {
        let s = &self.0;
        let last = s.last_emit_ms.load(Ordering::Relaxed);
        if last != NEVER && s.clock.now_ms().saturating_sub(last) < s.min_interval_ms {
            return;
        }
        // Busy (another thread is emitting) or poisoned: leave it pending (`done` is above `published`); the next call or `flush`
        // publishes the newest value.
        if let Ok(g) = self.0.inner.try_lock() {
            self.emit_live(g);
        }
    }

    /// Emits the newest value if a throttled update is waiting. Call at the end of a loop and before blocking for a long time.
    pub fn flush(&self) {
        let g = lock(&self.0.inner);
        if matches!(g.state, JobState::Running { phase: Some(_) })
            && self.0.done.load(Ordering::Relaxed) != g.published
        {
            self.emit_locked(g);
        }
    }

    /// Declares that the next step cannot be interrupted (a database checkpoint, a rename); the UI shows `reason` instead of an
    /// enabled Cancel. The returned guard restores cancelling when dropped.
    pub fn uncancellable(&self, reason: &str) -> UncancellableGuard {
        let mut g = lock(&self.0.inner);
        if !g.state.is_terminal() {
            g.cancellable = false;
            g.reason = Some(reason.to_string());
            self.emit_locked(g);
        }
        UncancellableGuard(self.clone())
    }

    /// Ends the job in a terminal state. Returns false (and changes nothing) if it already ended or the transition is illegal.
    pub fn finish(&self, to: JobState) -> bool {
        // The last throttled count goes out first, so the phase ends on its final value.
        self.flush();
        let mut g = lock(&self.0.inner);
        if !to.is_terminal() {
            return false;
        }
        match g.state.clone().transition(to) {
            Ok(next) => {
                g.state = next;
                g.cancellable = false;
                g.reason = None;
                g.eta_ms = None;
                self.emit_locked(g);
                true
            }
            Err(_) => false,
        }
    }
}

impl Counter for Reporter {
    fn add(&self, n: u64) {
        Reporter::add(self, n);
    }
}

/// Restores cancelling when dropped; see [`Reporter::uncancellable`].
pub struct UncancellableGuard(Reporter);

impl Drop for UncancellableGuard {
    fn drop(&mut self) {
        let mut g = lock(&(self.0).0.inner);
        if !g.state.is_terminal() {
            g.cancellable = true;
            g.reason = None;
            self.0.emit_locked(g);
        }
    }
}
