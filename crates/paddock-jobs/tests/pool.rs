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
    let handle = assert_nonblocking(Duration::from_millis(HANDLER_BUDGET_MS), || {
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
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (r, p) = (running.clone(), peak.clone());
            pool.spawn_job(reg.register("k", "K", None), move |_| {
                p.fetch_max(r.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(10));
                r.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            })
            .unwrap()
        })
        .collect();
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
    // Slow work: the future is pending first, then wakes.
    let fut = pool.offload(|| {
        std::thread::sleep(Duration::from_millis(30));
        "late"
    });
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
fn detects_a_blocking_handler() {
    let ok = assert_nonblocking(Duration::from_millis(200), || 5).unwrap();
    assert_eq!(ok, 5);
    let slow = assert_nonblocking(Duration::from_millis(5), || {
        std::thread::sleep(Duration::from_millis(30))
    })
    .unwrap_err();
    assert!(slow.elapsed >= Duration::from_millis(30));
    assert!(slow.to_string().contains("job pool"));
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
