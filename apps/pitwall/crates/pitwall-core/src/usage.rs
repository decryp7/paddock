//! Per-app usage over time (ADR 0007): requests, tokens, busy time, errors and peak memory per app, in hourly and
//! daily buckets, for the dashboard chart, the app detail view and the token/request budgets. Pure: events and times
//! come in as values; the server persists the state (versioned JSON, atomic write with a backup).
//!
//! Bounded: hourly buckets are kept [`HOURLY_RETENTION_DAYS`] days, daily buckets [`DAILY_RETENTION_DAYS`] days, and
//! each bucket tracks at most [`MAX_APPS`] apps (further apps count as [`OTHER`]).
//!
//! **Peak memory** is the memory attributed to the app: for every request it is the resident footprint (RAM plus
//! video memory) of the model that served it, so it includes models the app's requests loaded or kept hot. Within a
//! bucket the value is the largest footprint seen (a maximum, never a sum); it is not a measurement of the process.

use crate::limits::BudgetUse;
use crate::time::{day_start, hour_start, month_start, Millis, DAY, HOUR};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Counters of one app in one bucket (`UsageCounters` in `api.types.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Counters {
    /// Requests.
    pub requests: u64,
    /// Prompt tokens.
    pub prompt_tokens: u64,
    /// Generated tokens.
    pub completion_tokens: u64,
    /// Time spent serving.
    pub busy_ms: u64,
    /// Failed requests.
    pub errors: u64,
    /// The largest model footprint attributed to the app (bytes).
    pub peak_memory_bytes: u64,
}

impl Counters {
    /// Tokens both ways.
    pub fn tokens(&self) -> u64 {
        self.prompt_tokens.saturating_add(self.completion_tokens)
    }

    /// Adds another bucket: sums, except the peak, which is the maximum.
    pub fn merge(&mut self, o: &Counters) {
        self.requests = self.requests.saturating_add(o.requests);
        self.prompt_tokens = self.prompt_tokens.saturating_add(o.prompt_tokens);
        self.completion_tokens = self.completion_tokens.saturating_add(o.completion_tokens);
        self.busy_ms = self.busy_ms.saturating_add(o.busy_ms);
        self.errors = self.errors.saturating_add(o.errors);
        self.peak_memory_bytes = self.peak_memory_bytes.max(o.peak_memory_bytes);
    }
}

/// One finished request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageEvent<'a> {
    /// When it ended.
    pub at: Millis,
    /// The app.
    pub app: &'a str,
    /// Prompt tokens.
    pub prompt_tokens: u64,
    /// Generated tokens.
    pub completion_tokens: u64,
    /// How long it took.
    pub busy_ms: u64,
    /// Whether it failed.
    pub failed: bool,
    /// The footprint of the model that served it (bytes; 0 when unknown).
    pub memory_bytes: u64,
}

/// A usage range (`UsageRange` in `api.types.ts`): hourly buckets up to a week, daily buckets for 30 days.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageRange {
    /// The last 24 hours, hourly.
    #[serde(rename = "24h")]
    Day,
    /// The last 7 days, hourly.
    #[serde(rename = "7d")]
    Week,
    /// The last 30 days, daily.
    #[serde(rename = "30d")]
    Month,
}

impl UsageRange {
    /// The bucket length.
    pub fn step(self) -> Millis {
        match self {
            UsageRange::Day | UsageRange::Week => HOUR,
            UsageRange::Month => DAY,
        }
    }

    /// The number of buckets.
    pub fn buckets(self) -> usize {
        match self {
            UsageRange::Day => 24,
            UsageRange::Week => 7 * 24,
            UsageRange::Month => 30,
        }
    }
}

/// One app's series (`AppUsageSeries` in `api.types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsageSeries {
    /// The app (or [`OTHER`]).
    pub app_id: String,
    /// One entry per bucket of the range.
    pub points: Vec<Counters>,
    /// The whole range.
    pub totals: Counters,
}

/// Usage over a range (`UsageSeries` in `api.types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSeries {
    /// The range.
    pub range: UsageRange,
    /// Bucket length.
    pub step_ms: Millis,
    /// The start of every bucket, oldest first.
    pub starts: Vec<Millis>,
    /// All apps together, per bucket.
    pub total: Vec<Counters>,
    /// All apps together, over the range.
    pub totals: Counters,
    /// The busiest apps (at most [`MAX_SERIES`], the rest folded into [`OTHER`]), busiest first.
    pub apps: Vec<AppUsageSeries>,
}

