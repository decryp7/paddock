//! Anomalies over history: steady memory growth (leaks), periodic CPU spikes, restart loops.

use crate::history::{AppMinute, HistoryPoint};
use crate::stats::{linear_fit, mean_sd};
use crate::time::{Timestamp, HOUR, MINUTE};
use crate::units::{bytes, duration};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Something unusual Dyno noticed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anomaly {
    /// Stable id (kind + app), so the UI can dismiss it.
    pub id: String,
    /// What kind.
    pub kind: AnomalyKind,
    /// The app concerned ("System" for machine-wide patterns).
    pub app: String,
    /// Since when it is visible.
    pub since: Timestamp,
    /// One line.
    pub title: String,
    /// What it means and what to do.
    pub explanation: String,
    /// The numbers behind it.
    pub evidence: Vec<Evidence>,
}

/// Kinds of anomalies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnomalyKind {
    /// Memory grows steadily: likely a leak.
    MemoryGrowth,
    /// CPU spikes at a regular interval: a scheduled job or a polling loop.
    PeriodicSpike,
    /// An app keeps exiting and starting again.
    RestartLoop,
}

/// One labelled number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    /// Label.
    pub label: String,
    /// Value, formatted.
    pub value: String,
}

impl Evidence {
    /// A label and a value.
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Evidence {
        Evidence {
            label: label.into(),
            value: value.into(),
        }
    }
}

/// Thresholds for memory growth.
pub const MIN_GROWTH_HOURS: f64 = 2.0;
/// The least growth per hour that counts (bytes).
pub const MIN_GROWTH_PER_HOUR: f64 = 50.0 * 1024.0 * 1024.0;
/// The least total growth that counts (bytes).
pub const MIN_GROWTH_TOTAL: f64 = 300.0 * 1024.0 * 1024.0;

/// Apps whose memory grew steadily over the minutes in `rows` (at least 2 hours, ≥ 50 MiB/h, ≥ 300 MiB, R² ≥ 0.8).
pub fn memory_growth(rows: &[AppMinute]) -> Vec<Anomaly> {
    let mut by_app: BTreeMap<&str, Vec<&AppMinute>> = BTreeMap::new();
    for r in rows {
        by_app.entry(r.app.as_str()).or_default().push(r);
    }
    let mut out = Vec::new();
    for (app, mut pts) in by_app {
        pts.sort_by_key(|r| r.at);
        let (Some(first), Some(last)) = (pts.first(), pts.last()) else {
            continue;
        };
        let hours = last.at.since(first.at) as f64 / HOUR as f64;
        if hours < MIN_GROWTH_HOURS || pts.len() < 30 {
            continue;
        }
        let xy: Vec<(f64, f64)> = pts
            .iter()
            .map(|r| (r.at.since(first.at) as f64 / HOUR as f64, r.memory as f64))
            .collect();
        let Some(fit) = linear_fit(&xy) else { continue };
        let growth = fit.slope * hours;
        if fit.slope >= MIN_GROWTH_PER_HOUR && growth >= MIN_GROWTH_TOTAL && fit.r2 >= 0.8 {
            out.push(Anomaly {
                id: format!("memoryGrowth:{app}"),
                kind: AnomalyKind::MemoryGrowth,
                app: app.to_string(),
                since: first.at,
                title: format!(
                    "{app}'s memory grew {} in {}",
                    bytes(growth as u64),
                    duration((hours * 3600.0) as u64)
                ),
                explanation: format!(
                    "{app} keeps using more memory at a steady rate, which usually means a leak. Restarting it gives the \
                     memory back; if it keeps happening, look for an update."
                ),
                evidence: vec![
                    Evidence::new("Growth rate", format!("{}/h", bytes(fit.slope as u64))),
                    Evidence::new("Now", bytes(last.memory)),
                    Evidence::new("Steadiness (R²)", format!("{:.2}", fit.r2)),
                ],
            });
        }
    }
    out
}

