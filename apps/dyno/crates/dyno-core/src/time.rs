//! Time as values. The core never reads a clock: callers pass [`Timestamp`]s in.

use serde::{Deserialize, Serialize};

/// Milliseconds since the Unix epoch (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct Timestamp(pub i64);

/// One second in milliseconds.
pub const SECOND: i64 = 1_000;
/// One minute in milliseconds.
pub const MINUTE: i64 = 60 * SECOND;
/// One hour in milliseconds.
pub const HOUR: i64 = 60 * MINUTE;
/// One day in milliseconds.
pub const DAY: i64 = 24 * HOUR;

impl Timestamp {
    /// Milliseconds since the epoch.
    pub fn millis(self) -> i64 {
        self.0
    }

    /// This time plus `ms` milliseconds (saturating).
    pub fn plus(self, ms: i64) -> Timestamp {
        Timestamp(self.0.saturating_add(ms))
    }

    /// This time minus `ms` milliseconds (saturating).
    pub fn minus(self, ms: i64) -> Timestamp {
        Timestamp(self.0.saturating_sub(ms))
    }

    /// Milliseconds from `earlier` to `self` (negative when `earlier` is later).
    pub fn since(self, earlier: Timestamp) -> i64 {
        self.0.saturating_sub(earlier.0)
    }

    /// The start of the bucket of `width` milliseconds that contains this time (`width` ≤ 0 means 1).
    pub fn floor_to(self, width: i64) -> Timestamp {
        let w = width.max(1);
        Timestamp(self.0.div_euclid(w) * w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_saturates_and_floors() {
        let t = Timestamp(10_500);
        assert_eq!(t.plus(500), Timestamp(11_000));
        assert_eq!(t.minus(500).millis(), 10_000);
        assert_eq!(t.since(Timestamp(500)), 10_000);
        assert_eq!(Timestamp(i64::MAX).plus(1), Timestamp(i64::MAX));
        assert_eq!(Timestamp(i64::MIN).minus(1), Timestamp(i64::MIN));
        assert_eq!(t.floor_to(SECOND), Timestamp(10_000));
        assert_eq!(Timestamp(-1).floor_to(SECOND), Timestamp(-1_000));
        assert_eq!(t.floor_to(0), t);
        assert_eq!(DAY, 86_400_000);
        assert_eq!(HOUR / MINUTE, 60);
    }
}
