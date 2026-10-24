//! Local time of day for quiet hours. Taken from Spotter's `ClockTime` and `QuietHours` (`spotter-core/src/time.rs`), plus
//! the [`LocalTime`] port that turns the clock's epoch milliseconds into the user's local time, so quiet hours across
//! midnight are tested with a fixed offset instead of the machine's time zone.

use serde::{Deserialize, Serialize};

const MINUTES_PER_DAY: u16 = 1440;
const MS_PER_MINUTE: i64 = 60_000;

/// A local time of day, in minutes after midnight (`0..1440`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ClockTime(u16);

impl ClockTime {
    /// From hours and minutes.
    pub fn new(hour: u8, minute: u8) -> Option<ClockTime> {
        (hour < 24 && minute < 60).then(|| ClockTime(u16::from(hour) * 60 + u16::from(minute)))
    }

    /// From minutes after midnight.
    pub fn from_minutes(minutes: u16) -> Option<ClockTime> {
        (minutes < MINUTES_PER_DAY).then_some(ClockTime(minutes))
    }

    /// Minutes after midnight.
    pub fn minutes(self) -> u16 {
        self.0
    }
}

impl TryFrom<String> for ClockTime {
    type Error = String;

    /// `HH:MM` (24-hour).
    fn try_from(s: String) -> Result<ClockTime, String> {
        let bad = || format!("\"{s}\" is not a time of day (HH:MM, 24-hour).");
        let (h, m) = s.trim().split_once(':').ok_or_else(bad)?;
        if h.is_empty() || h.len() > 2 || m.len() != 2 {
            return Err(bad());
        }
        ClockTime::new(h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?).ok_or_else(bad)
    }
}

impl From<ClockTime> for String {
    fn from(t: ClockTime) -> String {
        format!("{:02}:{:02}", t.0 / 60, t.0 % 60)
    }
}

/// A daily span of local time when OS notifications wait. `start == end` is empty; `start > end` spans midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuietHours {
    /// When it starts.
    pub start: ClockTime,
    /// When it ends (exclusive).
    pub end: ClockTime,
}

impl QuietHours {
    /// Whether `now` (local) is inside.
    pub fn contains(&self, now: ClockTime) -> bool {
        let (s, e, n) = (self.start.0, self.end.0, now.0);
        if s <= e {
            s <= n && n < e
        } else {
            n >= s || n < e
        }
    }
}

/// The user's local time of day at an instant.
pub trait LocalTime: Send + Sync {
    /// The local time of day at `epoch_ms`.
    fn clock_time(&self, epoch_ms: i64) -> ClockTime;
}

/// The machine's time zone (daylight saving included).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemLocalTime;

impl LocalTime for SystemLocalTime {
    fn clock_time(&self, epoch_ms: i64) -> ClockTime {
        use chrono::{Local, TimeZone, Timelike};
        Local
            .timestamp_millis_opt(epoch_ms)
            .single()
            .and_then(|t| {
                ClockTime::new(u8::try_from(t.hour()).ok()?, u8::try_from(t.minute()).ok()?)
            })
            .unwrap_or_default()
    }
}

/// A fixed offset from UTC in minutes, for tests (no time zone database, no daylight saving).
#[derive(Debug, Default, Clone, Copy)]
pub struct FixedOffset(pub i32);

impl LocalTime for FixedOffset {
    fn clock_time(&self, epoch_ms: i64) -> ClockTime {
        let local = epoch_ms.div_euclid(MS_PER_MINUTE) + i64::from(self.0);
        let minutes = local.rem_euclid(i64::from(MINUTES_PER_DAY));
        ClockTime::from_minutes(u16::try_from(minutes).unwrap_or(0)).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> ClockTime {
        ClockTime::try_from(s.to_string()).unwrap()
    }

    #[test]
    fn clock_times_parse_and_print_as_hh_mm() {
        assert_eq!(t("07:05").minutes(), 425);
        assert_eq!(t("7:05"), t("07:05"));
        assert_eq!(String::from(t("23:59")), "23:59");
        for bad in [
            "", "24:00", "12:60", "12", "12:5", "ab:cd", "-1:00", "123:00", "12:30:00",
        ] {
            assert!(ClockTime::try_from(bad.to_string()).is_err(), "{bad}");
        }
        assert_eq!(ClockTime::from_minutes(1439), ClockTime::new(23, 59));
        assert_eq!(ClockTime::from_minutes(1440), None);
    }

    #[test]
    fn quiet_hours_within_a_day_across_midnight_and_empty() {
        let day = QuietHours {
            start: t("12:00"),
            end: t("13:00"),
        };
        assert!(day.contains(t("12:00")) && day.contains(t("12:59")));
        assert!(
            !day.contains(t("13:00")) && !day.contains(t("11:59")),
            "the end is exclusive"
        );
        let night = QuietHours {
            start: t("22:00"),
            end: t("07:00"),
        };
        assert!(
            night.contains(t("23:30")) && night.contains(t("00:00")) && night.contains(t("06:59"))
        );
        assert!(
            !night.contains(t("07:00"))
                && !night.contains(t("12:00"))
                && !night.contains(t("21:59"))
        );
        let empty = QuietHours {
            start: t("08:00"),
            end: t("08:00"),
        };
        assert!(!empty.contains(t("08:00")) && !empty.contains(t("03:00")));
        let json = serde_json::to_string(&night).unwrap();
        assert_eq!(json, r#"{"start":"22:00","end":"07:00"}"#);
        assert_eq!(serde_json::from_str::<QuietHours>(&json).unwrap(), night);
        assert!(serde_json::from_str::<QuietHours>(r#"{"start":"25:00","end":"07:00"}"#).is_err());
    }

    #[test]
    fn a_fixed_offset_maps_epoch_to_local_time_including_before_1970() {
        // 1970-01-01T00:00Z.
        assert_eq!(FixedOffset(0).clock_time(0), t("00:00"));
        assert_eq!(FixedOffset(120).clock_time(0), t("02:00"));
        assert_eq!(
            FixedOffset(-90).clock_time(0),
            t("22:30"),
            "wraps backwards over midnight"
        );
        assert_eq!(
            FixedOffset(0).clock_time(-60_000),
            t("23:59"),
            "before the epoch"
        );
        assert_eq!(
            FixedOffset(0).clock_time(3 * 3_600_000 + 5 * 60_000),
            t("03:05")
        );
    }

    #[test]
    fn the_system_zone_gives_some_time_of_day() {
        // The machine's zone is not controlled here: only that it is a valid time of day for a range of instants.
        for ms in [0, 1_792_656_000_000, i64::MAX, i64::MIN] {
            assert!(SystemLocalTime.clock_time(ms).minutes() < 1440);
        }
    }
}
