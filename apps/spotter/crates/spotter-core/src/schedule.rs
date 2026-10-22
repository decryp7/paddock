//! When to check next. A watch has one of three schedules ([`Schedule`]): every so often, at fixed times of day, or a
//! cron expression. This module holds the pure maths for all of them: the next check with jitter (so many watches on
//! one server do not fire together), exponential backoff after failures (so an offline share or a rate-limited API
//! is not hammered), the per-source minimum interval, the description in words and the preview of the next runs.
//!
//! Time is always passed in as values, never read. A machine that slept through several runs does one catch-up
//! check on wake, not a burst: the next check is always computed from the moment the last check finished.

use crate::calendar::{self, LocalZone, Weekday, MAX_TIMES};
use crate::limits::{MAX_BACKOFF_SECS, MAX_INTERVAL_SECS, MIN_INTERVAL_SECS};
use crate::time::{ClockTime, Timestamp};
use serde::{Deserialize, Serialize};

/// Shortest interval for a source on the network: a polite default for any site (the folder scan is local and may
/// run as often as [`MIN_INTERVAL_SECS`]).
pub const MIN_SITE_INTERVAL_SECS: u32 = 60;
/// Most seconds of random delay added to a calendar schedule's time, so watches on one host do not fire together.
pub const CALENDAR_JITTER_SECS: u32 = 30;
/// How many upcoming runs the preview lists.
pub const PREVIEW_RUNS: usize = 5;
/// How many upcoming runs are looked at to find out whether a schedule fires more often than the source allows.
const PROBE_RUNS: usize = 64;
/// 2024-01-01 00:00 UTC: where "does this expression ever match" starts looking.
const REFERENCE_MS: i64 = 1_704_067_200_000;

/// When a watch is checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum Schedule {
    /// Every so often, from the last check.
    Every {
        /// Seconds between checks.
        secs: u32,
    },
    /// On chosen days at chosen times of day.
    Times {
        /// The days of the week.
        days: Vec<Weekday>,
        /// The times of day (local, or in `zone`).
        times: Vec<ClockTime>,
        /// An IANA time zone; the computer's own when absent.
        #[serde(default)]
        zone: Option<String>,
    },
    /// A cron expression: five fields, or six with a leading seconds field.
    Cron {
        /// The expression.
        expr: String,
        /// An IANA time zone; the computer's own when absent.
        #[serde(default)]
        zone: Option<String>,
    },
}

impl Default for Schedule {
    fn default() -> Schedule {
        Schedule::Every { secs: 300 }
    }
}

/// What the next check depends on besides the schedule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    /// The shortest time between two checks of this source.
    pub min_interval_secs: u32,
    /// Consecutive failures so far (0 after a success).
    pub failures: u32,
    /// A random number in `[0, 1)` from the caller.
    pub jitter: f64,
}

/// The upcoming runs of a schedule, for the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulePreview {
    /// The schedule in words.
    pub description: String,
    /// The next runs (nominal times, without jitter), at most [`PREVIEW_RUNS`].
    pub next_runs: Vec<Timestamp>,
    /// Things worth knowing, such as a schedule that is clamped to the minimum interval.
    pub warnings: Vec<String>,
    /// The time zone the times are in.
    pub zone: String,
}

/// The parsed form of a [`Schedule`]: parse once, then work with the types.
enum Plan {
    Every(u32),
    Times {
        days: Vec<Weekday>,
        times: Vec<ClockTime>,
        zone: jiff::tz::TimeZone,
    },
    Cron {
        cron: Box<croner::Cron>,
        zone: jiff::tz::TimeZone,
    },
}

impl Plan {
    /// The first nominal run strictly after `after`.
    fn next_after(&self, after: Timestamp) -> Option<Timestamp> {
        match self {
            Plan::Every(secs) => Some(after.plus_secs(u64::from(*secs))),
            Plan::Times { days, times, zone } => calendar::next_at_times(days, times, zone, after),
            Plan::Cron { cron, zone } => calendar::next_cron(cron, zone, after),
        }
    }
}

impl Schedule {
    /// Every `secs` seconds.
    pub fn every(secs: u32) -> Schedule {
        Schedule::Every { secs }
    }

