//! History: what is stored, at which resolution, for how long, and how a range is answered (ADR 0002).
//!
//! Four tiers, each a downsampling of the one before: raw points every [`RAW_STEP_MS`] (kept `raw_hours`),
//! one-minute buckets (kept `minute_days`), quarter-hour buckets (kept `quarter_hour_days`) and hourly buckets (kept
//! `hour_days`). A bucket keeps the minimum, the average and the maximum, so a short spike or dip stays visible at
//! every zoom level.

use crate::metrics::SystemSample;
use crate::time::{Timestamp, DAY, HOUR, MINUTE, SECOND};
use serde::{Deserialize, Serialize};

/// One point of the system history.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPoint {
    /// Bucket start.
    pub at: Timestamp,
    /// Average CPU, percent.
    pub cpu: f32,
    /// Highest CPU, percent.
    pub cpu_max: f32,
    /// Lowest CPU, percent.
    #[serde(default)]
    pub cpu_min: f32,
    /// Average memory in use, percent.
    pub memory: f32,
    /// Highest memory in use, percent.
    pub memory_max: f32,
    /// Lowest memory in use, percent.
    #[serde(default)]
    pub memory_min: f32,
    /// Average disk reads, bytes per second.
    pub disk_read_bps: f64,
    /// Average disk writes, bytes per second.
    pub disk_write_bps: f64,
    /// Average received, bytes per second.
    pub net_rx_bps: f64,
    /// Average sent, bytes per second.
    pub net_tx_bps: f64,
    /// Average GPU, percent, when measured.
    pub gpu: Option<f32>,
    /// Highest temperature, °C, when measured.
    pub temperature: Option<f32>,
}

/// Raw points are this far apart (the agent averages its 1 s samples into them).
pub const RAW_STEP_MS: i64 = 5 * SECOND;

/// A storage tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tier {
    /// Every 5 seconds.
    Raw,
    /// Every minute.
    Minute,
    /// Every 15 minutes.
    QuarterHour,
    /// One-hour buckets.
    Hour,
}

impl Tier {
    /// Bucket width in milliseconds.
    pub fn step_ms(self) -> i64 {
        match self {
            Tier::Raw => RAW_STEP_MS,
            Tier::Minute => MINUTE,
            Tier::QuarterHour => 15 * MINUTE,
            Tier::Hour => HOUR,
        }
    }

    /// Every tier, finest first.
    pub const ALL: [Tier; 4] = [Tier::Raw, Tier::Minute, Tier::QuarterHour, Tier::Hour];
}

/// How long each tier is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Retention {
    /// Hours of raw points (1–72).
    pub raw_hours: u32,
    /// Days of minute buckets (1–30).
    pub minute_days: u32,
    /// Days of quarter-hour buckets (7–400).
    pub quarter_hour_days: u32,
    /// Days of hourly buckets kept.
    pub hour_days: u32,
}

impl Default for Retention {
    fn default() -> Self {
        Retention {
            raw_hours: 24,
            minute_days: 14,
            quarter_hour_days: 90,
            hour_days: 730,
        }
    }
}

impl Retention {
    /// The retention clamped to the allowed ranges.
    pub fn clamped(self) -> Retention {
        Retention {
            raw_hours: self.raw_hours.clamp(1, 72),
            minute_days: self.minute_days.clamp(1, 30),
            quarter_hour_days: self.quarter_hour_days.clamp(7, 400),
            hour_days: self.hour_days.clamp(30, 1100),
        }
    }

    /// Points older than this are deleted from `tier`.
    pub fn cutoff(self, tier: Tier, now: Timestamp) -> Timestamp {
        let r = self.clamped();
        now.minus(match tier {
            Tier::Raw => i64::from(r.raw_hours) * HOUR,
            Tier::Minute => i64::from(r.minute_days) * DAY,
            Tier::QuarterHour => i64::from(r.quarter_hour_days) * DAY,
            Tier::Hour => i64::from(r.hour_days) * DAY,
        })
    }
}

/// A range the UI asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryRange {
    /// The last hour.
    Hour,
    /// The last 24 hours.
    Day,
    /// The last 7 days.
    Week,
    /// The last 30 days.
    Month,
}

/// Where and how to read a range: the window, the tier to read and the resolution to return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// Start (inclusive).
    pub from: Timestamp,
    /// End (exclusive).
    pub to: Timestamp,
    /// The tier to read.
    pub tier: Tier,
    /// The bucket width of the answer (≥ the tier's step).
    pub resolution_ms: i64,
}

