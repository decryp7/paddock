//! Machine metrics for the dashboard (CPU, memory, free disk of the runner host, and a runner folder's size), kept
//! locally with bounded retention and downsampling.
//!
//! Samples arrive every [`SAMPLE_EVERY`]. Three tiers keep them: the raw samples of the last 6 hours, 5-minute
//! averages of the last 7 days and hourly averages of the last 90 days — at most a few thousand points per series
//! whatever the uptime. Free disk is aggregated as the minimum (the number that makes jobs fail), CPU and memory as
//! averages, with the peak CPU kept too.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::Duration;

/// How often the service samples.
pub const SAMPLE_EVERY: Duration = Duration::from_secs(30);

/// One sample (or one aggregated point).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineSample {
    /// When (the start of the bucket for aggregated points).
    pub at: Timestamp,
    /// CPU use of the whole machine, per mille (average).
    pub cpu_permille: u32,
    /// Peak CPU use in the bucket, per mille.
    pub cpu_peak_permille: u32,
    /// Memory in use.
    pub mem_used_bytes: u64,
    /// Memory installed.
    pub mem_total_bytes: u64,
    /// Free space on the disk of the runners' folders (minimum over the bucket).
    pub disk_free_bytes: u64,
    /// Size of that disk.
    pub disk_total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Tier {
    bucket_ms: u64,
    keep: usize,
    points: VecDeque<MachineSample>,
    #[serde(default)]
    open: Option<Acc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Acc {
    start: Timestamp,
    n: u64,
    cpu: u64,
    peak: u32,
    mem: u128,
    mem_total: u64,
    disk_min: u64,
    disk_total: u64,
}

impl Acc {
    fn new(start: Timestamp, s: &MachineSample) -> Self {
        Acc {
            start,
            n: 1,
            cpu: u64::from(s.cpu_permille),
            peak: s.cpu_peak_permille.max(s.cpu_permille),
            mem: u128::from(s.mem_used_bytes),
            mem_total: s.mem_total_bytes,
            disk_min: s.disk_free_bytes,
            disk_total: s.disk_total_bytes,
        }
    }

    fn add(&mut self, s: &MachineSample) {
        self.n += 1;
        self.cpu += u64::from(s.cpu_permille);
        self.peak = self.peak.max(s.cpu_peak_permille).max(s.cpu_permille);
        self.mem += u128::from(s.mem_used_bytes);
        self.mem_total = s.mem_total_bytes;
        self.disk_min = self.disk_min.min(s.disk_free_bytes);
        self.disk_total = s.disk_total_bytes;
    }

    fn point(&self) -> MachineSample {
        let n = self.n.max(1);
        MachineSample {
            at: self.start,
            cpu_permille: u32::try_from(self.cpu / n).unwrap_or(u32::MAX),
            cpu_peak_permille: self.peak,
            mem_used_bytes: u64::try_from(self.mem / u128::from(n)).unwrap_or(u64::MAX),
            mem_total_bytes: self.mem_total,
            disk_free_bytes: self.disk_min,
            disk_total_bytes: self.disk_total,
        }
    }
}

impl Tier {
    fn new(bucket: Duration, keep: usize) -> Self {
        Tier {
            bucket_ms: u64::try_from(bucket.as_millis()).unwrap_or(u64::MAX).max(1),
            keep,
            points: VecDeque::new(),
            open: None,
        }
    }

    fn add(&mut self, s: &MachineSample) {
        let start = Timestamp(s.at.0 - s.at.0 % self.bucket_ms);
        match &mut self.open {
            Some(acc) if acc.start == start => acc.add(s),
            Some(acc) if acc.start > start => {} // the clock went back: drop the sample rather than reorder
            open => {
                if let Some(done) = open.take() {
                    self.points.push_back(done.point());
                }
                *open = Some(Acc::new(start, s));
            }
        }
        while self.points.len() > self.keep {
            self.points.pop_front();
        }
    }

    fn all(&self) -> Vec<MachineSample> {
        let mut v: Vec<MachineSample> = self.points.iter().copied().collect();
        if let Some(acc) = &self.open {
            v.push(acc.point());
        }
        v
    }
}

/// The three tiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsStore {
    raw: Tier,
    five_min: Tier,
    hourly: Tier,
}

impl Default for MetricsStore {
    fn default() -> Self {
        MetricsStore {
            // 6 h of 30 s samples.
            raw: Tier::new(SAMPLE_EVERY, 720),
            five_min: Tier::new(Duration::from_secs(300), 7 * 24 * 12),
            hourly: Tier::new(Duration::from_secs(3600), 90 * 24),
        }
    }
}