/// Regular CPU spikes in a day of minute points: at least 4 spikes (≥ 50 % and 3 σ above the mean) whose intervals
/// vary by less than 20 %. `rows` name the app busiest at the spikes.
pub fn periodic_spikes(points: &[HistoryPoint], rows: &[AppMinute]) -> Option<Anomaly> {
    let values: Vec<f64> = points.iter().map(|p| f64::from(p.cpu_max)).collect();
    let (mean, sd) = mean_sd(&values);
    let threshold = (mean + 3.0 * sd).max(50.0);
    let mut spikes: Vec<Timestamp> = Vec::new();
    for p in points {
        if f64::from(p.cpu_max) >= threshold && spikes.last().is_none_or(|l| p.at.since(*l) > 2 * MINUTE) {
            spikes.push(p.at);
        }
    }
    if spikes.len() < 4 {
        return None;
    }
    let gaps: Vec<f64> = spikes.windows(2).map(|w| w[1].since(w[0]) as f64).collect();
    let (gap_mean, gap_sd) = mean_sd(&gaps);
    if gap_mean <= 0.0 || gap_sd / gap_mean > 0.2 {
        return None;
    }
    let mut culprits: BTreeMap<&str, f32> = BTreeMap::new();
    for r in rows
        .iter()
        .filter(|r| spikes.iter().any(|s| r.at.floor_to(MINUTE) == s.floor_to(MINUTE)))
    {
        *culprits.entry(r.app.as_str()).or_default() += r.cpu;
    }
    let app = culprits.into_iter().max_by(|a, b| a.1.total_cmp(&b.1)).map(|(a, _)| a.to_string());
    let every = duration((gap_mean / 1000.0).round() as u64);
    let who = app.clone().unwrap_or_else(|| "System".into());
    Some(Anomaly {
        id: format!("periodicSpike:{who}"),
        kind: AnomalyKind::PeriodicSpike,
        app: who.clone(),
        since: spikes[0],
        title: format!(
            "CPU spikes every {every}{}",
            app.as_ref().map(|a| format!(", mostly {a}")).unwrap_or_default()
        ),
        explanation: "A regular spike is usually a scheduled task, an updater or an app polling in a loop. If it disturbs you, \
                      a rule can lower its priority, or check the app's settings for a sync or update interval."
            .into(),
        evidence: vec![
            Evidence::new("Spikes", spikes.len().to_string()),
            Evidence::new("Interval", every),
            Evidence::new("Peak threshold", format!("{threshold:.0}%")),
        ],
    })
}

