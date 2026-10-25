//! Machine metrics for the dashboard (CPU, memory, free disk of the runner host, and a runner folder's size), kept
//! locally with bounded retention and downsampling.
//!
//! Samples arrive every [`SAMPLE_EVERY`]. Three tiers keep them: the raw samples of the last 6 hours, 5-minute
//! averages of the last 7 days and hourly averages of the last 90 days — at most a few thousand points per series
//! whatever the uptime. Free disk is aggregated as the minimum (the number that makes jobs fail), CPU and memory as
//! averages, with the peak CPU kept too.

use crate::time::Timestamp;
use crate::usage::{Usage, UsageAcc};
use serde::de::DeserializeOwned;
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
    /// What all runners together used of the above (their processes and everything those started), ADR 0007.
    #[serde(default)]
    pub runners: Usage,
}

/// One runner's use at a time (or one aggregated point).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePoint {
    /// When (the start of the bucket for aggregated points).
    pub at: Timestamp,
    /// The use.
    #[serde(flatten)]
    pub usage: Usage,
}

/// A kind of point the tiered store can keep: how its samples are folded into a bucket and back into a point.
pub trait Series: Copy + Serialize + DeserializeOwned {
    /// The running aggregate of one bucket.
    type Acc: Copy + Eq + std::fmt::Debug + Serialize + DeserializeOwned;
    /// When the point is from.
    fn at(&self) -> Timestamp;
    /// A bucket starting at `start` holding `s`.
    fn begin(start: Timestamp, s: &Self) -> Self::Acc;
    /// Adds `s` to the bucket.
    fn absorb(acc: &mut Self::Acc, s: &Self);
    /// The point the bucket stands for.
    fn point(acc: &Self::Acc) -> Self;
    /// Where the bucket starts.
    fn start(acc: &Self::Acc) -> Timestamp;
}

/// The running aggregate of a machine bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineAcc {
    start: Timestamp,
    n: u64,
    cpu: u64,
    peak: u32,
    mem: u128,
    mem_total: u64,
    disk_min: u64,
    disk_total: u64,
    #[serde(default)]
    runners: UsageAcc,
}

impl Series for MachineSample {
    type Acc = MachineAcc;

    fn at(&self) -> Timestamp {
        self.at
    }

    fn begin(start: Timestamp, s: &MachineSample) -> MachineAcc {
        let mut runners = UsageAcc::default();
        runners.add(&s.runners);
        MachineAcc {
            start,
            n: 1,
            cpu: u64::from(s.cpu_permille),
            peak: s.cpu_peak_permille.max(s.cpu_permille),
            mem: u128::from(s.mem_used_bytes),
            mem_total: s.mem_total_bytes,
            disk_min: s.disk_free_bytes,
            disk_total: s.disk_total_bytes,
            runners,
        }
    }

    fn absorb(acc: &mut MachineAcc, s: &MachineSample) {
        acc.n += 1;
        acc.cpu += u64::from(s.cpu_permille);
        acc.peak = acc.peak.max(s.cpu_peak_permille).max(s.cpu_permille);
        acc.mem += u128::from(s.mem_used_bytes);
        acc.mem_total = s.mem_total_bytes;
        acc.disk_min = acc.disk_min.min(s.disk_free_bytes);
        acc.disk_total = s.disk_total_bytes;
        acc.runners.add(&s.runners);
    }

    fn point(acc: &MachineAcc) -> MachineSample {
        let n = acc.n.max(1);
        MachineSample {
            at: acc.start,
            cpu_permille: u32::try_from(acc.cpu / n).unwrap_or(u32::MAX),
            cpu_peak_permille: acc.peak,
            mem_used_bytes: u64::try_from(acc.mem / u128::from(n)).unwrap_or(u64::MAX),
            mem_total_bytes: acc.mem_total,
            disk_free_bytes: acc.disk_min,
            disk_total_bytes: acc.disk_total,
            runners: acc.runners.mean(),
        }
    }

    fn start(acc: &MachineAcc) -> Timestamp {
        acc.start
    }
}