/// The window for a range ending at `end` (≈ 700 points for every range).
pub fn window(range: HistoryRange, end: Timestamp) -> Window {
    let (span, tier, resolution_ms) = match range {
        HistoryRange::Hour => (HOUR, Tier::Raw, RAW_STEP_MS),
        HistoryRange::Day => (DAY, Tier::Minute, 2 * MINUTE),
        HistoryRange::Week => (7 * DAY, Tier::QuarterHour, 15 * MINUTE),
        HistoryRange::Month => (30 * DAY, Tier::Hour, HOUR),
    };
    Window {
        from: end.minus(span),
        to: end,
        tier,
        resolution_ms,
    }
}

/// The window of a live view (no end chosen) asked for at `now`: it ends just after `now`, so a change recorded in the same
/// millisecond as the question is in the answer (a window's end is exclusive).
pub fn live_window(range: HistoryRange, now: Timestamp) -> Window {
    window(range, now.plus(1))
}

/// A point from one instantaneous sample.
pub fn point_from(s: &SystemSample) -> HistoryPoint {
    let (rx, tx) = s.network_totals();
    HistoryPoint {
        at: s.at,
        cpu: s.cpu.total,
        cpu_max: s.cpu.total,
        cpu_min: s.cpu.total,
        memory: s.memory_percent(),
        memory_max: s.memory_percent(),
        memory_min: s.memory_percent(),
        disk_read_bps: s.disk_io.read_bps,
        disk_write_bps: s.disk_io.write_bps,
        net_rx_bps: rx,
        net_tx_bps: tx,
        gpu: s.gpu.value().map(|g| g.utilization),
        temperature: s.hottest().map(|t| t.celsius),
    }
}

/// Minimum, average and maximum load of one logical core over a bucket, percent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CoreStat {
    /// Lowest load.
    pub min: f32,
    /// Average load.
    pub avg: f32,
    /// Highest load.
    pub max: f32,
}

/// Per-core load for one bucket (every logical core, in the order of `CpuSample::per_core`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CoreBucket {
    /// Bucket start.
    pub at: Timestamp,
    /// One entry per logical core.
    pub cores: Vec<CoreStat>,
}

/// The per-core raw point of a sample.
pub fn core_bucket_from(s: &SystemSample) -> CoreBucket {
    CoreBucket {
        at: s.at,
        cores: s.cpu.per_core.iter().map(|v| CoreStat { min: *v, avg: *v, max: *v }).collect(),
    }
}

/// Buckets per-core points like [`downsample`]: min of minima, mean of averages, max of maxima. A change in the
/// number of cores (hot-plug) starts a new bucket.
pub fn downsample_cores(points: &[CoreBucket], width_ms: i64) -> Vec<CoreBucket> {
    let mut out: Vec<(CoreBucket, u32)> = Vec::new();
    for p in points {
        let bucket = p.at.floor_to(width_ms);
        match out.last_mut() {
            Some((b, n)) if b.at == bucket && b.cores.len() == p.cores.len() => {
                for (a, s) in b.cores.iter_mut().zip(&p.cores) {
                    a.min = a.min.min(s.min);
                    a.max = a.max.max(s.max);
                    a.avg = (a.avg * *n as f32 + s.avg) / (*n as f32 + 1.0);
                }
                *n += 1;
            }
            _ => out.push((
                CoreBucket {
                    at: bucket,
                    cores: p.cores.clone(),
                },
                1,
            )),
        }
    }
    out.into_iter().map(|(b, _)| b).collect()
}

/// Buckets `points` (sorted by time) into buckets of `width_ms`: averages of averages, maxima of maxima.
pub fn downsample(points: &[HistoryPoint], width_ms: i64) -> Vec<HistoryPoint> {
    let mut out: Vec<HistoryPoint> = Vec::new();
    let mut acc = Acc::default();
    for p in points {
        let bucket = p.at.floor_to(width_ms);
        if acc.n > 0 && acc.at != bucket {
            out.push(acc.finish());
            acc = Acc::default();
        }
        acc.add(bucket, p);
    }
    if acc.n > 0 {
        out.push(acc.finish());
    }
    out
}

#[derive(Default)]
struct Acc {
    at: Timestamp,
    n: u32,
    cpu: f64,
    cpu_max: f32,
    cpu_min: f32,
    mem: f64,
    mem_max: f32,
    mem_min: f32,
    read: f64,
    write: f64,
    rx: f64,
    tx: f64,
    gpu: f64,
    gpu_n: u32,
    temp: Option<f32>,
}