/// Most distinct apps tracked per bucket.
pub const MAX_APPS: usize = 256;
/// What apps beyond the caps count as.
pub const OTHER: &str = "other";
/// Hourly buckets are kept this long.
pub const HOURLY_RETENTION_DAYS: u64 = 7;
/// Daily buckets are kept this long.
pub const DAILY_RETENTION_DAYS: u64 = 90;
/// Most apps in a series (the rest are folded into [`OTHER`]).
pub const MAX_SERIES: usize = 8;
/// The file format version of persisted usage.
pub const USAGE_VERSION: u32 = 1;

type Buckets = BTreeMap<Millis, BTreeMap<String, Counters>>;

/// Usage of every app per hour and per day.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    version: u32,
    hours: Buckets,
    days: Buckets,
}

impl Default for Usage {
    fn default() -> Self {
        Usage {
            version: USAGE_VERSION,
            hours: Buckets::new(),
            days: Buckets::new(),
        }
    }
}

fn add(buckets: &mut Buckets, start: Millis, app: &str, e: &UsageEvent) {
    let b = buckets.entry(start).or_default();
    let key = if b.contains_key(app) || b.len() < MAX_APPS { app } else { OTHER };
    let c = b.entry(key.to_string()).or_default();
    c.requests = c.requests.saturating_add(1);
    c.prompt_tokens = c.prompt_tokens.saturating_add(e.prompt_tokens);
    c.completion_tokens = c.completion_tokens.saturating_add(e.completion_tokens);
    c.busy_ms = c.busy_ms.saturating_add(e.busy_ms);
    c.errors = c.errors.saturating_add(u64::from(e.failed));
    c.peak_memory_bytes = c.peak_memory_bytes.max(e.memory_bytes);
}

impl Usage {
    /// Restores persisted usage; anything unreadable or of another version starts empty (the caller tries the backup
    /// first). Buckets outside the retention are dropped.
    pub fn restore(json: &str, now: Millis) -> Option<Usage> {
        let mut u: Usage = serde_json::from_str(json).ok().filter(|u: &Usage| u.version == USAGE_VERSION)?;
        u.prune(now);
        Some(u)
    }

    /// As JSON for persisting.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Records one finished request.
    pub fn record(&mut self, e: &UsageEvent) {
        add(&mut self.hours, hour_start(e.at), e.app, e);
        add(&mut self.days, day_start(e.at), e.app, e);
        self.prune(e.at);
    }

    /// Drops buckets older than the retention.
    pub fn prune(&mut self, now: Millis) {
        let h = hour_start(now).saturating_sub(HOURLY_RETENTION_DAYS * DAY);
        self.hours.retain(|s, _| *s >= h);
        let d = day_start(now).saturating_sub(DAILY_RETENTION_DAYS * DAY);
        self.days.retain(|s, _| *s >= d);
    }

    /// An app's counters on the UTC day of `at`.
    pub fn today(&self, at: Millis, app: &str) -> Counters {
        self.days.get(&day_start(at)).and_then(|d| d.get(app)).copied().unwrap_or_default()
    }

    /// An app's counters in the UTC calendar month of `at`, up to `at`'s day.
    pub fn month(&self, at: Millis, app: &str) -> Counters {
        let mut c = Counters::default();
        for (_, d) in self.days.range(month_start(at)..=day_start(at)) {
            if let Some(x) = d.get(app) {
                c.merge(x);
            }
        }
        c
    }

