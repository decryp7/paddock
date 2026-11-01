//! Calendar dates as day numbers (days since 1970-01-01, proleptic Gregorian, UTC), without a time zone database.

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// A day number.
pub type Day = i64;

/// Parses exactly `YYYY-MM-DD` (ASCII digits) naming a real calendar day; anything else is `None`.
pub fn parse_date(s: &str) -> Option<Day> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let digits = |r: std::ops::Range<usize>| -> Option<i64> {
        let part = &b[r];
        if part.iter().all(u8::is_ascii_digit) {
            part.iter().try_fold(0i64, |acc, d| Some(acc * 10 + i64::from(d - b'0')))
        } else {
            None
        }
    };
    let (y, m, d) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    let days_in_month = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => return None,
    };
    if d < 1 || d > days_in_month {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

/// `YYYY-MM-DD` of a day number.
pub fn format_date(day: Day) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The local calendar day at `unix_secs` for a zone `offset_secs` east of UTC.
pub fn local_day(unix_secs: i64, offset_secs: i64) -> Day {
    (unix_secs + offset_secs).div_euclid(86_400)
}

/// Today by the system clock in the machine's local time zone, like `date +%F` (which `verified:` dates come from).
pub fn today_local() -> Day {
    use chrono::{Local, Offset, TimeZone};
    let now = Local::now();
    let offset = i64::from(Local.offset_from_utc_datetime(&now.naive_utc()).fix().local_minus_utc());
    local_day(now.timestamp(), offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_dates_only() {
        assert_eq!(parse_date("1970-01-01"), Some(0));
        assert_eq!(parse_date("2026-10-31").map(format_date).as_deref(), Some("2026-10-31"));
        assert!(parse_date("2026-02-29").is_none());
        assert!(parse_date("2028-02-29").is_some());
        assert!(parse_date("2026-13-45").is_none());
        assert!(parse_date("2026-1-05").is_none());
        assert!(parse_date("٢٠٢٦-10-31").is_none());
        assert_eq!(parse_date("0000-02-29").map(format_date).as_deref(), Some("0000-02-29"));
    }

    #[test]
    fn the_local_day_follows_the_zone_offset() {
        // 2026-10-31 23:30 UTC is already 11-01 in UTC+8 and still 10-31 in UTC-5.
        let utc = 1_793_489_400;
        assert_eq!(format_date(local_day(utc, 0)), "2026-10-31");
        assert_eq!(format_date(local_day(utc, 8 * 3600)), "2026-11-01");
        assert_eq!(format_date(local_day(utc, -5 * 3600)), "2026-10-31");
        assert_eq!(format_date(local_day(0, -1)), "1969-12-31");
        let near = today_local() - local_day(SystemTimeNow::secs(), 0);
        assert!(near.abs() <= 1, "{near}");
    }

    struct SystemTimeNow;
    impl SystemTimeNow {
        fn secs() -> i64 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
                .unwrap_or(0)
        }
    }

    #[test]
    fn differences_are_whole_days() {
        let a = parse_date("2026-10-01").unwrap_or(0);
        let b = parse_date("2026-10-31").unwrap_or(0);
        assert_eq!(b - a, 30);
    }
}
