//! Locale- and timezone-free time text. Liveness never reads these strings (they are informational, for people and for old
//! readers); it compares the OS start time with the record's epoch.

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian (Howard Hinnant's civil_from_days).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `2026-10-31 22:19:00 UTC` for epoch seconds.
pub fn utc_text(secs: u64) -> String {
    let secs = i64::try_from(secs).unwrap_or(i64::MAX / 4);
    let (y, m, d) = civil(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", t / 3600, t % 3600 / 60, t % 60)
}

/// `22:19:00Z` for epoch seconds.
pub fn clock_text(secs: u64) -> String {
    let t = secs % 86_400;
    format!("{:02}:{:02}:{:02}Z", t / 3600, t % 3600 / 60, t % 60)
}

/// `2026-10-31T22:19:00.123456Z` for epoch microseconds (the CLEF `@t`).
pub fn rfc3339_micros(micros: u64) -> String {
    let secs = micros / 1_000_000;
    let (y, m, d) = civil(i64::try_from(secs / 86_400).unwrap_or(0));
    let t = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:06}Z",
        t / 3600,
        t % 3600 / 60,
        t % 60,
        micros % 1_000_000
    )
}

/// `2026-10-31` for epoch seconds (the log file's day).
pub fn day_text(secs: u64) -> String {
    let (y, m, d) = civil(i64::try_from(secs / 86_400).unwrap_or(0));
    format!("{y:04}-{m:02}-{d:02}")
}

/// One line: newlines, returns and tabs become spaces.
pub fn oneline(s: &str) -> String {
    s.chars().map(|c| if matches!(c, '\n' | '\r' | '\t') { ' ' } else { c }).collect()
}

/// `N min` for the seconds since `epoch`, or `an unknown time`.
pub fn mins(now: u64, epoch: Option<u64>) -> String {
    match epoch {
        Some(e) => format!("{} min", (now as i64 - e as i64) / 60),
        None => "an unknown time".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_instants() {
        assert_eq!(utc_text(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc_text(951_782_400), "2000-02-29 00:00:00 UTC");
        assert_eq!(utc_text(1_793_485_140), "2026-10-31 22:19:00 UTC");
        assert_eq!(rfc3339_micros(1_793_485_140_000_123), "2026-10-31T22:19:00.000123Z");
        assert_eq!(day_text(1_793_485_140), "2026-10-31");
        assert_eq!(clock_text(1_793_485_140), "22:19:00Z");
    }

    #[test]
    fn mins_and_oneline() {
        assert_eq!(mins(1000, Some(100)), "15 min");
        assert_eq!(mins(1000, None), "an unknown time");
        assert_eq!(oneline("a\nb\r\tc"), "a b  c");
    }
}
