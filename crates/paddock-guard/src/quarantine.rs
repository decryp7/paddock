use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use paddock_env::{Problem, VersionedFile};
use paddock_jobs::time::Clock;
use serde::{Deserialize, Serialize};

use crate::LOG_TARGET;

/// Longest `last_error` kept, in characters.
pub const MAX_ERROR_CHARS: usize = 500;

/// Format version of the state file.
const STATE_VERSION: u32 = 1;

/// When an owner stops being called: `max` failures within `window`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuarantinePolicy {
    /// Failures within the window that start a quarantine (at least 1).
    pub max: u32,
    /// How far back failures count.
    pub window: Duration,
}

impl Default for QuarantinePolicy {
    fn default() -> Self {
        Self {
            max: 3,
            window: Duration::from_secs(600),
        }
    }
}

/// What went wrong with a call; each counts towards quarantine. A typed error the owner returns on purpose is none of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Failure {
    /// A panic, or the owner's process died.
    Panic,
    /// A call overran its time budget.
    Hang,
    /// A reply that is oversized or cannot be decoded.
    Garbled,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Failure::Panic => "crashed",
            Failure::Hang => "overran its time budget",
            Failure::Garbled => "sent a garbled reply",
        })
    }
}

/// Why an owner is not called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Quarantined after repeated failures; [`Quarantine::release`] lifts it.
    Quarantined {
        /// The owner.
        owner: String,
    },
    /// A call is running past its budget; calls fail fast until it returns.
    NotResponding {
        /// The owner.
        owner: String,
        /// The last error recorded for it.
        why: String,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Quarantined { owner } => {
                write!(f, "\"{owner}\" is disabled after repeated failures")
            }
            Refusal::NotResponding { owner, why } => {
                write!(f, "\"{owner}\" is not responding: {why}")
            }
        }
    }
}

impl std::error::Error for Refusal {}

/// The health of one owner.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerStatus {
    /// Failures within the policy window now.
    pub failures: u32,
    /// All failures since the owner was last released.
    pub total: u64,
    /// The last failure's text, cut to [`MAX_ERROR_CHARS`].
    pub last_error: Option<String>,
    /// Calls are refused.
    pub quarantined: bool,
    /// A call is running past its time budget right now.
    pub hung: bool,
}

/// A quarantine started or was lifted; given to the observer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The owner.
    pub owner: String,
    /// True when the quarantine started, false when it was lifted.
    pub quarantined: bool,
}

type Observer = Arc<dyn Fn(&Change) + Send + Sync>;

#[derive(Default)]
struct Entry {
    /// When the failures in the window happened (clock milliseconds), oldest first.
    recent: VecDeque<u64>,
    total: u64,
    last_error: Option<String>,
    quarantined: bool,
    /// Calls past their budget that have not returned.
    hung: u32,
}

/// The quarantined owners as kept in the state file; failure times do not survive a restart (the clock does not).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct State {
    quarantined: BTreeMap<String, Saved>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Saved {
    total: u64,
    last_error: Option<String>,
}

/// Failure counts per owner, shared by every guarded adapter; decides who is no longer called.
///
/// Time comes from the injected clock (`now_ms`), so tests drive a `FakeClock`. A lock poisoned by a panicking observer or
/// thread is recovered, never propagated.
pub struct Quarantine {
    policy: QuarantinePolicy,
    clock: Arc<dyn Clock>,
    owners: Mutex<BTreeMap<String, Entry>>,
    observer: Mutex<Option<Observer>>,
    file: Option<VersionedFile<State>>,
    /// Serialises snapshot-and-write so an older snapshot never lands after a newer one.
    save: Mutex<()>,
}

