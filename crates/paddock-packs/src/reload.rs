use crate::{PackKind, PackStore};
use paddock_jobs::time::Clock;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What [`Reloader::poll`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polled {
    /// No change is waiting.
    Idle,
    /// Changes are waiting; look again after this long (on the reloader's clock).
    Waiting(Duration),
    /// The packs were read again; `true` when the set really changed.
    Reloaded(bool),
}

#[derive(Clone, Copy)]
struct Pending {
    first: Duration,
    last: Duration,
}

/// Hot reload without a restart: change events are debounced on a [`Clock`], then the store reloads once. A reload
/// waits until the folder has been quiet for `debounce`, but never longer than `max_wait` after the first event, so a
/// storm of events is one reload and a folder that never settles is still read. Pure: it does no waiting of its own;
/// [`crate::watch::spawn`] drives it from a thread and tests drive it by moving a fake clock.
pub struct Reloader<K: PackKind> {
    store: Arc<PackStore<K>>,
    clock: Arc<dyn Clock>,
    debounce: Duration,
    max_wait: Duration,
    pending: Mutex<Option<Pending>>,
}

impl<K: PackKind> Reloader<K> {
    /// Default quiet time before a reload.
    pub const DEBOUNCE: Duration = Duration::from_millis(300);
    /// Default longest wait after the first event of a burst.
    pub const MAX_WAIT: Duration = Duration::from_millis(1500);

    /// A reloader for `store` with the default timing.
    pub fn new(store: Arc<PackStore<K>>, clock: Arc<dyn Clock>) -> Self {
        Self::with_timing(store, clock, Self::DEBOUNCE, Self::MAX_WAIT)
    }

    /// A reloader with explicit timing (`max_wait` is raised to at least `debounce`).
    pub fn with_timing(
        store: Arc<PackStore<K>>,
        clock: Arc<dyn Clock>,
        debounce: Duration,
        max_wait: Duration,
    ) -> Self {
        Self {
            store,
            clock,
            debounce,
            max_wait: max_wait.max(debounce),
            pending: Mutex::new(None),
        }
    }

    /// The store this reloads.
    pub fn store(&self) -> &Arc<PackStore<K>> {
        &self.store
    }

    /// A change happened in the folder (any number of times).
    pub fn note_change(&self) {
        let now = self.clock.monotonic();
        let mut p = self.pending.lock().unwrap_or_else(|e| e.into_inner());
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

    /// Reloads if the quiet time (or the maximum wait) has passed, otherwise says how long to wait.
    pub fn poll(&self) -> Polled {
        let now = self.clock.monotonic();
        {
            let mut p = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            let Some(pending) = *p else {
                return Polled::Idle;
            };
            let due = (pending.last + self.debounce).min(pending.first + self.max_wait);
            if now < due {
                return Polled::Waiting(due - now);
            }
            *p = None;
        }
        Polled::Reloaded(self.store.reload())
    }

    /// Whether a change is waiting for its quiet time.
    pub fn has_pending(&self) -> bool {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// Reloads now, dropping any waiting change (start-up, a "Reload" button).
    pub fn reload_now(&self) -> bool {
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.store.reload()
    }
}
