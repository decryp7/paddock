//! Local metrics for the dashboard (ADR 0010): memory, CPU, throughput and latency, kept on this machine only, in
//! four tiers that downsample as they age (5 s for an hour, 1 min for a day, 10 min for a week, 1 h for the
//! retention period). Every series is bounded; per-app and per-model counts keep at most [`MAX_KEYS`] keys per
//! bucket (the rest count as `other`). Pure: samples and times come in as values; the server persists the state.

use crate::time::{Millis, DAY, HOUR, MINUTE, SECOND};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

/// Most distinct apps or models counted per bucket.
pub const MAX_KEYS: usize = 24;
/// Latency histogram bounds in ms (the last bucket is open).
const LAT_BOUNDS: [u32; 14] = [10, 25, 50, 100, 200, 350, 500, 750, 1000, 2000, 5000, 10_000, 30_000, 120_000];

/// A history range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Range {
    /// The last hour.
    #[serde(rename = "1h")]
    Hour,
    /// The last day.
    #[serde(rename = "24h")]
    Day,
    /// The last week.
    #[serde(rename = "7d")]
    Week,
    /// The last 30 days.
    #[serde(rename = "30d")]
    Month,
}

impl Range {
    /// Its length.
    pub fn span(self) -> Millis {
        match self {
            Range::Hour => HOUR,
            Range::Day => DAY,
            Range::Week => 7 * DAY,
            Range::Month => 30 * DAY,
        }
    }
}

/// One reading of the machine.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Sample {
    /// When.
    pub at: Millis,
    /// RAM in use.
    pub ram_used: u64,
    /// RAM total.
    pub ram_total: u64,
    /// VRAM in use.
    pub vram_used: Option<u64>,
    /// VRAM total.
    pub vram_total: Option<u64>,
    /// Pitwall and its engines.
    pub pitwall_rss: u64,
    /// CPU 0–100.
    pub cpu_percent: f32,
    /// Prompt tokens/s over the sample period.
    pub prompt_tps: f64,
    /// Generated tokens/s over the sample period.
    pub gen_tps: f64,
}

/// A latency histogram.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Latency {
    counts: Vec<u32>,
}

impl Latency {
    /// Adds one latency.
    pub fn add(&mut self, ms: u64) {
        if self.counts.is_empty() {
            self.counts = vec![0; LAT_BOUNDS.len() + 1];
        }
        let i = LAT_BOUNDS.iter().position(|b| ms <= u64::from(*b)).unwrap_or(LAT_BOUNDS.len());
        self.counts[i] = self.counts[i].saturating_add(1);
    }

    /// Adds another histogram.
    pub fn merge(&mut self, o: &Latency) {
        for (i, c) in o.counts.iter().enumerate() {
            if self.counts.is_empty() {
                self.counts = vec![0; LAT_BOUNDS.len() + 1];
            }
            if let Some(x) = self.counts.get_mut(i) {
                *x = x.saturating_add(*c);
            }
        }
    }

    /// The `p` quantile (0–1) as the upper bound of its bucket; `None` without data.
    pub fn quantile(&self, p: f64) -> Option<u32> {
        let total: u64 = self.counts.iter().map(|c| u64::from(*c)).sum();
        if total == 0 {
            return None;
        }
        let want = (p.clamp(0.0, 1.0) * total as f64).ceil().max(1.0) as u64;
        let mut cum = 0u64;
        for (i, c) in self.counts.iter().enumerate() {
            cum += u64::from(*c);
            if cum >= want {
                return Some(LAT_BOUNDS.get(i).copied().unwrap_or(LAT_BOUNDS[LAT_BOUNDS.len() - 1] * 2));
            }
        }
        None
    }
}

/// One time bucket.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Bucket {
    /// Its start.
    pub start: Millis,
    samples: u32,
    ram_used: u128,
    ram_total: u64,
    vram_used: u128,
    vram_samples: u32,
    vram_total: Option<u64>,
    pitwall_rss: u128,
    cpu: f64,
    prompt_tps: f64,
    gen_tps: f64,
    /// Requests that ended in this bucket.
    pub requests: u32,
    latency: Latency,
    by_app: BTreeMap<String, u32>,
    by_model: BTreeMap<String, u32>,
}

fn count_key(map: &mut BTreeMap<String, u32>, key: &str, n: u32) {
    let k = if map.contains_key(key) || map.len() < MAX_KEYS {
        key
    } else {
        "other"
    };
    *map.entry(k.to_string()).or_default() += n;
}

