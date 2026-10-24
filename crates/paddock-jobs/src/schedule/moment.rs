//! Time as values for schedules: [`UnixMs`] (an instant) and [`ClockTime`] (a local time of day). Schedule maths never
//! reads a clock; callers pass "now" in (from [`crate::Clock::wall`] through [`UnixMs::from_clock`]), which is what lets
//! tests drive every case, daylight-saving changes included, with a [`crate::FakeClock`].

use crate::time::Clock;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::UNIX_EPOCH;

/// Milliseconds since the Unix epoch (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixMs(pub i64);

impl UnixMs {
    /// The clock's calendar time ([`Clock::wall`]); a time before 1970 reads as 1970.
    pub fn from_clock(clock: &dyn Clock) -> UnixMs {
        let ms = clock
            .wall()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        UnixMs(i64::try_from(ms).unwrap_or(i64::MAX))
    }

    /// `self` plus `secs` seconds (saturating).
    pub fn plus_secs(self, secs: u64) -> UnixMs {
        UnixMs(
            self.0
                .saturating_add(i64::try_from(secs).unwrap_or(i64::MAX).saturating_mul(1000)),
        )
    }

    /// Reads an RFC 3339 instant (`2026-10-22T08:00:00Z`, offsets allowed); `None` for anything else.
    pub fn parse_rfc3339(text: &str) -> Option<UnixMs> {
        let t = text.trim();
        if t.is_empty() || t.len() > 64 {
            return None;
        }
        jiff::Timestamp::from_str(t)
            .ok()
            .map(|t| UnixMs(t.as_millisecond()))
    }
}

/// A local time of day, in minutes after midnight (`0..1440`); text form `HH:MM` (24-hour).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
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
