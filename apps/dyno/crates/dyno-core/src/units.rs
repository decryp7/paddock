//! Wording for numbers that the core puts into sentences (diagnoses, anomalies, reports). The UI formats its own
//! numbers; these are only for text the engine writes.

/// Bytes in binary units with one decimal: "512 B", "1.5 KB", "3.2 GB" (KB = 1024 bytes, as the OSes show).
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

/// A duration in the two largest units: "45s", "15m", "2h 5m", "3d 4h".
pub fn duration(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86_400, secs / 3_600 % 24, secs / 60 % 60, secs % 60);
    match (d, h, m) {
        (0, 0, 0) => format!("{s}s"),
        (0, 0, _) if s == 0 => format!("{m}m"),
        (0, 0, _) => format!("{m}m {s}s"),
        (0, _, 0) => format!("{h}h"),
        (0, _, _) => format!("{h}h {m}m"),
        (_, 0, _) => format!("{d}d"),
        _ => format!("{d}d {h}h"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_read_well() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(3 << 30), "3.0 GB");
        assert_eq!(
            bytes(u64::MAX),
            "16.0 EB"
                .replace("EB", "PB")
                .replace("16.0", &format!("{:.1}", u64::MAX as f64 / 1024f64.powi(5)))
        );
    }

    #[test]
    fn durations_read_well() {
        assert_eq!(duration(45), "45s");
        assert_eq!(duration(900), "15m");
        assert_eq!(duration(905), "15m 5s");
        assert_eq!(duration(7_200), "2h");
        assert_eq!(duration(7_500), "2h 5m");
        assert_eq!(duration(86_400 * 3), "3d");
        assert_eq!(duration(86_400 * 3 + 3_600 * 4), "3d 4h");
    }
}