    /// What an app has used against its budgets.
    pub fn budget_use(&self, at: Millis, app: &str) -> BudgetUse {
        let today = self.today(at, app);
        BudgetUse {
            tokens_today: today.tokens(),
            requests_today: today.requests,
            tokens_month: self.month(at, app).tokens(),
        }
    }

    /// Usage over `range` ending with the bucket that contains `now`. With `app`, only that app (empty if unknown);
    /// otherwise all apps, the busiest [`MAX_SERIES`] listed and the rest folded into [`OTHER`]. Every bucket of the
    /// range is present (zero when idle), so a chart needs no gap handling. Months older than the daily retention
    /// are simply empty. A week or month older than the hourly retention cannot occur: ranges are fixed.
    pub fn series(&self, range: UsageRange, now: Millis, app: Option<&str>) -> UsageSeries {
        let step = range.step();
        let n = range.buckets();
        let buckets = if step == HOUR { &self.hours } else { &self.days };
        let current = now - now % step;
        let first = current.saturating_sub((n as u64 - 1) * step);
        let starts: Vec<Millis> = (0..n as u64).map(|i| first + i * step).collect();
        // Aggregate per app across the window.
        let mut per_app: BTreeMap<String, Vec<Counters>> = BTreeMap::new();
        for (s, b) in buckets.range(first..=current) {
            let idx = ((s - first) / step) as usize;
            for (name, c) in b {
                if app.is_some_and(|a| a != name) {
                    continue;
                }
                let v = per_app.entry(name.clone()).or_insert_with(|| vec![Counters::default(); n]);
                if let Some(slot) = v.get_mut(idx) {
                    slot.merge(c);
                }
            }
        }
        if let Some(a) = app {
            per_app.entry(a.to_string()).or_insert_with(|| vec![Counters::default(); n]);
        }
        let mut apps: Vec<AppUsageSeries> = per_app.into_iter().map(|(app_id, points)| series_of(app_id, points)).collect();
        let mut total = vec![Counters::default(); n];
        let mut totals = Counters::default();
        for a in &apps {
            for (t, p) in total.iter_mut().zip(&a.points) {
                t.merge(p);
            }
            totals.merge(&a.totals);
        }
        apps.sort_by(|a, b| {
            b.totals
                .tokens()
                .cmp(&a.totals.tokens())
                .then(b.totals.requests.cmp(&a.totals.requests))
                .then(a.app_id.cmp(&b.app_id))
        });
        if apps.len() > MAX_SERIES {
            let rest = apps.split_off(MAX_SERIES - 1);
            let mut points = vec![Counters::default(); n];
            for r in rest {
                for (p, x) in points.iter_mut().zip(&r.points) {
                    p.merge(x);
                }
            }
            apps.push(series_of(OTHER.to_string(), points));
        }
        UsageSeries {
            range,
            step_ms: step,
            starts,
            total,
            totals,
            apps,
        }
    }
}

