use paddock_jobs::time::Clock;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a watch may stay silent before it looks at the folder anyway (one hour, Syncthing's default full-scan
/// interval beside its watcher). A backstop: notify 9 reports the drops it can see as a rescan (an inotify queue overflow,
/// an FSEvents dropped-events notice, a Windows `ReadDirectoryChangesW` overflow), but notify 8 lost the last two
/// silently, and a watch that ended after an error is not started again here. This bounds how stale a missed change can
/// stay.
pub const DEFAULT_RESYNC: Duration = Duration::from_secs(60 * 60);

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
    resync: Option<Duration>,
    pending: Mutex<Option<Pending>>,
    /// When the action last ran (or the debouncer was made), on the clock.
    last_due: Mutex<Duration>,
}

impl Debouncer {
    /// A debouncer with explicit timing (`max_wait` is raised to at least `quiet`) and the [`DEFAULT_RESYNC`] time.
    pub fn new(clock: Arc<dyn Clock>, quiet: Duration, max_wait: Duration) -> Self {
        let now = clock.monotonic();
        Self {
            clock,
            quiet,
            max_wait: max_wait.max(quiet),
            resync: Some(DEFAULT_RESYNC),
            pending: Mutex::new(None),
            last_due: Mutex::new(now),
        }
    }

    /// The same debouncer with another resync time: how long the watch may stay silent before
    /// [`resync_due`](Self::resync_due) says to look anyway. `None` never resyncs (only for a source known never to drop
    /// events, such as a test's fake).
    #[must_use]
    pub fn with_resync(mut self, every: Option<Duration>) -> Self {
        self.resync = every;
        self
    }

    fn last_due(&self) -> std::sync::MutexGuard<'_, Duration> {
        self.last_due.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether the resync time has passed since the action last ran, with no change waiting; when it says `true` it
    /// counts as a run, so the next resync is a full resync time later. The watcher thread asks this after
    /// [`poll`](Self::poll) and runs the action, so a change the OS never reported is found within the resync time.
    pub fn resync_due(&self) -> bool {
        let Some(every) = self.resync else {
            return false;
        };
        if self.has_pending() {
            return false;
        }
        let now = self.clock.monotonic();
        let mut last = self.last_due();
        if now.saturating_sub(*last) < every {
            return false;
        }
        *last = now;
        true
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
        drop(p);
        *self.last_due() = now;
        Due::Now
    }
}