impl Bucket {
    fn add_sample(&mut self, s: &Sample) {
        self.samples += 1;
        self.ram_used += u128::from(s.ram_used);
        self.ram_total = s.ram_total;
        if let Some(v) = s.vram_used {
            self.vram_used += u128::from(v);
            self.vram_samples += 1;
        }
        self.vram_total = s.vram_total.or(self.vram_total);
        self.pitwall_rss += u128::from(s.pitwall_rss);
        self.cpu += f64::from(s.cpu_percent);
        self.prompt_tps += s.prompt_tps;
        self.gen_tps += s.gen_tps;
    }

    fn merge(&mut self, o: &Bucket) {
        self.samples += o.samples;
        self.ram_used += o.ram_used;
        self.ram_total = self.ram_total.max(o.ram_total);
        self.vram_used += o.vram_used;
        self.vram_samples += o.vram_samples;
        self.vram_total = self.vram_total.or(o.vram_total);
        self.pitwall_rss += o.pitwall_rss;
        self.cpu += o.cpu;
        self.prompt_tps += o.prompt_tps;
        self.gen_tps += o.gen_tps;
        self.requests += o.requests;
        self.latency.merge(&o.latency);
        for (k, n) in &o.by_app {
            count_key(&mut self.by_app, k, *n);
        }
        for (k, n) in &o.by_model {
            count_key(&mut self.by_model, k, *n);
        }
    }

    fn avg(sum: u128, n: u32) -> u64 {
        if n == 0 {
            0
        } else {
            (sum / u128::from(n)).min(u128::from(u64::MAX)) as u64
        }
    }

    fn point(&self) -> HistoryPoint {
        let n = self.samples.max(1);
        HistoryPoint {
            at: self.start,
            ram_used: Self::avg(self.ram_used, self.samples),
            ram_total: self.ram_total,
            vram_used: (self.vram_samples > 0).then(|| Self::avg(self.vram_used, self.vram_samples)),
            vram_total: self.vram_total,
            pitwall_rss: Self::avg(self.pitwall_rss, self.samples),
            cpu_percent: (self.cpu / f64::from(n)) as f32,
            prompt_tps: self.prompt_tps / f64::from(n),
            gen_tps: self.gen_tps / f64::from(n),
            p50_ms: self.latency.quantile(0.5),
            p95_ms: self.latency.quantile(0.95),
            requests: self.requests,
        }
    }
}

/// A point of a history chart (`HistoryPoint` in `api.types.ts`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPoint {
    /// Bucket start.
    pub at: Millis,
    /// Average RAM in use.
    pub ram_used: u64,
    /// RAM total.
    pub ram_total: u64,
    /// Average VRAM in use.
    pub vram_used: Option<u64>,
    /// VRAM total.
    pub vram_total: Option<u64>,
    /// Average Pitwall memory.
    pub pitwall_rss: u64,
    /// Average CPU.
    pub cpu_percent: f32,
    /// Average prompt tokens/s.
    pub prompt_tps: f64,
    /// Average generated tokens/s.
    pub gen_tps: f64,
    /// Median latency.
    pub p50_ms: Option<u32>,
    /// 95th percentile latency.
    pub p95_ms: Option<u32>,
    /// Requests.
    pub requests: u32,
}

/// Counts per key, aligned with the points.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyedSeries {
    /// The app or model.
    pub key: String,
    /// One value per point.
    pub values: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Tier {
    step: Millis,
    cap: usize,
    buckets: VecDeque<Bucket>,
}

impl Tier {
    fn new(step: Millis, cap: usize) -> Tier {
        Tier {
            step,
            cap,
            buckets: VecDeque::new(),
        }
    }

    /// The bucket for `at`, creating it (and evicting the oldest) as needed; `None` for data older than the newest
    /// bucket that has no bucket of its own (late data that aged out is dropped).
    fn bucket(&mut self, at: Millis) -> Option<&mut Bucket> {
        let start = at - at % self.step;
        if self.buckets.back().is_none_or(|b| b.start < start) {
            self.buckets.push_back(Bucket {
                start,
                ..Bucket::default()
            });
            if self.buckets.len() > self.cap {
                self.buckets.pop_front();
            }
        }
        self.buckets.iter_mut().rev().find(|b| b.start == start)
    }
}

/// The metrics store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    version: u32,
    tiers: Vec<Tier>,
}

/// The file format version of persisted metrics.
pub const METRICS_VERSION: u32 = 1;

