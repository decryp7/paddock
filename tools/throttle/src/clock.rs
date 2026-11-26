//! The clock port: wall time and sleeping. The real clock sleeps; the fake one advances instead, so waits take no time.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Wall-clock time and sleeping.
pub trait Clock: Send + Sync {
    /// Microseconds since the Unix epoch.
    fn now_micros(&self) -> u64;
    /// Waits `d` (the fake clock advances by `d`).
    fn sleep(&self, d: Duration);
    /// Whole seconds since the Unix epoch.
    fn now_secs(&self) -> u64 {
        self.now_micros() / 1_000_000
    }
}

/// The system clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_micros(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// A clock that only moves when told to or when something sleeps.
#[derive(Debug)]
pub struct FakeClock(AtomicU64);

impl FakeClock {
    /// A fake clock at `secs` since the epoch.
    pub fn at_secs(secs: u64) -> FakeClock {
        FakeClock(AtomicU64::new(secs * 1_000_000))
    }
    /// Moves time forward.
    pub fn advance(&self, d: Duration) {
        self.0.fetch_add(u64::try_from(d.as_micros()).unwrap_or(0), Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now_micros(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
    fn sleep(&self, d: Duration) {
        self.advance(d);
    }
}
