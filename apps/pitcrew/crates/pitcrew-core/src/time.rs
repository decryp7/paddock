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

/// `t` as RFC 3339 in UTC with milliseconds, e.g. `2026-10-22T05:36:25.120Z`.
pub fn format_rfc3339(t: Timestamp) -> String {
    let ms = t.0 % 1000;
    let secs = t.0 / 1000;
    let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX / 2);
    let rem = secs % 86_400;
    // Civil date from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Parses an RFC 3339 time (`2026-10-22T13:36:25+08:00`, `…Z`, fractions allowed) as served by the Gitea API.
/// `None` for anything else, or a time before the Unix epoch.
pub fn parse_rfc3339(s: &str) -> Option<Timestamp> {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() < 20
        || b.len() > 40
        || b[4] != b'-'
        || b[7] != b'-'
        || !matches!(b[10], b'T' | b't' | b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (num(0..4)?, num(5..7)?, num(8..10)?, num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let mut i = 19;
    let mut ms = 0i64;
    if b.get(i) == Some(&b'.') {
        let start = i + 1;
        i = start;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let frac = s.get(start..i)?;
        if frac.is_empty() {
            return None;
        }
        let first3: String = frac.chars().chain("000".chars()).take(3).collect();
        ms = first3.parse().ok()?;
    }
    let offset_min = match s.get(i..)? {
        "Z" | "z" => 0,
        tz if tz.len() == 6 && matches!(tz.as_bytes()[0], b'+' | b'-') && tz.as_bytes()[3] == b':' => {
            let oh: i64 = tz.get(1..3)?.parse().ok()?;
            let om: i64 = tz.get(4..6)?.parse().ok()?;
            let sign = if tz.starts_with('-') { -1 } else { 1 };
            sign * (oh * 60 + om)
        }
        _ => return None,
    };
    // Days from the civil date (Howard Hinnant's algorithm).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + h * 3600 + mi * 60 + se - offset_min * 60;
    u64::try_from(secs.checked_mul(1000)?.checked_add(ms)?).ok().map(Timestamp)
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

    #[test]
    fn formats_rfc3339_and_round_trips() {
        assert_eq!(format_rfc3339(Timestamp(0)), "1970-01-01T00:00:00.000Z");
        assert_eq!(format_rfc3339(Timestamp(1_709_164_800_123)), "2024-02-29T00:00:00.123Z");
        for t in [1_792_647_385_500u64, 951_782_400_000, 4_102_444_799_999] {
            assert_eq!(parse_rfc3339(&format_rfc3339(Timestamp(t))), Some(Timestamp(t)));
        }
    }

    #[test]
    fn parses_rfc3339() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(Timestamp(0)));
        assert_eq!(parse_rfc3339("2026-10-22T05:36:25Z"), Some(Timestamp(1_792_647_385_000)));
        assert_eq!(parse_rfc3339("2026-10-22T13:36:25+08:00"), parse_rfc3339("2026-10-22T05:36:25Z"));
        assert_eq!(parse_rfc3339("2026-10-22T04:36:25.5-01:00"), Some(Timestamp(1_792_647_385_500)));
        assert_eq!(parse_rfc3339("2024-02-29T00:00:00.123456Z"), Some(Timestamp(1_709_164_800_123)));
        for bad in [
            "",
            "2026-10-22",
            "2026-13-01T00:00:00Z",
            "2026-10-22T25:00:00Z",
            "2026-10-22T00:00:00",
            "2026-10-22T00:00:00.Z",
            "1960-01-01T00:00:00Z",
            "2026-10-22T00:00:00+0800",
            "x026-10-22T00:00:00Z",
        ] {
            assert_eq!(parse_rfc3339(bad), None, "{bad}");
        }
    }
}
