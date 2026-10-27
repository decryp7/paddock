//! Disk and battery life forecasts (WP-24): "this SSD has about N years of writes left", "replace the battery in about
//! N months".
//!
//! Pure functions over the readings the agent stored (SSD wear counters and battery capacity, one reading every few
//! hours). Every estimate is a range with the data it stands on and a confidence; with too little history it says so, and
//! with no data there is no estimate. A reading series that jumps (a replaced disk or battery, a counter reset, a clock
//! that moved) is cut at the jump so the old part cannot distort the new one.

use crate::anomaly::Evidence;
use crate::hardware::Confidence;
use crate::stats::{robust_slope, RobustSlope};
use crate::time::{Timestamp, DAY};
use crate::units::bytes;
use serde::{Deserialize, Serialize};

/// Forecasts further away than this are reported as "more than 10 years".
pub const HORIZON_DAYS: f64 = 3_650.0;
/// An SSD forecast needs this much history ...
pub const SSD_MIN_DAYS: f64 = 14.0;
/// ... and this many readings.
pub const SSD_MIN_READINGS: usize = 4;
/// A battery forecast needs this much history ...
pub const BATTERY_MIN_DAYS: f64 = 30.0;
/// ... and this many readings.
pub const BATTERY_MIN_READINGS: usize = 4;
/// The capacity at which a battery is usually considered worn out.
pub const BATTERY_REPLACE_AT: f32 = 80.0;
/// A capacity reading this much higher than the one before means a different battery (or a recalibration far larger
/// than normal): the series starts again there.
pub const BATTERY_JUMP: f32 = 15.0;
/// Readings stamped further ahead of `now` than this come from a clock that was wrong and are ignored.
pub const FUTURE_SLACK_MS: i64 = DAY;

/// What is being forecast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LifeSubject {
    /// A solid-state disk.
    Ssd,
    /// The laptop battery.
    Battery,
}

/// How much life is left.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum LifeOutcome {
    /// A range of days (fast wear, expected, slow wear), capped at [`HORIZON_DAYS`].
    Remaining {
        /// If wear speeds up.
        low_days: f32,
        /// At the measured pace.
        mid_days: f32,
        /// If wear slows down.
        high_days: f32,
    },
    /// So far away that no number is worth showing.
    BeyondHorizon,
    /// Nothing wore measurably over the history.
    NoWearDetected,
    /// Already at or beyond the point where replacement is advised.
    AlreadyPastLimit,
    /// There is a reading but not enough history to see a pace.
    TooLittleHistory {
        /// Days of history there is.
        have_days: f32,
        /// Days needed.
        need_days: f32,
    },
    /// The machine does not report what is needed.
    Unavailable {
        /// Why, and what could help.
        reason: String,
    },
}

/// A forecast for one disk or battery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifeForecast {
    /// What it is.
    pub subject: LifeSubject,
    /// Its name (disk model, "Battery").
    pub name: String,
    /// The outcome.
    pub outcome: LifeOutcome,
    /// How sure.
    pub confidence: Confidence,
    /// One line, with the range in words.
    pub headline: String,
    /// What it means.
    pub explanation: String,
    /// The data the estimate stands on.
    pub basis: Vec<Evidence>,
    /// Days between the first and the last reading used.
    pub history_days: f32,
    /// How many readings were used.
    pub readings: u32,
    /// Set when the series was cut because a different disk or battery appeared (or a counter reset) at this time.
    pub restarted_at: Option<Timestamp>,
}

impl LifeForecast {
    /// The expected days left, when there is an estimate.
    pub fn mid_days(&self) -> Option<f32> {
        match self.outcome {
            LifeOutcome::Remaining { mid_days, .. } => Some(mid_days),
            _ => None,
        }
    }

    /// Whether this deserves the user's attention soon: a past-limit part, or under a year left with at least medium
    /// confidence.
    pub fn needs_attention(&self) -> bool {
        match self.outcome {
            LifeOutcome::AlreadyPastLimit => true,
            LifeOutcome::Remaining { high_days, .. } => high_days < 365.0 && self.confidence >= Confidence::Medium,
            _ => false,
        }
    }
}

/// What the forecasts say, for the Lifespan page and the report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifespanOverview {
    /// When it was computed.
    pub at: Timestamp,
    /// One forecast per SSD and battery.
    pub forecasts: Vec<LifeForecast>,
}

