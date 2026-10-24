use chrono::{DateTime, NaiveDate, NaiveDateTime};

/// Epoch milliseconds of `text`: RFC 3339, `YYYY-MM-DD HH:MM[:SS]` or a date, read as UTC unless it carries an offset.
/// Parse, don't validate: the error says what to write.
pub fn parse_time(text: &str) -> Result<i64, String> {
    let t = text.trim();
    if let Ok(d) = DateTime::parse_from_rfc3339(t) {
        return Ok(d.timestamp_millis());
    }
    for fmt in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(d) = NaiveDateTime::parse_from_str(t, fmt) {
            return Ok(d.and_utc().timestamp_millis());
        }
    }
    if let Ok(d) = NaiveDate::parse_from_str(t, "%Y-%m-%d")
        && let Some(dt) = d.and_hms_opt(0, 0, 0)
    {
        return Ok(dt.and_utc().timestamp_millis());
    }
    Err(format!(
        "\"{text}\" is not a time: write 2026-10-01, 2026-10-01 14:30 or 2026-10-01T14:30:00Z (UTC unless it has an offset)"
    ))
}

/// A byte count as text ("3.2 GB"), binary steps.
pub fn format_size(bytes: u64) -> String {
    let (mut v, mut u) = (bytes as f64, 0);
    while v >= 1024.0 && u < 4 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", ["B", "KB", "MB", "GB", "TB"][u])
    }
}

/// A size such as `512`, `64k`, `10MB`, `1.5g` (binary steps; a bare number is bytes) read into bytes.
pub fn parse_size(text: &str) -> Result<u64, String> {
    let bad = || {
        format!(
            "\"{text}\" is not a size: write a number of bytes or a number with k, M, G or T (for example 512M)"
        )
    };
    let t = text.trim().to_ascii_lowercase();
    let t = t.strip_suffix('b').unwrap_or(&t);
    let (num, mult) = match t.chars().last() {
        Some('k') => (&t[..t.len() - 1], 1u64 << 10),
        Some('m') => (&t[..t.len() - 1], 1 << 20),
        Some('g') => (&t[..t.len() - 1], 1 << 30),
        Some('t') => (&t[..t.len() - 1], 1 << 40),
        _ => (t, 1),
    };
    let n: f64 = num.trim().parse().map_err(|_| bad())?;
    if !n.is_finite() || n < 0.0 {
        return Err(bad());
    }
    let bytes = n * mult as f64;
    if bytes >= u64::MAX as f64 {
        return Err(bad());
    }
    Ok(bytes as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_documented_time_forms_as_utc() {
        assert_eq!(parse_time("1970-01-02").unwrap(), 86_400_000);
        assert_eq!(parse_time("1970-01-01 00:01").unwrap(), 60_000);
        assert_eq!(parse_time("1970-01-01T00:00:01Z").unwrap(), 1_000);
        assert_eq!(parse_time("1970-01-01T01:00:00+01:00").unwrap(), 0);
        assert_eq!(parse_time(" 1970-01-01 00:00:00.5 ").unwrap(), 500);
        let e = parse_time("yesterday").unwrap_err();
        assert!(e.contains("yesterday") && e.contains("2026-10-01"), "{e}");
        assert!(parse_time("").is_err());
    }

    #[test]
    fn sizes_round_trip_in_binary_steps() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(3 * (1 << 30)), "3.0 GB");
        assert_eq!(format_size(u64::MAX), "16777216.0 TB");
        assert_eq!(parse_size("512").unwrap(), 512);
        assert_eq!(parse_size("64k").unwrap(), 64 * 1024);
        assert_eq!(parse_size("10MB").unwrap(), 10 << 20);
        assert_eq!(parse_size(" 1.5g ").unwrap(), 3 << 29);
        assert_eq!(parse_size("2T").unwrap(), 2 << 40);
    }

    #[test]
    fn bad_sizes_say_what_to_write() {
        for bad in ["", "abc", "-1", "k", "1e999", "99999999999999999T", "nan"] {
            let e = parse_size(bad).unwrap_err();
            assert!(e.contains("is not a size"), "{bad}: {e}");
        }
    }
}
