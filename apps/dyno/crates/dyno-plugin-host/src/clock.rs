//! Time, injectable so backoff and quarantine windows are tested without waiting.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A monotonic clock.
pub trait Clock: Send + Sync {
    /// Now.
    fn now(&self) -> Instant;
}

/// The real clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// A clock that moves only when told to (tests).
#[derive(Debug)]
pub struct ManualClock {
    base: Instant,
    offset: Mutex<Duration>,
}

impl ManualClock {
    /// A clock standing at an arbitrary instant.
    pub fn new() -> Self {
        ManualClock {
            base: Instant::now(),
            offset: Mutex::new(Duration::ZERO),
        }
    }

    /// Moves it forward by `d`.
    pub fn advance(&self, d: Duration) {
        *self.offset.lock().unwrap_or_else(|e| e.into_inner()) += d;
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.base + *self.offset.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manual_clock_moves_only_when_told() {
        let c = ManualClock::default();
        let t = c.now();
        assert_eq!(c.now(), t);
        c.advance(Duration::from_secs(5));
        assert_eq!(c.now() - t, Duration::from_secs(5));
        assert!(SystemClock.now() >= t);
    }
}
