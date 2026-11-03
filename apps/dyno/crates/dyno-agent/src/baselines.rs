//! "Is this normal?" in the agent (WP-25): learns baselines from the history tiers (cached for half an hour), compares
//! the last few minutes with them and answers the Baselines request; optionally raises an alarm when something stays
//! strongly above its usual for several minutes. The statistics are in `dyno_core::baseline`; this module only gathers
//! values from the store and the recent samples.

use crate::agent::{lock, now, Shared};
use crate::store::{Store, FIRST_SEEN_KEY};
use dyno_core::alarms::feed::from_baseline;
use dyno_core::baseline::{
    judge, learn, learning_days, learning_status, ready_time, Baseline, BaselineItem, BaselineMetric, BaselineOverview, BaselineSettings,
    Judgement, Learning, Persistence, State, Strength,
};
use dyno_core::hardware::Confidence;
use dyno_core::history::{AppMinute, HistoryPoint, Tier};
use dyno_core::metrics::SystemSample;
use dyno_core::time::{LocalZone, Timestamp, DAY, MINUTE};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Remembers when Dyno first saw this machine, at every agent start: the learning counts from the first start, also of an
/// agent that no window has asked yet (the Windows `Run` entry at logon). None when the store cannot say.
///
/// A stored time that cannot be read starts over from now, and one more than a day after now (a clock that once ran
/// ahead) is moved back to now, so the first sighting only ever moves earlier; both are logged.
pub fn on_start(store: &Store, t: Timestamp) -> Option<Timestamp> {
    if let Ok(Some(raw)) = store.meta(FIRST_SEEN_KEY) {
        if raw.parse::<i64>().is_err() {
            log::warn!(Raw = raw.as_str(); "Baselines: stored first-seen time unreadable; learning starts from now");
        }
    }
    match store.first_seen(t) {
        Ok(f) if f.since(t) > DAY => {
            log::warn!(FirstSeenMs = f.millis(), NowMs = t.millis(); "Baselines: first-seen time is in the future; moved back to now");
            match store.set_meta(FIRST_SEEN_KEY, &t.millis().to_string()) {
                Ok(()) => Some(t),
                Err(e) => {
                    log::warn!(ErrorChain = e.to_string().as_str(); "Baselines: could not store the corrected first-seen time");
                    Some(t)
                }
            }
        }
        Ok(f) => Some(f),
        Err(e) => {
            log::warn!(ErrorChain = e.to_string().as_str(); "Baselines: first-seen time unavailable; learning starts over");
            None
        }
    }
}

/// Logs where the learning stands at start (ER-27): the data folder's kind and the store's (a fresh or in-memory store
/// restarts the learning), the first-seen time and the days counted by the rule.
pub fn log_start(
    folder: &str,
    store: &str,
    first_seen: Option<Timestamp>,
    settings: &BaselineSettings,
    t: Timestamp,
    zone: &dyn LocalZone,
) {
    let days = first_seen.map_or(0, |f| learning_days(f, t, zone));
    let needed = settings.clamped().learning_days;
    let ready_at = first_seen.map_or(-1, |f| ready_time(f, needed, zone).millis());
    log::info!(
        DataFolder = folder,
        Store = store,
        FirstSeenMs = first_seen.map_or(-1, |f| f.millis()),
        DaysDone = days,
        DaysNeeded = needed,
        ReadyAtMs = ready_at,
        Enabled = settings.enabled,
        Rule = dyno_core::baseline::LEARNING_RULE;
        "Baselines learning at start"
    );
}

/// Baselines are learned from this many days of history.
pub const LEARN_DAYS: i64 = 28;
/// A learned set is reused this long.
pub const CACHE_FOR: Duration = Duration::from_secs(30 * 60);
/// The current value is the mean of this many minutes.
pub const RECENT_MINUTES: i64 = 5;
/// Alarms are evaluated this often ...
pub const EVALUATE_EVERY: Duration = Duration::from_secs(60);
/// ... and need this many evaluations in a row (five minutes) before they fire.
pub const ALARM_STREAK: u32 = 5;
/// Apps considered: the busiest by memory.
pub const MAX_APPS: usize = 12;
/// Items returned at most.
pub const MAX_ITEMS: usize = 30;

type AppKey = (String, BaselineMetric);
/// `(time, value)` pairs.
type Series = Vec<(Timestamp, f64)>;

struct Learned {
    at: Instant,
    system: Vec<(BaselineMetric, Baseline)>,
    apps: HashMap<AppKey, Baseline>,
    app_names: Vec<String>,
}