/// A forecast for something the machine does not report.
pub fn unavailable(subject: LifeSubject, name: &str, reason: &str) -> LifeForecast {
    LifeForecast {
        subject,
        name: name.into(),
        outcome: LifeOutcome::Unavailable { reason: reason.into() },
        confidence: Confidence::Low,
        headline: match subject {
            LifeSubject::Ssd => "Disk life cannot be estimated".into(),
            LifeSubject::Battery => "Battery life cannot be estimated".into(),
        },
        explanation: reason.into(),
        basis: vec![],
        history_days: 0.0,
        readings: 0,
        restarted_at: None,
    }
}

/// One stored reading of an SSD's wear.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WearPoint {
    /// When.
    pub at: Timestamp,
    /// "Percentage used" of the rated endurance.
    pub percent_used: Option<f32>,
    /// Total bytes written.
    pub bytes_written: Option<u64>,
}

/// One stored reading of a battery's capacity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapacityPoint {
    /// When.
    pub at: Timestamp,
    /// Full-charge capacity as a percentage of the design capacity.
    pub health_percent: f32,
    /// Charge cycles.
    pub cycles: Option<u32>,
}

/// What is known about the SSD besides its stored readings.
#[derive(Debug, Clone)]
pub struct SsdFacts<'a> {
    /// Model name.
    pub name: &'a str,
    /// Rated endurance in bytes written, when known.
    pub rated_bytes: Option<u64>,
    /// Hours powered on, when reported.
    pub power_on_hours: Option<u64>,
}

/// What is known about the battery besides its stored readings.
#[derive(Debug, Clone)]
pub struct BatteryFacts<'a> {
    /// Name.
    pub name: &'a str,
    /// The maker's rated cycles, when known.
    pub rated_cycles: Option<u32>,
}

fn days_between(a: Timestamp, b: Timestamp) -> f64 {
    b.since(a) as f64 / DAY as f64
}

/// A span of days in words: "9 days", "6 weeks", "7 months", "4.5 years".
pub fn human_days(days: f64) -> String {
    let d = days.max(0.0);
    if d < 1.0 {
        "less than a day".into()
    } else if d < 14.0 {
        let n = d.round().max(1.0) as u32;
        format!("{n} day{}", if n == 1 { "" } else { "s" })
    } else if d < 70.0 {
        format!("{} weeks", (d / 7.0).round() as u32)
    } else if d < 700.0 {
        format!("{} months", (d / 30.4).round() as u32)
    } else {
        let y = d / 365.25;
        if y >= 9.5 {
            format!("{} years", y.round() as u32)
        } else {
            format!("{y:.1} years")
        }
    }
}

fn history_text(readings: usize, span_days: f64) -> String {
    if readings == 1 {
        "1 reading".into()
    } else {
        format!("{readings} readings over {}", human_days(span_days))
    }
}

fn range_text(low: f64, mid: f64, high: f64) -> String {
    if low >= HORIZON_DAYS {
        return "more than 10 years".into();
    }
    let (l, m, h) = (human_days(low), human_days(mid), human_days(high.min(HORIZON_DAYS)));
    if l == h {
        format!("about {m}")
    } else if high >= HORIZON_DAYS {
        format!("about {m} (at least {l})")
    } else {
        format!("about {m} ({l} to {h})")
    }
}

/// Days to consume `remaining` units at the three paces; each capped at the horizon.
fn project(remaining: f64, pace: &Pace) -> (f64, f64, f64) {
    let days = |rate: f64| {
        if rate <= f64::EPSILON {
            HORIZON_DAYS
        } else {
            (remaining / rate).min(HORIZON_DAYS)
        }
    };
    (days(pace.fast), days(pace.mid), days(pace.slow))
}

/// The pace of wear in units per day: fast, expected and slow.
#[derive(Debug, Clone, Copy)]
struct Pace {
    fast: f64,
    mid: f64,
    slow: f64,
    spread: f64,
}

/// Turns a robust slope (wear is a decrease or increase: `sign` makes it positive) into a pace with a range that widens
/// when the history is short.
fn pace(r: RobustSlope, sign: f64, span_days: f64) -> Pace {
    let mid = (r.slope * sign).max(0.0);
    let (a, b) = (r.low * sign, r.high * sign);
    let (q_slow, q_fast) = (a.min(b).max(0.0), a.max(b).max(0.0));
    let widen = (30.0 / span_days.max(1.0)).clamp(0.25, 1.0);
    let fast = q_fast.max(mid * (1.0 + widen));
    let slow = q_slow.min(mid * (1.0 - widen / 2.0));
    let spread = if slow > f64::EPSILON { fast / slow } else { f64::INFINITY };
    Pace { fast, mid, slow, spread }
}

fn confidence_of(span_days: f64, readings: usize, spread: f64) -> Confidence {
    if span_days >= 180.0 && readings >= 20 && spread <= 3.0 {
        Confidence::High
    } else if span_days >= 60.0 && readings >= 8 {
        Confidence::Medium
    } else {
        Confidence::Low
    }
}