/// The running aggregate of a runner-usage bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageBucket {
    start: Timestamp,
    acc: UsageAcc,
}

impl Series for UsagePoint {
    type Acc = UsageBucket;

    fn at(&self) -> Timestamp {
        self.at
    }

    fn begin(start: Timestamp, s: &UsagePoint) -> UsageBucket {
        let mut b = UsageBucket {
            start,
            acc: UsageAcc::default(),
        };
        b.acc.add(&s.usage);
        b
    }

    fn absorb(acc: &mut UsageBucket, s: &UsagePoint) {
        acc.acc.add(&s.usage);
    }

    fn point(acc: &UsageBucket) -> UsagePoint {
        UsagePoint {
            at: acc.start,
            usage: acc.acc.mean(),
        }
    }

    fn start(acc: &UsageBucket) -> Timestamp {
        acc.start
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(serialize = "P: Serialize", deserialize = "P: DeserializeOwned"))]
struct Tier<P: Series> {
    bucket_ms: u64,
    keep: usize,
    points: VecDeque<P>,
    #[serde(default)]
    open: Option<P::Acc>,
}

impl<P: Series> Tier<P> {
    fn new(bucket: Duration, keep: usize) -> Self {
        Tier {
            bucket_ms: u64::try_from(bucket.as_millis()).unwrap_or(u64::MAX).max(1),
            keep,
            points: VecDeque::new(),
            open: None,
        }
    }

    fn add(&mut self, s: &P) {
        let at = s.at().0;
        let start = Timestamp(at - at % self.bucket_ms);
        match &mut self.open {
            Some(acc) if P::start(acc) == start => P::absorb(acc, s),
            Some(acc) if P::start(acc) > start => {} // the clock went back: drop the sample rather than reorder
            open => {
                if let Some(done) = open.take() {
                    self.points.push_back(P::point(&done));
                }
                *open = Some(P::begin(start, s));
            }
        }
        while self.points.len() > self.keep {
            self.points.pop_front();
        }
    }

    fn all(&self) -> Vec<P> {
        let mut v: Vec<P> = self.points.iter().copied().collect();
        if let Some(acc) = &self.open {
            v.push(P::point(acc));
        }
        v
    }
}

/// How many points each tier of a [`TieredStore`] keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention {
    /// Raw samples (one per [`SAMPLE_EVERY`]).
    pub raw: usize,
    /// 5-minute averages.
    pub five_min: usize,
    /// Hourly averages.
    pub hourly: usize,
}

impl Retention {
    /// 6 hours raw, 7 days of 5 minutes, 90 days of hours: the machine series.
    pub const MACHINE: Retention = Retention {
        raw: 720,
        five_min: 7 * 24 * 12,
        hourly: 90 * 24,
    };
    /// 1 hour raw, 7 days of 5 minutes, 30 days of hours: one series per runner, kept smaller because there can be
    /// 16 of them in the state file.
    pub const RUNNER: Retention = Retention {
        raw: 120,
        five_min: 7 * 24 * 12,
        hourly: 30 * 24,
    };
}

/// The three tiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(serialize = "P: Serialize", deserialize = "P: DeserializeOwned"))]
pub struct TieredStore<P: Series> {
    raw: Tier<P>,
    five_min: Tier<P>,
    hourly: Tier<P>,
}

/// The machine's series.
pub type MetricsStore = TieredStore<MachineSample>;
/// One runner's series.
pub type UsageStore = TieredStore<UsagePoint>;

impl<P: Series> Default for TieredStore<P> {
    fn default() -> Self {
        Self::with_retention(Retention::MACHINE)
    }
}

impl UsageStore {
    /// An empty per-runner store with the smaller retention.
    pub fn for_runner() -> Self {
        Self::with_retention(Retention::RUNNER)
    }
}

impl<P: Series> TieredStore<P> {
    /// An empty store with the machine retention.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty store keeping `keep` points per tier.
    pub fn with_retention(keep: Retention) -> Self {
        TieredStore {
            raw: Tier::new(SAMPLE_EVERY, keep.raw),
            five_min: Tier::new(Duration::from_secs(300), keep.five_min),
            hourly: Tier::new(Duration::from_secs(3600), keep.hourly),
        }
    }