/// The cache and the alarm bookkeeping.
#[derive(Default)]
pub struct BaselineCentre {
    learned: Mutex<Option<Learned>>,
    persistence: Mutex<Persistence>,
    raised: Mutex<HashSet<String>>,
}

impl BaselineCentre {
    /// An empty centre.
    pub fn new() -> BaselineCentre {
        BaselineCentre::default()
    }

    /// Drops what was learned (settings changed, or history was cleared).
    pub fn forget(&self) {
        *lock(&self.learned) = None;
    }

    /// Learns from the store if the cache is stale. `apps` are the apps to learn per-app baselines for.
    fn ensure(&self, store: &Store, apps: &[String], t: Timestamp, zone: &dyn LocalZone) {
        let mut cache = lock(&self.learned);
        let fresh = cache
            .as_ref()
            .is_some_and(|l| l.at.elapsed() < CACHE_FOR && apps.iter().all(|a| l.app_names.contains(a)));
        if fresh {
            return;
        }
        let from = t.minus(LEARN_DAYS * DAY);
        let points = store.points(Tier::QuarterHour, from, t).unwrap_or_default();
        let system = BaselineMetric::SYSTEM
            .iter()
            .map(|m| (*m, learn(&series(&points, *m), zone)))
            .collect();
        let rows = if apps.is_empty() {
            Vec::new()
        } else {
            store.app_minutes_named(apps, from, t).unwrap_or_default()
        };
        let mut by_app: BTreeMap<&str, (Series, Series)> = BTreeMap::new();
        for r in &rows {
            let e = by_app.entry(r.app.as_str()).or_default();
            e.0.push((r.at, f64::from(r.cpu)));
            e.1.push((r.at, r.memory as f64));
        }
        let mut learned = HashMap::new();
        for (app, (cpu, mem)) in by_app {
            learned.insert((app.to_string(), BaselineMetric::AppCpu), learn(&cpu, zone));
            learned.insert((app.to_string(), BaselineMetric::AppMemory), learn(&mem, zone));
        }
        *cache = Some(Learned {
            at: Instant::now(),
            system,
            apps: learned,
            app_names: apps.to_vec(),
        });
    }

    /// The comparison of the last minutes with the baselines.
    pub fn overview(&self, shared: &Shared) -> BaselineOverview {
        let t = now();
        let (settings, zone, first_seen, recent, rows) = {
            let core = lock(&shared.core);
            let first = core.store.first_seen(t).ok();
            let recent: Vec<SystemSample> = core
                .recent
                .iter()
                .filter(|s| t.since(s.at) <= RECENT_MINUTES * MINUTE)
                .cloned()
                .collect();
            let rows = core.store.app_minutes(t.minus(RECENT_MINUTES * MINUTE), t).unwrap_or_default();
            (core.settings.baselines, core.zone.clone(), first, recent, rows)
        };
        let learning = learning_status(&settings, first_seen, t, zone.as_ref());
        let mut overview = BaselineOverview {
            at: t,
            learning,
            items: Vec::new(),
        };
        if !matches!(learning, Learning::Ready { .. }) {
            return overview;
        }
        let current_apps = current_apps(&rows);
        let names: Vec<String> = current_apps.iter().map(|a| a.0.clone()).collect();
        {
            let core = lock(&shared.core);
            self.ensure(&core.store, &names, t, zone.as_ref());
        }
        let cache = lock(&self.learned);
        let Some(l) = cache.as_ref() else { return overview };
        let mut items = Vec::new();
        for (metric, baseline) in &l.system {
            if let Some(v) = system_value(&recent, *metric) {
                items.push(item("System", *metric, v, &judge(baseline, *metric, v, t, zone.as_ref())));
            }
        }
        for (app, cpu, mem) in &current_apps {
            for (metric, v) in [(BaselineMetric::AppCpu, *cpu), (BaselineMetric::AppMemory, *mem)] {
                if let Some(b) = l.apps.get(&(app.clone(), metric)) {
                    items.push(item(app, metric, v, &judge(b, metric, v, t, zone.as_ref())));
                }
            }
        }
        // Unusual first, then the strongest, then the rest in a stable order.
        items.sort_by_key(|i| {
            (
                std::cmp::Reverse(rank(&i.judgement)),
                i.subject != "System",
                i.subject.clone(),
                i.label.clone(),
            )
        });
        items.truncate(MAX_ITEMS);
        overview.items = items;
        overview
    }