    /// Checks the schedule; the error lists every problem and says what to fix.
    pub fn check(&self) -> Result<(), Vec<String>> {
        // The machine's zone only matters for evaluating, not for whether the schedule is valid.
        self.plan(&LocalZone::default()).map(|_| ())
    }

    /// The zone the user chose, if any.
    fn chosen_zone(&self) -> Option<&str> {
        match self {
            Schedule::Every { .. } => None,
            Schedule::Times { zone, .. } | Schedule::Cron { zone, .. } => zone.as_deref(),
        }
    }

    fn plan(&self, local: &LocalZone) -> Result<Plan, Vec<String>> {
        match self {
            Schedule::Every { secs } => Ok(Plan::Every(clamp_interval(*secs))),
            Schedule::Times { days, times, zone } => {
                let mut problems = Vec::new();
                if days.is_empty() {
                    problems.push("Pick at least one day.".to_string());
                }
                if times.is_empty() {
                    problems.push("Add at least one time of day.".to_string());
                }
                if times.len() > MAX_TIMES {
                    problems.push(format!("Use at most {MAX_TIMES} times of day."));
                }
                let zone = calendar::resolve_zone(zone.as_deref(), local).map_err(|e| problems.push(e)).ok();
                match zone {
                    Some(zone) if problems.is_empty() => {
                        let (mut days, mut times) = (days.clone(), times.clone());
                        days.sort();
                        days.dedup();
                        times.sort();
                        times.dedup();
                        Ok(Plan::Times { days, times, zone })
                    }
                    _ => Err(problems),
                }
            }
            Schedule::Cron { expr, zone } => {
                let mut problems = Vec::new();
                let cron = calendar::parse_cron(expr).map_err(|e| problems.push(e)).ok();
                let zone = calendar::resolve_zone(zone.as_deref(), local).map_err(|e| problems.push(e)).ok();
                match (cron, zone) {
                    (Some(cron), Some(zone)) if problems.is_empty() => {
                        let plan = Plan::Cron {
                            cron: Box::new(cron),
                            zone,
                        };
                        // An expression that matches no real date (30 February) would never run.
                        if plan.next_after(Timestamp(REFERENCE_MS)).is_none() {
                            return Err(vec![
                                "This cron expression never matches a real date. Check the day and month fields.".into(),
                            ]);
                        }
                        Ok(plan)
                    }
                    _ => Err(problems),
                }
            }
        }
    }

    /// The schedule in words (`Every 6 hours`, `Weekdays at 09:00 and 14:00`, or the cron description).
    pub fn describe(&self) -> String {
        match self {
            Schedule::Every { secs } => format!("Every {}", span_text(*secs)),
            Schedule::Times { days, times, zone } => {
                let (mut days, mut times) = (days.clone(), times.clone());
                days.sort();
                days.dedup();
                times.sort();
                times.dedup();
                let times: Vec<String> = times.into_iter().map(String::from).collect();
                format!("{} at {}{}", days_text(&days), join_and(&times), zone_suffix(zone.as_deref()))
            }
            Schedule::Cron { expr, zone } => match calendar::parse_cron(expr) {
                Ok(cron) => format!("{}{}", cron.describe(), zone_suffix(zone.as_deref())),
                Err(_) => format!("Cron {}", expr.trim()),
            },
        }
    }
}

fn zone_suffix(zone: Option<&str>) -> String {
    zone.map(str::trim)
        .filter(|z| !z.is_empty())
        .map_or_else(String::new, |z| format!(" ({z})"))
}

fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

fn days_text(days: &[Weekday]) -> String {
    if days == Weekday::ALL {
        "Every day".into()
    } else if days == Weekday::WEEKDAYS {
        "Weekdays".into()
    } else if days == [Weekday::Sat, Weekday::Sun] {
        "Weekends".into()
    } else {
        join_and(&days.iter().map(|d| d.short().to_string()).collect::<Vec<_>>())
    }
}

/// `6 hours`, `day`, `15 minutes`, `90 seconds` (for "Every ...").
pub fn span_text(secs: u32) -> String {
    let unit = |n: u32, one: &str, many: &str| if n == 1 { one.to_string() } else { format!("{n} {many}") };
    if secs.is_multiple_of(86_400) {
        unit(secs / 86_400, "day", "days")
    } else if secs.is_multiple_of(3600) {
        unit(secs / 3600, "hour", "hours")
    } else if secs.is_multiple_of(60) {
        unit(secs / 60, "minute", "minutes")
    } else {
        unit(secs, "second", "seconds")
    }
}

