//! "Is this normal?" (WP-25): a baseline per metric and app, learned from history by day of week and hour of day.
//!
//! Pure statistics over `(time, value)` pairs. [`learn`] reduces the history to one mean per local hour, then keeps a
//! robust centre and spread (median and MAD) per weekday and hour, with coarser fallbacks (weekday or weekend, then
//! any day) when a slot has too few days behind it. [`judge`] compares a recent value with the expectation for this
//! moment and answers in words ("higher than usual for Tuesday mornings"), never by colour alone. A new install
//! learns silently: until [`Learning::Ready`] nothing is highlighted.

use crate::hardware::Confidence;
use crate::stats::{median, quantile};
use crate::time::{Timestamp, DAY, HOUR, MINUTE};
use crate::units::bytes;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Days of learning before highlights appear, when the user has not chosen.
pub const DEFAULT_LEARNING_DAYS: u32 = 7;
/// Observations (hours) a weekday-and-hour slot needs to be used on its own.
pub const MIN_EXACT: usize = 3;
/// Observations a weekday-or-weekend slot needs.
pub const MIN_DAY_TYPE: usize = 6;
/// Observations an hour-of-day slot needs.
pub const MIN_HOUR: usize = 8;
/// How many spreads from the median is unusual.
pub const SPREADS: f64 = 3.5;
/// A value this many times the unusual threshold is strongly unusual.
pub const STRONG_FACTOR: f64 = 2.0;

/// Settings for baselines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BaselineSettings {
    /// Learn baselines and show highlights.
    pub enabled: bool,
    /// Days of history to learn before highlighting anything (1–30).
    pub learning_days: u32,
    /// Raise an alarm when something stays strongly higher than usual (off until the user turns it on).
    pub alarms: bool,
}

impl Default for BaselineSettings {
    fn default() -> Self {
        BaselineSettings {
            enabled: true,
            learning_days: DEFAULT_LEARNING_DAYS,
            alarms: false,
        }
    }
}

impl BaselineSettings {
    /// The settings in their allowed ranges.
    pub fn clamped(mut self) -> BaselineSettings {
        self.learning_days = self.learning_days.clamp(1, 30);
        self
    }
}

/// What is learned about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BaselineMetric {
    /// CPU load, percent.
    Cpu,
    /// Memory in use, percent.
    Memory,
    /// Disk writes, bytes per second.
    DiskWrite,
    /// Network traffic (received plus sent), bytes per second.
    Network,
    /// The hottest temperature, °C.
    Temperature,
    /// GPU load, percent.
    Gpu,
    /// One app's CPU, percent.
    AppCpu,
    /// One app's memory, bytes.
    AppMemory,
}

impl BaselineMetric {
    /// The machine-wide metrics.
    pub const SYSTEM: [BaselineMetric; 6] = [
        BaselineMetric::Cpu,
        BaselineMetric::Memory,
        BaselineMetric::DiskWrite,
        BaselineMetric::Network,
        BaselineMetric::Temperature,
        BaselineMetric::Gpu,
    ];

    /// The per-app metrics.
    pub const APP: [BaselineMetric; 2] = [BaselineMetric::AppCpu, BaselineMetric::AppMemory];