impl MetricsStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a sample to every tier.
    pub fn push(&mut self, s: MachineSample) {
        self.raw.add(&s);
        self.five_min.add(&s);
        self.hourly.add(&s);
    }

    /// The newest sample.
    pub fn latest(&self) -> Option<MachineSample> {
        self.raw.open.map(|a| a.point())
    }

    /// The points of the last `range` up to `now`, from the finest tier that covers it.
    pub fn query(&self, now: Timestamp, range: Duration) -> Vec<MachineSample> {
        let from = Timestamp(now.0.saturating_sub(u64::try_from(range.as_millis()).unwrap_or(u64::MAX)));
        let tier = if range <= Duration::from_secs(6 * 3600) {
            &self.raw
        } else if range <= Duration::from_secs(7 * 24 * 3600) {
            &self.five_min
        } else {
            &self.hourly
        };
        tier.all().into_iter().filter(|p| p.at >= from && p.at <= now).collect()
    }

    /// Number of points kept (bounded).
    pub fn len(&self) -> usize {
        self.raw.points.len() + self.five_min.points.len() + self.hourly.points.len()
    }

    /// Whether nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.raw.open.is_none()
    }
}

/// A runner folder's size over time (hourly maximum, 90 days).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SizeSeries {
    points: VecDeque<(Timestamp, u64)>,
}

/// Most points a [`SizeSeries`] keeps.
pub const MAX_SIZE_POINTS: usize = 90 * 24;

impl SizeSeries {
    /// Records a size; samples within the same hour keep the largest.
    pub fn record(&mut self, at: Timestamp, bytes: u64) {
        let hour = Timestamp(at.0 - at.0 % 3_600_000);
        match self.points.back_mut() {
            Some((t, b)) if *t == hour => *b = (*b).max(bytes),
            Some((t, _)) if *t > hour => {}
            _ => self.points.push_back((hour, bytes)),
        }
        while self.points.len() > MAX_SIZE_POINTS {
            self.points.pop_front();
        }
    }

    /// The points of the last `range` up to `now`.
    pub fn query(&self, now: Timestamp, range: Duration) -> Vec<(Timestamp, u64)> {
        let from = Timestamp(now.0.saturating_sub(u64::try_from(range.as_millis()).unwrap_or(u64::MAX)));
        self.points.iter().copied().filter(|(t, _)| *t >= from && *t <= now).collect()
    }
}

/// CPU per mille from the share of busy time (0.0-1.0, clamped).
pub fn permille(share: f32) -> u32 {
    if share.is_nan() {
        return 0;
    }
    (share.clamp(0.0, 1.0) * 1000.0).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(at_s: u64, cpu: u32, disk: u64) -> MachineSample {
        MachineSample {
            at: Timestamp(at_s * 1000),
            cpu_permille: cpu,
            cpu_peak_permille: cpu,
            mem_used_bytes: 100,
            mem_total_bytes: 1000,
            disk_free_bytes: disk,
            disk_total_bytes: 10_000,
        }
    }

    #[test]
    fn aggregates_into_buckets() {
        let mut m = MetricsStore::new();
        assert!(m.is_empty());
        m.push(s(0, 100, 500));
        m.push(s(60, 300, 400));
        m.push(s(330, 500, 450));
        assert_eq!(m.latest().map(|p| p.cpu_permille), Some(500));
        let five = m.query(Timestamp(400_000), Duration::from_secs(24 * 3600));
        assert_eq!(five.len(), 2);
        assert_eq!(five[0].cpu_permille, 200, "average");
        assert_eq!(five[0].cpu_peak_permille, 300, "peak");
        assert_eq!(five[0].disk_free_bytes, 400, "minimum free disk");
        let raw = m.query(Timestamp(400_000), Duration::from_secs(3600));
        assert_eq!(raw.len(), 3);
        let hourly = m.query(Timestamp(400_000), Duration::from_secs(30 * 24 * 3600));
        assert_eq!(hourly.len(), 1);
        assert_eq!(hourly[0].cpu_permille, 300);
        assert!(m.len() >= 3);
    }

    #[test]
    fn retention_is_bounded_and_late_samples_dropped() {
        let mut m = MetricsStore::new();
        for i in 0..20_000u64 {
            m.push(s(i * 30, 10, 1));
        }
        assert!(m.len() <= 720 + 7 * 24 * 12 + 90 * 24);
        let before = m.clone();
        m.push(s(0, 999, 0));
        assert_eq!(m.latest(), before.latest(), "a sample from the past is ignored");
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<MetricsStore>(&json).unwrap(), m);
    }

    #[test]
    fn folder_sizes_keep_the_hourly_maximum() {
        let mut z = SizeSeries::default();
        z.record(Timestamp(1000), 5);
        z.record(Timestamp(2000), 9);
        z.record(Timestamp(1500), 7);
        z.record(Timestamp(3_600_000), 3);
        z.record(Timestamp(10), 100);
        assert_eq!(
            z.query(Timestamp(3_600_000), Duration::from_secs(7200)),
            vec![(Timestamp(0), 9), (Timestamp(3_600_000), 3)]
        );
        for i in 0..(MAX_SIZE_POINTS as u64 + 5) {
            z.record(Timestamp(7_200_000 + i * 3_600_000), i);
        }
        assert_eq!(z.points.len(), MAX_SIZE_POINTS);
        assert_eq!(permille(0.4567), 457);
        assert_eq!(permille(2.0), 1000);
        assert_eq!(permille(f32::NAN), 0);
    }
}