impl Metrics {
    /// An empty store keeping `retention_days` of hourly data.
    pub fn new(retention_days: u32) -> Metrics {
        Metrics {
            version: METRICS_VERSION,
            tiers: vec![
                Tier::new(5 * SECOND, 720),
                Tier::new(MINUTE, 1440),
                Tier::new(10 * MINUTE, 1008),
                Tier::new(HOUR, retention_days.clamp(1, 365) as usize * 24),
            ],
        }
    }

    /// Restores a persisted store; anything unreadable or of another version starts empty.
    pub fn restore(json: &str, retention_days: u32) -> Metrics {
        match serde_json::from_str::<Metrics>(json) {
            Ok(mut m) if m.version == METRICS_VERSION && m.tiers.len() == 4 => {
                if let Some(t) = m.tiers.get_mut(3) {
                    t.cap = retention_days.clamp(1, 365) as usize * 24;
                    while t.buckets.len() > t.cap {
                        t.buckets.pop_front();
                    }
                }
                m
            }
            _ => Metrics::new(retention_days),
        }
    }

    /// As JSON for persisting.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Applies a change to the bucket of `at` in every tier.
    fn each(&mut self, at: Millis, f: impl Fn(&mut Bucket)) {
        for t in &mut self.tiers {
            if let Some(b) = t.bucket(at) {
                f(b);
            }
        }
    }

    /// Records a machine sample.
    pub fn sample(&mut self, s: &Sample) {
        self.each(s.at, |b| b.add_sample(s));
    }

    /// Records a finished request.
    pub fn request(&mut self, at: Millis, app: &str, model: &str, latency_ms: u64) {
        self.each(at, |b| {
            b.requests = b.requests.saturating_add(1);
            b.latency.add(latency_ms);
            count_key(&mut b.by_app, app, 1);
            count_key(&mut b.by_model, model, 1);
        });
    }

    /// The history of a range, from the finest tier that covers it, at most `max_points` points (merged further).
    pub fn history(&self, range: Range, now: Millis, max_points: usize) -> (Millis, Vec<HistoryPoint>, Vec<KeyedSeries>, Vec<KeyedSeries>) {
        let tier = match range {
            Range::Hour => &self.tiers[0],
            Range::Day => &self.tiers[1],
            Range::Week => &self.tiers[2],
            Range::Month => &self.tiers[3],
        };
        let from = now.saturating_sub(range.span());
        let buckets: Vec<&Bucket> = tier.buckets.iter().filter(|b| b.start >= from && b.start <= now).collect();
        let group = buckets.len().div_ceil(max_points.max(1)).max(1);
        let merged: Vec<Bucket> = buckets
            .chunks(group)
            .map(|c| {
                let mut b = Bucket {
                    start: c[0].start,
                    ..Bucket::default()
                };
                for x in c {
                    b.merge(x);
                }
                b
            })
            .collect();
        let points: Vec<HistoryPoint> = merged.iter().map(Bucket::point).collect();
        let keyed = |get: fn(&Bucket) -> &BTreeMap<String, u32>| {
            let mut keys: Vec<String> = merged.iter().flat_map(|b| get(b).keys().cloned()).collect();
            keys.sort();
            keys.dedup();
            keys.into_iter()
                .map(|k| KeyedSeries {
                    values: merged.iter().map(|b| get(b).get(&k).copied().unwrap_or(0)).collect(),
                    key: k,
                })
                .collect::<Vec<_>>()
        };
        let by_app = keyed(|b| &b.by_app);
        let by_model = keyed(|b| &b.by_model);
        (tier.step * group as u64, points, by_app, by_model)
    }