    /// The metric's wire name (as serialised), used in ids.
    pub fn key(self) -> &'static str {
        match self {
            BaselineMetric::Cpu => "cpu",
            BaselineMetric::Memory => "memory",
            BaselineMetric::DiskWrite => "diskWrite",
            BaselineMetric::Network => "network",
            BaselineMetric::Temperature => "temperature",
            BaselineMetric::Gpu => "gpu",
            BaselineMetric::AppCpu => "appCpu",
            BaselineMetric::AppMemory => "appMemory",
        }
    }

    /// The name shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            BaselineMetric::Cpu | BaselineMetric::AppCpu => "CPU",
            BaselineMetric::Memory | BaselineMetric::AppMemory => "Memory",
            BaselineMetric::DiskWrite => "Disk writes",
            BaselineMetric::Network => "Network",
            BaselineMetric::Temperature => "Temperature",
            BaselineMetric::Gpu => "GPU",
        }
    }

    /// The smallest difference from the usual that is worth mentioning, in the metric's unit (so 3 % CPU never counts).
    pub fn min_deviation(self) -> f64 {
        match self {
            BaselineMetric::Cpu | BaselineMetric::Gpu => 20.0,
            BaselineMetric::Memory => 10.0,
            BaselineMetric::AppCpu => 10.0,
            BaselineMetric::AppMemory => 300.0 * 1024.0 * 1024.0,
            BaselineMetric::DiskWrite => 20.0 * 1024.0 * 1024.0,
            BaselineMetric::Network => 2.0 * 1024.0 * 1024.0,
            BaselineMetric::Temperature => 10.0,
        }
    }

    /// Formats a value of this metric.
    pub fn format(self, v: f64) -> String {
        match self {
            BaselineMetric::Cpu | BaselineMetric::Memory | BaselineMetric::Gpu | BaselineMetric::AppCpu => format!("{v:.0}%"),
            BaselineMetric::AppMemory => bytes(v.max(0.0) as u64),
            BaselineMetric::Temperature => format!("{v:.0} °C"),
            BaselineMetric::DiskWrite | BaselineMetric::Network => format!("{}/s", bytes(v.max(0.0) as u64)),
        }
    }
}

/// How far the learning has come.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Learning {
    /// Turned off in the settings.
    Off,
    /// Still collecting; nothing is highlighted yet.
    Learning {
        /// Whole days seen so far.
        days_done: u32,
        /// Days needed.
        days_needed: u32,
    },
    /// Ready to judge.
    Ready {
        /// Days of history behind the baselines.
        days: u32,
    },
}

/// The learning state from when Dyno first saw this machine.
pub fn learning_status(settings: &BaselineSettings, first_seen: Option<Timestamp>, now: Timestamp) -> Learning {
    if !settings.enabled {
        return Learning::Off;
    }
    let needed = settings.clamped().learning_days;
    let days = first_seen.map_or(0, |f| (now.since(f).max(0) / DAY) as u32);
    if days >= needed {
        Learning::Ready { days }
    } else {
        Learning::Learning {
            days_done: days,
            days_needed: needed,
        }
    }
}

/// Robust centre and spread of a slot.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotStats {
    /// Observations (hours) behind it.
    pub n: u32,
    /// Typical value.
    pub median: f64,
    /// Typical spread (MAD scaled to a standard deviation).
    pub spread: f64,
    /// The lower quartile.
    pub q1: f64,
    /// The upper quartile.
    pub q3: f64,
}

fn stats(values: &[f64]) -> Option<SlotStats> {
    let m = median(values)?;
    let devs: Vec<f64> = values.iter().map(|v| (v - m).abs()).collect();
    Some(SlotStats {
        n: values.len() as u32,
        median: m,
        spread: median(&devs)? * 1.4826,
        q1: quantile(values, 0.25)?,
        q3: quantile(values, 0.75)?,
    })
}

/// How specific the expectation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    /// This weekday at this hour.
    WeekdayHour,
    /// Weekdays (or the weekend) at this hour.
    DayTypeHour,
    /// Any day at this hour.
    Hour,
}

/// What is normal at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Expectation {
    /// The slot's statistics.
    pub stats: SlotStats,
    /// How specific.
    pub level: Level,
}

/// What was learned for one metric (or one app's metric).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Baseline {
    exact: HashMap<(u8, u8), SlotStats>,
    day_type: HashMap<(bool, u8), SlotStats>,
    hour: HashMap<u8, SlotStats>,
    /// Distinct local days that contributed.
    pub days: u32,
}

/// The local weekday (0 = Monday) and hour of `at`, given the local offset from UTC in minutes.
pub fn local_slot(at: Timestamp, utc_offset_minutes: i32) -> (u8, u8) {
    let local = at.millis().saturating_add(i64::from(utc_offset_minutes) * MINUTE);
    let day = local.div_euclid(DAY);
    // 1970-01-01 was a Thursday (weekday 3 with Monday = 0).
    let weekday = (day + 3).rem_euclid(7) as u8;
    let hour = (local.rem_euclid(DAY) / HOUR) as u8;
    (weekday, hour)
}

