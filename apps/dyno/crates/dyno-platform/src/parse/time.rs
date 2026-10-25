//! Process start times. Dyno identifies a process by its pid and start time with **second** precision (that is what
//! `sysinfo` reports on every OS), stored as milliseconds in a [`Timestamp`].

use dyno_core::time::Timestamp;

/// Seconds between 1601-01-01 (the Windows FILETIME epoch) and 1970-01-01.
const FILETIME_UNIX_OFFSET_SECS: u64 = 11_644_473_600;

/// The start time of a process from a Windows FILETIME (100 ns ticks since 1601), floored to the second.
pub fn started_from_filetime(ticks: u64) -> Timestamp {
    let secs = (ticks / 10_000_000).saturating_sub(FILETIME_UNIX_OFFSET_SECS);
    started_from_unix_secs(secs)
}

/// The start time of a process from Unix seconds.
pub fn started_from_unix_secs(secs: u64) -> Timestamp {
    Timestamp(i64::try_from(secs).unwrap_or(i64::MAX / 1000).saturating_mul(1000))
}

/// A Windows FILETIME (100 ns ticks since 1601) for Unix milliseconds.
pub fn filetime_from_unix_millis(ms: i64) -> u64 {
    let ms = u64::try_from(ms).unwrap_or(0);
    (ms / 1000 + FILETIME_UNIX_OFFSET_SECS) * 10_000_000 + (ms % 1000) * 10_000
}

/// Whether a live process's start time matches the one an action was aimed at (both at second precision; one
/// second of slack absorbs rounding differences between APIs).
pub fn same_start(live: Timestamp, wanted: Timestamp) -> bool {
    live.since(wanted).abs() <= 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetimes_floor_to_seconds() {
        // 2024-01-01T00:00:00.5Z
        let ft = filetime_from_unix_millis(1_704_067_200_500);
        assert_eq!(started_from_filetime(ft), Timestamp(1_704_067_200_000));
        assert_eq!(started_from_filetime(0), Timestamp(0));
        assert_eq!(started_from_unix_secs(5), Timestamp(5000));
        assert_eq!(started_from_unix_secs(u64::MAX).0, (i64::MAX / 1000) * 1000);
        assert_eq!(filetime_from_unix_millis(-5), FILETIME_UNIX_OFFSET_SECS * 10_000_000);
    }

    #[test]
    fn start_times_match_within_a_second() {
        assert!(same_start(Timestamp(10_000), Timestamp(10_000)));
        assert!(same_start(Timestamp(11_000), Timestamp(10_000)));
        assert!(!same_start(Timestamp(12_000), Timestamp(10_000)));
        assert!(!same_start(Timestamp(0), Timestamp(10_000)));
    }
}
