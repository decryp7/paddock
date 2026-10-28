//! Wording for numbers that the core puts into sentences (diagnoses, anomalies, reports). The UI formats its own
//! numbers; these are only for text the engine writes.

pub use paddock_units::Kind;

/// A size as text, from the family's one definition ([`paddock_units`]): "512 bytes", "1.5 KB", "16 GB". Pick
/// [`Kind::Storage`] (decimal) for files, disks and transfer rates, [`Kind::Memory`] (binary steps, same labels) for RAM,
/// process memory, swap and commit.
pub fn bytes(n: u64, kind: Kind) -> String {
    paddock_units::format_bytes(n, kind)
}

/// A limit as text in the largest whole unit: "128 KB", "8 MB" (see [`paddock_units::format_whole`]).
pub fn whole(n: u64, kind: Kind) -> String {
    paddock_units::format_whole(n, kind)
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
    fn bytes_follow_their_kind() {
        assert_eq!(bytes(0, Kind::Storage), "0 bytes");
        assert_eq!(bytes(1_500, Kind::Storage), "1.5 KB");
        assert_eq!(bytes(16 << 30, Kind::Memory), "16 GB");
        assert_eq!(bytes(16 << 30, Kind::Storage), "17 GB");
        assert_eq!(whole(128_000, Kind::Storage), "128 KB");
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