impl Acc {
    fn add(&mut self, at: Timestamp, p: &HistoryPoint) {
        self.at = at;
        self.n += 1;
        self.cpu_min = if self.n == 1 { p.cpu_min } else { self.cpu_min.min(p.cpu_min) };
        self.mem_min = if self.n == 1 {
            p.memory_min
        } else {
            self.mem_min.min(p.memory_min)
        };
        self.cpu += f64::from(p.cpu);
        self.cpu_max = self.cpu_max.max(p.cpu_max);
        self.mem += f64::from(p.memory);
        self.mem_max = self.mem_max.max(p.memory_max);
        self.read += p.disk_read_bps;
        self.write += p.disk_write_bps;
        self.rx += p.net_rx_bps;
        self.tx += p.net_tx_bps;
        if let Some(g) = p.gpu {
            self.gpu += f64::from(g);
            self.gpu_n += 1;
        }
        if let Some(t) = p.temperature {
            self.temp = Some(self.temp.map_or(t, |m| m.max(t)));
        }
    }

    fn finish(&self) -> HistoryPoint {
        let n = f64::from(self.n);
        HistoryPoint {
            at: self.at,
            cpu: (self.cpu / n) as f32,
            cpu_max: self.cpu_max,
            cpu_min: self.cpu_min,
            memory: (self.mem / n) as f32,
            memory_max: self.mem_max,
            memory_min: self.mem_min,
            disk_read_bps: self.read / n,
            disk_write_bps: self.write / n,
            net_rx_bps: self.rx / n,
            net_tx_bps: self.tx / n,
            gpu: (self.gpu_n > 0).then(|| (self.gpu / f64::from(self.gpu_n)) as f32),
            temperature: self.temp,
        }
    }
}

/// One app's average use in one stored minute (the agent keeps the top [`TOP_APPS_PER_MINUTE`] per minute).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppMinute {
    /// Minute start.
    pub at: Timestamp,
    /// App name.
    pub app: String,
    /// Average CPU, percent of the machine.
    pub cpu: f32,
    /// Average memory, bytes.
    pub memory: u64,
}

/// Apps stored per minute (by CPU, plus the same number by memory).
pub const TOP_APPS_PER_MINUTE: usize = 10;

/// An app's average use over a range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Consumer {
    /// App name.
    pub app: String,
    /// Average CPU over the minutes recorded in the range, percent of the machine.
    pub cpu_avg: f32,
    /// Average memory, bytes.
    pub memory_avg: u64,
}

