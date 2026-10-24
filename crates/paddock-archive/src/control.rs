use paddock_jobs::{Cancelled, JobContext, Phase};

/// How a running extraction reports and is stopped. [`JobContext`] is one (a job in the Jobs list with Cancel); [`NoControl`]
/// is another (a plain call, tests, the fuzz target).
pub trait Control {
    /// A new phase starts on its own scale (bytes; `total` when known).
    fn phase(&self, id: &str, label: &str, total: Option<u64>);
    /// How far the current phase is. Never goes backwards.
    fn progress(&self, done: u64);
    /// `Err` once the work was cancelled or timed out; checked at least once per chunk written.
    fn check(&self) -> Result<(), Cancelled>;
}

/// No progress, no cancel.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoControl;

impl Control for NoControl {
    fn phase(&self, _: &str, _: &str, _: Option<u64>) {}
    fn progress(&self, _: u64) {}
    fn check(&self) -> Result<(), Cancelled> {
        Ok(())
    }
}

impl Control for JobContext {
    fn phase(&self, id: &str, label: &str, total: Option<u64>) {
        JobContext::phase(self, Phase::bytes(id, label, total));
    }
    fn progress(&self, done: u64) {
        self.set(done);
    }
    fn check(&self) -> Result<(), Cancelled> {
        self.cancel_token().check()
    }
}