    /// Requests per app in the buckets since `from` (the per-app "today" view uses the minute tier).
    pub fn requests_since(&self, from: Millis) -> BTreeMap<String, u32> {
        let mut out = BTreeMap::new();
        for b in self.tiers[1].buckets.iter().filter(|b| b.start >= from) {
            for (k, n) in &b.by_app {
                *out.entry(k.clone()).or_default() += n;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GIB;

    fn s(at: Millis, ram_gib: u64) -> Sample {
        Sample {
            at,
            ram_used: ram_gib * GIB,
            ram_total: 32 * GIB,
            vram_used: Some(GIB),
            vram_total: Some(12 * GIB),
            pitwall_rss: GIB,
            cpu_percent: 10.0,
            prompt_tps: 100.0,
            gen_tps: 20.0,
        }
    }

    #[test]
    fn latency_quantiles() {
        let mut l = Latency::default();
        assert_eq!(l.quantile(0.5), None);
        for ms in [5, 30, 30, 30, 400, 400, 400, 400, 400, 999_999] {
            l.add(ms);
        }
        assert_eq!(l.quantile(0.5), Some(500));
        assert_eq!(l.quantile(0.1), Some(10));
        assert_eq!(l.quantile(1.0), Some(240_000), "open bucket");
        let mut m = Latency::default();
        m.merge(&l);
        assert_eq!(m, l);
    }

    #[test]
    fn samples_downsample_and_stay_bounded() {
        let mut m = Metrics::new(1);
        for i in 0..2000u64 {
            m.sample(&s(i * 5 * SECOND, 8 + i % 2));
        }
        m.request(1000, "imt", "qwen", 120);
        m.request(1001, "pitcrew", "qwen", 900);
        let now = 2000 * 5 * SECOND;
        let (step, pts, apps, models) = m.history(Range::Hour, now, 1000);
        assert_eq!(step, 5 * SECOND);
        assert!(pts.len() <= 720 + 1);
        assert!(pts.iter().all(|p| p.ram_used >= 8 * GIB && p.ram_used <= 9 * GIB));
        assert!(apps.is_empty() && models.is_empty(), "the requests are older than an hour");
        let (step, pts, apps, models) = m.history(Range::Day, now, 60);
        assert!(pts.len() <= 60 && step >= MINUTE);
        assert_eq!(apps.iter().map(|k| k.key.as_str()).collect::<Vec<_>>(), vec!["imt", "pitcrew"]);
        assert_eq!(models[0].values.iter().sum::<u32>(), 2);
        assert_eq!(apps[0].values.len(), pts.len());
        let first = &pts[0];
        assert_eq!(first.requests, 2);
        assert!(first.p95_ms.unwrap() >= 750);
        assert_eq!(first.vram_total, Some(12 * GIB));
        assert!((first.gen_tps - 20.0).abs() < 1e-9);
        assert_eq!(m.tiers[0].buckets.len(), 720, "the 5 s tier keeps an hour");
        assert_eq!(m.requests_since(0).get("imt"), Some(&1));
        let (_, pts, _, _) = m.history(Range::Week, now, 10);
        assert!(!pts.is_empty());
        let (_, pts, _, _) = m.history(Range::Month, now, 10);
        assert!(!pts.is_empty());
    }

    #[test]
    fn keys_are_capped() {
        let mut m = Metrics::new(30);
        for i in 0..(MAX_KEYS + 5) {
            m.request(10, &format!("app{i}"), "m", 1);
        }
        let (_, _, apps, _) = m.history(Range::Hour, 20, 10);
        assert_eq!(apps.len(), MAX_KEYS + 1);
        assert!(apps.iter().any(|k| k.key == "other" && k.values[0] == 5));
    }

    #[test]
    fn persistence_is_tolerant() {
        let mut m = Metrics::new(30);
        m.sample(&s(0, 4));
        let json = m.to_json();
        assert_eq!(Metrics::restore(&json, 30), m);
        let shrunk = Metrics::restore(&json, 1);
        assert_eq!(shrunk.tiers[3].cap, 24);
        assert_eq!(Metrics::restore("garbage", 7), Metrics::new(7));
        let other = json.replace("\"version\":1", "\"version\":9");
        assert_eq!(Metrics::restore(&other, 7), Metrics::new(7));
        let mut big = Metrics::new(30);
        for h in 0..100 {
            big.sample(&s(h * HOUR, 1));
        }
        assert_eq!(Metrics::restore(&big.to_json(), 1).tiers[3].buckets.len(), 24);
    }

    #[test]
    fn late_samples_and_missing_vram() {
        let mut m = Metrics::new(1);
        m.sample(&s(10 * SECOND, 1));
        m.sample(&Sample {
            vram_used: None,
            vram_total: None,
            ..s(11 * SECOND, 3)
        });
        m.sample(&s(SECOND, 50));
        let (_, pts, _, _) = m.history(Range::Hour, 20 * SECOND, 100);
        assert_eq!(pts.len(), 1, "the late sample had no bucket and was dropped");
        assert_eq!(pts[0].ram_used, 2 * GIB);
        assert_eq!(pts[0].vram_used, Some(GIB));
        let empty = Bucket::default().point();
        assert_eq!((empty.ram_used, empty.vram_used, empty.p50_ms), (0, None, None));
        assert_eq!(Range::Week.span(), 7 * DAY);
    }
}
