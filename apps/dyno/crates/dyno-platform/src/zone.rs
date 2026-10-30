//! The user's local time zone from the OS: the [`LocalZone`] port for the agent's local days, quiet hours and
//! maintenance windows. Reads the system zone through `jiff` (the IANA database on macOS and Linux, the Windows zone
//! mapped to IANA with a bundled copy of the database), so daylight saving and a zone changed while the agent runs are
//! both followed. Nothing is written and nothing leaves the machine.

use dyno_core::time::{LocalZone, Timestamp};
use jiff::tz::TimeZone;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether a failed read of the OS zone was already logged (once per process: it is read at every lookup).
static FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);

/// The zone the OS is set to, read again at every lookup (`jiff` caches it and notices a change). When the OS zone cannot
/// be read, the lookup answers UTC, logs why once, and [`LocalZone::describe`] says so in the diagnostics bundle.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemZone;

impl SystemZone {
    fn read() -> Result<TimeZone, String> {
        TimeZone::try_system().map_err(|e| e.to_string())
    }
}

impl LocalZone for SystemZone {
    fn offset_minutes(&self, at: Timestamp) -> i32 {
        match SystemZone::read() {
            Ok(tz) => offset_in(&tz, at),
            Err(e) => {
                log_failure_once(&e);
                0
            }
        }
    }

    fn describe(&self) -> String {
        match SystemZone::read() {
            Ok(tz) => tz
                .iana_name()
                .map_or_else(|| "the OS zone (no IANA name)".to_string(), str::to_string),
            Err(e) => unreadable(&e),
        }
    }
}

fn unreadable(error: &str) -> String {
    format!("UTC: the OS time zone could not be read ({error})")
}

/// Logs a failed read the first time only; returns whether it logged.
fn log_failure_once(error: &str) -> bool {
    let first = !FAILURE_LOGGED.swap(true, Ordering::Relaxed);
    if first {
        eprintln!(
            "dyno: {}; local days, quiet hours and maintenance windows use UTC",
            unreadable(error)
        );
    }
    first
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

    fn describe(&self) -> String {
        self.0.iana_name().map_or_else(|| "a POSIX rule".to_string(), str::to_string)
    }
}

/// The offset of `tz` at `at` in minutes; UTC for an instant `jiff` cannot represent (beyond the year 9999).
fn offset_in(tz: &TimeZone, at: Timestamp) -> i32 {
    jiff::Timestamp::from_millisecond(at.millis()).map_or(0, |t| tz.to_offset(t).seconds() / 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::time::{day_start, local_epoch_day, local_minute, next_day_start, HOUR};

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

    /// Zones whose clocks change at midnight: the day the clocks go forward has no midnight (it starts at 01:00), and the
    /// day they go back in Havana and the Azores has two (it starts at the first).
    #[test]
    fn days_start_at_midnight_or_at_the_jump_over_it() {
        // 2026-01-01T00:00:00Z.
        const YEAR: i64 = 1_767_225_600_000;
        for (name, rule) in [
            ("Havana", "CST5CDT,M3.2.0/0,M11.1.0/1"),
            ("Santiago", "<-04>4<-03>,M9.1.6/24,M4.1.6/24"),
            ("Azores", "<-01>1<+00>,M3.5.0/0,M10.5.0/1"),
        ] {
            let z = TzZone::posix(rule).unwrap();
            let mut jumps = 0;
            for h in 0..(365 * 24) {
                let t = Timestamp(YEAR + h * HOUR);
                let start = day_start(t, &z);
                let next = next_day_start(t, &z);
                assert!(start <= t && t < next, "{name} {t:?}");
                assert_eq!(
                    local_epoch_day(start, &z),
                    local_epoch_day(t, &z),
                    "{name} {t:?}: the same local day"
                );
                assert!(
                    local_epoch_day(start.minus(1), &z) < local_epoch_day(start, &z),
                    "{name} {t:?}: the first instant"
                );
                assert!((23 * HOUR..=25 * HOUR).contains(&next.since(start)), "{name} {t:?}");
                if local_minute(start, &z) != 0 {
                    // Only on the day the clocks jumped from 00:00 to 01:00.
                    assert_eq!(local_minute(start, &z), 60, "{name} {t:?}");
                    assert_ne!(z.offset_minutes(start.minus(1)), z.offset_minutes(start), "{name} {t:?}");
                    jumps += 1;
                }
            }
            assert_eq!(jumps, 23, "{name}: the 23 hours of the one day a year without a midnight");
        }
    }

    #[test]
    fn a_failed_read_is_logged_once_and_described() {
        assert!(unreadable("no zone").contains("could not be read (no zone)"));
        FAILURE_LOGGED.store(false, Ordering::Relaxed);
        assert!(log_failure_once("no zone"));
        assert!(!log_failure_once("no zone"), "only once");
        assert_eq!(TzZone::posix("<+08>-8").unwrap().describe(), "a POSIX rule");
        assert!(!SystemZone.describe().is_empty());
    }

    #[test]
    fn the_system_zone_answers_a_real_offset() {
        // Whatever zone the machine running the test is in: a whole number of quarter hours within UTC±14.
        let m = SystemZone.offset_minutes(Timestamp(SPRING));
        assert!((-14 * 60..=14 * 60).contains(&m) && m % 15 == 0, "{m}");
    }
}
