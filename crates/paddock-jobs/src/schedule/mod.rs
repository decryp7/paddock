//! Schedules for the whole family (plan X33): when to do something next. A schedule has one of three kinds
//! ([`Schedule`]): every so often, at fixed times of day, or a cron expression, each in the computer's time zone or a
//! chosen IANA one, daylight saving included. The pure maths gives the next run with jitter (so many schedules on one
//! host do not fire together), exponential back-off after failures, a minimum interval per source, the description in
//! words, the preview of the next runs for the editor, and the catch-up decision after a machine slept ([`due`]).
//! [`pause`] holds the same ingredients for pause windows and quiet hours.
//!
//! Time is always passed in as values ([`UnixMs`], from [`crate::Clock::wall`]), never read, so every case is a
//! [`crate::FakeClock`] test. The next run is always computed from the moment the last one finished or the wake time,
//! so a machine that slept through several runs does one catch-up run, not a burst, unless [`CatchUp::All`] asks for more.

mod calendar;
mod moment;
pub mod pause;

pub use calendar::{
    LocalZone, MAX_CRON_LEN, MAX_TIMES, MAX_ZONE_LEN, Weekday, parse_cron, resolve_zone,
    wall_clock, zone_label, zone_named,
};
pub use moment::{ClockTime, UnixMs};

use serde::{Deserialize, Serialize};

/// Shortest interval a schedule may have (10 seconds); shorter ones are clamped.
pub const MIN_INTERVAL_SECS: u32 = 10;
/// Longest interval (7 days).
pub const MAX_INTERVAL_SECS: u32 = 7 * 24 * 3600;
/// Longest wait after repeated failures (6 hours), unless the interval itself is longer.
pub const MAX_BACKOFF_SECS: u32 = 6 * 3600;

/// Shortest interval for a source on the network: a polite default for any site (local work may run as often as
/// [`MIN_INTERVAL_SECS`]).
pub const MIN_SITE_INTERVAL_SECS: u32 = 60;
/// Most seconds of random delay added to a calendar schedule's time, so watches on one host do not fire together.
pub const CALENDAR_JITTER_SECS: u32 = 30;
/// How many upcoming runs the preview lists.
pub const PREVIEW_RUNS: usize = 5;
/// How many upcoming runs are looked at to find out whether a schedule fires more often than the source allows.
const PROBE_RUNS: usize = 64;
/// 2024-01-01 00:00 UTC: where "does this expression ever match" starts looking.
const REFERENCE_MS: i64 = 1_704_067_200_000;

