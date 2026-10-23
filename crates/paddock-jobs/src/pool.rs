//! Background execution: commands return at once, work runs on a bounded pool and reports through events (ADR 0007).

use crate::error::{JobError, panic_text};
use crate::job::JobContext;
use crate::model::JobState;
use crate::workers::Workers;
use std::fmt;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// A command handler should return within this budget (ms); anything longer belongs on the pool.
pub const HANDLER_BUDGET_MS: u64 = 50;

type Task = Box<dyn FnOnce() + Send>;

/// The pool no longer accepts work (it was shut down).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolClosed;

impl fmt::Display for PoolClosed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the app is shutting down")
    }
}

impl std::error::Error for PoolClosed {}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A bounded pool of worker threads for jobs. Submitting never blocks and never runs work on the caller's thread: a command
/// registers the job, calls [`spawn_job`](Self::spawn_job) and returns its id at once; the job stays `queued` until a worker is free
/// (that is the concurrency limit) and reports through its reporter. Dropping the pool finishes queued tasks and joins the workers;
/// cancel jobs first for a fast exit.
pub struct JobPool {
    tx: Mutex<Option<Sender<Task>>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl JobPool {
    pub fn new(workers: Workers) -> Self {
        let (tx, rx) = mpsc::channel::<Task>();
        let rx = Arc::new(Mutex::new(rx));
        let threads = (0..workers.get())
            .filter_map(|i| {
                let rx = rx.clone();
                thread::Builder::new()
                    .name(format!("paddock-job-{i}"))
                    .spawn(move || {
                        loop {
                            let task = lock(&rx).recv();
                            match task {
                                Ok(t) => {
                                    let _ = catch_unwind(AssertUnwindSafe(t));
                                }
                                Err(_) => return,
                            }
                        }
                    })
                    .ok()
            })
            .collect();
        JobPool {
            tx: Mutex::new(Some(tx)),
            threads: Mutex::new(threads),
        }
    }

    fn submit(&self, task: Task) -> Result<(), PoolClosed> {
        match lock(&self.tx).as_ref() {
            Some(tx) => tx.send(task).map_err(|_| PoolClosed),
            None => Err(PoolClosed),
        }
    }

    /// Runs `body` as the job `ctx` on a pool thread and returns immediately. A job cancelled while queued never starts.
    pub fn spawn_job<T: Send + 'static>(
        &self,
        ctx: JobContext,
        body: impl FnOnce(&JobContext) -> Result<T, JobError> + Send + 'static,
    ) -> Result<JobHandle<T>, PoolClosed> {
        let (tx, rx) = mpsc::channel();
        self.submit(Box::new(move || {
            let result = if matches!(ctx.reporter().snapshot().state, JobState::Cancelled { .. }) {
                Err(JobError::Cancelled(
                    ctx.cancel_token()
                        .reason()
                        .unwrap_or(crate::CancelReason::User),
                ))
            } else {
                ctx.run(body)
            };
            let _ = tx.send(result);
        }))?;
        Ok(JobHandle(rx))
    }

    /// Blocking-work offload for async runtimes (Tauri async commands, tokio): the closure runs on the pool and the returned future
    /// resolves with its result, so the runtime's threads never block. Needs no runtime of its own.
    pub fn offload<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static) -> Offload<T> {
        let slot = Arc::new(Mutex::new(Slot {
            value: None,
            waker: None,
            closed: false,
        }));
        let s2 = slot.clone();
        let sent = self.submit(Box::new(move || {
            let out =
                catch_unwind(AssertUnwindSafe(f)).map_err(|p| JobError::Panicked(panic_text(&*p)));
            let mut s = lock(&s2);
            s.value = Some(out);
            if let Some(w) = s.waker.take() {
                w.wake();
            }
        }));
        if sent.is_err() {
            lock(&slot).closed = true;
        }
        Offload(slot)
    }

    /// Stops accepting work and waits for running and queued tasks.
    pub fn shutdown(&self) {
        lock(&self.tx).take();
        let threads = std::mem::take(&mut *lock(&self.threads));
        for t in threads {
            let _ = t.join();
        }
    }
}

impl Drop for JobPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The eventual result of [`JobPool::spawn_job`]; most callers ignore it and listen to events.
pub struct JobHandle<T>(Receiver<Result<T, JobError>>);

impl<T> JobHandle<T> {
    /// Waits for the job (tests, shutdown). Never call this from a command handler.
    pub fn join(self) -> Result<T, JobError> {
        self.0.recv().unwrap_or_else(|_| {
            Err(JobError::Failed(
                "the job was dropped before it ran".to_string(),
            ))
        })
    }

    /// The result if the job already ended.
    pub fn try_result(&self) -> Option<Result<T, JobError>> {
        self.0.try_recv().ok()
    }
}

struct Slot<T> {
    value: Option<Result<T, JobError>>,
    waker: Option<Waker>,
    closed: bool,
}

/// Future of [`JobPool::offload`]; resolves to `Err(JobError::Panicked)` if the closure panicked and to `Err(JobError::Failed)`
/// when the pool was already shut down.
pub struct Offload<T>(Arc<Mutex<Slot<T>>>);

impl<T> Future for Offload<T> {
    type Output = Result<T, JobError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut s = lock(&self.0);
        if let Some(v) = s.value.take() {
            Poll::Ready(v)
        } else if s.closed {
            Poll::Ready(Err(JobError::Failed(PoolClosed.to_string())))
        } else {
            s.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

/// A handler that took longer than its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlowHandler {
    pub elapsed: Duration,
    pub budget: Duration,
}

impl fmt::Display for SlowHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "handler blocked for {:?} (budget {:?}); move the work to the job pool",
            self.elapsed, self.budget
        )
    }
}

impl std::error::Error for SlowHandler {}

/// Test helper (and optional debug-build guard): runs a command handler and fails if it blocked longer than `budget`. Use it in the
/// tests of every Tauri command: `assert_nonblocking(Duration::from_millis(HANDLER_BUDGET_MS), || start_import(...))`.
pub fn assert_nonblocking<T>(
    budget: Duration,
    handler: impl FnOnce() -> T,
) -> Result<T, SlowHandler> {
    let t = Instant::now();
    let out = handler();
    let elapsed = t.elapsed();
    if elapsed > budget {
        Err(SlowHandler { elapsed, budget })
    } else {
        Ok(out)
    }
}
