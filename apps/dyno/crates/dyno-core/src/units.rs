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

/// Storage sizes (disks, volumes, files) in decimal steps like Finder and the disk makers, written exactly as the UI's
/// `formatStorage` writes them from 1,000 bytes up: one decimal below 10 ("1.5 KB"), none from 10 ("995 GB"), a trailing ".0"
/// dropped ("4 TB"), and a value that would round to 1,000 moves to the next unit ("999.9 KB" is "1 MB"). One case table,
/// `ui/projects/paddock-ui/format/src/storage-cases.json`, is read by both tests. Under 1,000 it is "N B" (the UI says "N bytes").
/// Memory and transfer use [`bytes`].
pub fn storage(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if n < 1000 {
        return format!("{n} B");
    }
    // The size the UI rounds to when it decides whether a value has reached the next unit.
    let rounded = |v: f64| if v < 10.0 { (v * 10.0).round() / 10.0 } else { v.round() };
    let mut v = n as f64 / 1000.0;
    let mut u = 1;
    while u < UNITS.len() - 1 && rounded(v) >= 1000.0 {
        v /= 1000.0;
        u += 1;
    }
    let text = if v < 10.0 {
        let r = (v * 10.0).round() / 10.0;
        if r.fract() == 0.0 {
            format!("{r:.0}")
        } else {
            format!("{r:.1}")
        }
    } else {
        format!("{:.0}", v.round())
    };
    format!("{text} {}", UNITS[u])
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

    /// The same table the UI's formatStorage spec reads: the two never disagree.
    #[test]
    fn storage_writes_what_the_ui_writes() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../ui/projects/paddock-ui/format/src/storage-cases.json");
        let table: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let cases = table["cases"].as_array().unwrap();
        assert!(cases.len() >= 10);
        for c in cases {
            let (bytes, text) = (c[0].as_u64().unwrap(), c[1].as_str().unwrap());
            assert_eq!(storage(bytes), text, "{bytes} bytes");
        }
    }

    #[test]
    fn storage_below_a_kilobyte_is_bytes_and_memory_stays_binary() {
        assert_eq!((storage(0).as_str(), storage(999).as_str()), ("0 B", "999 B"));
        assert_eq!(bytes(1000 * 1000 * 1000), "953.7 MB");
        assert_eq!(storage(1000 * 1000 * 1000), "1 GB");
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
