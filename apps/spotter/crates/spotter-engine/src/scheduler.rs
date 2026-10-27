//! The scheduler: picks the watches that are due, runs them with a bounded number at once, and sends the digest.
//! Backoff, jitter and the minimum interval come from the core's schedule; time comes from the engine's clock, so
//! tests drive it with a manual clock and call [`Scheduler::tick`] directly.
use crate::check::CheckReport;
use crate::engine::Engine;
use std::time::Duration;
use tokio::sync::watch;

/// How often the scheduler looks for due watches.
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Drives an [`Engine`].
#[derive(Clone)]
pub struct Scheduler {
    engine: Engine,
    poll: Duration,
}

impl Scheduler {
    /// A scheduler polling every [`POLL_INTERVAL`].
    pub fn new(engine: Engine) -> Scheduler {
        Scheduler {
            engine,
            poll: POLL_INTERVAL,
        }
    }

    /// The same with another poll interval.
    pub fn with_poll_interval(mut self, poll: Duration) -> Scheduler {
        self.poll = poll;
        self
    }

    /// One pass: runs every due check (at most `max_concurrent` at once) and flushes the digest when due.
    pub async fn tick(&self) -> Vec<CheckReport> {
        let due = self.engine.due_watches(self.engine.now());
        let reports = if due.is_empty() {
            Vec::new()
        } else {
            self.engine.check_many(due).await
        };
        // A failing digest must not stop the scheduler.
        let _ = self.engine.flush_digest().await;
        reports
    }

    /// Runs until `stop` becomes true (or its sender is dropped).
    pub async fn run(&self, mut stop: watch::Receiver<bool>) {
        loop {
            if *stop.borrow() {
                return;
            }
            self.tick().await;
            tokio::select! {
                _ = tokio::time::sleep(self.poll) => {}
                changed = stop.changed() => {
                    if changed.is_err() || *stop.borrow() {
                        return;
                    }
                }
            }
        }
    }
}
