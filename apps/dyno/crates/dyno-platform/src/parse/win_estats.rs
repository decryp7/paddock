//! Windows per-connection TCP statistics (`GetPerTcpConnectionEStats`): reading the byte counters out of the
//! `TCP_ESTATS_DATA_ROD_v0` struct, and what a refusal to enable collection means for the user. Pure, so it is tested
//! on every OS; the Windows adapter only makes the calls.

use dyno_core::network_traffic::{TrafficCounter, MAX_COUNTERS};
use dyno_core::ports::PlatformError;
use std::collections::HashMap;

/// `ERROR_ACCESS_DENIED`.
pub const ERROR_ACCESS_DENIED: u32 = 5;
/// `ERROR_NOT_FOUND`: the connection ended between the table read and the call.
pub const ERROR_NOT_FOUND: u32 = 1168;

/// Offsets inside `TCP_ESTATS_DATA_ROD_v0`: `DataBytesOut` is the first field, `DataBytesIn` the third.
const OFF_DATA_BYTES_OUT: usize = 0;
const OFF_DATA_BYTES_IN: usize = 16;

/// `(bytes_in, bytes_out)` from a `TCP_ESTATS_DATA_ROD_v0` blob; `None` when it is too short.
pub fn rod_bytes(rod: &[u8]) -> Option<(u64, u64)> {
    let at = |o: usize| -> Option<u64> { Some(u64::from_ne_bytes(rod.get(o..o.checked_add(8)?)?.try_into().ok()?)) };
    Some((at(OFF_DATA_BYTES_IN)?, at(OFF_DATA_BYTES_OUT)?))
}

/// Why collection could not be switched on, as the reply the page shows.
pub fn enable_failure(code: u32) -> PlatformError {
    if code == ERROR_ACCESS_DENIED {
        PlatformError::Unsupported(
            "Traffic per program is not available without administrator rights on Windows: switching on TCP statistics needs them \
             (event tracing too). Install Dyno's helper under Settings > Helper to see it."
                .into(),
        )
    } else {
        PlatformError::Failed(format!("Windows would not switch on TCP statistics (error {code})."))
    }
}

/// Sums connections `(pid, bytes_in, bytes_out)` per program; `names` supplies the program name of a pid.
pub fn counters_by_pid(conns: &[(u32, u64, u64)], names: &HashMap<u32, String>) -> Vec<TrafficCounter> {
    let mut by_pid: HashMap<u32, TrafficCounter> = HashMap::new();
    for (pid, rx, tx) in conns {
        let c = by_pid.entry(*pid).or_insert_with(|| TrafficCounter {
            pid: *pid,
            name: names.get(pid).cloned().unwrap_or_else(|| format!("pid {pid}")),
            bytes_in: 0,
            bytes_out: 0,
        });
        c.bytes_in = c.bytes_in.saturating_add(*rx);
        c.bytes_out = c.bytes_out.saturating_add(*tx);
    }
    let mut v: Vec<_> = by_pid.into_values().collect();
    v.sort_by_key(|c| c.pid);
    v.truncate(MAX_COUNTERS);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counters_are_read_from_the_rod_struct() {
        let mut rod = vec![0u8; 96];
        rod[0..8].copy_from_slice(&1_234u64.to_ne_bytes());
        rod[8..16].copy_from_slice(&9u64.to_ne_bytes()); // DataSegsOut, ignored
        rod[16..24].copy_from_slice(&5_678u64.to_ne_bytes());
        assert_eq!(rod_bytes(&rod), Some((5_678, 1_234)));
        assert_eq!(rod_bytes(&rod[..23]), None);
        assert_eq!(rod_bytes(&[]), None);
    }

    #[test]
    fn a_refusal_says_administrator_rights_and_other_errors_say_the_code() {
        assert!(matches!(enable_failure(ERROR_ACCESS_DENIED), PlatformError::Unsupported(m) if m.contains("administrator")));
        assert!(matches!(enable_failure(87), PlatformError::Failed(m) if m.contains("87")));
    }

    #[test]
    fn connections_are_summed_per_program_with_a_fallback_name() {
        let names = HashMap::from([(4, "alpha".to_string())]);
        let c = counters_by_pid(&[(4, 10, 1), (9, 5, 5), (4, 20, 2)], &names);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].pid, c[0].name.as_str(), c[0].bytes_in, c[0].bytes_out), (4, "alpha", 30, 3));
        assert_eq!(c[1].name, "pid 9");
    }
}
