//! The user's local time zone from the OS: the [`LocalZone`] port for the agent's local days, quiet hours and
//! maintenance windows. Reads the system zone through `jiff` (the IANA database on macOS and Linux, the Windows zone
//! mapped to IANA with a bundled copy of the database), so daylight saving and a zone changed while the agent runs are
//! both followed. Nothing is written and nothing leaves the machine.

use dyno_core::time::{LocalZone, Timestamp};
use jiff::tz::TimeZone;

/// The zone the OS is set to, read again at every lookup (`jiff` caches it and notices a change).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemZone;

impl LocalZone for SystemZone {
    fn offset_minutes(&self, at: Timestamp) -> i32 {
        offset_in(&TimeZone::system(), at)
    }
}

/// One fixed `jiff` zone (tests use POSIX rules such as `CET-1CEST,M3.5.0,M10.5.0/3`, which need no database).
#[derive(Debug, Clone)]
pub struct TzZone(pub TimeZone);

impl TzZone {
    /// The zone for a POSIX TZ rule, if it parses.
    pub fn posix(rule: &str) -> Option<TzZone> {
        TimeZone::posix(rule).ok().map(TzZone)
    }
}

impl LocalZone for TzZone {
    fn offset_minutes(&self, at: Timestamp) -> i32 {
        offset_in(&self.0, at)
    }
}

/// The offset of `tz` at `at` in minutes; UTC for an instant `jiff` cannot represent (beyond the year 9999).
fn offset_in(tz: &TimeZone, at: Timestamp) -> i32 {
    jiff::Timestamp::from_millisecond(at.millis()).map_or(0, |t| tz.to_offset(t).seconds() / 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::time::{day_start, next_day_start, HOUR};

    // 2026-03-29T01:00:00Z and 2026-10-25T01:00:00Z: the European clock changes.
    const SPRING: i64 = 1_774_746_000_000;
    const AUTUMN: i64 = 1_792_890_000_000;

    #[test]
    fn daylight_saving_moves_the_offset_and_the_day_length() {
        let berlin = TzZone::posix("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        assert_eq!(berlin.offset_minutes(Timestamp(SPRING - 1)), 60);
        assert_eq!(berlin.offset_minutes(Timestamp(SPRING)), 120);
        assert_eq!(berlin.offset_minutes(Timestamp(AUTUMN - 1)), 120);
        assert_eq!(berlin.offset_minutes(Timestamp(AUTUMN)), 60);
        let spring_day = Timestamp(SPRING + 5 * HOUR);
        assert_eq!(next_day_start(spring_day, &berlin).since(day_start(spring_day, &berlin)), 23 * HOUR);
        let autumn_day = Timestamp(AUTUMN + 5 * HOUR);
        assert_eq!(next_day_start(autumn_day, &berlin).since(day_start(autumn_day, &berlin)), 25 * HOUR);
        let singapore = TzZone::posix("<+08>-8").unwrap();
        assert_eq!(singapore.offset_minutes(Timestamp(SPRING)), 480);
        assert!(TzZone::posix("not a rule").is_none());
        // Beyond what jiff represents: UTC rather than a panic.
        assert_eq!(berlin.offset_minutes(Timestamp(i64::MAX)), 0);
    }

    #[test]
    fn the_system_zone_answers_a_real_offset() {
        // Whatever zone the machine running the test is in: a whole number of quarter hours within UTC±14.
        let m = SystemZone.offset_minutes(Timestamp(SPRING));
        assert!((-14 * 60..=14 * 60).contains(&m) && m % 15 == 0, "{m}");
    }
}
