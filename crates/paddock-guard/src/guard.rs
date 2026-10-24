use std::fmt;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use paddock_jobs::CancelToken;
use paddock_jobs::time::Clock;

use crate::quarantine::{Failure, Quarantine, Refusal};
use crate::{Crashed, catch};

/// Why a guarded call did not give the function's own result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError<E> {
    /// The owner is quarantined or not responding; the function was not run.
    Refused(Refusal),
    /// The function panicked (counted towards quarantine).
    Crashed(Crashed),
    /// The function did not finish within its budget; it was abandoned and the owner is marked hung (counted).
    Hung {
        /// What overran: `owner (kind)`.
        what: String,
        /// The budget it overran.
        budget: Duration,
    },
    /// A helper thread could not be started, or ended without a result. Not counted.
    Unavailable(String),
    /// The function's own typed error: the owner behaved, so it is not counted.
    Failed(E),
}

impl<E: fmt::Display> fmt::Display for CallError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallError::Refused(r) => r.fmt(f),
            CallError::Crashed(c) => c.fmt(f),
            CallError::Hung { what, budget } => write!(
                f,
                "{what} did not finish within {budget:?}; the call was abandoned"
            ),
            CallError::Unavailable(why) => f.write_str(why),
            CallError::Failed(e) => e.fmt(f),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for CallError<E> {}

/// What a guarded function came to.
type Outcome<T, E> = Result<T, CallError<E>>;

enum Msg<T, E> {
    Done(Outcome<T, E>),
    Timeout,
    /// Every sender is gone without a result.
    Gone,
}

/// Progress of a detached call, shared by the caller and the helper thread.
#[derive(Debug, PartialEq, Eq)]
enum Phase {
    Running,
    Abandoned,
    Finished,
}

/// The state of a detached call when its budget ran out.
enum Settled<T, E> {
    /// The function finished just before the budget ran out; this is its result.
    Finished(Outcome<T, E>),
    /// Still running: now abandoned.
    Abandoned,
    /// Finished, but the result is gone (cannot happen unless the helper broke its protocol).
    Lost,
}

/// The building block of guarded adapters: runs calls to an owner (a plugin, a worker) with its [`Quarantine`] in front.
///
/// A call is refused while the owner is quarantined or hung; a panic becomes [`CallError::Crashed`]; a call past its budget
/// is abandoned ([`CallError::Hung`]); each of those is recorded, and the owner is quarantined by the policy. The function's
/// own typed error ([`CallError::Failed`]) passes through uncounted. Replies that cannot be decoded are the adapter's to
/// report: [`Quarantine::record`] with [`Failure::Garbled`].
#[derive(Clone)]
pub struct Guard {
    quarantine: Arc<Quarantine>,
    clock: Arc<dyn Clock>,
}

impl Guard {
    /// A guard recording into `quarantine`, measuring budgets on `clock`.
    pub fn new(quarantine: Arc<Quarantine>, clock: Arc<dyn Clock>) -> Self {
        Self { quarantine, clock }
    }

    /// The quarantine failures are recorded into.
    pub fn quarantine(&self) -> &Arc<Quarantine> {
        &self.quarantine
    }

    /// Runs `f` on this thread: refused when the owner is quarantined or hung, panics caught and counted. There is no budget
    /// (a running call cannot be stopped from here); `f` may borrow.
    pub fn call_here<T, E>(
        &self,
        owner: &str,
        kind: &str,
        f: impl FnOnce() -> Result<T, E>,
    ) -> Outcome<T, E> {
        self.quarantine.admit(owner).map_err(CallError::Refused)?;
        self.run(owner, kind, f)
    }

