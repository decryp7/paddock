use chrono::{DateTime, Duration, Utc};
use std::sync::Mutex;

/// The time source of the log, injected so rotation, the daily budget and de-duplication are tested without sleeping.
pub trait Clock: Send + Sync + 'static {
    /// The current time (UTC).
    fn now(&self) -> DateTime<Utc>;
}

/// The system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// A clock that only moves when told to, for tests.
#[derive(Debug)]
pub struct ManualClock(Mutex<DateTime<Utc>>);

impl ManualClock {
    /// A clock standing at `start`.
    pub fn new(start: DateTime<Utc>) -> Self {
        ManualClock(Mutex::new(start))
    }

    /// Moves the clock forward (or back, with a negative duration).
    pub fn advance(&self, by: Duration) {
        let mut t = crate::error::lock(&self.0);
        *t += by;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *crate::error::lock(&self.0)
    }
}
