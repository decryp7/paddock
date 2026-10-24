mod common;
use common::Collect;
use paddock_jobs::*;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

struct Park(std::thread::Thread);
impl Wake for Park {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = Box::pin(f);
    let waker = Waker::from(Arc::new(Park(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park_timeout(Duration::from_secs(5));
    }
}

#[test]
fn spawn_returns_at_once_and_the_job_runs_off_thread() {
    let reg = JobRegistry::new(Collect::new());
    let pool = JobPool::new(Workers::new(2));
    let caller = std::thread::current().id();
    let ctx = reg.register("scan", "Scan", None);
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let rx = Mutex::new(rx);
    let handle = assert_nonblocking(&pool, || {
        pool.spawn_job(ctx.clone(), move |c| {
            assert_ne!(std::thread::current().id(), caller);
            rx.lock().unwrap().recv().unwrap(); // blocks the worker, not the handler
            c.phase(Phase::items("x", "X", 1));
            c.add(1);
            Ok(9)
        })
        .unwrap()
    })
    .expect("spawning must not block");
    assert!(handle.try_result().is_none());
    tx.send(()).unwrap();
    assert_eq!(handle.join().unwrap(), 9);
    assert!(matches!(ctx.reporter().snapshot().state, JobState::Done));
}

#[test]
fn concurrency_is_bounded_by_the_pool() {
    let reg = JobRegistry::new(Collect::new());
    let pool = JobPool::new(Workers::new(2));
    let (running, peak) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (started_tx, started) = std::sync::mpsc::channel();
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (r, p, tx, g) = (
                running.clone(),
                peak.clone(),
                started_tx.clone(),
                gate.clone(),
            );
            pool.spawn_job(reg.register("k", "K", None), move |_| {
                p.fetch_max(r.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                tx.send(()).unwrap();
                // Every job waits until the test opens the gate: both workers are busy at once.
                drop(g.1.wait_while(g.0.lock().unwrap(), |open| !*open).unwrap());
                r.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            })
            .unwrap()
        })
        .collect();
    started.recv().unwrap();
    started.recv().unwrap();
    // Two workers, both held: a third job cannot have started.
    assert!(started.try_recv().is_err());
    assert_eq!(running.load(Ordering::SeqCst), 2);
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    handles.into_iter().for_each(|h| h.join().unwrap());
    assert_eq!(peak.load(Ordering::SeqCst), 2);
}

#[test]
fn a_job_cancelled_while_queued_never_starts() {
    let reg = JobRegistry::new(Collect::new());
    let pool = JobPool::new(Workers::one());
    let gate = Arc::new(Mutex::new(()));
    let held = gate.lock().unwrap();
    let g2 = gate.clone();
    let first = pool.spawn_job(reg.register("a", "A", None), move |_| {
        let _g = g2.lock().unwrap();
        Ok(())
    });
    let queued = reg.register("b", "B", None);
    let id = queued.id();
    let ran = Arc::new(AtomicUsize::new(0));
    let r2 = ran.clone();
    let second = pool.spawn_job(queued, move |_| {
        r2.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    assert!(reg.cancel(id));
    drop(held);
    first.unwrap().join().unwrap();
    assert!(matches!(
        second.unwrap().join(),
        Err(JobError::Cancelled(CancelReason::User))
    ));
    assert_eq!(ran.load(Ordering::SeqCst), 0);
}

#[test]
fn offload_resolves_and_contains_panics() {
    let pool = JobPool::new(Workers::new(1));
    assert_eq!(block_on(pool.offload(|| 21 * 2)).unwrap(), 42);
    let e = block_on(pool.offload(|| -> u8 { panic!("blocking bug") })).unwrap_err();
    assert!(matches!(e, JobError::Panicked(m) if m == "blocking bug"));
    // The pool survives a panic.
    assert_eq!(block_on(pool.offload(|| 1)).unwrap(), 1);
    // Work that has not finished: the future is pending first, then wakes.
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let mut fut = Box::pin(pool.offload(move || {
        wait.recv().unwrap();
        "late"
    }));
    assert!(
        fut.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    go.send(()).unwrap();
    assert_eq!(block_on(fut).unwrap(), "late");
}

#[test]
fn closed_pool_refuses_work_without_panicking() {
    let reg = JobRegistry::new(Collect::new());
    let pool = JobPool::new(Workers::one());
    pool.shutdown();
    pool.shutdown();
    assert!(
        pool.spawn_job(reg.register("a", "A", None), |_| Ok(()))
            .is_err()
    );
    assert_eq!(PoolClosed.to_string(), "the app is shutting down");
    assert!(block_on(pool.offload(|| 1)).is_err());
}

#[test]
fn dropped_handle_does_not_matter_and_queue_drains_on_drop() {
    let reg = JobRegistry::new(Collect::new());
    let done = Arc::new(AtomicUsize::new(0));
    {
        let pool = JobPool::new(Workers::one());
        for _ in 0..5 {
            let d = done.clone();
            let _ = pool.spawn_job(reg.register("a", "A", None), move |_| {
                d.fetch_add(1, Ordering::SeqCst);
                Ok(())
            });
        }
    }
    assert_eq!(done.load(Ordering::SeqCst), 5);
}

#[test]
fn detects_a_handler_that_waits_for_its_job() {
    let pool = JobPool::new(Workers::one());
    assert_eq!(assert_nonblocking(&pool, || 5).unwrap(), 5);
    // A handler that joins its job cannot return while the pool is held: whatever the guard, it is reported.
    let reg = JobRegistry::new(Collect::new());
    let blocked = assert_nonblocking_within(&pool, Duration::from_millis(1), || {
        pool.spawn_job(reg.register("a", "A", None), |_| Ok(7))
            .unwrap()
            .join()
    })
    .unwrap_err();
    assert_eq!(blocked.after, Duration::from_millis(1));
    assert!(blocked.to_string().contains("job pool"));
    assert_eq!(BLOCKED_AFTER, Duration::from_secs(30));
    // The pool works again once released.
    assert_eq!(block_on(pool.offload(|| 3)).unwrap(), 3);
}

#[test]
fn join_of_a_lost_job_reports_it() {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    drop(tx);
    let _ = rx;
    // A job that panics inside the body is a failed job, and the handle still yields the error.
    let reg = JobRegistry::new(Collect::new());
    let pool = JobPool::new(Workers::one());
    let h = pool
        .spawn_job(reg.register("a", "A", None), |_| -> Result<(), JobError> {
            panic!("x")
        })
        .unwrap();
    assert!(matches!(h.join(), Err(JobError::Panicked(_))));
}