impl fmt::Debug for Quarantine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Quarantine")
            .field("policy", &self.policy)
            .field("owners", &self.all())
            .finish()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Quarantine {
    /// A quarantine kept in memory only.
    pub fn new(policy: QuarantinePolicy, clock: Arc<dyn Clock>) -> Self {
        Self {
            policy: QuarantinePolicy {
                max: policy.max.max(1),
                ..policy
            },
            clock,
            owners: Mutex::new(BTreeMap::new()),
            observer: Mutex::new(None),
            file: None,
            save: Mutex::new(()),
        }
    }

    /// A quarantine whose quarantined owners are kept in the versioned state file at `path`. A missing, damaged or newer
    /// file gives an empty quarantine; the [`Problem`] (if any) is returned for the app to report once.
    pub fn persistent(
        policy: QuarantinePolicy,
        clock: Arc<dyn Clock>,
        path: impl Into<PathBuf>,
    ) -> (Self, Option<Problem>) {
        let file = VersionedFile::<State>::new(path, STATE_VERSION);
        let loaded = file.load();
        let mut q = Self::new(policy, clock);
        *lock(&q.owners) = loaded
            .value
            .quarantined
            .into_iter()
            .map(|(owner, saved)| {
                let entry = Entry {
                    total: saved.total,
                    last_error: saved.last_error,
                    quarantined: true,
                    ..Entry::default()
                };
                (owner, entry)
            })
            .collect();
        q.file = Some(file);
        (q, loaded.problem)
    }

    /// The policy in force.
    pub fn policy(&self) -> QuarantinePolicy {
        self.policy
    }

    /// Calls `observer` (outside every lock) when a quarantine starts or is lifted; replaces the previous observer.
    pub fn set_observer(&self, observer: impl Fn(&Change) + Send + Sync + 'static) {
        *lock(&self.observer) = Some(Arc::new(observer));
    }

    /// Counts a failure of `owner` (`error` is kept as its last error). Returns true when it starts the quarantine.
    pub fn record(&self, owner: &str, failure: Failure, error: &str) -> bool {
        let now = self.clock.now_ms();
        let window = u64::try_from(self.policy.window.as_millis()).unwrap_or(u64::MAX);
        let started = {
            let mut owners = lock(&self.owners);
            let e = owners.entry(owner.to_string()).or_default();
            while e
                .recent
                .front()
                .is_some_and(|t| now.saturating_sub(*t) >= window)
            {
                e.recent.pop_front();
            }
            e.recent.push_back(now);
            e.total += 1;
            e.last_error = Some(error.chars().take(MAX_ERROR_CHARS).collect());
            if failure == Failure::Hang {
                e.hung += 1;
            }
            let started = !e.quarantined && e.recent.len() >= self.policy.max as usize;
            e.quarantined |= started;
            started
        };
        log::error!(target: LOG_TARGET, "{owner} {} (contained): {error}", failure);
        if started {
            log::error!(target: LOG_TARGET, "{owner} quarantined after {} failures", self.policy.max);
            self.persist();
            self.notify(owner, true);
        }
        started
    }

    /// A call that overran its budget has returned (or was abandoned and finished): the owner is responsive again.
    pub fn hang_ended(&self, owner: &str) {
        if let Some(e) = lock(&self.owners).get_mut(owner) {
            e.hung = e.hung.saturating_sub(1);
        }
    }

    /// Whether calls to `owner` are refused.
    pub fn is_quarantined(&self, owner: &str) -> bool {
        lock(&self.owners).get(owner).is_some_and(|e| e.quarantined)
    }

    /// Whether `owner` may be called now.
    pub fn admit(&self, owner: &str) -> Result<(), Refusal> {
        match lock(&self.owners).get(owner) {
            Some(e) if e.quarantined => Err(Refusal::Quarantined {
                owner: owner.to_string(),
            }),
            Some(e) if e.hung > 0 => Err(Refusal::NotResponding {
                owner: owner.to_string(),
                why: e.last_error.clone().unwrap_or_default(),
            }),
            _ => Ok(()),
        }
    }

    /// Forgets an owner's failures and lifts its quarantine and hung state (the user re-enabled it).
    pub fn release(&self, owner: &str) {
        let was = lock(&self.owners)
            .remove(owner)
            .is_some_and(|e| e.quarantined);
        if was {
            self.persist();
            self.notify(owner, false);
        }
    }

    /// The health of `owner` (all zero for one never seen).
    pub fn status(&self, owner: &str) -> OwnerStatus {
        let now = self.clock.now_ms();
        lock(&self.owners)
            .get(owner)
            .map(|e| self.status_of(e, now))
            .unwrap_or_default()
    }

    /// The health of every owner with a recorded failure, by owner.
    pub fn all(&self) -> BTreeMap<String, OwnerStatus> {
        let now = self.clock.now_ms();
        lock(&self.owners)
            .iter()
            .map(|(k, e)| (k.clone(), self.status_of(e, now)))
            .collect()
    }

    fn status_of(&self, e: &Entry, now: u64) -> OwnerStatus {
        let window = u64::try_from(self.policy.window.as_millis()).unwrap_or(u64::MAX);
        let failures = e
            .recent
            .iter()
            .filter(|t| now.saturating_sub(**t) < window)
            .count();
        OwnerStatus {
            failures: u32::try_from(failures).unwrap_or(u32::MAX),
            total: e.total,
            last_error: e.last_error.clone(),
            quarantined: e.quarantined,
            hung: e.hung > 0,
        }
    }

    fn notify(&self, owner: &str, quarantined: bool) {
        let observer = lock(&self.observer).clone();
        if let Some(o) = observer {
            o(&Change {
                owner: owner.to_string(),
                quarantined,
            });
        }
    }

    /// Writes the quarantined owners; best effort (the quarantine is in force in memory either way).
    fn persist(&self) {
        let Some(file) = &self.file else { return };
        let _one_at_a_time = lock(&self.save);
        let state = State {
            quarantined: lock(&self.owners)
                .iter()
                .filter(|(_, e)| e.quarantined)
                .map(|(k, e)| {
                    let saved = Saved {
                        total: e.total,
                        last_error: e.last_error.clone(),
                    };
                    (k.clone(), saved)
                })
                .collect(),
        };
        if let Err(e) = file.save(&state) {
            log::warn!(target: LOG_TARGET, "could not save the quarantine: {e}");
        }
    }
}

#[cfg(test)]
mod tests;
