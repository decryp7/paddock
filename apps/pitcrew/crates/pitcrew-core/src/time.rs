//! Time as plain values, so every rule can be tested without a clock.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A point in time in milliseconds since the Unix epoch. The service reads the clock once per tick and passes it in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Timestamp(pub u64);

impl Timestamp {
    /// The time `d` later (saturating).
    pub fn plus(self, d: Duration) -> Self {
        Timestamp(self.0.saturating_add(duration_ms(d)))
    }

    /// The time elapsed since `earlier`; zero when `earlier` is in the future (clock adjustments never panic).
    pub fn since(self, earlier: Timestamp) -> Duration {
        Duration::from_millis(self.0.saturating_sub(earlier.0))
    }
}

/// A duration in whole milliseconds, saturating at `u64::MAX`.
pub fn duration_ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_saturates_instead_of_panicking() {
        assert_eq!(Timestamp(u64::MAX).plus(Duration::from_secs(1)), Timestamp(u64::MAX));
        assert_eq!(Timestamp(5).since(Timestamp(10)), Duration::ZERO);
        assert_eq!(Timestamp(10_000).since(Timestamp(4_000)), Duration::from_secs(6));
        assert_eq!(duration_ms(Duration::MAX), u64::MAX);
    }
}
