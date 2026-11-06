//! A bounded producer-consumer pipeline for work that is a chain of stages (read, transform, commit).
//!
//! `read` (the caller's thread, usually IO) cuts the input into chunks and feeds a small bounded queue; a `work`
//! stage maps every item of a chunk on a bounded rayon pool (CPU); a single `commit` stage consumes the results
//! on its own thread. Every queue is bounded, so a slow stage back-pressures the ones before it and memory stays
//! at a few chunks. Chunks and the items inside them keep their order through every stage, so the commit stage
//! sees exactly the sequence a plain `for item in input { commit(item, work(&item)) }` would: the output is
//! deterministic and identical to the sequential path. Cancel stops the reader; the queued chunks drain.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::{Condvar, Mutex};

/// Chunks queued between two stages.
pub const QUEUE_DEPTH: usize = 2;

/// Items per chunk when the caller has no better idea.
pub const DEFAULT_CHUNK: usize = 2048;

/// How a pipeline runs.
pub struct Stages<'a> {
    /// The pool of the CPU stage.
    pub pool: &'a rayon::ThreadPool,
    /// Items per chunk (at least 1).
    pub chunk: usize,
    /// Set to stop reading; whatever was read is still committed.
    pub cancel: Option<&'a AtomicBool>,
}

impl Stages<'_> {
    /// Runs `input` through `work` (parallel, in order) into `commit` (one thread, in order). `commit` also gets
    /// the number of items committed so far, for progress. Returns false when it stopped early because of cancel.
    pub fn run<I, T>(
        &self,
        input: impl Iterator<Item = I>,
        work: impl Fn(&I) -> T + Sync,
        mut commit: impl FnMut(I, T, usize) + Send,
    ) -> bool
    where
        I: Send + Sync,
        T: Send,
    {
        use rayon::prelude::*;
        let chunk = self.chunk.max(1);
        let cancelled = || self.cancel.is_some_and(|c| c.load(Ordering::Relaxed));
        let mut stopped = false;
        std::thread::scope(|s| {
            let (read_tx, read_rx) = sync_channel::<Vec<I>>(QUEUE_DEPTH);
            let (done_tx, done_rx) = sync_channel::<(Vec<I>, Vec<T>)>(QUEUE_DEPTH);
            let work = &work;
            let pool = self.pool;
            s.spawn(move || {
                for items in read_rx {
                    let results: Vec<T> = pool.install(|| items.par_iter().map(work).collect());
                    if done_tx.send((items, results)).is_err() {
                        break;
                    }
                }
            });
            s.spawn(move || {
                let mut n = 0usize;
                for (items, results) in done_rx {
                    for (item, result) in items.into_iter().zip(results) {
                        n += 1;
                        commit(item, result, n);
                    }
                }
            });
            let mut input = input;
            loop {
                if cancelled() {
                    stopped = true;
                    break;
                }
                let items: Vec<I> = input.by_ref().take(chunk).collect();
                if items.is_empty() {
                    break;
                }
                if read_tx.send(items).is_err() {
                    break;
                }
            }
            drop(read_tx);
        });
        !stopped
    }
}

/// A byte budget shared by a reader and a committer: the reader [`acquire`](ByteBudget::acquire)s the estimated size
/// of each item before it hands the item on, the committer [`release`](ByteBudget::release)s it once the item is done.
/// An item larger than the whole budget still runs, alone (so the pipeline never stalls on one big item).
pub struct ByteBudget {
    cap: usize,
    state: Mutex<BudgetState>,
    freed: Condvar,
}

#[derive(Default)]
struct BudgetState {
    used: usize,
    peak: usize,
}