/// Sorts by time, drops duplicate times and readings from the future (a clock that was wrong).
fn clean<T: Copy>(points: &[T], at: impl Fn(&T) -> Timestamp, now: Timestamp) -> Vec<T> {
    let mut v: Vec<T> = points.iter().copied().filter(|p| at(p).since(now) <= FUTURE_SLACK_MS).collect();
    v.sort_by_key(|p| at(p));
    v.dedup_by_key(|p| at(p));
    v
}

/// The latest run of SSD readings with no counter reset in it, and when it started if the series was cut.
fn ssd_segment(points: &[WearPoint]) -> (Vec<WearPoint>, Option<Timestamp>) {
    let mut start = 0;
    for i in 1..points.len() {
        let (a, b) = (&points[i - 1], &points[i]);
        let pct_back = matches!((a.percent_used, b.percent_used), (Some(x), Some(y)) if y + 1.0 < x);
        let bytes_back = matches!((a.bytes_written, b.bytes_written), (Some(x), Some(y)) if y < x);
        if pct_back || bytes_back {
            start = i;
        }
    }
    (points[start..].to_vec(), (start > 0).then(|| points[start].at))
}

/// The latest run of battery readings with no replacement in it, and when it started if the series was cut.
fn battery_segment(points: &[CapacityPoint]) -> (Vec<CapacityPoint>, Option<Timestamp>) {
    let mut start = 0;
    for i in 1..points.len() {
        let (a, b) = (&points[i - 1], &points[i]);
        let cycles_back = matches!((a.cycles, b.cycles), (Some(x), Some(y)) if y + 20 < x);
        if b.health_percent >= a.health_percent + BATTERY_JUMP || cycles_back {
            start = i;
        }
    }
    (points[start..].to_vec(), (start > 0).then(|| points[start].at))
}

fn restart_note(restarted: Option<Timestamp>, what: &str) -> String {
    match restarted {
        Some(_) => format!(" Dyno noticed a different {what} appeared (readings jumped), so only the readings since then are used."),
        None => String::new(),
    }
}

