//! The folder watcher that keeps the index fresh.

use crate::IndexService;
use paddock_jobs::time::Clock;
use paddock_watch::{ChangeSource, Debouncer, WatchError, WatchHandle, spawn};
use std::sync::Arc;
use std::time::Duration;

/// How long the repository must be quiet before a run starts, and the longest a burst of changes can delay it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchTiming {
    /// Quiet time after the last change.
    pub quiet: Duration,
    /// Longest wait after the first change of a burst.
    pub max_wait: Duration,
}

impl Default for WatchTiming {
    fn default() -> Self {
        WatchTiming {
            quiet: Duration::from_secs(1),
            max_wait: Duration::from_secs(5),
        }
    }
}

impl IndexService {
    /// Re-runs the index when the repository changes: a burst of changes becomes one run after `timing.quiet`.
    ///
    /// The run happens on the watcher's thread and is waited for, so two runs never overlap; changes that arrive meanwhile
    /// are debounced into the next run (the plan sees the folder as it is then, so none is lost). The source reports that
    /// something changed, not what, so every change starts a run; the shortcut (ADR 0006) makes a run over an unchanged
    /// tree cheap. Dropping the handle stops the watch; a run in flight finishes.
    pub fn watch(&self, source: &dyn ChangeSource, clock: Arc<dyn Clock>, timing: WatchTiming) -> Result<WatchHandle, WatchError> {
        let debouncer = Arc::new(Debouncer::new(clock, timing.quiet, timing.max_wait));
        let service = IndexService {
            registry: self.registry.clone(),
            pool: self.pool.clone(),
            setup: self.setup.clone(),
        };
        spawn(debouncer, source, &self.setup.root, true, move || {
            if let Ok(job) = service.start() {
                let _ = job.join();
            }
        })
    }
}
