//! Time as values. The core never reads a clock: callers pass [`Timestamp`]s in. It never reads the machine's time zone
//! either: local days, hours and minutes come through the [`LocalZone`] port (the OS adapter in `dyno-platform`, a
//! [`FixedZone`] in tests).

use serde::{Deserialize, Serialize};
use std::sync::Arc;

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

/// The user's local time zone as a port: the offset from UTC in force at an instant, daylight saving included. Every
/// local day, hour and minute the engine computes (the day an issue counts for, quiet hours, "until tomorrow",
/// maintenance windows, the slots a baseline learns, the local time in a backup's name) goes through it.
pub trait LocalZone: Send + Sync + std::fmt::Debug {
    /// The minutes to add to UTC to get the local wall time at `at` (+480 for UTC+8, -300 for New York in winter).
    fn offset_minutes(&self, at: Timestamp) -> i32;
}

/// A zone shared between threads.
pub type Zone = Arc<dyn LocalZone>;

/// The largest offset any zone uses, in minutes (UTC+14). Offsets outside are clamped, so a faulty adapter cannot move a
/// day by more than that.
pub const MAX_OFFSET_MINUTES: i32 = 14 * 60;

/// A zone with one offset all year (UTC, or a test's fixed zone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedZone(pub i32);

impl FixedZone {
    /// Coordinated Universal Time.
    pub const UTC: FixedZone = FixedZone(0);

    /// This zone, shared.
    pub fn shared(minutes: i32) -> Zone {
        Arc::new(FixedZone(minutes))
    }
}

impl LocalZone for FixedZone {
    fn offset_minutes(&self, _at: Timestamp) -> i32 {
        self.0
    }
}

fn offset_ms(zone: &dyn LocalZone, at: Timestamp) -> i64 {
    i64::from(zone.offset_minutes(at).clamp(-MAX_OFFSET_MINUTES, MAX_OFFSET_MINUTES)) * MINUTE
}

/// The local wall time at `at`, as milliseconds since the local epoch (1970-01-01 00:00 local).
pub fn to_local(at: Timestamp, zone: &dyn LocalZone) -> i64 {
    at.millis().saturating_add(offset_ms(zone, at))
}

/// The instant whose local wall time is `local` (milliseconds since the local epoch). A wall time that occurs twice
/// (clocks set back) gives one of the two; one that does not occur (clocks set forward) gives an instant at the jump.
pub fn from_local(local: i64, zone: &dyn LocalZone) -> Timestamp {
    // Guess with the offset at the wall time read as UTC, then correct once with the offset at the guess: exact for every
    // zone whose offset changes at most once within 14 hours.
    let guess = Timestamp(local.saturating_sub(offset_ms(zone, Timestamp(local))));
    Timestamp(local.saturating_sub(offset_ms(zone, guess)))
}

/// The start of the local day that contains `at` (never later than `at`).
pub fn day_start(at: Timestamp, zone: &dyn LocalZone) -> Timestamp {
    let midnight = to_local(at, zone).div_euclid(DAY) * DAY;
    from_local(midnight, zone).min(at)
}

/// The start of the next local day after `at` (always later than `at`; 23 or 25 hours on when the clocks change).
pub fn next_day_start(at: Timestamp, zone: &dyn LocalZone) -> Timestamp {
    let midnight = to_local(at, zone).div_euclid(DAY) * DAY;
    from_local(midnight.saturating_add(DAY), zone).max(at.plus(1))
}

/// The start of the local day before the one that contains `at`.
pub fn previous_day_start(at: Timestamp, zone: &dyn LocalZone) -> Timestamp {
    day_start(day_start(at, zone).minus(1), zone)
}

/// The minute of the local day at `at` (0 to 1439).
pub fn local_minute(at: Timestamp, zone: &dyn LocalZone) -> u16 {
    (to_local(at, zone).rem_euclid(DAY) / MINUTE) as u16
}

