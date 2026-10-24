//! What a long operation reports to and asks of its caller: phases with counts, and whether the user pressed Cancel. A port, so
//! the core is tested without a job registry; `paddock_storage::tauri` implements it over a job (rule 11).

/// Progress and cancellation of one operation.
pub trait Run: Sync {
    /// A new phase. `total` is the number of items it will go through, or `None` while that is unknown (the count then grows).
    fn phase(&self, id: &str, label: &str, total: Option<u64>);
    /// `n` more items of the phase are done.
    fn add(&self, n: u64);
    /// Whether the user cancelled; checked between items and inside long walks.
    fn cancelled(&self) -> bool;
}

/// Runs without reporting and never cancels.
impl Run for () {
    fn phase(&self, _: &str, _: &str, _: Option<u64>) {}
    fn add(&self, _: u64) {}
    fn cancelled(&self) -> bool {
        false
    }
}

/// Records what happened, for tests and for callers that only want the numbers.
#[derive(Default)]
pub struct Recorder {
    phases: std::sync::Mutex<Vec<(String, Option<u64>)>>,
    done: std::sync::atomic::AtomicU64,
    cancel: std::sync::atomic::AtomicBool,
    cancel_after: std::sync::atomic::AtomicU64,
    checks: std::sync::atomic::AtomicU64,
    cancel_on_check: std::sync::atomic::AtomicU64,
}

impl Recorder {
    /// A recorder that has not been cancelled.
    pub fn new() -> Recorder {
        Recorder {
            cancel_after: std::sync::atomic::AtomicU64::new(u64::MAX),
            cancel_on_check: std::sync::atomic::AtomicU64::new(u64::MAX),
            ..Recorder::default()
        }
    }

    /// Cancels once `items` items have been reported done (deterministic, no timing).
    pub fn cancel_after(self, items: u64) -> Recorder {
        self.cancel_after
            .store(items, std::sync::atomic::Ordering::SeqCst);
        self
    }

    /// Cancels from the `n`-th time it is asked whether to stop (counting from 0): to stop in the middle of a long step.
    pub fn cancel_on_check(self, n: u64) -> Recorder {
        self.cancel_on_check
            .store(n, std::sync::atomic::Ordering::SeqCst);
        self
    }

    /// Cancels now.
    pub fn cancel(&self) {
        self.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// The phases so far, as `(label, total)`.
    pub fn phases(&self) -> Vec<(String, Option<u64>)> {
        self.phases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Items reported done in total.
    pub fn done(&self) -> u64 {
        self.done.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Run for Recorder {
    fn phase(&self, _: &str, label: &str, total: Option<u64>) {
        self.phases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((label.to_string(), total));
    }

    fn add(&self, n: u64) {
        self.done.fetch_add(n, std::sync::atomic::Ordering::SeqCst);
    }

    fn cancelled(&self) -> bool {
        let asked = self
            .checks
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.cancel.load(std::sync::atomic::Ordering::SeqCst)
            || asked
                >= self
                    .cancel_on_check
                    .load(std::sync::atomic::Ordering::SeqCst)
            || self.done.load(std::sync::atomic::Ordering::SeqCst)
                >= self.cancel_after.load(std::sync::atomic::Ordering::SeqCst)
    }
}
