//! Wall-clock time for the notification centre (messages carry epoch milliseconds and are saved), injectable so every
//! rule that depends on time is tested without sleeping.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
}

/// The system's clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
}

/// A clock moved by hand, for tests.
#[derive(Debug, Default, Clone)]
pub struct ManualClock(Arc<AtomicI64>);

impl ManualClock {
    /// A clock at `ms`.
    pub fn at(ms: i64) -> ManualClock {
        ManualClock(Arc::new(AtomicI64::new(ms)))
    }

    /// Moves it forward by `ms`.
    pub fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clocks_tell_the_time() {
        assert!(SystemClock.now_ms() > 1_700_000_000_000, "after 2023");
        let c = ManualClock::at(5);
        c.clone().advance(10);
        assert_eq!(c.now_ms(), 15);
        assert_eq!(ManualClock::default().now_ms(), 0);
    }
}