/// The local day at `at` as days since 1970-01-01 (local), for weekdays and dates.
pub fn local_epoch_day(at: Timestamp, zone: &dyn LocalZone) -> i64 {
    to_local(at, zone).div_euclid(DAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Central European Time with summer time in 2026: +60, +120 from 29 March 01:00 UTC to 25 October 01:00 UTC.
    #[derive(Debug)]
    struct Berlin2026;
    const SPRING: i64 = 1_774_746_000_000; // 2026-03-29T01:00:00Z
    const AUTUMN: i64 = 1_792_890_000_000; // 2026-10-25T01:00:00Z
    impl LocalZone for Berlin2026 {
        fn offset_minutes(&self, at: Timestamp) -> i32 {
            if (SPRING..AUTUMN).contains(&at.millis()) {
                120
            } else {
                60
            }
        }
    }

    #[test]
    fn local_days_follow_the_zone() {
        let utc8 = FixedZone(480);
        // 2026-10-29T23:30:00Z is 07:30 on 30 October in UTC+8: its day began at 16:00 UTC the day before.
        let t = Timestamp(1_793_316_600_000);
        assert_eq!(day_start(t, &utc8), Timestamp(1_793_289_600_000));
        assert_eq!(next_day_start(t, &utc8), Timestamp(1_793_289_600_000 + DAY));
        assert_eq!(previous_day_start(t, &utc8), Timestamp(1_793_289_600_000 - DAY));
        assert_eq!(local_minute(t, &utc8), 7 * 60 + 30);
        assert_eq!(day_start(t, &FixedZone::UTC), Timestamp(1_793_232_000_000));
        assert_eq!(local_epoch_day(t, &utc8), local_epoch_day(t, &FixedZone::UTC) + 1);
        assert_eq!(from_local(to_local(t, &utc8), &utc8), t);
        // A faulty adapter cannot shift a day by more than 14 hours.
        assert_eq!(to_local(t, &FixedZone(10_000)), t.millis() + 14 * HOUR);
        assert_eq!(FixedZone::shared(60).offset_minutes(t), 60);
    }

    #[test]
    fn days_are_23_and_25_hours_long_when_the_clocks_change() {
        let z = Berlin2026;
        // 29 March: midnight was +60 (23:00 UTC the day before), the next midnight +120 (22:00 UTC).
        let spring_day = Timestamp(SPRING + 5 * HOUR);
        let start = day_start(spring_day, &z);
        assert_eq!(start, Timestamp(SPRING - 2 * HOUR));
        assert_eq!(next_day_start(spring_day, &z).since(start), 23 * HOUR);
        // 25 October: 25 hours.
        let autumn_day = Timestamp(AUTUMN + 5 * HOUR);
        let start = day_start(autumn_day, &z);
        assert_eq!(start, Timestamp(AUTUMN - 3 * HOUR));
        assert_eq!(next_day_start(autumn_day, &z).since(start), 25 * HOUR);
        assert_eq!(previous_day_start(next_day_start(autumn_day, &z), &z), start);
        // Wall times on either side of the change.
        assert_eq!(local_minute(Timestamp(SPRING - 1), &z), 2 * 60 - 1);
        assert_eq!(local_minute(Timestamp(SPRING), &z), 3 * 60);
        assert_eq!(local_minute(Timestamp(AUTUMN), &z), 2 * 60);
        // A wall time in the spring gap maps to an instant at the jump; one in the autumn overlap to one of the two.
        let gap = to_local(Timestamp(SPRING - 1), &z) + 30 * MINUTE;
        assert!(from_local(gap, &z).since(Timestamp(SPRING)).abs() <= HOUR);
        let overlap = to_local(Timestamp(AUTUMN), &z) + 30 * MINUTE;
        let back = from_local(overlap, &z);
        assert_eq!(to_local(back, &z), overlap);
    }

    #[test]
    fn day_boundaries_hold_at_the_extremes() {
        for z in [FixedZone(-MAX_OFFSET_MINUTES), FixedZone::UTC, FixedZone(MAX_OFFSET_MINUTES)] {
            for t in [Timestamp(0), Timestamp(-1), Timestamp(1_793_316_600_000)] {
                let s = day_start(t, &z);
                assert!(s <= t && next_day_start(t, &z) > t, "{z:?} {t:?}");
                assert_eq!(local_minute(s, &z), 0);
            }
        }
    }

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
