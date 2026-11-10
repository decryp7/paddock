//! Timestamps of the Gitea API (`2026-11-10T17:49:12+08:00`) and the calendar, without a date crate.

use std::time::{SystemTime, UNIX_EPOCH};

/// Days from 1970-01-01 to the civil date (proleptic Gregorian; Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Milliseconds since the epoch of an ISO 8601 timestamp with a `Z` or `+HH:MM` zone (fractions of a second allowed), like
/// `Date.parse` for the shapes the API prints. `None` for anything else.
pub fn parse_ms(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let num = |from: usize, len: usize| -> Option<i64> {
        let part = s.get(from..from + len)?;
        part.bytes().all(|c| c.is_ascii_digit()).then(|| part.parse().ok()).flatten()
    };
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (y, mo, d, h, mi, sec) = (num(0, 4)?, num(5, 2)?, num(8, 2)?, num(11, 2)?, num(14, 2)?, num(17, 2)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let mut i = 19;
    let mut frac_ms = 0.0;
    if b.get(i) == Some(&b'.') {
        let start = i + 1;
        i = start;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return None;
        }
        frac_ms = format!("0.{}", &s[start..i]).parse::<f64>().ok()? * 1000.0;
    }
    let offset_min = match b.get(i)? {
        b'Z' | b'z' if i + 1 == b.len() => 0,
        sign @ (b'+' | b'-') if i + 6 == b.len() && b[i + 3] == b':' => {
            let (oh, om) = (num(i + 1, 2)?, num(i + 4, 2)?);
            let v = oh * 60 + om;
            if *sign == b'-' { -v } else { v }
        }
        _ => return None,
    };
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec - offset_min * 60;
    Some(secs as f64 * 1000.0 + frac_ms.round())
}

/// A timestamp of the API as milliseconds, `None` for a missing one or Gitea's zero time (`1970-01-01T08:00:00+08:00`, `0001-...`).
pub fn ts(s: &str) -> Option<f64> {
    if s.is_empty() || s.starts_with("1970") || s.starts_with("0001") {
        None
    } else {
        parse_ms(s)
    }
}

/// `YYYY-MM-DD` (UTC) of a Unix time in seconds.
pub fn date_of(secs: i64) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// The clock: seconds since the epoch.
pub fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_with_zones_and_fractions_are_read_like_date_parse() {
        // 2026-11-10T00:00:00Z is 1_794_268_800 s (checked with `date -u`).
        assert_eq!(parse_ms("2026-11-10T00:00:00Z"), Some(1_794_268_800_000.0));
        assert_eq!(parse_ms("2026-11-10T08:00:00+08:00"), Some(1_794_268_800_000.0));
        assert_eq!(parse_ms("2026-11-09T19:30:00-04:30"), Some(1_794_268_800_000.0));
        assert_eq!(parse_ms("2026-11-10T00:00:01.250Z"), Some(1_794_268_801_250.0));
        assert_eq!(parse_ms("2026-11-10T00:00:00.4427582Z"), Some(1_794_268_800_443.0));
        assert_eq!(parse_ms("2026-11-10"), None);
        assert_eq!(parse_ms("2026-13-10T00:00:00Z"), None);
        assert_eq!(parse_ms("2026-11-10T00:00:00"), None);
    }

    #[test]
    fn gitea_zero_times_are_no_time() {
        assert_eq!(ts("1970-01-01T08:00:00+08:00"), None);
        assert_eq!(ts("0001-01-01T00:00:00Z"), None);
        assert_eq!(ts(""), None);
        assert_eq!(ts("2026-11-10T00:00:00Z"), Some(1_794_268_800_000.0));
    }

    #[test]
    fn the_calendar_date_is_utc() {
        assert_eq!(date_of(1_794_268_800), "2026-11-10");
        assert_eq!(date_of(1_794_268_799), "2026-11-09");
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(1_709_164_800), "2024-02-29");
    }
}
