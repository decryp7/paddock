//! Test doubles for the cache: a clock the test moves (rule 60: tests control time).

use crate::disk::centre::Clock;
use std::sync::atomic::{AtomicI64, Ordering};

/// A clock that only moves when the test says.
pub struct FakeClock(AtomicI64);

impl FakeClock {
    /// A clock at `wall` milliseconds.
    pub fn new(wall: i64) -> FakeClock {
        FakeClock(AtomicI64::new(wall))
    }

    /// Moves it forward.
    pub fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.0.load(Ordering::SeqCst)).unwrap_or(0)
    }

    fn wall_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}