/// Learns a baseline from `(time, value)` pairs in any order. Non-finite values are ignored; each local hour counts once
/// (its mean), so a dense history does not outweigh a sparse one.
pub fn learn(samples: &[(Timestamp, f64)], utc_offset_minutes: i32) -> Baseline {
    let mut hours: BTreeMap<i64, (f64, u32)> = BTreeMap::new();
    for (at, v) in samples.iter().filter(|(_, v)| v.is_finite()) {
        let local = at.millis().saturating_add(i64::from(utc_offset_minutes) * MINUTE);
        let e = hours.entry(local.div_euclid(HOUR)).or_insert((0.0, 0));
        e.0 += v;
        e.1 += 1;
    }
    let mut exact: HashMap<(u8, u8), Vec<f64>> = HashMap::new();
    let mut day_type: HashMap<(bool, u8), Vec<f64>> = HashMap::new();
    let mut hour: HashMap<u8, Vec<f64>> = HashMap::new();
    let mut days = std::collections::BTreeSet::new();
    for (h, (sum, n)) in hours {
        let mean = sum / f64::from(n);
        let (weekday, hod) = local_slot(Timestamp(h * HOUR), 0);
        days.insert(h.div_euclid(24));
        exact.entry((weekday, hod)).or_default().push(mean);
        day_type.entry((weekday >= 5, hod)).or_default().push(mean);
        hour.entry(hod).or_default().push(mean);
    }
    Baseline {
        exact: exact.into_iter().filter_map(|(k, v)| stats(&v).map(|s| (k, s))).collect(),
        day_type: day_type.into_iter().filter_map(|(k, v)| stats(&v).map(|s| (k, s))).collect(),
        hour: hour.into_iter().filter_map(|(k, v)| stats(&v).map(|s| (k, s))).collect(),
        days: days.len() as u32,
    }
}

impl Baseline {
    /// What is normal at `at`: the most specific slot with enough observations, if any.
    pub fn expect(&self, at: Timestamp, utc_offset_minutes: i32) -> Option<Expectation> {
        let (weekday, hour) = local_slot(at, utc_offset_minutes);
        let pick =
            |s: Option<&SlotStats>, min: usize, level: Level| s.filter(|s| s.n as usize >= min).map(|s| Expectation { stats: *s, level });
        pick(self.exact.get(&(weekday, hour)), MIN_EXACT, Level::WeekdayHour)
            .or_else(|| pick(self.day_type.get(&(weekday >= 5, hour)), MIN_DAY_TYPE, Level::DayTypeHour))
            .or_else(|| pick(self.hour.get(&hour), MIN_HOUR, Level::Hour))
    }
}

/// How unusual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Strength {
    /// Clearly outside the usual.
    Notable,
    /// Far outside the usual.
    Strong,
}

/// The verdict on one value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// Dyno is still learning or learning is off: no opinion.
    NotJudged,
    /// This time of week has too little history behind it.
    NoBaseline,
    /// Within the usual range.
    Usual,
    /// Above the usual range.
    Higher(Strength),
    /// Below the usual range.
    Lower(Strength),
}

/// The judgement of one value, with the words that explain it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Judgement {
    /// The verdict.
    pub state: State,
    /// How sure.
    pub confidence: Confidence,
    /// A short phrase: "Higher than usual for Tuesday mornings", "Usual for this time", "Still learning".
    pub phrase: String,
    /// The reasoning: "CPU is 78% now; it is usually about 12% (5% to 20%) on Tuesday mornings."
    pub explanation: String,
    /// The usual value, formatted (empty when not judged).
    pub usual: String,
}

impl Judgement {
    /// No opinion yet, with the reason.
    pub fn not_judged(phrase: &str, explanation: &str) -> Judgement {
        Judgement {
            state: State::NotJudged,
            confidence: Confidence::Low,
            phrase: phrase.into(),
            explanation: explanation.into(),
            usual: String::new(),
        }
    }

    /// Whether this is a highlight (above or below usual).
    pub fn is_unusual(&self) -> bool {
        matches!(self.state, State::Higher(_) | State::Lower(_))
    }
}

const WEEKDAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

