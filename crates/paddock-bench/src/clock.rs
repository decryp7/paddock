use std::cell::Cell;
use std::time::{Duration, Instant};

/// A monotonic clock, passed in so the harness is tested without waiting.
pub trait Clock {
    /// Time since an arbitrary fixed start; never goes backwards.
    fn now(&self) -> Duration;
}

/// The real clock.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock(Instant);

impl SystemClock {
    /// Starts counting now.
    pub fn new() -> SystemClock {
        SystemClock(Instant::now())
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        SystemClock::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}

/// A clock that moves only when told to (tests of the harness and of an app's benchmark logic).
#[derive(Debug, Default)]
pub struct ManualClock(Cell<Duration>);

impl ManualClock {
    /// A clock at zero.
    pub fn new() -> ManualClock {
        ManualClock::default()
    }

    /// Moves the clock forward.
    pub fn advance(&self, by: Duration) {
        self.0.set(self.0.get() + by);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        self.0.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_moves_only_when_advanced() {
        let c = ManualClock::new();
        assert_eq!(c.now(), Duration::ZERO);
        c.advance(Duration::from_millis(250));
        c.advance(Duration::from_millis(250));
        assert_eq!(c.now(), Duration::from_millis(500));
    }

    #[test]
    fn system_clock_never_goes_backwards() {
        let c = SystemClock::default();
        let a = c.now();
        assert!(c.now() >= a);
    }
}