    /// Runs `f` of `kind` for `owner`. Without a `budget` it runs on this thread like [`call_here`](Self::call_here). With one
    /// it runs on a helper thread and the caller stops waiting once the budget has passed on the clock: the owner is marked
    /// hung (a counted failure), further calls are refused until the abandoned call returns (its result is dropped).
    pub fn call<T, E>(
        &self,
        owner: &str,
        kind: &str,
        budget: Option<Duration>,
        f: impl FnOnce() -> Result<T, E> + Send + 'static,
    ) -> Outcome<T, E>
    where
        T: Send + 'static,
        E: Send + 'static,
    {
        let Some(budget) = budget else {
            return self.call_here(owner, kind, f);
        };
        self.quarantine.admit(owner).map_err(CallError::Refused)?;
        let (tx, rx) = mpsc::channel::<Msg<T, E>>();
        let phase = Arc::new(Mutex::new(Phase::Running));
        let (guard, owner_owned, kind_owned, shared, done) = (
            self.clone(),
            owner.to_string(),
            kind.to_string(),
            phase.clone(),
            tx.clone(),
        );
        let what = format!("{owner} ({kind})");
        spawn("paddock-guard-worker", move || {
            let outcome = guard.run(&owner_owned, &kind_owned, f);
            let mut p = shared.lock().unwrap_or_else(PoisonError::into_inner);
            if *p == Phase::Abandoned {
                guard.quarantine.hang_ended(&owner_owned);
            }
            *p = Phase::Finished;
            // Sent under the lock, so a caller that sees `Finished` finds the result queued.
            let _ = done.send(Msg::Done(outcome));
        })
        .map_err(|e| CallError::Unavailable(unavailable(&what, &e)))?;
        let cancel = CancelToken::new();
        let (clock, token) = (self.clock.clone(), cancel.clone());
        // Without a timer thread (it could not start) the call simply runs unbudgeted.
        let _timer = spawn("paddock-guard-timer", move || {
            if clock.wait(budget, &token).is_ok() {
                let _ = tx.send(Msg::Timeout);
            }
        });
        let abandon = || {
            let message =
                format!("{what} did not finish within {budget:?}; the call was abandoned");
            self.quarantine.record(owner, Failure::Hang, &message);
        };
        let first = rx.recv().unwrap_or(Msg::Gone);
        let result = resolve(first, &phase, &rx, abandon, &what, budget);
        cancel.cancel();
        result
    }

    /// Runs `f` catching a panic and recording it.
    fn run<T, E>(
        &self,
        owner: &str,
        kind: &str,
        f: impl FnOnce() -> Result<T, E>,
    ) -> Outcome<T, E> {
        match catch(&format!("{owner} ({kind})"), f) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(e)) => Err(CallError::Failed(e)),
            Err(crash) => {
                self.quarantine
                    .record(owner, Failure::Panic, &crash.to_string());
                Err(CallError::Crashed(crash))
            }
        }
    }
}

/// The outcome of a call from the first message it produced.
fn resolve<T, E>(
    first: Msg<T, E>,
    phase: &Mutex<Phase>,
    rx: &Receiver<Msg<T, E>>,
    abandon: impl FnOnce(),
    what: &str,
    budget: Duration,
) -> Outcome<T, E> {
    match first {
        Msg::Done(outcome) => outcome,
        Msg::Timeout => match settle_timeout(phase, rx, abandon) {
            Settled::Finished(outcome) => outcome,
            Settled::Abandoned => Err(CallError::Hung {
                what: what.to_string(),
                budget,
            }),
            Settled::Lost => Err(CallError::Unavailable(lost(what))),
        },
        Msg::Gone => Err(CallError::Unavailable(lost(what))),
    }
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(f)
        .map(|_| ())
}

fn unavailable(what: &str, e: &std::io::Error) -> String {
    format!("{what}: could not start a helper thread: {e}")
}

fn lost(what: &str) -> String {
    format!("{what} ended without a result")
}

/// The budget ran out: take the result if the function just finished, else abandon it (`abandon` runs under the lock the
/// helper takes to finish, so its `hang_ended` never comes before the hang is counted).
fn settle_timeout<T, E>(
    phase: &Mutex<Phase>,
    rx: &Receiver<Msg<T, E>>,
    abandon: impl FnOnce(),
) -> Settled<T, E> {
    let mut p = phase.lock().unwrap_or_else(PoisonError::into_inner);
    if *p == Phase::Finished {
        return match rx
            .try_iter()
            .find_map(|m| if let Msg::Done(o) = m { Some(o) } else { None })
        {
            Some(outcome) => Settled::Finished(outcome),
            None => Settled::Lost,
        };
    }
    *p = Phase::Abandoned;
    abandon();
    Settled::Abandoned
}

#[cfg(test)]
mod tests;