/// Apps that started at least 5 times within 10 minutes. `starts` are (app, when) of an app appearing after it had
/// no process.
pub fn restart_loops(starts: &[(String, Timestamp)], now: Timestamp) -> Vec<Anomaly> {
    let mut by_app: BTreeMap<&str, Vec<Timestamp>> = BTreeMap::new();
    for (app, at) in starts {
        if now.since(*at) <= HOUR {
            by_app.entry(app.as_str()).or_default().push(*at);
        }
    }
    let mut out = Vec::new();
    for (app, mut times) in by_app {
        times.sort();
        let worst = times
            .iter()
            .enumerate()
            .map(|(i, t)| (times[i..].iter().take_while(|u| u.since(*t) <= 10 * MINUTE).count(), *t))
            .max_by_key(|(n, _)| *n);
        if let Some((n, since)) = worst.filter(|(n, _)| *n >= 5) {
            out.push(Anomaly {
                id: format!("restartLoop:{app}"),
                kind: AnomalyKind::RestartLoop,
                app: app.to_string(),
                since,
                title: format!("{app} restarted {n} times in 10 minutes"),
                explanation: format!(
                    "{app} keeps exiting and being started again, which costs CPU and disk each time. It may be crashing; \
                     check for an update, or disable it in the startup manager if it is a background helper."
                ),
                evidence: vec![Evidence::new("Starts in the last hour", times.len().to_string())],
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(app: &str, minutes: i64, start_mib: u64, per_min_kib: u64) -> Vec<AppMinute> {
        (0..minutes)
            .map(|m| AppMinute {
                at: Timestamp(m * MINUTE),
                app: app.into(),
                cpu: 1.0,
                memory: (start_mib << 20) + (m as u64) * (per_min_kib << 10),
            })
            .collect()
    }

    #[test]
    fn steady_growth_is_a_leak_and_flat_or_noisy_is_not() {
        // 5 hours, +2 MiB a minute = 120 MiB/h, 600 MiB total.
        let mut all = rows("Teams", 300, 500, 2048);
        all.extend(rows("Flat", 300, 500, 0));
        all.extend(rows("Short", 60, 500, 8192));
        let mut noisy = rows("Noisy", 300, 500, 0);
        for (i, r) in noisy.iter_mut().enumerate() {
            r.memory += if i % 2 == 0 { 0 } else { 2 << 30 };
        }
        all.extend(noisy);
        let found = memory_growth(&all);
        assert_eq!(found.len(), 1, "{found:?}");
        let a = &found[0];
        assert_eq!(a.kind, AnomalyKind::MemoryGrowth);
        assert_eq!(a.app, "Teams");
        assert!(a.title.contains("Teams's memory grew"));
        assert_eq!(a.evidence.len(), 3);
        let same_minute: Vec<AppMinute> = (0..40)
            .map(|_| AppMinute {
                at: Timestamp(0),
                app: "X".into(),
                cpu: 0.0,
                memory: 1,
            })
            .collect();
        assert!(memory_growth(&same_minute).is_empty());
    }

    fn day(spike_every: Option<i64>, jitter: bool) -> Vec<HistoryPoint> {
        (0..24 * 60)
            .map(|m| {
                let spike = spike_every.is_some_and(|e| m % e == 7 || (jitter && m % 37 == 0));
                HistoryPoint {
                    at: Timestamp(m * MINUTE),
                    cpu: 8.0,
                    cpu_max: if spike { 95.0 } else { 10.0 + (m % 3) as f32 },
                    ..Default::default()
                }
            })
            .collect()
    }

    #[test]
    fn regular_spikes_are_found_with_their_app() {
        let spikes = day(Some(15), false);
        let culprit = vec![AppMinute {
            at: Timestamp(7 * MINUTE),
            app: "Updater".into(),
            cpu: 80.0,
            memory: 0,
        }];
        let a = periodic_spikes(&spikes, &culprit).unwrap();
        assert_eq!(a.kind, AnomalyKind::PeriodicSpike);
        assert_eq!(a.app, "Updater");
        assert!(a.title.contains("every 15m"), "{}", a.title);
        let b = periodic_spikes(&spikes, &[]).unwrap();
        assert_eq!(b.app, "System");
        assert!(periodic_spikes(&day(None, false), &[]).is_none());
        assert!(
            periodic_spikes(&day(Some(15), true), &[]).is_none(),
            "an irregular extra spike breaks the rhythm"
        );
        assert!(periodic_spikes(&[], &[]).is_none());
    }

    #[test]
    fn restart_loops_need_five_starts_in_ten_minutes() {
        let now = Timestamp(2 * HOUR);
        let mut starts: Vec<(String, Timestamp)> = (0..6).map(|i| ("Crashy".to_string(), now.minus(i * MINUTE))).collect();
        starts.extend((0..4).map(|i| ("Fine".to_string(), now.minus(i * MINUTE))));
        starts.extend((0..9).map(|i| ("Old".to_string(), Timestamp(i * MINUTE))));
        starts.extend((0..6).map(|i| ("Slow".to_string(), now.minus(i * 5 * MINUTE))));
        let found = restart_loops(&starts, now);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].app, "Crashy");
        assert!(found[0].title.contains("6 times"));
    }
}