/// When something runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum Schedule {
    /// Every so often, from the last check.
    Every {
        /// Seconds between runs.
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

/// The upcoming runs of a schedule, for the editor (camelCase on the wire).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulePreview {
    /// The schedule in words.
    pub description: String,
    /// The next runs (nominal times, without jitter), at most [`PREVIEW_RUNS`].
    pub next_runs: Vec<UnixMs>,
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
    fn next_after(&self, after: UnixMs) -> Option<UnixMs> {
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
                let zone = calendar::resolve_zone(zone.as_deref(), local)
                    .map_err(|e| problems.push(e))
                    .ok();
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
                let cron = calendar::parse_cron(expr)
                    .map_err(|e| problems.push(e))
                    .ok();
                let zone = calendar::resolve_zone(zone.as_deref(), local)
                    .map_err(|e| problems.push(e))
                    .ok();
                match (cron, zone) {
                    (Some(cron), Some(zone)) if problems.is_empty() => {
                        let plan = Plan::Cron {
                            cron: Box::new(cron),
                            zone,
                        };
                        // An expression that matches no real date (30 February) would never run.
                        if plan.next_after(UnixMs(REFERENCE_MS)).is_none() {
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
                format!(
                    "{} at {}{}",
                    days_text(&days),
                    join_and(&times),
                    zone_suffix(zone.as_deref())
                )
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
        join_and(
            &days
                .iter()
                .map(|d| d.short().to_string())
                .collect::<Vec<_>>(),
        )
    }
}

/// `6 hours`, `day`, `15 minutes`, `90 seconds` (for "Every ...").
pub fn span_text(secs: u32) -> String {
    let unit = |n: u32, one: &str, many: &str| {
        if n == 1 {
            one.to_string()
        } else {
            format!("{n} {many}")
        }
    };
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
pub fn next_check(
    schedule: &Schedule,
    local: &LocalZone,
    now: UnixMs,
    timing: Timing,
) -> Result<UnixMs, Vec<String>> {
    let plan = schedule.plan(local)?;
    let min = timing
        .min_interval_secs
        .clamp(MIN_INTERVAL_SECS, MAX_INTERVAL_SECS);
    if let Plan::Every(secs) = plan {
        return Ok(now.plus_secs(next_delay_secs(
            secs.max(min),
            timing.failures,
            timing.jitter,
        )));
    }
    let wait = if timing.failures == 0 {
        u64::from(min)
    } else {
        next_delay_secs(min, timing.failures, timing.jitter)
    };
    // The run at exactly `now + wait` still counts.
    let after = UnixMs(now.plus_secs(wait).0 - 1);
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
pub fn preview(
    schedule: &Schedule,
    local: &LocalZone,
    now: UnixMs,
    min_interval_secs: u32,
) -> Result<SchedulePreview, Vec<String>> {
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
        next_runs.extend(
            std::iter::from_fn(|| {
                let run = plan.next_after(after)?;
                after = UnixMs(run.0.saturating_add(step_ms) - 1);
                Some(run)
            })
            .take(PREVIEW_RUNS),
        );
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
fn tightest_gap_ms(plan: &Plan, now: UnixMs) -> Option<i64> {
    let runs: Vec<i64> = std::iter::successors(plan.next_after(now), |r| plan.next_after(*r))
        .take(PROBE_RUNS)
        .map(|r| r.0)
        .collect();
    runs.windows(2).map(|w| w[1] - w[0]).min()
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
    let jitter = if jitter.is_finite() {
        jitter.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let factor = 0.9 + 0.2 * jitter;
    ((base as f64) * factor)
        .round()
        .max(f64::from(MIN_INTERVAL_SECS)) as u64
}

/// What to do about runs that fell due while nothing was running (the machine slept, the app was closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "policy", rename_all = "kebab-case")]
pub enum CatchUp {
    /// Forget the missed runs and wait for the next one.
    Skip,
    /// Run once for all the missed ones (the default: no burst after waking).
    #[default]
    Once,
    /// Run every missed one, at most `max` of them (the rest are dropped).
    All {
        /// The most runs to make up.
        max: u32,
    },
}

/// Most missed runs counted by [`due`]; a schedule behind by more is reported as this many.
pub const MAX_MISSED: u32 = 1000;

/// What [`due`] decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Due {
    /// Runs that fell due after the last run, up to `now` (at most [`MAX_MISSED`]).
    pub missed: u32,
    /// Runs to make now under the catch-up policy.
    pub run_now: u32,
    /// The next run after `now`.
    pub next: UnixMs,
}

/// Which runs are due at `now` for a schedule whose last run was at `last_run`, under `catch_up`. An interval schedule
/// counts whole intervals since `last_run`; a calendar schedule counts its nominal times in `(last_run, now]`, so
/// daylight-saving changes neither lose nor repeat a run. The next run is always after `now`.
pub fn due(
    schedule: &Schedule,
    local: &LocalZone,
    last_run: UnixMs,
    now: UnixMs,
    catch_up: CatchUp,
) -> Result<Due, Vec<String>> {
    let plan = schedule.plan(local)?;
    let missed = match &plan {
        Plan::Every(secs) => {
            let elapsed = now.0.saturating_sub(last_run.0).max(0);
            u32::try_from(elapsed / (i64::from(*secs) * 1000))
                .unwrap_or(u32::MAX)
                .min(MAX_MISSED)
        }
        _ => std::iter::successors(plan.next_after(last_run), |r| plan.next_after(*r))
            .take_while(|run| *run <= now)
            .take(MAX_MISSED as usize)
            .count() as u32,
    };
    let next = plan
        .next_after(now)
        .ok_or_else(|| vec!["This schedule has no upcoming run.".to_string()])?;
    let run_now = match catch_up {
        CatchUp::Skip => 0,
        CatchUp::Once => missed.min(1),
        CatchUp::All { max } => missed.min(max),
    };
    Ok(Due {
        missed,
        run_now,
        next,
    })
}