impl ByteBudget {
    /// A budget of `cap` bytes.
    pub fn new(cap: usize) -> ByteBudget {
        ByteBudget {
            cap,
            state: Mutex::new(BudgetState::default()),
            freed: Condvar::new(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BudgetState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Waits until `bytes` fit (or nothing else is in flight), then takes them. Returns false, taking nothing, when
    /// `cancel` was set while waiting.
    pub fn acquire(&self, bytes: usize, cancel: Option<&AtomicBool>) -> bool {
        let mut s = self.lock();
        while s.used > 0 && s.used.saturating_add(bytes) > self.cap {
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return false;
            }
            // Woken by every release; the timeout only lets a cancel through.
            s = self
                .freed
                .wait_timeout(s, std::time::Duration::from_millis(100))
                .map(|r| r.0)
                .unwrap_or_else(|p| p.into_inner().0);
        }
        s.used += bytes;
        s.peak = s.peak.max(s.used);
        true
    }

    /// Gives `bytes` back.
    pub fn release(&self, bytes: usize) {
        let mut s = self.lock();
        s.used = s.used.saturating_sub(bytes);
        drop(s);
        self.freed.notify_all();
    }

    /// The most bytes held at once so far.
    pub fn peak(&self) -> usize {
        self.lock().peak
    }

    /// The budget's size.
    pub fn cap(&self) -> usize {
        self.cap
    }
}

impl Stages<'_> {
    /// Like [`Stages::run`] for items that are already one unit of work each: every item is its own task on the pool, at
    /// most `window` items are read, running or waiting to be committed at once, and `commit` sees them in input order on
    /// its own thread. `input` may block (for a [`ByteBudget`], say) because every item it yields is started at once;
    /// whatever blocks it must be freed by `commit`. Returns false when it stopped early because of cancel.
    pub fn run_windowed<I, T>(
        &self,
        window: usize,
        input: impl Iterator<Item = I>,
        work: impl Fn(&I) -> T + Sync,
        mut commit: impl FnMut(I, T, usize) + Send,
    ) -> bool
    where
        I: Send,
        T: Send,
    {
        let cancelled = || self.cancel.is_some_and(|c| c.load(Ordering::Relaxed));
        let mut stopped = false;
        std::thread::scope(|s| {
            let (order_tx, order_rx) = sync_channel::<Receiver<(I, T)>>(window.max(1));
            let committer = s.spawn(move || {
                let mut n = 0usize;
                for result in order_rx {
                    // A task that panicked sent nothing; the pool's scope reports the panic.
                    let Ok((item, value)) = result.recv() else { continue };
                    n += 1;
                    commit(item, value, n);
                }
            });
            let work = &work;
            self.pool.in_place_scope(|scope| {
                for item in input {
                    if cancelled() {
                        stopped = true;
                        break;
                    }
                    let (tx, rx) = sync_channel(1);
                    // Blocks while `window` items wait to be committed.
                    if order_tx.send(rx).is_err() {
                        break;
                    }
                    scope.spawn(move |_| {
                        let value = work(&item);
                        let _ = tx.send((item, value));
                    });
                }
                drop(order_tx);
            });
            let _ = committer.join();
        });
        !stopped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn pool(n: usize) -> rayon::ThreadPool {
        rayon::ThreadPoolBuilder::new().num_threads(n).build().unwrap()
    }

    #[test]
    fn the_commit_sees_the_sequential_order_whatever_the_work_takes() {
        let p = pool(4);
        let stages = Stages {
            pool: &p,
            chunk: 7,
            cancel: None,
        };
        let mut seen = Vec::new();
        let done = stages.run(
            0..1000u32,
            |i| {
                if i % 13 == 0 {
                    std::thread::sleep(std::time::Duration::from_micros(200));
                }
                u64::from(*i) * 3
            },
            |i, t, n| seen.push((i, t, n)),
        );
        assert!(done);
        let want: Vec<_> = (0..1000u32).map(|i| (i, u64::from(i) * 3, i as usize + 1)).collect();
        assert_eq!(seen, want);
    }

    #[test]
    fn an_empty_input_commits_nothing() {
        let p = pool(2);
        let stages = Stages {
            pool: &p,
            chunk: 0,
            cancel: None,
        };
        assert!(stages.run(std::iter::empty::<u8>(), |_| (), |_, (), _| panic!("nothing to commit")));
    }

    #[test]
    fn cancel_stops_reading_and_the_queued_chunks_still_commit_in_order() {
        let p = pool(2);
        let cancel = AtomicBool::new(false);
        let stages = Stages {
            pool: &p,
            chunk: 10,
            cancel: Some(&cancel),
        };
        let read = AtomicUsize::new(0);
        let input = (0..100_000u32).inspect(|_| {
            if read.fetch_add(1, Ordering::Relaxed) == 50 {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        let mut seen = Vec::new();
        let done = stages.run(input, |i| *i, |i, _, _| seen.push(i));
        assert!(!done);
        assert!(
            seen.len() >= 50 && seen.len() < 1000,
            "bounded read-ahead after cancel: {}",
            seen.len()
        );
        assert!(seen.windows(2).all(|w| w[1] == w[0] + 1), "in order, no gaps");
    }

    #[test]
    fn windowed_items_commit_in_order_and_at_most_a_window_is_in_flight() {
        let p = pool(4);
        let stages = Stages {
            pool: &p,
            chunk: 1,
            cancel: None,
        };
        let live = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let input = (0..200u32).inspect(|_| {
            let now = live.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
        });
        let mut seen = Vec::new();
        let done = stages.run_windowed(
            3,
            input,
            |i| {
                if i % 7 == 0 {
                    std::thread::yield_now();
                }
                u64::from(*i) * 2
            },
            |i, t, n| {
                live.fetch_sub(1, Ordering::SeqCst);
                seen.push((i, t, n));
            },
        );
        assert!(done);
        let want: Vec<_> = (0..200u32).map(|i| (i, u64::from(i) * 2, i as usize + 1)).collect();
        assert_eq!(seen, want);
        // The window (3) waiting in the queue, one being committed, one being read.
        assert!(
            peak.load(Ordering::SeqCst) <= 3 + 2,
            "in flight at most a window: {}",
            peak.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn windowed_work_runs_several_items_at_once() {
        let p = pool(4);
        let stages = Stages {
            pool: &p,
            chunk: 1,
            cancel: None,
        };
        let running = AtomicUsize::new(0);
        let both = AtomicUsize::new(0);
        stages.run_windowed(
            4,
            0..4u8,
            |_| {
                running.fetch_add(1, Ordering::SeqCst);
                // Waits on the other items instead of sleeping: only true parallelism gets past.
                while running.load(Ordering::SeqCst) < 4 {
                    std::thread::yield_now();
                }
                both.fetch_add(1, Ordering::SeqCst);
            },
            |_, (), _| {},
        );
        assert_eq!(both.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn windowed_cancel_stops_reading_and_what_was_started_still_commits() {
        let p = pool(2);
        let cancel = AtomicBool::new(false);
        let stages = Stages {
            pool: &p,
            chunk: 1,
            cancel: Some(&cancel),
        };
        let input = (0..100_000u32).inspect(|i| {
            if *i == 50 {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        let mut seen = Vec::new();
        let done = stages.run_windowed(4, input, |i| *i, |i, _, _| seen.push(i));
        assert!(!done);
        assert!(seen.len() >= 50 && seen.len() <= 60, "{}", seen.len());
        assert!(seen.windows(2).all(|w| w[1] == w[0] + 1));
    }

    #[test]
    fn a_budget_holds_the_reader_until_the_committer_gives_bytes_back() {
        let budget = ByteBudget::new(100);
        assert!(budget.acquire(60, None));
        // 60 + 60 does not fit: the second waits for the release below.
        std::thread::scope(|s| {
            let waiter = s.spawn(|| budget.acquire(60, None));
            // Nothing to sleep on: the release below is what lets the waiter through, whenever it gets there.
            budget.release(60);
            assert!(waiter.join().unwrap());
        });
        assert!(budget.peak() <= 100, "peak {}", budget.peak());
        // An item over the whole budget runs alone.
        budget.release(60);
        assert!(budget.acquire(500, None));
        assert_eq!(budget.peak(), 500);
    }

    #[test]
    fn a_budget_takes_bytes_up_to_exactly_its_cap_and_says_its_size() {
        let budget = ByteBudget::new(100);
        assert_eq!(budget.cap(), 100);
        // A cancelled wait only matters if the bytes do not fit: 60 + 40 is exactly the cap and fits at once.
        let cancel = AtomicBool::new(true);
        assert!(budget.acquire(60, Some(&cancel)));
        assert!(budget.acquire(40, Some(&cancel)), "60 + 40 = 100 fits a cap of 100");
        assert!(!budget.acquire(1, Some(&cancel)), "one more byte does not");
        // Releasing makes room again.
        budget.release(40);
        assert!(budget.acquire(40, Some(&cancel)));
        assert_eq!(budget.peak(), 100);
    }

    #[test]
    fn a_budget_wait_ends_on_cancel() {
        let budget = ByteBudget::new(10);
        assert!(budget.acquire(10, None));
        let cancel = AtomicBool::new(true);
        assert!(!budget.acquire(5, Some(&cancel)));
    }
}
