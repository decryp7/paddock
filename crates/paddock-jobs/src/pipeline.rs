use crate::cancel::CancelToken;
use crate::error::{JobError, panic_text};
use crate::workers::Workers;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Condvar, Mutex};
use std::thread;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(20);

type Outcome<R> = (u64, Result<R, JobError>);

/// A bounded pipeline: `source` is pulled on its own thread (so discovery continues while earlier items are processed), `work`
/// runs on `workers` threads, and `sink` receives the results **in source order** on the calling thread.
///
/// At most `window` items (at least `workers`) are in flight between "pulled from the source" and "handed to the sink", so memory is
/// bounded no matter how slow one item is. The first error in source order ends the run after the earlier results were delivered
/// (deterministic); a cancel ends it promptly with `JobError::Cancelled`. Panics in `source` or `work` become `JobError::Panicked`.
pub fn pipeline_ordered<T, R, I, W, S>(
    source: I,
    workers: Workers,
    window: usize,
    cancel: &CancelToken,
    work: W,
    mut sink: S,
) -> Result<(), JobError>
where
    T: Send,
    R: Send,
    I: Iterator<Item = T> + Send,
    W: Fn(T) -> Result<R, JobError> + Sync,
    S: FnMut(R) -> Result<(), JobError>,
{
    let window = window.max(workers.get());
    let stop = AtomicBool::new(false);
    let permits = (Mutex::new(0usize), Condvar::new());
    let (work_tx, work_rx) = mpsc::channel::<(u64, T)>();
    let work_rx = Mutex::new(work_rx);
    let (res_tx, res_rx) = mpsc::channel::<Outcome<R>>();

    thread::scope(|scope| {
        let producer_tx = res_tx.clone();
        let (stop, permits) = (&stop, &permits);
        scope.spawn(move || {
            let (inflight, cv) = permits;
            let mut source = source;
            let mut seq = 0u64;
            loop {
                {
                    let mut n = inflight.lock().unwrap_or_else(|p| p.into_inner());
                    while *n >= window && !stop.load(Ordering::Relaxed) && !cancel.is_cancelled() {
                        n = cv
                            .wait_timeout(n, POLL)
                            .unwrap_or_else(|p| p.into_inner())
                            .0;
                    }
                    if stop.load(Ordering::Relaxed) || cancel.is_cancelled() {
                        return;
                    }
                    *n += 1;
                }
                match catch_unwind(AssertUnwindSafe(|| source.next())) {
                    Ok(Some(item)) => {
                        let _ = work_tx.send((seq, item));
                        seq += 1;
                    }
                    Ok(None) => return,
                    Err(p) => {
                        let _ = producer_tx.send((seq, Err(JobError::Panicked(panic_text(&*p)))));
                        return;
                    }
                }
            }
        });

        for _ in 0..workers.get() {
            let tx = res_tx.clone();
            let (stop, work, work_rx) = (stop, &work, &work_rx);
            scope.spawn(move || {
                loop {
                    if stop.load(Ordering::Relaxed) || cancel.is_cancelled() {
                        return;
                    }
                    let next = work_rx
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .recv_timeout(POLL);
                    match next {
                        Ok((seq, item)) => {
                            let out = catch_unwind(AssertUnwindSafe(|| work(item)))
                                .unwrap_or_else(|p| Err(JobError::Panicked(panic_text(&*p))));
                            let _ = tx.send((seq, out));
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            });
        }
        drop(res_tx);

        let outcome = deliver(&res_rx, cancel, permits, &mut sink);
        stop.store(true, Ordering::Relaxed);
        permits.1.notify_all();
        outcome
    })
}

fn deliver<R, S>(
    res_rx: &mpsc::Receiver<Outcome<R>>,
    cancel: &CancelToken,
    permits: &(Mutex<usize>, Condvar),
    sink: &mut S,
) -> Result<(), JobError>
where
    S: FnMut(R) -> Result<(), JobError>,
{
    let mut pending: BTreeMap<u64, Result<R, JobError>> = BTreeMap::new();
    let mut next = 0u64;
    loop {
        while let Some(out) = pending.remove(&next) {
            sink(out?)?;
            next += 1;
            *permits.0.lock().unwrap_or_else(|p| p.into_inner()) -= 1;
            permits.1.notify_one();
        }
        cancel.check()?;
        match res_rx.recv_timeout(POLL) {
            Ok((seq, out)) => {
                pending.insert(seq, out);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                // Everything that will ever arrive has arrived; a gap can only come from a stop.
                cancel.check()?;
                return pending.is_empty().then_some(()).ok_or_else(|| {
                    JobError::Failed("pipeline ended with missing results".to_string())
                });
            }
        }
    }
}