    /// Raises an alarm for each value that stayed strongly above its usual for [`ALARM_STREAK`] evaluations, and returns
    /// it when the value is back (only when the user turned baseline alarms on).
    pub fn evaluate_alarms(&self, shared: &Arc<Shared>) {
        let settings: BaselineSettings = lock(&shared.core).settings.baselines;
        if !settings.enabled || !settings.alarms {
            let had: Vec<String> = lock(&self.raised).drain().collect();
            for key in had {
                self.release(shared, &key);
            }
            return;
        }
        let o = self.overview(shared);
        let t = now();
        let mut seen = Vec::new();
        for i in &o.items {
            let key = format!("{}:{:?}", i.subject, i.metric);
            seen.push(key.clone());
            let strong_high = matches!(i.judgement.state, State::Higher(Strength::Strong)) && i.judgement.confidence >= Confidence::Medium;
            let fire = lock(&self.persistence).update(&key, strong_high, ALARM_STREAK);
            if fire {
                let value = parse_current(&i.current);
                let obs = from_baseline(&i.subject, i.metric, value, &i.judgement, true);
                observe(shared, obs, t);
                lock(&self.raised).insert(key);
            } else if !strong_high && lock(&self.raised).remove(&key) {
                self.release(shared, &key);
            }
        }
        lock(&self.persistence).retain(&seen);
    }

    fn release(&self, shared: &Arc<Shared>, key: &str) {
        let Some((subject, metric)) = key.split_once(':') else { return };
        let metric = BaselineMetric::SYSTEM
            .iter()
            .chain(BaselineMetric::APP.iter())
            .find(|m| format!("{m:?}") == metric)
            .copied();
        if let Some(m) = metric {
            let j = Judgement::not_judged("Usual again", "");
            observe(shared, from_baseline(subject, m, 0.0, &j, false), now());
        }
    }
}

fn parse_current(text: &str) -> f64 {
    text.chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>()
        .parse()
        .unwrap_or(0.0)
}

fn rank(j: &Judgement) -> u8 {
    match j.state {
        State::Higher(Strength::Strong) | State::Lower(Strength::Strong) => 3,
        State::Higher(Strength::Notable) | State::Lower(Strength::Notable) => 2,
        State::Usual => 1,
        State::NotJudged | State::NoBaseline => 0,
    }
}

fn item(subject: &str, metric: BaselineMetric, v: f64, j: &Judgement) -> BaselineItem {
    BaselineItem {
        id: format!("{}:{subject}", metric.key()),
        subject: subject.into(),
        metric,
        label: metric.label().into(),
        current: metric.format(v),
        judgement: j.clone(),
    }
}

fn observe(shared: &Arc<Shared>, o: dyno_core::alarms::feed::Observation, t: Timestamp) {
    let mut core = lock(&shared.core);
    let core = &mut *core;
    let changes = core.alarms.observe(&core.store, o, t);
    for c in &changes {
        let _ = core.recorder.on_alarm(&core.store, c, t);
    }
    crate::sampler::push_alarm_changes(shared, changes);
}

/// Called by the background thread every minute.
pub fn evaluate_alarms(shared: &Arc<Shared>) {
    shared.baselines.evaluate_alarms(shared);
}

/// One metric's history as `(time, value)`; points without a reading are skipped.
fn series(points: &[HistoryPoint], m: BaselineMetric) -> Series {
    points
        .iter()
        .filter_map(|p| {
            let v = match m {
                BaselineMetric::Cpu => Some(f64::from(p.cpu)),
                BaselineMetric::Memory => Some(f64::from(p.memory)),
                BaselineMetric::DiskWrite => Some(p.disk_write_bps),
                BaselineMetric::Network => Some(p.net_rx_bps + p.net_tx_bps),
                BaselineMetric::Temperature => p.temperature.map(f64::from),
                BaselineMetric::Gpu => p.gpu.map(f64::from),
                BaselineMetric::AppCpu | BaselineMetric::AppMemory => None,
            }?;
            Some((p.at, v))
        })
        .collect()
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, n) = values.fold((0.0, 0u32), |(s, n), v| (s + v, n + 1));
    (n > 0).then(|| sum / f64::from(n))
}

/// The recent mean of a system metric from the recent samples.
fn system_value(recent: &[SystemSample], m: BaselineMetric) -> Option<f64> {
    match m {
        BaselineMetric::Cpu => mean(recent.iter().map(|s| f64::from(s.cpu.total))),
        BaselineMetric::Memory => mean(recent.iter().map(|s| f64::from(s.memory_percent()))),
        BaselineMetric::DiskWrite => mean(recent.iter().map(|s| s.disk_io.write_bps)),
        BaselineMetric::Network => mean(recent.iter().map(|s| {
            let (rx, tx) = s.network_totals();
            rx + tx
        })),
        BaselineMetric::Temperature => mean(recent.iter().filter_map(|s| s.hottest().map(|t| f64::from(t.celsius)))),
        BaselineMetric::Gpu => mean(recent.iter().filter_map(|s| s.gpu.value().map(|g| f64::from(g.utilization)))),
        BaselineMetric::AppCpu | BaselineMetric::AppMemory => None,
    }
}