fn part_of_day(hour: u8) -> &'static str {
    match hour {
        5..=11 => "mornings",
        12..=16 => "afternoons",
        17..=21 => "evenings",
        _ => "nights",
    }
}

/// "Tuesday mornings", "weekday mornings", "this time of day".
fn when_words(level: Level, at: Timestamp, utc_offset_minutes: i32) -> String {
    let (weekday, hour) = local_slot(at, utc_offset_minutes);
    match level {
        Level::WeekdayHour => format!("{} {}", WEEKDAYS[usize::from(weekday) % 7], part_of_day(hour)),
        Level::DayTypeHour => format!("{} {}", if weekday >= 5 { "weekend" } else { "weekday" }, part_of_day(hour)),
        Level::Hour => "this time of day".into(),
    }
}

/// Judges `value` (a recent average, not a single reading) against what is normal at `at`.
pub fn judge(baseline: &Baseline, metric: BaselineMetric, value: f64, at: Timestamp, utc_offset_minutes: i32) -> Judgement {
    let Some(e) = baseline.expect(at, utc_offset_minutes) else {
        return Judgement {
            state: State::NoBaseline,
            confidence: Confidence::Low,
            phrase: "Not enough history for this time of week".into(),
            explanation: format!(
                "Dyno has not yet seen enough of this time of the week to know what is normal for {}.",
                metric.label()
            ),
            usual: String::new(),
        };
    };
    let when = when_words(e.level, at, utc_offset_minutes);
    let threshold = (SPREADS * e.stats.spread).max(metric.min_deviation());
    let diff = value - e.stats.median;
    let usual = metric.format(e.stats.median);
    let confidence = match (e.level, e.stats.n) {
        (Level::WeekdayHour, 8..) => Confidence::High,
        (Level::WeekdayHour, _) | (Level::DayTypeHour, 12..) => Confidence::Medium,
        _ => Confidence::Low,
    };
    let range = format!("{} to {}", metric.format(e.stats.q1), metric.format(e.stats.q3));
    let context = format!(
        "{} is {} now; it is usually about {usual} ({range}) on {when}, from {} similar hours.",
        metric.label(),
        metric.format(value),
        e.stats.n
    );
    let (state, phrase) = if diff.abs() <= threshold {
        (State::Usual, format!("Usual for {when}"))
    } else {
        let strength = if diff.abs() >= threshold * STRONG_FACTOR {
            Strength::Strong
        } else {
            Strength::Notable
        };
        if diff > 0.0 {
            (State::Higher(strength), format!("Higher than usual for {when}"))
        } else {
            (State::Lower(strength), format!("Lower than usual for {when}"))
        }
    };
    Judgement {
        state,
        confidence,
        phrase,
        explanation: context,
        usual,
    }
}

/// One line of the "Is this normal?" view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaselineItem {
    /// Stable across refreshes: the metric and the subject (`cpu:System`, `appCpu:Chrome`), so the UI can key rows on it.
    pub id: String,
    /// "System" or an app's name.
    pub subject: String,
    /// What is measured.
    pub metric: BaselineMetric,
    /// The metric's name.
    pub label: String,
    /// The recent value, formatted.
    pub current: String,
    /// The verdict in words.
    pub judgement: Judgement,
}

/// What the dashboard and the Baselines view show.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaselineOverview {
    /// When it was computed.
    pub at: Timestamp,
    /// How far the learning has come.
    pub learning: Learning,
    /// System metrics first, then apps; unusual ones before usual ones.
    pub items: Vec<BaselineItem>,
}

/// Counts consecutive flagged evaluations per key so a brief spike never raises an alarm.
#[derive(Debug, Default)]
pub struct Persistence {
    streaks: HashMap<String, u32>,
}

impl Persistence {
    /// Records one evaluation of `key`; true once it has been flagged `needed` times in a row.
    pub fn update(&mut self, key: &str, flagged: bool, needed: u32) -> bool {
        if !flagged {
            self.streaks.remove(key);
            return false;
        }
        let n = self.streaks.entry(key.to_string()).or_insert(0);
        *n = n.saturating_add(1);
        *n >= needed
    }

