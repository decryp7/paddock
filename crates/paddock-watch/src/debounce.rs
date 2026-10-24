use paddock_jobs::time::Clock;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What [`Debouncer::poll`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    /// No change is waiting.
    Idle,
    /// Changes are waiting; look again after this long (on the debouncer's clock).
    Waiting(Duration),
    /// The quiet time (or the longest wait) has passed: act now. The waiting change is cleared.
    Now,
}

#[derive(Clone, Copy)]
struct Pending {
    first: Duration,
    last: Duration,
}

/// Turns a storm of change events into one action. Due once the folder has been quiet for `quiet`, but never later than
/// `max_wait` after the first event of the burst, so a folder that never settles is still looked at. Pure: it does no
/// waiting; the watcher thread or a test calls [`poll`](Self::poll).
pub struct Debouncer {
    clock: Arc<dyn Clock>,
    quiet: Duration,
    max_wait: Duration,
    pending: Mutex<Option<Pending>>,
}

impl Debouncer {
    /// A debouncer with explicit timing (`max_wait` is raised to at least `quiet`).
    pub fn new(clock: Arc<dyn Clock>, quiet: Duration, max_wait: Duration) -> Self {
        Self {
            clock,
            quiet,
            max_wait: max_wait.max(quiet),
            pending: Mutex::new(None),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, Option<Pending>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A change happened (any number of times).
    pub fn note_change(&self) {
        let now = self.clock.monotonic();
        let mut p = self.state();
        *p = Some(match *p {
            Some(old) => Pending {
                first: old.first,
                last: now,
            },
            None => Pending {
                first: now,
                last: now,
            },
        });
    }

    /// Whether a change is waiting for its quiet time.
    pub fn has_pending(&self) -> bool {
        self.state().is_some()
    }

    /// Drops any waiting change.
    pub fn clear(&self) {
        *self.state() = None;
    }

    /// Whether to act now; clears the waiting change when it says [`Due::Now`].
    pub fn poll(&self) -> Due {
        let now = self.clock.monotonic();
        let mut p = self.state();
        let Some(pending) = *p else { return Due::Idle };
        let due = (pending.last + self.quiet).min(pending.first + self.max_wait);
        if now < due {
            return Due::Waiting(due - now);
        }
        *p = None;
        Due::Now
    }
}