fn series_of(app_id: String, points: Vec<Counters>) -> AppUsageSeries {
    let mut totals = Counters::default();
    for p in &points {
        totals.merge(p);
    }
    AppUsageSeries { app_id, points, totals }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(at: Millis, app: &str, p: u64, c: u64, mem: u64, failed: bool) -> UsageEvent<'_> {
        UsageEvent {
            at,
            app,
            prompt_tokens: p,
            completion_tokens: c,
            busy_ms: 10,
            failed,
            memory_bytes: mem,
        }
    }

    // A fixed instant well after the epoch: the 40th day plus 5 hours.
    const NOW: Millis = 40 * DAY + 5 * HOUR + 123;

    #[test]
    fn counts_per_day_month_and_peak() {
        let mut u = Usage::default();
        u.record(&ev(10, "a", 5, 7, 100, false));
        u.record(&ev(20, "a", 1, 1, 300, true));
        u.record(&ev(30, "a", 1, 1, 200, false));
        u.record(&ev(DAY + 1, "a", 9, 9, 50, false));
        let c = u.today(30, "a");
        assert_eq!(
            (c.requests, c.tokens(), c.busy_ms, c.errors, c.peak_memory_bytes),
            (3, 16, 30, 1, 300)
        );
        assert_eq!(u.today(30, "other"), Counters::default());
        assert_eq!(u.today(DAY + 5, "a").requests, 1);
        // Both days are in January 1970.
        assert_eq!(u.month(DAY + 5, "a").requests, 4);
        assert_eq!(u.month(30, "a").requests, 3, "the future day is not counted");
        let b = u.budget_use(DAY + 5, "a");
        assert_eq!((b.tokens_today, b.requests_today, b.tokens_month), (18, 1, 34));
    }

    #[test]
    fn month_resets_on_the_first() {
        let mut u = Usage::default();
        let jan31 = 30 * DAY + 5;
        u.record(&ev(jan31, "a", 100, 0, 0, false));
        u.record(&ev(31 * DAY + 1, "a", 7, 0, 0, false));
        assert_eq!(u.month(31 * DAY + 2, "a").tokens(), 7, "February starts fresh");
        assert_eq!(u.month(jan31, "a").tokens(), 100);
    }

    #[test]
    fn retention_drops_old_buckets() {
        let mut u = Usage::default();
        u.record(&ev(NOW, "a", 1, 0, 0, false));
        u.record(&ev(NOW - 8 * DAY, "a", 1, 0, 0, false));
        // Recorded last, so it prunes against its own time: the first event is now in its future and kept.
        assert_eq!(u.hours.len(), 2);
        u.prune(NOW);
        assert_eq!(u.hours.len(), 1, "hourly buckets older than 7 days are dropped");
        assert_eq!(u.days.len(), 2, "daily buckets live 90 days");
        u.prune(NOW + 91 * DAY);
        assert!(u.days.is_empty() && u.hours.is_empty());
        // Exactly at the boundary is kept.
        let mut b = Usage::default();
        b.record(&ev(NOW, "a", 1, 0, 0, false));
        b.prune(hour_start(NOW) + HOURLY_RETENTION_DAYS * DAY);
        assert_eq!(b.hours.len(), 1);
        b.prune(hour_start(NOW) + HOURLY_RETENTION_DAYS * DAY + HOUR);
        assert!(b.hours.is_empty());
    }

    #[test]
    fn apps_are_capped_per_bucket() {
        let mut u = Usage::default();
        for i in 0..MAX_APPS + 3 {
            u.record(&ev(NOW, &format!("a{i}"), 1, 0, 0, false));
        }
        assert_eq!(u.today(NOW, &format!("a{}", MAX_APPS + 1)).requests, 0);
        assert_eq!(u.today(NOW, OTHER).requests, 3);
        assert_eq!(u.days[&day_start(NOW)].len(), MAX_APPS + 1);
        // A known app keeps counting.
        u.record(&ev(NOW, "a0", 1, 0, 0, false));
        assert_eq!(u.today(NOW, "a0").requests, 2);
    }

    #[test]
    fn series_are_complete_and_ordered() {
        let mut u = Usage::default();
        u.record(&ev(NOW, "big", 100, 50, 900, false));
        u.record(&ev(NOW - 2 * HOUR, "big", 10, 5, 1000, true));
        u.record(&ev(NOW - HOUR, "small", 1, 1, 10, false));
        u.record(&ev(NOW - 30 * HOUR, "big", 1, 1, 1, false));
        let s = u.series(UsageRange::Day, NOW, None);
        assert_eq!((s.step_ms, s.starts.len(), s.total.len()), (HOUR, 24, 24));
        assert_eq!(*s.starts.last().unwrap(), hour_start(NOW));
        assert_eq!(s.starts[1] - s.starts[0], HOUR);
        assert_eq!(s.apps.iter().map(|a| a.app_id.as_str()).collect::<Vec<_>>(), ["big", "small"]);
        assert_eq!(s.apps[0].totals.requests, 2, "30 h ago is outside 24 h");
        assert_eq!(s.apps[0].totals.peak_memory_bytes, 1000);
        assert_eq!(s.apps[0].points[23].tokens(), 150);
        assert_eq!(s.apps[0].points[21].errors, 1);
        assert_eq!(s.totals.requests, 3);
        assert_eq!(s.totals.peak_memory_bytes, 1000);
        assert_eq!(s.total[22].requests, 1);
        // A week sees the old one too; a month is daily.
        assert_eq!(u.series(UsageRange::Week, NOW, None).totals.requests, 4);
        let m = u.series(UsageRange::Month, NOW, None);
        assert_eq!((m.step_ms, m.starts.len()), (DAY, 30));
        assert_eq!(m.totals.requests, 4);
        assert_eq!(*m.starts.last().unwrap(), day_start(NOW));
        // One app, known or not, always has full-length points.
        let one = u.series(UsageRange::Day, NOW, Some("small"));
        assert_eq!((one.apps.len(), one.apps[0].points.len(), one.totals.requests), (1, 24, 1));
        let none = u.series(UsageRange::Day, NOW, Some("ghost"));
        assert_eq!((none.apps.len(), none.apps[0].totals), (1, Counters::default()));
        // Early in history the window saturates at zero rather than underflowing.
        assert_eq!(Usage::default().series(UsageRange::Month, 5, None).starts[0], 0);
    }

    #[test]
    fn series_fold_the_tail_into_other() {
        let mut u = Usage::default();
        for i in 0..12u64 {
            u.record(&ev(NOW, &format!("app{i:02}"), 100 - i, 0, 0, false));
        }
        let s = u.series(UsageRange::Day, NOW, None);
        assert_eq!(s.apps.len(), MAX_SERIES);
        assert_eq!(s.apps[0].app_id, "app00");
        let other = s.apps.last().unwrap();
        assert_eq!(other.app_id, OTHER);
        assert_eq!(other.totals.requests, 12 - (MAX_SERIES as u64 - 1));
        assert_eq!(s.totals.requests, 12, "the total still includes everything");
    }

    #[test]
    fn ties_break_by_requests_then_name() {
        let mut u = Usage::default();
        u.record(&ev(NOW, "b", 5, 0, 0, false));
        u.record(&ev(NOW, "a", 5, 0, 0, false));
        u.record(&ev(NOW, "c", 2, 0, 0, false));
        u.record(&ev(NOW, "c", 3, 0, 0, false));
        let names: Vec<_> = u.series(UsageRange::Day, NOW, None).apps.into_iter().map(|a| a.app_id).collect();
        assert_eq!(names, ["c", "a", "b"]);
    }

    #[test]
    fn persistence_is_tolerant() {
        let mut u = Usage::default();
        u.record(&ev(NOW, "a", 1, 2, 3, false));
        let json = u.to_json();
        assert_eq!(Usage::restore(&json, NOW), Some(u.clone()));
        assert_eq!(Usage::restore("{not json", NOW), None);
        assert_eq!(Usage::restore("", NOW), None);
        assert_eq!(Usage::restore(&json.replace("\"version\":1", "\"version\":9"), NOW), None);
        // Restoring prunes what has aged out while the app was off.
        let old = Usage::restore(&json, NOW + 200 * DAY).unwrap();
        assert!(old.days.is_empty() && old.hours.is_empty());
        // Unknown counter fields default; the peak is optional in older files.
        let partial = json.replace(",\"peakMemoryBytes\":3", "");
        assert_eq!(Usage::restore(&partial, NOW).unwrap().today(NOW, "a").peak_memory_bytes, 0);
    }

    #[test]
    fn counters_saturate() {
        let mut c = Counters {
            prompt_tokens: u64::MAX,
            ..Counters::default()
        };
        c.merge(&Counters {
            prompt_tokens: 5,
            completion_tokens: 1,
            ..Counters::default()
        });
        assert_eq!(c.prompt_tokens, u64::MAX);
        assert_eq!(c.tokens(), u64::MAX);
        assert_eq!(UsageRange::Day.buckets(), 24);
        assert_eq!(serde_json::to_value(UsageRange::Month).unwrap(), "30d");
    }
}