/// The `n` biggest consumers over the minutes in `rows`, by CPU time (the minutes an app was not in the top list
/// count as zero, so a short burst does not outrank a steady load).
pub fn top_consumers(rows: &[AppMinute], n: usize) -> Vec<Consumer> {
    use std::collections::{BTreeMap, BTreeSet};
    let minutes: BTreeSet<Timestamp> = rows.iter().map(|r| r.at).collect();
    let total = minutes.len().max(1) as f64;
    let mut sums: BTreeMap<&str, (f64, f64)> = BTreeMap::new();
    for r in rows {
        let e = sums.entry(r.app.as_str()).or_default();
        e.0 += f64::from(r.cpu);
        e.1 += r.memory as f64;
    }
    let mut out: Vec<Consumer> = sums
        .into_iter()
        .map(|(app, (cpu, mem))| Consumer {
            app: app.to_string(),
            cpu_avg: (cpu / total) as f32,
            memory_avg: (mem / total) as u64,
        })
        .collect();
    out.sort_by(|a, b| b.cpu_avg.total_cmp(&a.cpu_avg).then(b.memory_avg.cmp(&a.memory_avg)));
    out.truncate(n);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::fixtures::sample;
    use crate::metrics::{GpuSample, Reading, Temperature};

    fn p(at: i64, cpu: f32) -> HistoryPoint {
        HistoryPoint {
            at: Timestamp(at),
            cpu,
            cpu_max: cpu,
            cpu_min: cpu,
            memory: 50.0,
            memory_max: 50.0,
            memory_min: 50.0,
            ..Default::default()
        }
    }

    #[test]
    fn windows_pick_tiers_with_about_700_points() {
        let end = Timestamp(100 * DAY);
        for r in [HistoryRange::Hour, HistoryRange::Day, HistoryRange::Week, HistoryRange::Month] {
            let w = window(r, end);
            let points = w.to.since(w.from) / w.resolution_ms;
            assert!((600..=800).contains(&points), "{r:?}: {points}");
            assert!(w.resolution_ms >= w.tier.step_ms());
        }
        assert_eq!(Tier::ALL.map(Tier::step_ms), [5_000, 60_000, 900_000, 3_600_000]);
    }

    #[test]
    fn a_live_window_holds_what_happened_in_the_millisecond_it_was_asked() {
        let now = Timestamp(100 * DAY);
        for r in [HistoryRange::Hour, HistoryRange::Day, HistoryRange::Week, HistoryRange::Month] {
            let w = live_window(r, now);
            assert!(w.from <= now && now < w.to, "{r:?}: {w:?}");
            // A plain window ending at `now` leaves `now` out: the live view once missed a change made a moment before.
            assert!(now >= window(r, now).to);
            assert_eq!(w.to.since(w.from), window(r, now).to.since(window(r, now).from));
        }
    }

    #[test]
    fn retention_is_clamped_and_cuts_off() {
        let r = Retention {
            raw_hours: 0,
            minute_days: 99,
            quarter_hour_days: 1,
            hour_days: 5,
        }
        .clamped();
        assert_eq!((r.raw_hours, r.minute_days, r.quarter_hour_days, r.hour_days), (1, 30, 7, 30));
        let d = Retention::default();
        let now = Timestamp(1000 * DAY);
        assert_eq!(d.cutoff(Tier::Raw, now), now.minus(24 * HOUR));
        assert_eq!(d.cutoff(Tier::Minute, now), now.minus(14 * DAY));
        assert_eq!(d.cutoff(Tier::QuarterHour, now), now.minus(90 * DAY));
        assert_eq!(d.cutoff(Tier::Hour, now), now.minus(730 * DAY));
    }

    #[test]
    fn downsampling_keeps_averages_and_spikes() {
        let pts = vec![p(0, 10.0), p(1_000, 90.0), p(2_000, 20.0), p(60_000, 30.0)];
        let d = downsample(&pts, MINUTE);
        assert_eq!(d.len(), 2);
        assert!((d[0].cpu - 40.0).abs() < 1e-4);
        assert_eq!(d[0].cpu_max, 90.0);
        assert_eq!(d[0].cpu_min, 10.0);
        assert_eq!(d[0].at, Timestamp(0));
        assert_eq!(d[1].at, Timestamp(60_000));
        assert_eq!(d[0].gpu, None);
        assert!(downsample(&[], MINUTE).is_empty());
        let mut a = p(0, 1.0);
        a.gpu = Some(10.0);
        a.temperature = Some(60.0);
        let mut b = p(1, 1.0);
        b.gpu = Some(30.0);
        b.temperature = Some(70.0);
        let d = downsample(&[a, b, p(2, 1.0)], MINUTE);
        assert_eq!(d[0].gpu, Some(20.0));
        assert_eq!(d[0].temperature, Some(70.0));
    }

    #[test]
    fn core_buckets_keep_min_avg_max_per_core() {
        let c = |at, a: f32, b: f32| CoreBucket {
            at: Timestamp(at),
            cores: vec![CoreStat { min: a, avg: a, max: a }, CoreStat { min: b, avg: b, max: b }],
        };
        let d = downsample_cores(&[c(0, 10.0, 0.0), c(1_000, 90.0, 0.0), c(60_000, 5.0, 5.0)], MINUTE);
        assert_eq!(d.len(), 2);
        assert_eq!(
            d[0].cores[0],
            CoreStat {
                min: 10.0,
                avg: 50.0,
                max: 90.0
            }
        );
        assert_eq!(d[1].at, Timestamp(60_000));
        let mut three = c(2_000, 1.0, 1.0);
        three.cores.push(CoreStat::default());
        assert_eq!(
            downsample_cores(&[c(0, 1.0, 1.0), three], MINUTE).len(),
            2,
            "hot-plug starts a new bucket"
        );
        let s = crate::metrics::fixtures::sample(7);
        assert_eq!(core_bucket_from(&s).cores.len(), 8);
    }

    #[test]
    fn points_from_samples() {
        let mut s = sample(5);
        s.gpu = Reading::Available(GpuSample {
            name: "g".into(),
            utilization: 33.0,
            memory_used: 0,
            memory_total: 0,
        });
        s.temperatures = Reading::Available(vec![Temperature {
            label: "cpu".into(),
            celsius: 71.0,
            critical: None,
        }]);
        let pt = point_from(&s);
        assert_eq!(pt.cpu, 10.0);
        assert_eq!(pt.memory, 50.0);
        assert_eq!(pt.gpu, Some(33.0));
        assert_eq!(pt.temperature, Some(71.0));
        assert_eq!(pt.net_rx_bps, 1e5);
    }

    #[test]
    fn consumers_average_over_all_minutes() {
        let row = |at, app: &str, cpu, memory| AppMinute {
            at: Timestamp(at),
            app: app.into(),
            cpu,
            memory,
        };
        let rows = vec![
            row(0, "Steady", 20.0, 100),
            row(60_000, "Steady", 20.0, 100),
            row(120_000, "Steady", 20.0, 100),
            row(120_000, "Burst", 50.0, 300),
            row(120_000, "Tiny", 0.0, 1),
        ];
        let c = top_consumers(&rows, 2);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].app, "Steady");
        assert!((c[0].cpu_avg - 20.0).abs() < 1e-4);
        assert_eq!(c[1].app, "Burst");
        assert_eq!(c[1].memory_avg, 100);
        assert!(top_consumers(&[], 5).is_empty());
    }
}