/// When the next check of a schedule runs, computed at `now` (the moment the last check finished or the schedule
/// was saved). A calendar schedule never runs closer than the source's minimum interval to `now`, so an expression
/// that fires too often is clamped; after failures the wait grows like the interval schedule's backoff. A machine
/// that slept through several runs gets one check, because the answer is always after `now`.
pub fn next_check(schedule: &Schedule, local: &LocalZone, now: Timestamp, timing: Timing) -> Result<Timestamp, Vec<String>> {
    let plan = schedule.plan(local)?;
    let min = timing.min_interval_secs.clamp(MIN_INTERVAL_SECS, MAX_INTERVAL_SECS);
    if let Plan::Every(secs) = plan {
        return Ok(now.plus_secs(next_delay_secs(secs.max(min), timing.failures, timing.jitter)));
    }
    let wait = if timing.failures == 0 {
        u64::from(min)
    } else {
        next_delay_secs(min, timing.failures, timing.jitter)
    };
    // The run at exactly `now + wait` still counts.
    let after = Timestamp(now.plus_secs(wait).0 - 1);
    let nominal = plan
        .next_after(after)
        .ok_or_else(|| vec!["This schedule has no upcoming run.".to_string()])?;
    let jitter = if timing.jitter.is_finite() {
        timing.jitter.clamp(0.0, 1.0)
    } else {
        0.5
    };
    Ok(nominal.plus_secs((jitter * f64::from(CALENDAR_JITTER_SECS)) as u64))
}

/// The upcoming runs, the description and the warnings for the editor. `now` is the current time.
pub fn preview(schedule: &Schedule, local: &LocalZone, now: Timestamp, min_interval_secs: u32) -> Result<SchedulePreview, Vec<String>> {
    let plan = schedule.plan(local)?;
    let min = min_interval_secs.clamp(MIN_INTERVAL_SECS, MAX_INTERVAL_SECS);
    let step_ms = i64::from(min) * 1000;
    let mut warnings = Vec::new();
    let mut next_runs = Vec::new();
    if let Plan::Every(secs) = &plan {
        if *secs < min {
            warnings.push(format!(
                "This source is checked at most every {}; that is used instead.",
                span_text(min)
            ));
        }
        let each = u64::from((*secs).max(min));
        next_runs.extend((1..=PREVIEW_RUNS as u64).map(|i| now.plus_secs(each * i)));
    } else {
        let mut after = now;
        while next_runs.len() < PREVIEW_RUNS {
            let Some(run) = plan.next_after(after) else { break };
            next_runs.push(run);
            after = Timestamp(run.0.saturating_add(step_ms) - 1);
        }
        if let Some(gap) = tightest_gap_ms(&plan, now).filter(|gap| *gap < step_ms) {
            warnings.push(format!(
                "This runs as often as every {}, more often than this source is checked. It is clamped to at most once every {}, and the extra times are skipped.",
                span_text(u32::try_from((gap / 1000).max(1)).unwrap_or(u32::MAX)),
                span_text(min)
            ));
        }
    }
    Ok(SchedulePreview {
        description: schedule.describe(),
        next_runs,
        warnings,
        zone: calendar::zone_label(schedule.chosen_zone(), local),
    })
}

/// The smallest gap between consecutive runs among the next [`PROBE_RUNS`] (`None` with fewer than two).
fn tightest_gap_ms(plan: &Plan, now: Timestamp) -> Option<i64> {
    let mut previous: Option<Timestamp> = None;
    let mut tightest: Option<i64> = None;
    let mut after = now;
    for _ in 0..PROBE_RUNS {
        let Some(run) = plan.next_after(after) else { break };
        if let Some(p) = previous {
            tightest = Some(tightest.map_or(run.0 - p.0, |t| t.min(run.0 - p.0)));
        }
        previous = Some(run);
        after = run;
    }
    tightest
}

/// Clamps an interval to the allowed range.
pub fn clamp_interval(secs: u32) -> u32 {
    secs.clamp(MIN_INTERVAL_SECS, MAX_INTERVAL_SECS)
}