/// Apps in the last minutes with their mean CPU and memory, the biggest by memory first.
fn current_apps(rows: &[AppMinute]) -> Vec<(String, f64, f64)> {
    let mut by: BTreeMap<&str, (f64, f64, u32)> = BTreeMap::new();
    for r in rows {
        let e = by.entry(r.app.as_str()).or_default();
        e.0 += f64::from(r.cpu);
        e.1 += r.memory as f64;
        e.2 += 1;
    }
    let mut v: Vec<(String, f64, f64)> = by
        .into_iter()
        .map(|(a, (c, m, n))| (a.to_string(), c / f64::from(n), m / f64::from(n)))
        .collect();
    v.sort_by(|a, b| b.2.total_cmp(&a.2));
    v.truncate(MAX_APPS);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(at: i64, cpu: f32) -> HistoryPoint {
        HistoryPoint {
            at: Timestamp(at),
            cpu,
            memory: 40.0,
            temperature: Some(50.0),
            ..Default::default()
        }
    }

    #[test]
    fn items_have_stable_ids_from_the_metric_and_the_subject() {
        let j = judge(
            &Baseline::default(),
            BaselineMetric::Cpu,
            5.0,
            Timestamp(0),
            &dyno_core::time::FixedZone::UTC,
        );
        assert_eq!(item("System", BaselineMetric::Cpu, 5.0, &j).id, "cpu:System");
        assert_eq!(item("Chrome", BaselineMetric::AppMemory, 5.0, &j).id, "appMemory:Chrome");
        for m in BaselineMetric::SYSTEM.iter().chain(BaselineMetric::APP.iter()) {
            assert_eq!(serde_json::to_value(m).unwrap(), m.key(), "the key is the wire name");
        }
    }

    #[test]
    fn series_skip_missing_readings() {
        let mut p = point(0, 10.0);
        p.temperature = None;
        let pts = [p, point(1, 20.0)];
        assert_eq!(series(&pts, BaselineMetric::Cpu).len(), 2);
        assert_eq!(series(&pts, BaselineMetric::Temperature).len(), 1);
        assert!(series(&pts, BaselineMetric::Gpu).is_empty());
        assert!(series(&pts, BaselineMetric::AppCpu).is_empty());
    }

    #[test]
    fn apps_are_ranked_by_memory_and_capped() {
        let rows: Vec<AppMinute> = (0..20)
            .flat_map(|i| {
                (0..2).map(move |m| AppMinute {
                    at: Timestamp(m * MINUTE),
                    app: format!("app{i}"),
                    cpu: 2.0,
                    memory: (i as u64 + 1) * 1_000,
                })
            })
            .collect();
        let apps = current_apps(&rows);
        assert_eq!(apps.len(), MAX_APPS);
        assert_eq!(apps[0].0, "app19");
        assert_eq!(apps[0].2, 20_000.0);
        assert_eq!(apps[0].1, 2.0);
    }

    #[test]
    fn current_values_come_from_the_recent_samples() {
        let mut platform = dyno_platform::fake::Fake::new().platform();
        let mut a = platform.sensors.sample(Timestamp(0));
        let mut b = a.clone();
        a.cpu.total = 10.0;
        b.cpu.total = 30.0;
        assert_eq!(system_value(&[a.clone(), b.clone()], BaselineMetric::Cpu), Some(20.0));
        assert_eq!(system_value(&[], BaselineMetric::Cpu), None);
        assert!(system_value(&[a], BaselineMetric::AppCpu).is_none());
        assert!(system_value(&[b], BaselineMetric::Network).is_some());
    }

    #[test]
    fn items_rank_unusual_first_and_parse_back() {
        let strong = Judgement {
            state: State::Higher(Strength::Strong),
            ..Judgement::not_judged("", "")
        };
        let usual = Judgement {
            state: State::Usual,
            ..Judgement::not_judged("", "")
        };
        assert!(rank(&strong) > rank(&usual) && rank(&usual) > rank(&Judgement::not_judged("", "")));
        assert_eq!(parse_current("72%"), 72.0);
        assert_eq!(parse_current("1.5 GB"), 1.5);
        assert_eq!(parse_current("x"), 0.0);
    }
}
