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

/// A byte count as text ("3.2 GB"): `Kind::Storage` for files and disks, `Kind::Memory` for RAM (`paddock_units`).
pub fn format_size(bytes: u64, kind: paddock_units::Kind) -> String {
    paddock_units::format_bytes(bytes, kind)
}

/// A size such as `512`, `64k`, `10MB`, `1.5g`, `2GiB` (a bare number is bytes) read into bytes with the steps of `kind`.
pub fn parse_size(text: &str, kind: paddock_units::Kind) -> Result<u64, String> {
    paddock_units::parse_size(text, kind)
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
    fn sizes_follow_the_kind() {
        use paddock_units::Kind::{Memory, Storage};
        assert_eq!(format_size(1536, Storage), "1.5 KB");
        assert_eq!(format_size(3 << 30, Memory), "3 GB");
        assert_eq!(parse_size("512", Storage).unwrap(), 512);
        assert_eq!(parse_size("64k", Storage).unwrap(), 64_000);
        assert_eq!(parse_size("10MB", Memory).unwrap(), 10 << 20);
        assert_eq!(parse_size(" 1.5g ", Storage).unwrap(), 1_500_000_000);
        assert_eq!(parse_size("2GiB", Storage).unwrap(), 2 << 30);
    }

    #[test]
    fn bad_sizes_say_what_to_write() {
        for bad in ["", "abc", "-1", "k", "1e999", "99999999999999999T", "nan"] {
            let e = parse_size(bad, paddock_units::Kind::Storage).unwrap_err();
            assert!(e.contains("is not a size"), "{bad}: {e}");
        }
    }
}