/// Forecasts how long an SSD has left, from stored readings (any order).
pub fn ssd_life(facts: &SsdFacts<'_>, readings: &[WearPoint], now: Timestamp) -> LifeForecast {
    let cleaned = clean(readings, |p| p.at, now);
    let (seg, restarted) = ssd_segment(&cleaned);
    let mut base = LifeForecast {
        subject: LifeSubject::Ssd,
        name: facts.name.into(),
        outcome: LifeOutcome::NoWearDetected,
        confidence: Confidence::Low,
        headline: String::new(),
        explanation: String::new(),
        basis: vec![],
        history_days: 0.0,
        readings: seg.len() as u32,
        restarted_at: restarted,
    };
    let Some(last) = seg.last() else {
        return unavailable(LifeSubject::Ssd, facts.name, "Dyno has no wear reading for this disk yet.");
    };
    let span = seg.first().map_or(0.0, |f| days_between(f.at, last.at));
    base.history_days = span as f32;
    if let Some(p) = last.percent_used {
        base.basis.push(Evidence::new("Used so far", format!("{p:.0}% of rated endurance")));
    }
    if let Some(w) = last.bytes_written {
        base.basis.push(Evidence::new("Written so far", bytes(w)));
    }
    if let Some(r) = facts.rated_bytes {
        base.basis.push(Evidence::new("Rated for", bytes(r)));
    }
    base.basis.push(Evidence::new("History", history_text(seg.len(), span)));
    let note = restart_note(restarted, "disk");

    if last.percent_used.is_some_and(|p| p >= 100.0)
        || matches!((last.bytes_written, facts.rated_bytes), (Some(w), Some(r)) if r > 0 && w >= r)
    {
        base.outcome = LifeOutcome::AlreadyPastLimit;
        base.confidence = Confidence::High;
        base.headline = "This disk has used up its rated endurance".into();
        base.explanation = format!(
            "Drives usually keep working past their rating, but they are no longer covered by it and failures become more likely. \
             Back up what matters and plan a replacement.{note}"
        );
        return base;
    }

    // Candidate estimates, each (low, mid, high, spread, what it stands on).
    let mut candidates: Vec<(f64, f64, f64, f64, &str)> = Vec::new();
    let enough = span >= SSD_MIN_DAYS && seg.len() >= SSD_MIN_READINGS;
    if enough {
        let pct: Vec<(f64, f64)> = seg
            .iter()
            .filter_map(|p| p.percent_used.map(|v| (days_between(seg[0].at, p.at), f64::from(v))))
            .collect();
        if pct.len() >= SSD_MIN_READINGS {
            if let (Some(r), Some(u)) = (robust_slope(&pct), last.percent_used) {
                let p = pace(r, 1.0, span);
                // Whole-percent counters step: when nothing moved, at most one step happened over the history.
                let p = if p.mid <= f64::EPSILON {
                    Pace {
                        fast: 1.0 / span.max(1.0),
                        mid: 0.5 / span.max(1.0),
                        slow: 0.0,
                        spread: f64::INFINITY,
                    }
                } else {
                    p
                };
                let (l, m, h) = project(f64::from(100.0 - u), &p);
                candidates.push((l, m, h, p.spread, "the disk's own wear counter"));
            }
        }
        if let Some(rated) = facts.rated_bytes.filter(|r| *r > 0) {
            let written: Vec<(f64, f64)> = seg
                .iter()
                .filter_map(|p| p.bytes_written.map(|v| (days_between(seg[0].at, p.at), v as f64)))
                .collect();
            if written.len() >= SSD_MIN_READINGS {
                if let (Some(r), Some(w)) = (robust_slope(&written), last.bytes_written) {
                    let p = pace(r, 1.0, span);
                    let (l, m, h) = project(rated.saturating_sub(w) as f64, &p);
                    candidates.push((l, m, h, p.spread, "the bytes written against the rated endurance"));
                    base.basis
                        .push(Evidence::new("Writing", format!("{}/day", bytes(r.slope.max(0.0) as u64))));
                }
            }
        }
    }
    if candidates.is_empty() {
        // The disk's lifetime average is the only pace available: a weak but honest estimate.
        if let (Some(pct), Some(hours)) = (last.percent_used, facts.power_on_hours.filter(|h| *h >= 24 * 14)) {
            if pct > 0.0 {
                let per_day = f64::from(pct) / (hours as f64 / 24.0);
                let p = Pace {
                    fast: per_day * 1.5,
                    mid: per_day,
                    slow: per_day * 0.6,
                    spread: 2.5,
                };
                let (l, m, h) = project(f64::from(100.0 - pct), &p);
                base.basis.push(Evidence::new("Powered on", format!("{} hours", hours)));
                candidates.push((l, m, h, p.spread, "the average over the disk's whole life"));
            }
        }
    }
    let Some(&(low, mid, high, spread, from)) = candidates.iter().min_by(|a, b| a.1.total_cmp(&b.1)) else {
        base.outcome = if enough {
            LifeOutcome::NoWearDetected
        } else {
            LifeOutcome::TooLittleHistory {
                have_days: span as f32,
                need_days: SSD_MIN_DAYS as f32,
            }
        };
        if enough {
            base.confidence = Confidence::Medium;
            base.headline = "No measurable wear".into();
            base.explanation = format!("Nothing was recorded as worn over {}.{note}", human_days(span));
        } else {
            base.headline = "Not enough history for a forecast yet".into();
            base.explanation = format!(
                "Dyno has {} of readings for this disk and needs about {} to see how fast it wears.{note}",
                human_days(span),
                human_days(SSD_MIN_DAYS)
            );
        }
        return base;
    };
    base.confidence = if from.starts_with("the average") {
        Confidence::Low
    } else {
        confidence_of(span, seg.len(), spread)
    };
    base.explanation = format!("Based on {from}, at the pace of the last {}.{note}", human_days(span));
    if low >= HORIZON_DAYS {
        base.outcome = LifeOutcome::BeyondHorizon;
        base.headline = "More than 10 years of writes left".into();
    } else {
        base.outcome = LifeOutcome::Remaining {
            low_days: low as f32,
            mid_days: mid as f32,
            high_days: high as f32,
        };
        base.headline = format!("{} of writes left", range_text(low, mid, high));
    }
    base
}

