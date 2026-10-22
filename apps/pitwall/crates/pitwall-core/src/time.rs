//! Time as values: Unix milliseconds in, text out. Nothing here reads a clock.

/// Unix time in milliseconds.
pub type Millis = u64;

/// One second in milliseconds.
pub const SECOND: Millis = 1000;
/// One minute in milliseconds.
pub const MINUTE: Millis = 60 * SECOND;
/// One hour in milliseconds.
pub const HOUR: Millis = 60 * MINUTE;
/// One day in milliseconds.
pub const DAY: Millis = 24 * HOUR;

/// Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
fn civil(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    (y, m, d)
}

/// RFC 3339 in UTC with milliseconds, e.g. `2026-10-22T09:15:03.120Z` (Ollama's `created_at`).
pub fn rfc3339(ms: Millis) -> String {
    let secs = ms / 1000;
    let (y, m, d) = civil(secs / 86_400);
    let rem = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, ms % 1000)
}

/// The start of the UTC day `ms` falls in.
pub fn day_start(ms: Millis) -> Millis {
    ms - ms % DAY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_instants() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(rfc3339(1_709_251_199_999), "2024-02-29T23:59:59.999Z");
        assert_eq!(rfc3339(1_735_689_600_000), "2025-01-01T00:00:00.000Z");
    }

    #[test]
    fn day_starts() {
        assert_eq!(day_start(DAY + 5), DAY);
        assert_eq!(day_start(0), 0);
    }
}