/// Seconds until the next check after `failures` consecutive failures (0 after a success). `jitter` is a random
/// number in `[0, 1)` from the caller; it spreads the delay by ±10 %. After failures the delay doubles per failure
/// up to [`MAX_BACKOFF_SECS`] (or the interval, when that is longer).
pub fn next_delay_secs(interval_secs: u32, failures: u32, jitter: f64) -> u64 {
    let interval = u64::from(clamp_interval(interval_secs));
    let base = if failures == 0 {
        interval
    } else {
        let cap = interval.max(u64::from(MAX_BACKOFF_SECS));
        interval.saturating_mul(1u64 << failures.min(16)).min(cap)
    };
    let jitter = if jitter.is_finite() { jitter.clamp(0.0, 1.0) } else { 0.5 };
    let factor = 0.9 + 0.2 * jitter;
    ((base as f64) * factor).round().max(f64::from(MIN_INTERVAL_SECS)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{wall_clock, zone_named};

    fn ts(text: &str) -> Timestamp {
        Timestamp::parse(text).unwrap()
    }

    fn t(h: u8, m: u8) -> ClockTime {
        ClockTime::new(h, m).unwrap()
    }

    fn berlin() -> LocalZone {
        LocalZone {
            name: Some("Europe/Berlin".into()),
            offset_minutes: 60,
        }
    }

    fn timing(min: u32) -> Timing {
        Timing {
            min_interval_secs: min,
            failures: 0,
            jitter: 0.0,
        }
    }

    fn at_times(days: &[Weekday], times: &[ClockTime], zone: Option<&str>) -> Schedule {
        Schedule::Times {
            days: days.to_vec(),
            times: times.to_vec(),
            zone: zone.map(Into::into),
        }
    }

    fn cron(expr: &str) -> Schedule {
        Schedule::Cron {
            expr: expr.into(),
            zone: None,
        }
    }

    /// The preview's runs as wall-clock text in `zone`.
    fn walls(p: &SchedulePreview, zone: &str) -> Vec<String> {
        let z = zone_named(zone).unwrap();
        p.next_runs.iter().map(|r| wall_clock(*r, &z)).collect()
    }

    #[test]
    fn intervals_jitter_and_back_off() {
        assert_eq!(next_delay_secs(60, 0, 0.5), 60);
        assert_eq!(next_delay_secs(60, 0, 0.0), 54);
        assert_eq!(next_delay_secs(60, 0, 1.0), 66);
        assert_eq!(next_delay_secs(60, 1, 0.5), 120);
        assert_eq!(next_delay_secs(60, 3, 0.5), 480);
        assert_eq!(next_delay_secs(60, 40, 0.5), u64::from(MAX_BACKOFF_SECS));
        assert_eq!(
            next_delay_secs(MAX_INTERVAL_SECS, 5, 0.5),
            u64::from(MAX_INTERVAL_SECS),
            "long intervals cap at themselves"
        );
        assert_eq!(next_delay_secs(1, 0, 0.0), u64::from(MIN_INTERVAL_SECS), "never below the minimum");
        assert_eq!(next_delay_secs(60, 0, f64::NAN), 60);
        assert_eq!(next_delay_secs(60, 0, 7.0), 66);
        assert_eq!(clamp_interval(u32::MAX), MAX_INTERVAL_SECS);
    }

    #[test]
    fn every_mode_clamps_to_the_source_minimum() {
        let every = |secs| Schedule::every(secs);
        let now = ts("2026-10-22T08:00:00Z");
        let mid = |min| Timing {
            jitter: 0.5,
            ..timing(min)
        };
        let next = |s: &Schedule, min| next_check(s, &berlin(), now, mid(min)).unwrap();
        assert_eq!(next(&every(900), 60), now.plus_secs(900));
        assert_eq!(next(&every(30), 60), now.plus_secs(60), "below the site minimum");
        assert_eq!(next(&every(1), 10), now.plus_secs(10));
        assert_eq!(next(&every(u32::MAX), 10), now.plus_secs(u64::from(MAX_INTERVAL_SECS)));
        let jittery = Timing { jitter: 1.0, ..timing(60) };
        assert_eq!(next_check(&every(1000), &berlin(), now, jittery).unwrap(), now.plus_secs(1100));
        let failing = Timing {
            failures: 2,
            jitter: 0.5,
            ..timing(60)
        };
        assert_eq!(next_check(&every(100), &berlin(), now, failing).unwrap(), now.plus_secs(400));
        let p = preview(&every(30), &berlin(), now, 60).unwrap();
        assert!(p.warnings[0].contains("at most every minute"), "{:?}", p.warnings);
        assert_eq!(p.next_runs[0], now.plus_secs(60));
        let p = preview(&every(6 * 3600), &berlin(), now, 60).unwrap();
        assert_eq!(
            (p.description.as_str(), p.next_runs.len(), p.warnings.len()),
            ("Every 6 hours", 5, 0)
        );
        assert_eq!(p.next_runs[4], now.plus_secs(30 * 3600));
        assert_eq!(p.zone, "Europe/Berlin");
    }

    #[test]
    fn descriptions_are_in_words() {
        assert_eq!(Schedule::every(900).describe(), "Every 15 minutes");
        assert_eq!(Schedule::every(3600).describe(), "Every hour");
        assert_eq!(Schedule::every(86_400).describe(), "Every day");
        assert_eq!(Schedule::every(2 * 86_400).describe(), "Every 2 days");
        assert_eq!(Schedule::every(60).describe(), "Every minute");
        assert_eq!(Schedule::every(90).describe(), "Every 90 seconds");
        let d = |days: &[Weekday], times: &[ClockTime], zone| at_times(days, times, zone).describe();
        assert_eq!(
            d(&Weekday::WEEKDAYS, &[t(14, 0), t(9, 0), t(9, 0)], None),
            "Weekdays at 09:00 and 14:00"
        );
        assert_eq!(d(&Weekday::ALL, &[t(7, 30)], Some("Asia/Tokyo")), "Every day at 07:30 (Asia/Tokyo)");
        assert_eq!(d(&[Weekday::Sun, Weekday::Sat], &[t(8, 0)], None), "Weekends at 08:00");
        assert_eq!(
            d(&[Weekday::Fri, Weekday::Mon, Weekday::Wed], &[t(6, 0), t(12, 0), t(18, 0)], None),
            "Mon, Wed and Fri at 06:00, 12:00 and 18:00"
        );
        assert_eq!(d(&[Weekday::Tue], &[t(6, 0)], None), "Tue at 06:00");
        assert!(
            cron("0 9 * * 1-5").describe().contains("09:00"),
            "{}",
            cron("0 9 * * 1-5").describe()
        );
        let zoned = Schedule::Cron {
            expr: "0 9 * * *".into(),
            zone: Some("UTC".into()),
        };
        assert!(zoned.describe().ends_with("(UTC)"));
        assert_eq!(cron("nonsense").describe(), "Cron nonsense");
    }

    #[test]
    fn at_times_runs_on_the_chosen_days_in_the_chosen_zone() {
        let s = at_times(&Weekday::WEEKDAYS, &[t(14, 0), t(9, 0)], None);
        // Thursday 2026-10-22 10:00 in Berlin (08:00Z): the next runs are 14:00, then Friday 09:00 and 14:00, then Monday.
        let p = preview(&s, &berlin(), ts("2026-10-22T08:00:00Z"), 60).unwrap();
        assert_eq!(
            walls(&p, "Europe/Berlin"),
            [
                "2026-10-22 14:00",
                "2026-10-23 09:00",
                "2026-10-23 14:00",
                "2026-10-26 09:00",
                "2026-10-26 14:00"
            ]
        );
        assert!(p.warnings.is_empty());
        // A chosen zone wins over the machine's.
        let tokyo = at_times(&Weekday::ALL, &[t(9, 0)], Some("Asia/Tokyo"));
        let p = preview(&tokyo, &berlin(), ts("2026-10-22T08:00:00Z"), 60).unwrap();
        assert_eq!(p.next_runs[0], ts("2026-10-23T00:00:00Z"));
        assert_eq!(p.zone, "Asia/Tokyo");
        // With only an offset (no zone name) the machine's offset is used.
        let offset = LocalZone {
            name: None,
            offset_minutes: -330,
        };
        let p = preview(&at_times(&Weekday::ALL, &[t(9, 0)], None), &offset, ts("2026-10-22T08:00:00Z"), 60).unwrap();
        assert_eq!((p.next_runs[0], p.zone.as_str()), (ts("2026-10-22T14:30:00Z"), "UTC-05:30"));
        let east = LocalZone {
            name: Some("Not/AZone".into()),
            offset_minutes: 120,
        };
        assert_eq!(calendar::zone_label(None, &east), "UTC+02:00");
        // A weekday that is only seven days away.
        let sundays = at_times(&[Weekday::Thu], &[t(8, 0)], Some("UTC"));
        assert_eq!(
            next_check(&sundays, &berlin(), ts("2026-10-22T08:00:00Z"), timing(60)).unwrap(),
            ts("2026-10-29T08:00:00Z")
        );
    }

    #[test]
    fn at_times_follow_daylight_saving() {
        let daily = |h, m| at_times(&Weekday::ALL, &[t(h, m)], Some("Europe/Berlin"));
        let runs = |s: &Schedule, now: &str| walls(&preview(s, &LocalZone::default(), ts(now), 60).unwrap(), "Europe/Berlin");
        // Spring forward 2026-03-29: 02:00 CET becomes 03:00 CEST, so 02:30 does not exist and runs once, shifted.
        let spring = runs(&daily(2, 30), "2026-03-27T12:00:00Z");
        assert_eq!(&spring[..3], ["2026-03-28 02:30", "2026-03-29 03:30", "2026-03-30 02:30"]);
        // 09:00 stays 09:00 on the wall clock across the change (an hour earlier in UTC).
        let p = preview(&daily(9, 0), &LocalZone::default(), ts("2026-03-27T12:00:00Z"), 60).unwrap();
        assert_eq!(
            (p.next_runs[0], p.next_runs[1]),
            (ts("2026-03-28T08:00:00Z"), ts("2026-03-29T07:00:00Z"))
        );
        // Fall back 2026-10-25: 03:00 CEST becomes 02:00 CET, so 02:30 happens twice and runs once, the first time.
        let fall = daily(2, 30);
        let first = next_check(&fall, &LocalZone::default(), ts("2026-10-24T12:00:00Z"), timing(60)).unwrap();
        assert_eq!(first, ts("2026-10-25T00:30:00Z"), "02:30 CEST");
        let after_first = next_check(&fall, &LocalZone::default(), first, timing(60)).unwrap();
        assert_eq!(after_first, ts("2026-10-26T01:30:00Z"), "the repeated 02:30 CET is skipped");
        // The clocks-go-back day has 25 hours: 09:00 CET is 25 hours after 09:00 CEST the day before.
        let nine = daily(9, 0);
        let a = next_check(&nine, &LocalZone::default(), ts("2026-10-23T08:00:00Z"), timing(60)).unwrap();
        let b = next_check(&nine, &LocalZone::default(), a, timing(60)).unwrap();
        assert_eq!((b.0 - a.0) / 3_600_000, 25);
    }

    #[test]
    fn cron_follows_daylight_saving() {
        let zoned = |expr: &str| Schedule::Cron {
            expr: expr.into(),
            zone: Some("Europe/Berlin".into()),
        };
        let go = |expr: &str, now: &str| walls(&preview(&zoned(expr), &LocalZone::default(), ts(now), 60).unwrap(), "Europe/Berlin");
        // A time inside the spring gap runs once, at the first instant after it.
        let spring = go("30 2 * * *", "2026-03-27T12:00:00Z");
        assert_eq!(&spring[..3], ["2026-03-28 02:30", "2026-03-29 03:00", "2026-03-30 02:30"]);
        // A repeated time runs once.
        let fall = go("30 2 * * *", "2026-10-24T12:00:00Z");
        assert_eq!(&fall[..3], ["2026-10-25 02:30", "2026-10-26 02:30", "2026-10-27 02:30"]);
        let utc: Vec<_> = preview(&zoned("30 2 * * *"), &LocalZone::default(), ts("2026-10-24T12:00:00Z"), 60)
            .unwrap()
            .next_runs;
        assert_eq!(utc[0], ts("2026-10-25T00:30:00Z"));
        assert_eq!(utc[1], ts("2026-10-26T01:30:00Z"));
    }

    #[test]
    fn cron_parses_five_or_six_fields_and_says_what_is_wrong() {
        let local = berlin();
        let now = ts("2026-10-22T08:00:00Z");
        let p = preview(&cron("0 9 * * 1-5"), &local, now, 60).unwrap();
        assert_eq!(walls(&p, "Europe/Berlin")[0], "2026-10-23 09:00");
        assert_eq!(walls(&p, "Europe/Berlin")[2], "2026-10-27 09:00");
        let with_seconds = preview(&cron("30 0 9 * * *"), &local, now, 60).unwrap();
        assert_eq!(with_seconds.next_runs[0], ts("2026-10-23T07:00:30Z"));
        assert!(preview(&cron("@daily"), &local, now, 60).is_ok());
        let err = |s: &Schedule| s.check().unwrap_err().join(" ");
        assert!(err(&cron("")).contains("Enter a cron expression"));
        assert!(err(&cron("   ")).contains("Enter a cron expression"));
        assert!(err(&cron("* * *")).contains("five fields") && err(&cron("* * *")).contains("has 3"));
        assert!(err(&cron("* * * * * * *")).contains("has 7"));
        assert!(
            err(&cron("61 * * * *")).contains("not a valid cron expression"),
            "{}",
            err(&cron("61 * * * *"))
        );
        assert!(err(&cron("a b c d e")).contains("not a valid cron expression"));
        assert!(err(&cron(&"*/5 ".repeat(100))).contains("too long"));
        assert!(err(&cron("0 0 30 2 *")).contains("never matches"));
        let bad_zone = Schedule::Cron {
            expr: "0 9 * * *".into(),
            zone: Some("Mars/Base".into()),
        };
        assert!(err(&bad_zone).contains("Unknown time zone \"Mars/Base\""));
        let both = Schedule::Cron {
            expr: "x".into(),
            zone: Some("Mars/Base".into()),
        };
        assert_eq!(both.check().unwrap_err().len(), 2, "every problem is listed");
        assert!(cron("0 9 * * *").check().is_ok());
    }

    #[test]
    fn at_times_say_what_is_missing() {
        let err = |s: &Schedule| s.check().unwrap_err();
        assert_eq!(err(&at_times(&[], &[t(9, 0)], None)), ["Pick at least one day."]);
        assert_eq!(err(&at_times(&Weekday::ALL, &[], None)), ["Add at least one time of day."]);
        assert_eq!(err(&at_times(&[], &[], None)).len(), 2);
        let many: Vec<ClockTime> = (0..25).map(|m| t(1, m)).collect();
        assert!(err(&at_times(&Weekday::ALL, &many, None))[0].contains("at most 24"));
        for bad in ["", "Berlin", "../etc", &"a/".repeat(40)] {
            let zone = if bad.is_empty() { Some("   ") } else { Some(bad) };
            let r = at_times(&Weekday::ALL, &[t(9, 0)], zone).check();
            // An empty zone means "the machine's"; anything else must be a real IANA name.
            assert_eq!(r.is_err(), !bad.is_empty(), "{bad:?}");
        }
        assert!(Weekday::ALL.iter().all(|d| d.short().len() == 3));
    }

    #[test]
    fn too_frequent_schedules_are_clamped_with_a_warning() {
        let now = ts("2026-10-22T08:00:00Z");
        // Every minute against a five-minute minimum.
        let p = preview(&cron("* * * * *"), &berlin(), now, 300).unwrap();
        assert!(
            p.warnings[0].contains("every minute") && p.warnings[0].contains("clamped to at most once every 5 minutes"),
            "{:?}",
            p.warnings
        );
        let gaps: Vec<i64> = p.next_runs.windows(2).map(|w| (w[1].0 - w[0].0) / 1000).collect();
        assert_eq!(gaps, [300, 300, 300, 300]);
        // Every second (six fields).
        let p = preview(&cron("* * * * * *"), &berlin(), now, 60).unwrap();
        assert!(p.warnings[0].contains("every second"), "{:?}", p.warnings);
        assert!(p.next_runs.windows(2).all(|w| w[1].0 - w[0].0 >= 60_000));
        // Two times of day closer than the minimum.
        let close = at_times(&Weekday::ALL, &[t(9, 0), t(9, 2)], Some("UTC"));
        let p = preview(&close, &berlin(), now, 600).unwrap();
        assert_eq!(p.warnings.len(), 1);
        assert_eq!(wall_clock(p.next_runs[0], &zone_named("UTC").unwrap()), "2026-10-22 09:00");
        assert_eq!(
            wall_clock(p.next_runs[1], &zone_named("UTC").unwrap()),
            "2026-10-23 09:00",
            "09:02 is skipped, not run late"
        );
        // And in the real schedule: the next check is at least the minimum after the last one.
        let after = ts("2026-10-22T08:00:10Z");
        let next = next_check(&cron("* * * * *"), &berlin(), after, timing(300)).unwrap();
        assert_eq!(next, ts("2026-10-22T08:06:00Z"));
        assert!(next.0 - after.0 >= 300_000);
        assert!(next.0 - after.0 < 360_000, "the first run at or after the minimum");
        // A schedule that is fine has no warning.
        assert!(preview(&cron("0 * * * *"), &berlin(), now, 300).unwrap().warnings.is_empty());
    }

    #[test]
    fn jitter_is_deterministic_and_small() {
        let s = at_times(&Weekday::ALL, &[t(9, 0)], Some("UTC"));
        let now = ts("2026-10-22T08:00:00Z");
        let at = |jitter| next_check(&s, &berlin(), now, Timing { jitter, ..timing(60) }).unwrap();
        assert_eq!(at(0.0), ts("2026-10-22T09:00:00Z"));
        assert_eq!(at(0.5), ts("2026-10-22T09:00:15Z"));
        assert_eq!(at(0.5), at(0.5), "same seed, same answer");
        assert_eq!(at(1.0), ts("2026-10-22T09:00:30Z"));
        assert_eq!(at(9.0), at(1.0), "out-of-range jitter is clamped");
        assert_eq!(at(f64::NAN), at(0.5));
        // Two watches on the same host get different times when their random numbers differ.
        assert_ne!(at(0.1), at(0.9));
    }

    #[test]
    fn a_machine_that_slept_gets_one_catch_up_check() {
        let s = at_times(&Weekday::ALL, &[t(9, 0), t(10, 0), t(11, 0)], Some("UTC"));
        // The last check finished at 08:00; the laptop slept until 12:30, missing three runs.
        let woke = ts("2026-10-22T12:30:00Z");
        let next = next_check(&s, &berlin(), woke, timing(60)).unwrap();
        assert_eq!(
            next,
            ts("2026-10-23T09:00:00Z"),
            "the next check lies after the wake time: no burst of three"
        );
        let every = next_check(&Schedule::every(900), &berlin(), woke, Timing { jitter: 0.5, ..timing(60) }).unwrap();
        assert_eq!(every, woke.plus_secs(900));
        let c = next_check(&cron("*/15 * * * *"), &berlin(), woke, timing(60)).unwrap();
        assert_eq!(c, ts("2026-10-22T12:45:00Z"));
    }

    #[test]
    fn schedules_use_a_tagged_wire_format() {
        let json = serde_json::to_string(&at_times(&[Weekday::Mon, Weekday::Fri], &[t(9, 5)], None)).unwrap();
        assert_eq!(json, r#"{"mode":"times","days":["mon","fri"],"times":["09:05"],"zone":null}"#);
        let back: Schedule = serde_json::from_str(r#"{"mode":"cron","expr":"0 9 * * *"}"#).unwrap();
        assert_eq!(back, cron("0 9 * * *"));
        assert_eq!(
            serde_json::to_string(&Schedule::every(60)).unwrap(),
            r#"{"mode":"every","secs":60}"#
        );
        assert!(serde_json::from_str::<Schedule>(r#"{"mode":"hourly"}"#).is_err());
        assert_eq!(Schedule::default(), Schedule::every(300));
    }

    #[test]
    fn calendar_helpers_cover_their_edges() {
        assert!(calendar::parse_cron("0 9 * * *").is_ok());
        assert!(calendar::zone_named("UTC").is_ok());
        assert!(calendar::zone_named(" Europe/Berlin ").is_ok());
        let far = LocalZone {
            name: None,
            offset_minutes: 100_000,
        };
        assert!(
            calendar::resolve_zone(None, &far).is_ok(),
            "an absurd offset is clamped, not an error"
        );
        assert!(calendar::next_at_times(&[], &[t(9, 0)], &zone_named("UTC").unwrap(), Timestamp(0)).is_none());
        assert!(calendar::next_at_times(&Weekday::ALL, &[t(9, 0)], &zone_named("UTC").unwrap(), Timestamp(i64::MAX)).is_none());
        assert_eq!(wall_clock(Timestamp(i64::MAX), &zone_named("UTC").unwrap()), "");
    }
}