/// Forecasts when a battery should be replaced, from stored readings (any order).
pub fn battery_life(facts: &BatteryFacts<'_>, readings: &[CapacityPoint], now: Timestamp) -> LifeForecast {
    let cleaned = clean(readings, |p| p.at, now);
    let cleaned: Vec<CapacityPoint> = cleaned
        .into_iter()
        .filter(|p| p.health_percent.is_finite() && p.health_percent > 0.0)
        .collect();
    let (seg, restarted) = battery_segment(&cleaned);
    let Some(last) = seg.last() else {
        return unavailable(
            LifeSubject::Battery,
            facts.name,
            "Dyno has no capacity reading for this battery yet.",
        );
    };
    let span = seg.first().map_or(0.0, |f| days_between(f.at, last.at));
    let note = restart_note(restarted, "battery");
    let mut f = LifeForecast {
        subject: LifeSubject::Battery,
        name: facts.name.into(),
        outcome: LifeOutcome::NoWearDetected,
        confidence: Confidence::Low,
        headline: String::new(),
        explanation: String::new(),
        basis: vec![Evidence::new(
            "Holds",
            format!("{:.0}% of its original charge", last.health_percent),
        )],
        history_days: span as f32,
        readings: seg.len() as u32,
        restarted_at: restarted,
    };
    if let Some(c) = last.cycles {
        f.basis.push(Evidence::new(
            "Charge cycles",
            facts.rated_cycles.map_or(format!("{c}"), |r| format!("{c} of about {r}")),
        ));
    }
    f.basis.push(Evidence::new("History", history_text(seg.len(), span)));

    if last.health_percent <= BATTERY_REPLACE_AT {
        f.outcome = LifeOutcome::AlreadyPastLimit;
        f.confidence = Confidence::High;
        f.headline = "Time to think about a new battery".into();
        f.explanation = format!(
            "It now holds {:.0}% of its original charge; below {BATTERY_REPLACE_AT:.0}% a battery is usually considered worn out, \
             and runtime keeps shrinking.{note}",
            last.health_percent
        );
        return f;
    }
    if span < BATTERY_MIN_DAYS || seg.len() < BATTERY_MIN_READINGS {
        f.outcome = LifeOutcome::TooLittleHistory {
            have_days: span as f32,
            need_days: BATTERY_MIN_DAYS as f32,
        };
        f.headline = "Not enough history for a forecast yet".into();
        f.explanation = format!(
            "Dyno has {} of readings for this battery and needs about {} to see how fast its capacity fades.{note}",
            human_days(span),
            human_days(BATTERY_MIN_DAYS)
        );
        return f;
    }
    let xy: Vec<(f64, f64)> = seg
        .iter()
        .map(|p| (days_between(seg[0].at, p.at), f64::from(p.health_percent)))
        .collect();
    let Some(r) = robust_slope(&xy) else {
        f.headline = "Not enough history for a forecast yet".into();
        f.outcome = LifeOutcome::TooLittleHistory {
            have_days: span as f32,
            need_days: BATTERY_MIN_DAYS as f32,
        };
        return f;
    };
    let p = pace(r, -1.0, span);
    let headroom = f64::from(last.health_percent - BATTERY_REPLACE_AT);
    let (mut low, mut mid, mut high) = project(headroom, &p);
    let mut from = "its capacity trend";
    // Cycles give a second opinion when the maker's rating is known: take the sooner of the two.
    if let (Some(rated), true) = (
        facts.rated_cycles.filter(|r| *r > 0),
        seg.iter().filter(|p| p.cycles.is_some()).count() >= BATTERY_MIN_READINGS,
    ) {
        let cy: Vec<(f64, f64)> = seg
            .iter()
            .filter_map(|p| p.cycles.map(|c| (days_between(seg[0].at, p.at), f64::from(c))))
            .collect();
        if let (Some(cr), Some(c)) = (robust_slope(&cy), last.cycles) {
            let cp = pace(cr, 1.0, span);
            let (l2, m2, h2) = project(f64::from(rated.saturating_sub(c)), &cp);
            f.basis
                .push(Evidence::new("Cycling", format!("{:.1} cycles a month", cr.slope.max(0.0) * 30.4)));
            if m2 < mid {
                (low, mid, high) = (l2, m2, h2);
                from = "its charge-cycle count";
            }
        }
    }
    f.confidence = confidence_of(span, seg.len(), p.spread);
    f.explanation = format!("Based on {from} over the last {}.{note}", human_days(span));
    if p.mid <= 0.0005 && from == "its capacity trend" {
        f.outcome = LifeOutcome::NoWearDetected;
        f.headline = "No measurable capacity loss".into();
        f.explanation = format!("Its capacity did not fall over the last {}.{note}", human_days(span));
    } else if low >= HORIZON_DAYS {
        f.outcome = LifeOutcome::BeyondHorizon;
        f.headline = "Years of battery life left".into();
    } else {
        f.outcome = LifeOutcome::Remaining {
            low_days: low as f32,
            mid_days: mid as f32,
            high_days: high as f32,
        };
        f.headline = format!("Replace in {}", range_text(low, mid, high));
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    fn t(day: f64) -> Timestamp {
        Timestamp((day * DAY as f64) as i64)
    }

    /// An SSD that wears `per_day` percent a day, read every `every` days up to `days`.
    fn wear(days: u32, every: u32, start_pct: f32, per_day: f32, gb_per_day: u64) -> Vec<WearPoint> {
        (0..=days / every)
            .map(|i| {
                let d = (i * every) as f32;
                WearPoint {
                    at: t(f64::from(d)),
                    percent_used: Some((start_pct + per_day * d).round()),
                    bytes_written: Some(1_000 * GB + gb_per_day * GB * u64::from(i * every)),
                }
            })
            .collect()
    }

    fn facts(rated: Option<u64>) -> SsdFacts<'static> {
        SsdFacts {
            name: "Test SSD",
            rated_bytes: rated,
            power_on_hours: None,
        }
    }

    #[test]
    fn a_steady_ssd_gets_a_range() {
        // 10 % used, 0.02 %/day for 200 days => 14 % now, 86 % left at 0.02/day = 4300 days: beyond the horizon.
        let f = ssd_life(&facts(None), &wear(200, 2, 10.0, 0.05, 0), t(200.0));
        // 0.05 %/day => 86/0.05... start 10 -> 20 after 200 days; remaining 80 % at 0.05 = 1600 days (~4.4 years).
        let LifeOutcome::Remaining {
            low_days,
            mid_days,
            high_days,
        } = f.outcome
        else {
            panic!("{f:?}")
        };
        assert!(low_days < mid_days && mid_days < high_days, "{low_days} {mid_days} {high_days}");
        assert!((mid_days - 1600.0).abs() < 200.0, "{mid_days}");
        assert_eq!(f.confidence, Confidence::High, "{f:?}");
        assert!(f.headline.contains("years") && f.headline.contains(" to "), "{}", f.headline);
        assert!(f.basis.iter().any(|e| e.label == "History"));
        assert_eq!(f.readings, 101);
        assert!(f.restarted_at.is_none());
    }

    #[test]
    fn rated_bytes_give_an_estimate_too_and_the_sooner_one_wins() {
        // 1 TB/day... 2 GB/day written, rated 600 TB: ~299,000 days; percent says 1600: the percent estimate is sooner.
        let f = ssd_life(&facts(Some(600_000 * GB)), &wear(200, 2, 10.0, 0.05, 2), t(200.0));
        assert!(matches!(f.outcome, LifeOutcome::Remaining { .. }), "{f:?}");
        assert!(f.explanation.contains("wear counter"), "{}", f.explanation);
        // Heavy writing against a small rating: the bytes estimate is sooner.
        let g = ssd_life(&facts(Some(1_800 * GB)), &wear(200, 2, 0.0, 0.0, 3), t(200.0));
        // written 1000 GB + 600 GB = 1600 GB of 1800 => 200 GB at 3 GB/day = ~67 days
        let LifeOutcome::Remaining { mid_days, .. } = g.outcome else {
            panic!("{g:?}")
        };
        assert!((mid_days - 67.0).abs() < 15.0, "{mid_days}");
        assert!(g.explanation.contains("bytes written"), "{}", g.explanation);
        assert!(g.basis.iter().any(|e| e.label == "Writing"));
    }

    #[test]
    fn too_little_history_says_so_and_gives_no_number() {
        let f = ssd_life(&facts(None), &wear(6, 1, 3.0, 0.1, 1), t(6.0));
        assert!(matches!(f.outcome, LifeOutcome::TooLittleHistory { .. }), "{f:?}");
        assert!(f.mid_days().is_none());
        assert!(f.headline.contains("Not enough history"));
        assert!(!f.needs_attention());
        // Plenty of days but only three readings.
        let sparse = ssd_life(&facts(None), &wear(60, 30, 3.0, 0.1, 1), t(60.0));
        assert!(matches!(sparse.outcome, LifeOutcome::TooLittleHistory { .. }), "{sparse:?}");
    }

    #[test]
    fn the_disks_own_lifetime_average_is_a_low_confidence_fallback() {
        let mut one = wear(0, 1, 10.0, 0.0, 0);
        one[0].percent_used = Some(10.0);
        let f = ssd_life(
            &SsdFacts {
                name: "d",
                rated_bytes: None,
                power_on_hours: Some(24 * 400),
            },
            &one,
            t(0.0),
        );
        let LifeOutcome::Remaining { mid_days, .. } = f.outcome else {
            panic!("{f:?}")
        };
        // 10 % in 400 days = 0.025 %/day => 90 % left = 3600 days.
        assert!((mid_days - 3_600.0).abs() < 50.0, "{mid_days}");
        assert_eq!(f.confidence, Confidence::Low);
        assert!(f.explanation.contains("whole life"));
        // No power-on hours and a single reading: nothing to go on.
        let g = ssd_life(&facts(None), &one, t(0.0));
        assert!(matches!(g.outcome, LifeOutcome::TooLittleHistory { .. }));
    }

    #[test]
    fn no_wear_is_not_a_forecast() {
        let f = ssd_life(&facts(None), &wear(60, 2, 0.0, 0.0, 0), t(60.0));
        assert!(
            matches!(f.outcome, LifeOutcome::BeyondHorizon | LifeOutcome::Remaining { .. }),
            "{f:?}"
        );
        // With neither counter, there is nothing measurable.
        let blank: Vec<WearPoint> = (0..30)
            .map(|i| WearPoint {
                at: t(f64::from(i)),
                percent_used: None,
                bytes_written: None,
            })
            .collect();
        let b = ssd_life(&facts(None), &blank, t(30.0));
        assert_eq!(b.outcome, LifeOutcome::NoWearDetected);
        assert!(b.headline.contains("No measurable"));
    }

    #[test]
    fn a_disk_past_its_rating_is_flagged() {
        let mut pts = wear(30, 2, 100.0, 0.0, 0);
        pts.iter_mut().for_each(|p| p.percent_used = Some(103.0));
        let f = ssd_life(&facts(None), &pts, t(30.0));
        assert_eq!(f.outcome, LifeOutcome::AlreadyPastLimit);
        assert!(f.needs_attention());
        let by_bytes = ssd_life(&facts(Some(500 * GB)), &wear(30, 2, 1.0, 0.0, 0), t(30.0));
        assert_eq!(by_bytes.outcome, LifeOutcome::AlreadyPastLimit);
    }

    #[test]
    fn a_replaced_disk_starts_the_series_again() {
        let mut pts = wear(120, 2, 40.0, 0.05, 1);
        // On day 121 the counters restart: a new disk.
        pts.extend((0..=40).map(|i| WearPoint {
            at: t(121.0 + f64::from(i) * 2.0),
            percent_used: Some(0.0),
            bytes_written: Some(5 * GB + u64::try_from(i).unwrap() * 2 * GB),
        }));
        let f = ssd_life(&facts(None), &pts, t(201.0));
        assert_eq!(f.restarted_at, Some(t(121.0)));
        assert!(f.explanation.contains("different disk"), "{}", f.explanation);
        assert!(f.history_days <= 80.5, "only the new disk's readings count: {}", f.history_days);
        assert_ne!(
            f.mid_days().map(|d| d < 1000.0),
            Some(true),
            "the old disk's 40 % must not shorten the new disk's life: {f:?}"
        );
    }

    #[test]
    fn clock_changes_and_unordered_readings_are_tolerated() {
        let mut pts = wear(100, 2, 10.0, 0.05, 1);
        pts.reverse();
        pts.push(WearPoint {
            at: t(5_000.0),
            percent_used: Some(99.0),
            bytes_written: None,
        }); // from a clock set years ahead
        pts.push(pts[3]); // a duplicate time
        let f = ssd_life(&facts(None), &pts, t(100.0));
        assert!(f.restarted_at.is_none());
        assert!(
            matches!(f.outcome, LifeOutcome::Remaining { .. } | LifeOutcome::BeyondHorizon),
            "{f:?}"
        );
        assert!(ssd_life(&facts(None), &[], t(0.0)).mid_days().is_none());
        assert!(matches!(
            ssd_life(&facts(None), &[], t(0.0)).outcome,
            LifeOutcome::Unavailable { .. }
        ));
    }

    fn cap(months: u32, start: f32, per_month: f32) -> Vec<CapacityPoint> {
        (0..=months * 4)
            .map(|w| CapacityPoint {
                at: t(f64::from(w) * 7.0),
                health_percent: start - per_month * (w as f32 / 4.0),
                cycles: Some(w * 10),
            })
            .collect()
    }

    fn bat(rated: Option<u32>) -> BatteryFacts<'static> {
        BatteryFacts {
            name: "Battery",
            rated_cycles: rated,
        }
    }

    #[test]
    fn a_fading_battery_gets_replace_in_months() {
        // 96 % falling 0.5 points a month over 8 months: now 92 %, 12 points above 80 => 24 months.
        let f = battery_life(&bat(None), &cap(8, 96.0, 0.5), t(8.0 * 28.0));
        let LifeOutcome::Remaining {
            low_days,
            mid_days,
            high_days,
        } = f.outcome
        else {
            panic!("{f:?}")
        };
        assert!((mid_days / 30.4 - 24.0).abs() < 3.0, "{mid_days}");
        assert!(low_days <= mid_days && mid_days <= high_days);
        assert!(f.headline.starts_with("Replace in about"), "{}", f.headline);
        assert!(f.confidence >= Confidence::Medium, "{f:?}");
        assert!(f.basis.iter().any(|e| e.label == "Holds"));
        assert!(f.basis.iter().any(|e| e.label == "Charge cycles"));
    }

    #[test]
    fn cycles_can_call_for_a_sooner_replacement() {
        // Capacity barely moves but the cycle count runs towards a rating of 400: cycles are the sooner estimate.
        let mut pts = cap(8, 99.0, 0.05);
        pts.iter_mut().for_each(|p| p.cycles = p.cycles.map(|c| c * 3));
        let f = battery_life(&bat(Some(400)), &pts, t(8.0 * 28.0));
        assert!(f.explanation.contains("cycle count"), "{}", f.explanation);
        assert!(f.mid_days().is_some_and(|d| d < 365.0), "{f:?}");
        assert!(f.basis.iter().any(|e| e.label == "Cycling"));
    }

    #[test]
    fn battery_edge_cases() {
        let past = battery_life(&bat(None), &cap(8, 84.0, 1.0), t(224.0));
        assert_eq!(past.outcome, LifeOutcome::AlreadyPastLimit);
        assert!(past.needs_attention());
        let young = battery_life(&bat(None), &cap(0, 99.0, 0.0), t(0.0));
        assert!(matches!(young.outcome, LifeOutcome::TooLittleHistory { .. }), "{young:?}");
        assert!(young.explanation.contains("needs about"));
        let flat = battery_life(&bat(None), &cap(8, 95.0, 0.0), t(224.0));
        assert_eq!(flat.outcome, LifeOutcome::NoWearDetected);
        assert!(matches!(
            battery_life(&bat(None), &[], t(0.0)).outcome,
            LifeOutcome::Unavailable { .. }
        ));
        let nan = [CapacityPoint {
            at: t(0.0),
            health_percent: f32::NAN,
            cycles: None,
        }];
        assert!(matches!(
            battery_life(&bat(None), &nan, t(0.0)).outcome,
            LifeOutcome::Unavailable { .. }
        ));
    }

    #[test]
    fn a_new_battery_starts_the_series_again() {
        let mut pts = cap(8, 90.0, 1.5); // old battery down to ~78 %
        let last = pts.last().unwrap().at;
        pts.extend((0..8).map(|i| CapacityPoint {
            at: last.plus((i + 1) * 7 * DAY),
            health_percent: 100.0,
            cycles: Some(i as u32),
        }));
        let f = battery_life(&bat(None), &pts, last.plus(60 * DAY));
        assert!(f.restarted_at.is_some());
        assert!(
            f.explanation.contains("different battery") || matches!(f.outcome, LifeOutcome::TooLittleHistory { .. }),
            "{f:?}"
        );
        assert!(
            !matches!(f.outcome, LifeOutcome::AlreadyPastLimit),
            "the old battery's wear must not carry over: {f:?}"
        );
        // The same by cycles dropping (the capacity looks continuous).
        let mut again = cap(6, 95.0, 0.2);
        let n = again.len();
        again[n - 1].cycles = Some(2);
        assert!(battery_life(&bat(None), &again, t(300.0)).restarted_at.is_some());
    }

    #[test]
    fn wording_and_attention() {
        assert_eq!(human_days(0.2), "less than a day");
        assert_eq!(history_text(1, 0.0), "1 reading");
        assert_eq!(human_days(1.0), "1 day");
        assert_eq!(human_days(9.0), "9 days");
        assert_eq!(human_days(42.0), "6 weeks");
        assert_eq!(human_days(300.0), "10 months");
        assert_eq!(human_days(1_600.0), "4.4 years");
        assert_eq!(human_days(3_650.0), "10 years");
        assert_eq!(range_text(4_000.0, 4_500.0, 5_000.0), "more than 10 years");
        assert!(range_text(1_000.0, 2_000.0, 4_000.0).contains("at least"));
        assert!(range_text(100.0, 101.0, 102.0).starts_with("about 3 months"));
        let mut f = unavailable(LifeSubject::Ssd, "x", "Needs smartctl");
        assert!(!f.needs_attention() && f.explanation == "Needs smartctl");
        f.outcome = LifeOutcome::Remaining {
            low_days: 100.0,
            mid_days: 200.0,
            high_days: 300.0,
        };
        assert!(!f.needs_attention(), "low confidence is never alarming");
        f.confidence = Confidence::Medium;
        assert!(f.needs_attention());
        assert_eq!(
            unavailable(LifeSubject::Battery, "b", "r").headline,
            "Battery life cannot be estimated"
        );
    }
}