    /// Adds a sample to every tier.
    pub fn push(&mut self, s: P) {
        self.raw.add(&s);
        self.five_min.add(&s);
        self.hourly.add(&s);
    }

    /// The newest sample.
    pub fn latest(&self) -> Option<P> {
        self.raw.open.as_ref().map(P::point)
    }

    /// The points of the last `range` up to `now`, from the finest tier that covers it.
    pub fn query(&self, now: Timestamp, range: Duration) -> Vec<P> {
        let from = Timestamp(now.0.saturating_sub(u64::try_from(range.as_millis()).unwrap_or(u64::MAX)));
        let tier = if range <= Duration::from_secs(6 * 3600) && self.raw.keep * SAMPLE_EVERY.as_secs() as usize >= range.as_secs() as usize
        {
            &self.raw
        } else if range <= Duration::from_secs(7 * 24 * 3600) {
            &self.five_min
        } else {
            &self.hourly
        };
        tier.all().into_iter().filter(|p| p.at() >= from && p.at() <= now).collect()
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
            runners: Usage::tick(cpu / 2, 40, 3),
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
    fn runner_use_is_aggregated_next_to_the_machine() {
        let mut m = MetricsStore::new();
        let mut a = s(0, 100, 500);
        a.runners = Usage::tick(40, 1_000, 2);
        let mut b = s(60, 300, 500);
        b.runners = Usage::tick(120, 3_000, 5);
        m.push(a);
        m.push(b);
        let five = m.query(Timestamp(400_000), Duration::from_secs(24 * 3600));
        assert_eq!(five.len(), 1);
        let r = five[0].runners;
        assert_eq!(
            (r.cpu_permille, r.cpu_peak_permille, r.mem_bytes, r.mem_peak_bytes, r.processes),
            (80, 120, 2_000, 3_000, 5)
        );
    }

    #[test]
    fn history_written_before_runner_scoped_metrics_still_loads() {
        let mut m = MetricsStore::new();
        m.push(s(0, 100, 500));
        let mut v = serde_json::to_value(&m).unwrap();
        // Strip the new fields everywhere, as an older Pitcrew wrote it.
        fn strip(v: &mut serde_json::Value) {
            match v {
                serde_json::Value::Object(o) => {
                    o.remove("runners");
                    o.values_mut().for_each(strip);
                }
                serde_json::Value::Array(a) => a.iter_mut().for_each(strip),
                _ => {}
            }
        }
        strip(&mut v);
        assert!(!v.to_string().contains("runners"));
        let old: MetricsStore = serde_json::from_value(v).unwrap();
        assert_eq!(old.latest().map(|p| p.runners), Some(Usage::default()));
    }

    #[test]
    fn a_runner_series_keeps_its_own_smaller_retention() {
        let mut u = UsageStore::for_runner();
        assert!(u.is_empty() && u.latest().is_none());
        for i in 0..20_000u64 {
            u.push(UsagePoint {
                at: Timestamp(i * 30_000),
                usage: Usage::tick((i % 1000) as u32, i, 1),
            });
        }
        assert!(u.len() <= 120 + 7 * 24 * 12 + 30 * 24);
        let now = Timestamp(19_999 * 30_000);
        assert!(
            u.query(now, Duration::from_secs(1800)).len() <= 61,
            "short ranges come from the raw tier"
        );
        let day = u.query(now, Duration::from_secs(24 * 3600));
        assert!(
            day.len() > 200 && day.len() <= 290,
            "a day comes from the 5-minute tier, not a thin raw tier: {}",
            day.len()
        );
        let month = u.query(now, Duration::from_secs(30 * 24 * 3600));
        assert!(month.len() > 100);
        let json = serde_json::to_string(&u).unwrap();
        assert_eq!(serde_json::from_str::<UsageStore>(&json).unwrap(), u);
        assert!(json.contains("\"cpuPermille\""), "usage is flattened into the point");
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
