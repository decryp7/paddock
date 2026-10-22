//! Time as values: the core never reads a clock. [`Timestamp`] is milliseconds since the Unix epoch; [`ClockTime`]
//! is a local time of day for quiet hours.

use serde::{Deserialize, Serialize};
use time::format_description::well_known::{Rfc2822, Rfc3339};
use time::OffsetDateTime;

/// Milliseconds since the Unix epoch (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(pub i64);

impl Timestamp {
    /// From seconds.
    pub fn from_secs(secs: i64) -> Timestamp {
        Timestamp(secs.saturating_mul(1000))
    }

    /// `self` plus `secs` seconds.
    pub fn plus_secs(self, secs: u64) -> Timestamp {
        Timestamp(self.0.saturating_add(i64::try_from(secs).unwrap_or(i64::MAX).saturating_mul(1000)))
    }

    /// Reads the date formats feeds and APIs use: RFC 3339 / ISO 8601 (`2026-10-22T08:00:00Z`, also with a space
    /// instead of `T` or without a zone, taken as UTC), RFC 2822 (`Thu, 22 Oct 2026 08:00:00 GMT`), and plain dates
    /// (`2026-10-22`). `None` for anything else.
    pub fn parse(text: &str) -> Option<Timestamp> {
        let t = text.trim();
        if t.is_empty() || t.len() > 64 {
            return None;
        }
        let ms = |d: OffsetDateTime| Timestamp((d.unix_timestamp_nanos() / 1_000_000) as i64);
        if let Ok(d) = OffsetDateTime::parse(t, &Rfc3339) {
            return Some(ms(d));
        }
        if let Ok(d) = OffsetDateTime::parse(t, &Rfc2822) {
            return Some(ms(d));
        }
        // "GMT"/"UTC" zone names in RFC 822 dates, which the strict parser refuses.
        if let Some(stripped) = t.strip_suffix(" GMT").or_else(|| t.strip_suffix(" UTC")) {
            if let Ok(d) = OffsetDateTime::parse(&format!("{stripped} +0000"), &Rfc2822) {
                return Some(ms(d));
            }
        }
        let iso = t.replacen(' ', "T", 1);
        let date_time = if iso.len() == 10 {
            format!("{iso}T00:00:00Z")
        } else {
            format!("{iso}Z")
        };
        OffsetDateTime::parse(&date_time, &Rfc3339).ok().map(ms)
    }
}

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
        (minutes < 1440).then_some(ClockTime(minutes))
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
        if h.is_empty() || m.len() != 2 {
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

/// A daily span of local time when notifications wait. `start == end` is empty; `start > end` spans midnight.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_from_feeds_and_apis_parse() {
        let base = Timestamp::parse("2026-10-22T08:00:00Z").unwrap();
        assert_eq!(base, Timestamp(1_792_656_000_000));
        assert_eq!(Timestamp::parse("2026-10-22T10:00:00+02:00"), Some(base));
        assert_eq!(Timestamp::parse("2026-10-22T08:00:00.000Z"), Some(base));
        assert_eq!(Timestamp::parse("Thu, 22 Oct 2026 08:00:00 +0000"), Some(base));
        assert_eq!(Timestamp::parse("Thu, 22 Oct 2026 08:00:00 GMT"), Some(base));
        assert_eq!(Timestamp::parse("Thu, 22 Oct 2026 08:00:00 UTC"), Some(base));
        assert_eq!(Timestamp::parse("2026-10-22 08:00:00"), Some(base));
        assert_eq!(Timestamp::parse("2026-10-22T08:00:00"), Some(base));
        assert_eq!(Timestamp::parse("2026-10-22"), Some(Timestamp(1_792_627_200_000)));
        for bad in ["", "yesterday", "2026-13-01", &"9".repeat(65), "Thu, 22 Oct 2026 08:00:00 XYZ"] {
            assert_eq!(Timestamp::parse(bad), None, "{bad}");
        }
        assert_eq!(Timestamp::from_secs(2).plus_secs(3), Timestamp(5000));
        assert_eq!(Timestamp(i64::MAX).plus_secs(u64::MAX), Timestamp(i64::MAX));
    }

    #[test]
    fn clock_times_and_quiet_hours() {
        let t = |s: &str| ClockTime::try_from(s.to_string()).unwrap();
        assert_eq!(t("07:05").minutes(), 425);
        assert_eq!(String::from(t("7:05")), "07:05");
        for bad in ["24:00", "12:60", "12", "x:00", "12:5", ":30", "12:xx"] {
            assert!(ClockTime::try_from(bad.to_string()).is_err(), "{bad}");
        }
        assert_eq!(ClockTime::from_minutes(1439), ClockTime::new(23, 59));
        assert_eq!(ClockTime::from_minutes(1440), None);

        let night = QuietHours {
            start: t("22:00"),
            end: t("07:00"),
        };
        assert!(night.contains(t("23:30")) && night.contains(t("00:00")) && night.contains(t("06:59")));
        assert!(!night.contains(t("07:00")) && !night.contains(t("12:00")) && night.contains(t("22:00")));
        let lunch = QuietHours {
            start: t("12:00"),
            end: t("13:00"),
        };
        assert!(lunch.contains(t("12:30")) && !lunch.contains(t("13:00")) && !lunch.contains(t("11:59")));
        let empty = QuietHours {
            start: t("12:00"),
            end: t("12:00"),
        };
        assert!(!empty.contains(t("12:00")));
        let json = serde_json::to_string(&night).unwrap();
        assert_eq!(json, r#"{"start":"22:00","end":"07:00"}"#);
        assert_eq!(serde_json::from_str::<QuietHours>(&json).unwrap(), night);
    }
}