    /// Forgets keys not in `keep` (an app that closed).
    pub fn retain(&mut self, keep: &[String]) {
        self.streaks.retain(|k, _| keep.contains(k));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-05 was a Monday; midnight UTC.
    const MONDAY: i64 = 1_791_158_400_000;

    fn at(day: i64, hour: i64) -> Timestamp {
        Timestamp(MONDAY + day * DAY + hour * HOUR)
    }

    /// Hourly samples for `days` days: busy (70) on weekday 9-12, quiet (10) otherwise, +/- 2 of deterministic wobble.
    fn office(days: i64) -> Vec<(Timestamp, f64)> {
        (0..days * 24)
            .map(|h| {
                let (day, hour) = (h / 24, h % 24);
                let weekday = day % 7 < 5;
                let base = if weekday && (9..12).contains(&hour) { 70.0 } else { 10.0 };
                (at(day, hour), base + ((h * 7) % 5) as f64 - 2.0)
            })
            .collect()
    }

    #[test]
    fn slots_follow_the_local_clock() {
        assert_eq!(local_slot(at(0, 9), 0), (0, 9));
        assert_eq!(local_slot(at(1, 23), 0), (1, 23));
        assert_eq!(local_slot(at(5, 3), 0), (5, 3));
        assert_eq!(local_slot(at(0, 23), 120), (1, 1), "UTC+2 is already Tuesday 01:00");
        assert_eq!(local_slot(at(0, 1), -120), (6, 23), "UTC-2 is still Sunday evening");
        assert_eq!(local_slot(Timestamp(0), 0), (3, 0), "the epoch was a Thursday");
    }

    #[test]
    fn a_busy_morning_is_usual_and_a_busy_night_is_not() {
        let b = learn(&office(70), 0);
        assert_eq!(b.days, 70);
        let busy = judge(&b, BaselineMetric::Cpu, 72.0, at(70, 10), 0);
        assert_eq!(busy.state, State::Usual, "{busy:?}");
        assert!(busy.phrase.starts_with("Usual for Monday mornings"), "{}", busy.phrase);
        assert_eq!(busy.confidence, Confidence::High);
        let night = judge(&b, BaselineMetric::Cpu, 72.0, at(70, 3), 0);
        assert_eq!(night.state, State::Higher(Strength::Strong), "{night:?}");
        assert_eq!(night.phrase, "Higher than usual for Monday nights");
        assert!(
            night.explanation.contains("72%") && night.explanation.contains("usually about 10%"),
            "{}",
            night.explanation
        );
        assert!(night.is_unusual() && !busy.is_unusual());
        let quiet_morning = judge(&b, BaselineMetric::Cpu, 5.0, at(70, 10), 0);
        assert_eq!(quiet_morning.state, State::Lower(Strength::Strong), "{quiet_morning:?}");
        assert!(quiet_morning.phrase.starts_with("Lower than usual"));
    }

    #[test]
    fn small_differences_never_count() {
        let b = learn(&office(28), 0);
        // 10 -> 25 is far in spreads (spread is ~1.5) but under the 20-point floor for CPU.
        assert_eq!(judge(&b, BaselineMetric::Cpu, 25.0, at(28, 3), 0).state, State::Usual);
        assert_eq!(
            judge(&b, BaselineMetric::Cpu, 35.0, at(28, 3), 0).state,
            State::Higher(Strength::Notable)
        );
    }

    #[test]
    fn sparse_history_falls_back_and_admits_it() {
        // Two weeks: each weekday-hour slot has 2 observations (< 3), weekday-hour has 10 (>= 6).
        let b = learn(&office(14), 0);
        let j = judge(&b, BaselineMetric::Cpu, 70.0, at(14, 10), 0);
        assert_eq!(j.state, State::Usual);
        assert!(j.phrase.contains("weekday mornings"), "{}", j.phrase);
        assert_eq!(j.confidence, Confidence::Low);
        // Three days: nothing has enough days behind it.
        let tiny = learn(&office(3), 0);
        let none = judge(&tiny, BaselineMetric::Cpu, 70.0, at(3, 10), 0);
        assert_eq!(none.state, State::NoBaseline);
        assert!(none.phrase.contains("Not enough history"));
        assert!(learn(&[], 0).expect(at(0, 0), 0).is_none());
    }

    #[test]
    fn the_hour_fallback_is_used_for_the_weekend_with_little_data() {
        // 9 days: weekend slots have 2 days of data per hour (< MIN_DAY_TYPE) but any-day-at-this-hour has 9.
        let b = learn(&office(9), 0);
        let j = judge(&b, BaselineMetric::Cpu, 70.0, at(12, 3), 0); // a Saturday night
        assert!(j.phrase.contains("this time of day"), "{}", j.phrase);
        assert_eq!(j.state, State::Higher(Strength::Strong));
    }

    #[test]
    fn dense_history_counts_each_hour_once_and_bad_values_are_dropped() {
        let mut dense: Vec<(Timestamp, f64)> = Vec::new();
        for (t, v) in office(28) {
            for m in 0..4 {
                dense.push((t.plus(m * 15 * MINUTE), v));
            }
        }
        dense.push((at(1, 1), f64::NAN));
        dense.push((at(1, 1), f64::INFINITY));
        let b = learn(&dense, 0);
        let e = b.expect(at(28, 10), 0).unwrap();
        assert_eq!(e.level, Level::WeekdayHour);
        assert_eq!(e.stats.n, 4, "four Mondays, not sixteen readings");
        assert!(e.stats.q1 <= e.stats.median && e.stats.median <= e.stats.q3);
    }

    #[test]
    fn learning_runs_silently_for_the_configured_days() {
        let s = BaselineSettings::default();
        let first = at(0, 0);
        assert_eq!(
            learning_status(&s, Some(first), at(3, 0)),
            Learning::Learning {
                days_done: 3,
                days_needed: 7
            }
        );
        assert_eq!(learning_status(&s, Some(first), at(7, 1)), Learning::Ready { days: 7 });
        assert_eq!(
            learning_status(&s, None, at(7, 1)),
            Learning::Learning {
                days_done: 0,
                days_needed: 7
            }
        );
        let long = BaselineSettings { learning_days: 99, ..s };
        assert_eq!(long.clamped().learning_days, 30);
        assert_eq!(BaselineSettings { learning_days: 0, ..s }.clamped().learning_days, 1);
        assert_eq!(
            learning_status(&BaselineSettings { enabled: false, ..s }, Some(first), at(30, 0)),
            Learning::Off
        );
        // A clock that moved backwards after the first run learns for the full period again, never negative.
        assert_eq!(
            learning_status(&s, Some(at(10, 0)), at(0, 0)),
            Learning::Learning {
                days_done: 0,
                days_needed: 7
            }
        );
    }

    #[test]
    fn metrics_format_with_units() {
        assert_eq!(BaselineMetric::Cpu.format(12.4), "12%");
        assert_eq!(BaselineMetric::Temperature.format(71.6), "72 °C");
        assert_eq!(
            BaselineMetric::Network.format(3.0 * 1024.0 * 1024.0),
            format!("{}/s", bytes(3 * 1024 * 1024))
        );
        assert_eq!(BaselineMetric::SYSTEM.len(), 6);
        assert_eq!(BaselineMetric::AppMemory.format(2.0 * 1024.0 * 1024.0 * 1024.0), bytes(2 << 30));
        assert_eq!(BaselineMetric::AppCpu.label(), "CPU");
        assert!(BaselineMetric::APP.iter().all(|m| m.min_deviation() > 0.0));
        assert_eq!(BaselineMetric::DiskWrite.label(), "Disk writes");
        assert!(BaselineMetric::Memory.min_deviation() > 0.0);
        assert_eq!(Judgement::not_judged("Still learning", "x").state, State::NotJudged);
    }

    #[test]
    fn alarms_need_a_persistent_streak() {
        let mut p = Persistence::default();
        assert!(!p.update("cpu", true, 3));
        assert!(!p.update("cpu", true, 3));
        assert!(p.update("cpu", true, 3));
        assert!(!p.update("cpu", false, 3));
        assert!(!p.update("cpu", true, 3), "a break resets the streak");
        p.update("chrome", true, 3);
        p.retain(&["cpu".into()]);
        assert!(!p.update("chrome", true, 2), "forgotten apps start over");
    }
}
