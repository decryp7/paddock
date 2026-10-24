use crate::time::{Clock, Deadline, SystemClock};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

const LIVE: u8 = 0;
const USER: u8 = 1;
const DEADLINE: u8 = 2;

/// Why work was asked to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CancelReason {
    /// The user (or the app, e.g. on quit) asked.
    User,
    /// The job's own deadline passed.
    Deadline,
}

impl fmt::Display for CancelReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            CancelReason::User => "stopped by request",
            CancelReason::Deadline => "time limit reached",
        })
    }
}

/// The error of [`CancelToken::check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled(pub CancelReason);

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for Cancelled {}

#[derive(Debug, Default)]
struct Inner {
    state: AtomicU8,
    deadline: Option<Deadline>,
}

/// A cooperative cancel flag shared by clones. Workers call [`check`](Self::check) between small units of work.
/// The first reason set wins; a token never un-cancels. An optional deadline cancels it with [`CancelReason::Deadline`].
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<Inner>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// A token that cancels itself `limit` from now on the system clock.
    pub fn with_deadline(limit: Duration) -> Self {
        Self::with_deadline_on(Arc::new(SystemClock), limit)
    }

    /// A token that cancels itself `limit` from now on `clock` (tests pass a [`FakeClock`](crate::time::FakeClock) and
    /// run the limit out with `advance`).
    pub fn with_deadline_on(clock: Arc<dyn Clock>, limit: Duration) -> Self {
        CancelToken(Arc::new(Inner {
            state: AtomicU8::new(LIVE),
            deadline: Some(Deadline::after(clock, limit)),
        }))
    }

    /// Requests a stop for `reason`; later calls keep the first reason.
    pub fn cancel_with(&self, reason: CancelReason) {
        let code = match reason {
            CancelReason::User => USER,
            CancelReason::Deadline => DEADLINE,
        };
        let _ = self
            .0
            .state
            .compare_exchange(LIVE, code, Ordering::SeqCst, Ordering::SeqCst);
    }

    /// Requests a stop on behalf of the user.
    pub fn cancel(&self) {
        self.cancel_with(CancelReason::User);
    }

    pub fn is_cancelled(&self) -> bool {
        self.reason().is_some()
    }

    /// The reason, once cancelled (or once the deadline passed).
    pub fn reason(&self) -> Option<CancelReason> {
        if self.0.state.load(Ordering::Relaxed) == LIVE
            && self.0.deadline.as_ref().is_some_and(Deadline::passed)
        {
            self.cancel_with(CancelReason::Deadline);
        }
        match self.0.state.load(Ordering::SeqCst) {
            USER => Some(CancelReason::User),
            DEADLINE => Some(CancelReason::Deadline),
            _ => None,
        }
    }

    /// `Err` once cancelled; use with `?` between units of work.
    pub fn check(&self) -> Result<(), Cancelled> {
        match self.reason() {
            Some(r) => Err(Cancelled(r)),
            None => Ok(()),
        }
    }
}
