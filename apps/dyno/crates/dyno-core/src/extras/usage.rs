//! Network and energy per app. Network bytes are the cumulative per-process counters the OS keeps (macOS `nettop`; the
//! existing traffic port), summed per app group. Energy impact is what the OS itself reports where it does (macOS
//! `top -o power`); Windows and Linux have no per-process energy meter an unprivileged tool can read, and say so.

use super::{clean, clip};
use crate::network_traffic::TrafficCounter;
use crate::process::Pid;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Most apps listed.
pub const MAX_APPS: usize = 30;

/// One process's energy impact as the OS reported it.
#[derive(Debug, Clone, PartialEq)]
pub struct EnergySample {
    /// Process id.
    pub pid: Pid,
    /// The OS's number (macOS: the Activity Monitor "Energy Impact" scale, 0 and up).
    pub impact: f32,
}

/// One app's row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsageRow {
    /// The app.
    pub app: String,
    /// Bytes received since its processes started.
    pub bytes_in: Option<u64>,
    /// Bytes sent since its processes started.
    pub bytes_out: Option<u64>,
    /// Energy impact right now, summed over its processes.
    pub energy_impact: Option<f32>,
}

/// The "Network and energy per app" view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    /// Apps, biggest first (by traffic, then energy).
    pub apps: Vec<AppUsageRow>,
    /// Why there are no byte counters.
    pub network_unavailable: Option<String>,
    /// Why there are no energy numbers.
    pub energy_unavailable: Option<String>,
}

/// `top -l 2 -o power -stats pid,command,power`: the rows of the **last** sample (the first one measures since boot).
/// Each row is `pid`, a command that may hold spaces, and the power number last.
pub fn parse_top_power(text: &str) -> Vec<EnergySample> {
    let last = text.rsplit("PID").next().unwrap_or(text);
    let mut out = Vec::new();
    for line in last.lines().map(clean) {
        let mut parts = line.split_whitespace();
        let (Some(pid), Some(power)) = (parts.next(), parts.next_back()) else {
            continue;
        };
        let (Ok(pid), Ok(impact)) = (pid.parse::<Pid>(), power.parse::<f32>()) else {
            continue;
        };
        if impact.is_finite() && impact >= 0.0 {
            out.push(EnergySample { pid, impact });
        }
    }
    out
}

/// Sums counters and energy per app. `apps` maps each app name to its process ids.
pub fn build(apps: &[(String, Vec<Pid>)], counters: Option<&[TrafficCounter]>, energy: Option<&[EnergySample]>) -> Vec<AppUsageRow> {
    let by_pid: HashMap<Pid, &TrafficCounter> = counters.unwrap_or_default().iter().map(|c| (c.pid, c)).collect();
    let power: HashMap<Pid, f32> = energy.unwrap_or_default().iter().map(|e| (e.pid, e.impact)).collect();
    let mut rows: Vec<AppUsageRow> = apps
        .iter()
        .filter_map(|(app, pids)| {
            let net: Vec<&&TrafficCounter> = pids.iter().filter_map(|p| by_pid.get(p)).collect();
            let en: Vec<f32> = pids.iter().filter_map(|p| power.get(p).copied()).collect();
            let row = AppUsageRow {
                app: clip(app, 100),
                bytes_in: counters.map(|_| net.iter().fold(0u64, |a, c| a.saturating_add(c.bytes_in))),
                bytes_out: counters.map(|_| net.iter().fold(0u64, |a, c| a.saturating_add(c.bytes_out))),
                energy_impact: energy.map(|_| en.iter().sum()),
            };
            let quiet = row.bytes_in.unwrap_or(0) == 0 && row.bytes_out.unwrap_or(0) == 0 && row.energy_impact.unwrap_or(0.0) <= 0.0;
            (!quiet).then_some(row)
        })
        .collect();
    let traffic = |r: &AppUsageRow| r.bytes_in.unwrap_or(0).saturating_add(r.bytes_out.unwrap_or(0));
    rows.sort_by(|a, b| {
        traffic(b)
            .cmp(&traffic(a))
            .then(b.energy_impact.unwrap_or(0.0).total_cmp(&a.energy_impact.unwrap_or(0.0)))
            .then(a.app.cmp(&b.app))
    });
    rows.truncate(MAX_APPS);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOP: &str = "Processes: 300 total
PID    COMMAND          POWER
1      launchd          9.9
2      old              9.9

Processes: 300 total
PID    COMMAND          POWER
412    Google Chrome H  12.5
413    Google Chrome H  3.0
77     WindowServer     8.1
5      idle             0.0
bad    line here        x
";

    #[test]
    fn only_the_last_sample_counts() {
        let e = parse_top_power(TOP);
        assert_eq!(e.len(), 4);
        assert_eq!(e[0], EnergySample { pid: 412, impact: 12.5 });
        assert!(e.iter().all(|s| s.pid != 1));
        assert!(parse_top_power("").is_empty());
    }

    fn counter(pid: u32, i: u64, o: u64) -> TrafficCounter {
        TrafficCounter {
            pid,
            name: "x".into(),
            bytes_in: i,
            bytes_out: o,
        }
    }

    #[test]
    fn rows_sum_per_app_and_sort_by_traffic_then_energy() {
        let apps = vec![
            ("Chrome".to_string(), vec![412, 413]),
            ("WindowServer".to_string(), vec![77]),
            ("Quiet".to_string(), vec![5]),
        ];
        let counters = [counter(412, 100, 10), counter(413, 50, 5), counter(77, 0, 0)];
        let energy = parse_top_power(TOP);
        let rows = build(&apps, Some(&counters), Some(&energy));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].app, "Chrome");
        assert_eq!((rows[0].bytes_in, rows[0].bytes_out), (Some(150), Some(15)));
        assert_eq!(rows[0].energy_impact, Some(15.5));
        assert_eq!(rows[1].app, "WindowServer");
    }

    #[test]
    fn missing_sources_leave_their_fields_empty() {
        let apps = vec![("A".to_string(), vec![1])];
        let rows = build(&apps, None, Some(&[EnergySample { pid: 1, impact: 2.0 }]));
        assert_eq!(rows[0].bytes_in, None);
        assert_eq!(rows[0].energy_impact, Some(2.0));
        let rows = build(&apps, Some(&[counter(1, 3, 4)]), None);
        assert_eq!(rows[0].energy_impact, None);
        assert!(build(&apps, None, None).is_empty());
    }
}
