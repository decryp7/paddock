//! Background execution: commands return at once, work runs on a bounded pool and reports through events (ADR 0008).

use crate::error::{JobError, panic_text};
use crate::job::JobContext;
use crate::model::JobState;
use crate::workers::Workers;
use std::fmt;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// How long [`assert_nonblocking`] waits for a handler that does not return while the pool is held: a hang guard that
/// only a broken handler reaches, never a measure of a working one.
pub const BLOCKED_AFTER: Duration = Duration::from_secs(30);

/// Pool holds in force; workers start no task while there is one.
type Gate = Arc<(Mutex<usize>, Condvar)>;

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
    gate: Gate,
}

/// Holds a [`JobPool`]: no queued job starts until it is dropped (see [`JobPool::hold`]).
pub struct PoolHold(Gate);

impl Drop for PoolHold {
    fn drop(&mut self) {
        *lock(&self.0.0) -= 1;
        self.0.1.notify_all();
    }
}

impl JobPool {
    pub fn new(workers: Workers) -> Self {
        let (tx, rx) = mpsc::channel::<Task>();
        let rx = Arc::new(Mutex::new(rx));
        let gate: Gate = Arc::new((Mutex::new(0), Condvar::new()));
        let threads = (0..workers.get())
            .filter_map(|i| {
                let (rx, gate) = (rx.clone(), gate.clone());
                thread::Builder::new()
                    .name(format!("paddock-job-{i}"))
                    .spawn(move || {
                        loop {
                            let task = lock(&rx).recv();
                            match task {
                                Ok(t) => {
                                    let held = lock(&gate.0);
                                    drop(
                                        gate.1
                                            .wait_while(held, |h| *h > 0)
                                            .unwrap_or_else(|p| p.into_inner()),
                                    );
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
            gate,
        }
    }

    /// Holds the pool: queued jobs (and offloaded work) do not start until the returned guard is dropped. For tests
    /// that prove a command returns without waiting for its job ([`assert_nonblocking`]).
    pub fn hold(&self) -> PoolHold {
        *lock(&self.gate.0) += 1;
        PoolHold(self.gate.clone())
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

/// A handler that did not return while the pool was held: it waits for work it should have left to a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Blocked {
    /// The hang guard it ran into.
    pub after: Duration,
}

impl fmt::Display for Blocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the handler did not return while its jobs could not start (waited {:?}); it waits for work that belongs on the job pool",
            self.after
        )
    }
}

impl std::error::Error for Blocked {}

/// Test helper for every command that starts work: runs `handler` while `pool` is held, so no job can start, let alone
/// finish. A handler that returns did not wait for its job, whatever the machine's load; one that waits for a job never
/// returns, and after the hang guard [`BLOCKED_AFTER`] the pool is released, the handler finishes, and the result is
/// `Err(Blocked)`. Nothing is measured: success is structural.
pub fn assert_nonblocking<T: Send>(
    pool: &JobPool,
    handler: impl FnOnce() -> T + Send,
) -> Result<T, Blocked> {
    assert_nonblocking_within(pool, BLOCKED_AFTER, handler)
}

/// [`assert_nonblocking`] with another hang guard (the helper's own tests).
pub fn assert_nonblocking_within<T: Send>(
    pool: &JobPool,
    guard: Duration,
    handler: impl FnOnce() -> T + Send,
) -> Result<T, Blocked> {
    let hold = pool.hold();
    thread::scope(|scope| {
        let (tx, rx) = mpsc::channel();
        let worker = scope.spawn(move || {
            let _ = tx.send(handler());
        });
        let returned = rx.recv_timeout(guard);
        drop(hold);
        let _ = worker.join();
        returned.map_err(|_| Blocked { after: guard })
    })
}
