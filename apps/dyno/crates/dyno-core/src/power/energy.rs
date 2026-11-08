//! The energy report (ADR 0032 section 6, D10), pure. Dyno logs its own power events; the time they do not cover is
//! **Unknown (Dyno was not running)**, never guessed. Energy is the person's own watts times the time (labelled
//! "Estimated from your numbers"); nothing is claimed as measured until a whole-system reading exists, and CO2 and cost show
//! only when the person gave a factor and a tariff. No savings claims.

use crate::helper_counters::EnergyDomain;
use crate::read_via::ReadVia;
use crate::time::{day_start, next_day_start, LocalZone, Timestamp, DAY, MINUTE};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How often a running agent notes that it is alive.
pub const ALIVE_EVERY: i64 = 5 * MINUTE;
/// How long after its last sign of life the computer still counts as on (two missed notes).
pub const ALIVE_GRACE: i64 = 2 * ALIVE_EVERY;
/// The longest report, in days.
pub const MAX_DAYS: u32 = 366;

/// What Dyno noted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PowerLogKind {
    /// The agent started (the computer was on).
    AgentStarted,
    /// The agent stopped cleanly.
    AgentStopped,
    /// The computer went to sleep.
    Slept,
    /// The computer woke.
    Woke,
    /// The agent was running at this moment.
    Alive,
}

/// One note, in Dyno's database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerLogEntry {
    /// When.
    pub at: Timestamp,
    /// What.
    pub kind: PowerLogKind,
}

/// What the computer was doing during a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanState {
    /// On.
    On,
    /// Asleep.
    Sleep,
    /// Dyno was not running, so it does not know.
    Unknown,
}

/// A stretch of time in one state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Start (inclusive).
    pub from: Timestamp,
    /// End (exclusive).
    pub to: Timestamp,
    /// The state.
    pub state: SpanState,
}

fn after(kind: PowerLogKind) -> SpanState {
    match kind {
        PowerLogKind::AgentStarted | PowerLogKind::Woke | PowerLogKind::Alive => SpanState::On,
        PowerLogKind::Slept => SpanState::Sleep,
        PowerLogKind::AgentStopped => SpanState::Unknown,
    }
}

/// The spans from `from` to `min(to, now)`. `entries` are in time order and start early enough to say what the state was at
/// `from` (the caller reads a week before it).
pub fn timeline(entries: &[PowerLogEntry], from: Timestamp, to: Timestamp, now: Timestamp) -> Vec<Span> {
    let end = to.min(now);
    let mut spans: Vec<Span> = Vec::new();
    let mut push = |a: Timestamp, b: Timestamp, state: SpanState| {
        let (a, b) = (a.max(from), b.min(end));
        if a >= b {
            return;
        }
        match spans.last_mut() {
            Some(last) if last.state == state && last.to == a => last.to = b,
            _ => spans.push(Span { from: a, to: b, state }),
        }
    };
    let mut prev: Option<PowerLogEntry> = None;
    // The next event (or "now", which counts as a sign of life) closes the span that the previous one opened.
    let closers = entries.iter().map(|e| Some(*e)).chain(std::iter::once(None));
    for next in closers {
        let (at, kind) = next.map_or((end.max(now), PowerLogKind::Alive), |e| (e.at, e.kind));
        match prev {
            None => push(from.min(at), at, SpanState::Unknown),
            Some(p) => match after(p.kind) {
                SpanState::On => {
                    let gap = at.since(p.at);
                    let explicit_end = matches!(kind, PowerLogKind::Slept | PowerLogKind::AgentStopped);
                    if explicit_end || gap <= ALIVE_GRACE {
                        push(p.at, at, SpanState::On);
                    } else {
                        let on_until = p.at.plus(ALIVE_GRACE);
                        push(p.at, on_until, SpanState::On);
                        push(on_until, at, SpanState::Unknown);
                    }
                }
                SpanState::Sleep => {
                    // A start without a wake means the computer was shut down while asleep.
                    let state = if kind == PowerLogKind::AgentStarted {
                        SpanState::Unknown
                    } else {
                        SpanState::Sleep
                    };
                    push(p.at, at, state);
                }
                SpanState::Unknown => push(p.at, at, SpanState::Unknown),
            },
        }
        if let Some(e) = next {
            prev = Some(e);
        }
    }
    spans
}

/// The settings of the energy report; unset numbers mean "not shown".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EnergySettings {
    /// Log the power events and show the report.
    pub enabled: bool,
    /// Watts this computer draws when on.
    pub watts_on: Option<f32>,
    /// Watts it draws asleep.
    pub watts_sleep: Option<f32>,
    /// Price of one kWh.
    pub tariff_per_kwh: Option<f32>,
    /// The currency's code or symbol, shown beside the cost.
    pub currency: Option<String>,
    /// Grams of CO2 per kWh.
    pub co2_g_per_kwh: Option<f32>,
}

impl Default for EnergySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            watts_on: None,
            watts_sleep: None,
            tariff_per_kwh: None,
            currency: None,
            co2_g_per_kwh: None,
        }
    }
}

impl EnergySettings {
    /// Every number in range (a number out of range is dropped, never clamped into a wrong figure).
    pub fn normalised(mut self) -> Self {
        let ok = |v: Option<f32>, max: f32| v.filter(|x| x.is_finite() && (0.0..=max).contains(x));
        self.watts_on = ok(self.watts_on, 10_000.0);
        self.watts_sleep = ok(self.watts_sleep, 10_000.0);
        self.tariff_per_kwh = ok(self.tariff_per_kwh, 1_000.0);
        self.co2_g_per_kwh = ok(self.co2_g_per_kwh, 2_000.0);
        self.currency = self
            .currency
            .map(|c| c.trim().chars().filter(|c| !c.is_control()).take(8).collect::<String>())
            .filter(|c| !c.is_empty());
        self
    }
}

/// Where the energy figures come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnergySource {
    /// The person's watts times the time: "Estimated from your numbers".
    Estimated,
    /// No watts were given, so there is no energy figure.
    NotSet,
}

/// One day (or the whole range) of the report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnergyDay {
    /// The local day's start (the range's start for the totals).
    pub day_start: Timestamp,
    /// Seconds on.
    pub on_secs: u64,
    /// Seconds asleep.
    pub sleep_secs: u64,
    /// Seconds Dyno was not running.
    pub unknown_secs: u64,
    /// Energy in kWh, when watts were given.
    pub kwh: Option<f64>,
    /// Cost, when a tariff was given too.
    pub cost: Option<f64>,
    /// Kilograms of CO2, when a factor was given.
    pub co2_kg: Option<f64>,
    /// CPU energy the helper measured on this day, in kWh. It is the processor's alone (not the machine's), so it sits beside the
    /// estimate and never replaces it; `None` when nothing was measured.
    #[serde(default)]
    pub cpu_kwh: Option<f64>,
}

/// The report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnergyReport {
    /// Whether the log is on.
    pub enabled: bool,
    /// Oldest day first.
    pub days: Vec<EnergyDay>,
    /// The sum of the days.
    pub totals: EnergyDay,
    /// Where the energy comes from.
    pub source: EnergySource,
    /// Whether sleep time was priced (the person gave sleep watts); otherwise it counts as 0 W and the page says so.
    pub sleep_energy_counted: bool,
    /// The currency to show beside the cost.
    pub currency: Option<String>,
    /// The first note Dyno has, when there is one.
    pub tracking_since: Option<Timestamp>,
    /// Who measured the CPU energy: the helper, or why nobody could (`Local` where this computer is not asked, so nothing shows).
    #[serde(default)]
    pub cpu_via: ReadVia,
}

fn figures(on: u64, sleep: u64, s: &EnergySettings) -> (Option<f64>, Option<f64>, Option<f64>) {
    let Some(w_on) = s.watts_on else { return (None, None, None) };
    let kwh = (on as f64 * f64::from(w_on) + sleep as f64 * f64::from(s.watts_sleep.unwrap_or(0.0))) / 3_600_000.0;
    (
        Some(kwh),
        s.tariff_per_kwh.map(|t| kwh * f64::from(t)),
        s.co2_g_per_kwh.map(|g| kwh * f64::from(g) / 1000.0),
    )
}

/// The report for the last `days` local days up to `now` (the last one is partial).
pub fn report(entries: &[PowerLogEntry], days: u32, now: Timestamp, zone: &dyn LocalZone, s: &EnergySettings) -> EnergyReport {
    let days = days.clamp(1, MAX_DAYS);
    let mut first = day_start(now, zone);
    for _ in 1..days {
        first = day_start(first.minus(1), zone);
    }
    let spans = timeline(entries, first, next_day_start(now, zone), now);
    let mut out: Vec<EnergyDay> = Vec::new();
    let mut start = first;
    while start <= now {
        let end = next_day_start(start, zone);
        let (mut on, mut sleep, mut unknown) = (0u64, 0u64, 0u64);
        for sp in &spans {
            let (a, b) = (sp.from.max(start), sp.to.min(end));
            if a < b {
                let secs = u64::try_from(b.since(a) / 1000).unwrap_or(0);
                match sp.state {
                    SpanState::On => on += secs,
                    SpanState::Sleep => sleep += secs,
                    SpanState::Unknown => unknown += secs,
                }
            }
        }
        let (kwh, cost, co2_kg) = figures(on, sleep, s);
        out.push(EnergyDay {
            day_start: start,
            on_secs: on,
            sleep_secs: sleep,
            unknown_secs: unknown,
            kwh,
            cost,
            co2_kg,
            cpu_kwh: None,
        });
        start = end;
    }
    let (on, sleep, unknown) = out
        .iter()
        .fold((0, 0, 0), |a, d| (a.0 + d.on_secs, a.1 + d.sleep_secs, a.2 + d.unknown_secs));
    let (kwh, cost, co2_kg) = figures(on, sleep, s);
    let totals = EnergyDay {
        day_start: first,
        on_secs: on,
        sleep_secs: sleep,
        unknown_secs: unknown,
        kwh,
        cost,
        co2_kg,
        cpu_kwh: None,
    };
    EnergyReport {
        enabled: s.enabled,
        days: out,
        totals,
        source: if s.watts_on.is_some() {
            EnergySource::Estimated
        } else {
            EnergySource::NotSet
        },
        sleep_energy_counted: s.watts_sleep.is_some(),
        currency: s.currency.clone(),
        tracking_since: entries.first().map(|e| e.at),
        cpu_via: ReadVia::Local,
    }
}

/// The longest gap between two readings that still counts as one stretch of time. Time the helper was not asked is not guessed.
pub const CPU_MAX_GAP_MS: i64 = 3 * MINUTE;
/// The most days the meter keeps.
pub const CPU_MAX_DAYS: usize = MAX_DAYS as usize + 8;

/// The domains that are the processor package: macOS `CPU Power`, Linux RAPL `package-N`. Others (GPU, the neural engine, a
/// sub-domain that the package already includes) are left out so nothing counts twice.
pub fn cpu_package_domains(all: &[EnergyDomain]) -> Vec<EnergyDomain> {
    all.iter()
        .filter(|d| {
            let label = d.label.to_ascii_lowercase();
            label == "cpu power" || label.starts_with("package")
        })
        .cloned()
        .collect()
}

/// Joules between two readings of one domain, `dt_ms` apart; `None` when they cannot be compared.
fn joules_between(prev: &EnergyDomain, cur: &EnergyDomain, dt_ms: i64) -> Option<f64> {
    if let (Some(a), Some(b)) = (prev.microjoules, cur.microjoules) {
        let micro = if b >= a {
            b - a
        } else {
            // The counter wrapped; with no wrap point it was reset, and nothing is claimed.
            cur.wraps_at_microjoules.filter(|w| *w >= a)? - a + b
        };
        return Some(micro as f64 / 1_000_000.0);
    }
    let now = f64::from(cur.milliwatts?);
    let watts = prev.milliwatts.map_or(now, |p| (f64::from(p) + now) / 2.0) / 1000.0;
    Some(watts * dt_ms as f64 / 1000.0)
}

/// Adds up the CPU's measured energy per local day from the helper's readings (pure; the caller keeps the clock and the file).
///
/// A cumulative counter (microjoules, with its wrap point) contributes the difference between two readings; an instantaneous
/// power (milliwatts) contributes the mean of the two readings times the time between them. A gap over [`CPU_MAX_GAP_MS`], a
/// first reading and a counter that went backwards without a wrap point contribute nothing. A reading counts on the local day it
/// was taken (at most one reading interval of error at midnight).
#[derive(Debug, Clone, Default)]
pub struct CpuEnergyMeter {
    days: BTreeMap<i64, f64>,
    prev: Option<(Timestamp, Vec<EnergyDomain>)>,
}

impl CpuEnergyMeter {
    /// A meter that continues from `days` (joules by local day start, in milliseconds).
    pub fn new(days: BTreeMap<i64, f64>) -> CpuEnergyMeter {
        CpuEnergyMeter { days, prev: None }
    }

    /// The joules by local day start.
    pub fn days(&self) -> &BTreeMap<i64, f64> {
        &self.days
    }

    /// Takes the reading at `at`.
    pub fn feed(&mut self, at: Timestamp, domains: &[EnergyDomain], zone: &dyn LocalZone) {
        if let Some((then, before)) = self.prev.take() {
            let dt = at.since(then);
            if dt > 0 && dt <= CPU_MAX_GAP_MS {
                let joules: f64 = domains
                    .iter()
                    .filter_map(|cur| {
                        let prev = before.iter().find(|p| p.label == cur.label)?;
                        joules_between(prev, cur, dt)
                    })
                    .sum();
                if joules.is_finite() && joules > 0.0 {
                    *self.days.entry(day_start(at, zone).0).or_insert(0.0) += joules;
                }
            }
        }
        while self.days.len() > CPU_MAX_DAYS {
            self.days.pop_first();
        }
        self.prev = Some((at, domains.to_vec()));
    }

    /// A reading failed: the time until the next one is not counted.
    pub fn lose_thread(&mut self) {
        self.prev = None;
    }
}

impl EnergyReport {
    /// Puts the measured CPU energy (`joules` by local day start) beside the estimate, with who measured it.
    pub fn with_cpu_energy(mut self, joules: &BTreeMap<i64, f64>, via: ReadVia) -> EnergyReport {
        let kwh = |j: f64| j / 3_600_000.0;
        let mut total: Option<f64> = None;
        for d in &mut self.days {
            d.cpu_kwh = joules.get(&d.day_start.0).copied().map(kwh);
            if let Some(k) = d.cpu_kwh {
                *total.get_or_insert(0.0) += k;
            }
        }
        self.totals.cpu_kwh = total;
        self.cpu_via = via;
        self
    }
}

/// Seconds in a day, for tests and the UI's labels.
pub const DAY_SECS: u64 = (DAY / 1000) as u64;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{FixedZone, HOUR, SECOND};
    use PowerLogKind::*;

    const T0: Timestamp = Timestamp(1_700_000_000_000 - 1_700_000_000_000 % DAY);

    fn e(at: i64, kind: PowerLogKind) -> PowerLogEntry {
        PowerLogEntry { at: T0.plus(at), kind }
    }
    fn st(spans: &[Span]) -> Vec<(i64, i64, SpanState)> {
        spans.iter().map(|s| (s.from.since(T0), s.to.since(T0), s.state)).collect()
    }
    use SpanState::*;

    #[test]
    fn before_the_first_note_is_unknown_and_the_agent_running_is_on() {
        let spans = timeline(
            &[e(HOUR, AgentStarted), e(HOUR + 5 * MINUTE, Alive)],
            T0,
            T0.plus(2 * HOUR),
            T0.plus(HOUR + 8 * MINUTE),
        );
        assert_eq!(st(&spans), [(0, HOUR, Unknown), (HOUR, HOUR + 8 * MINUTE, On)]);
    }

    #[test]
    fn sleep_is_sleep_until_the_wake_and_a_start_after_sleep_means_it_was_shut_down() {
        let v = [
            e(0, AgentStarted),
            e(HOUR, Slept),
            e(3 * HOUR, Woke),
            e(3 * HOUR + MINUTE, Slept),
            e(5 * HOUR, AgentStarted),
        ];
        let spans = timeline(&v, T0, T0.plus(6 * HOUR), T0.plus(5 * HOUR));
        assert_eq!(
            st(&spans),
            [
                (0, HOUR, On),
                (HOUR, 3 * HOUR, Sleep),
                (3 * HOUR, 3 * HOUR + MINUTE, On),
                (3 * HOUR + MINUTE, 5 * HOUR, Unknown)
            ]
        );
    }

    #[test]
    fn a_clean_stop_ends_on_time_and_the_gap_is_unknown() {
        let v = [e(0, AgentStarted), e(HOUR, AgentStopped), e(2 * HOUR, AgentStarted)];
        let spans = timeline(&v, T0, T0.plus(3 * HOUR), T0.plus(2 * HOUR + MINUTE));
        assert_eq!(
            st(&spans),
            [(0, HOUR, On), (HOUR, 2 * HOUR, Unknown), (2 * HOUR, 2 * HOUR + MINUTE, On)]
        );
    }

    #[test]
    fn a_crash_counts_as_on_only_for_the_grace_after_the_last_sign_of_life() {
        let v = [e(0, AgentStarted), e(5 * MINUTE, Alive), e(2 * HOUR, AgentStarted)];
        let spans = timeline(&v, T0, T0.plus(3 * HOUR), T0.plus(2 * HOUR));
        assert_eq!(
            st(&spans),
            [(0, 5 * MINUTE + ALIVE_GRACE, On), (5 * MINUTE + ALIVE_GRACE, 2 * HOUR, Unknown)]
        );
        // The same for "now": the agent died long ago, then the clock moved on.
        let spans = timeline(&v[..2], T0, T0.plus(3 * HOUR), T0.plus(HOUR));
        assert_eq!(
            st(&spans),
            [(0, 5 * MINUTE + ALIVE_GRACE, On), (5 * MINUTE + ALIVE_GRACE, HOUR, Unknown)]
        );
    }

    #[test]
    fn only_the_asked_range_is_returned_and_no_entries_is_all_unknown() {
        let v = [e(0, AgentStarted), e(HOUR, Slept), e(3 * HOUR, Woke)];
        let spans = timeline(&v, T0.plus(2 * HOUR), T0.plus(4 * HOUR), T0.plus(4 * HOUR));
        assert_eq!(
            st(&spans),
            [
                (2 * HOUR, 3 * HOUR, Sleep),
                (3 * HOUR, 3 * HOUR + ALIVE_GRACE, On),
                (3 * HOUR + ALIVE_GRACE, 4 * HOUR, Unknown)
            ]
        );
        assert_eq!(st(&timeline(&[], T0, T0.plus(HOUR), T0.plus(HOUR))), [(0, HOUR, Unknown)]);
        assert!(timeline(&[], T0, T0, T0).is_empty());
    }

    #[test]
    fn the_report_splits_days_by_the_local_zone_and_prices_only_what_the_person_gave() {
        let zone = FixedZone(0);
        let now = T0.plus(DAY + 12 * HOUR);
        let v = [
            e(DAY - 2 * HOUR, AgentStarted),
            e(DAY + HOUR, Slept),
            e(DAY + 5 * HOUR, Woke),
            e(DAY + 5 * HOUR + 3 * MINUTE, Alive),
            e(DAY + 12 * HOUR, Alive),
        ];
        let none = report(&v, 2, now, &zone, &EnergySettings::default());
        assert_eq!(none.source, EnergySource::NotSet);
        assert_eq!(none.days.len(), 2);
        assert_eq!(none.days[0].day_start, T0);
        assert_eq!((none.days[0].on_secs, none.days[0].unknown_secs), (2 * 3600, 22 * 3600));
        assert_eq!(none.days[1].sleep_secs, 4 * 3600);
        assert!(none.days.iter().all(|d| d.kwh.is_none() && d.cost.is_none() && d.co2_kg.is_none()));
        assert_eq!(none.tracking_since, Some(T0.plus(DAY - 2 * HOUR)));

        let s = EnergySettings {
            watts_on: Some(100.0),
            watts_sleep: Some(2.0),
            tariff_per_kwh: Some(0.5),
            co2_g_per_kwh: Some(400.0),
            currency: Some("S$".into()),
            ..Default::default()
        };
        let r = report(&v, 1, now, &zone, &s);
        assert_eq!(r.days.len(), 1);
        let d = &r.days[0];
        let expect_kwh = (d.on_secs as f64 * 100.0 + d.sleep_secs as f64 * 2.0) / 3_600_000.0;
        assert!((d.kwh.unwrap() - expect_kwh).abs() < 1e-9);
        assert!((d.cost.unwrap() - expect_kwh * 0.5).abs() < 1e-9);
        assert!((d.co2_kg.unwrap() - expect_kwh * 0.4).abs() < 1e-9);
        assert!(r.sleep_energy_counted && r.source == EnergySource::Estimated);
        assert_eq!(r.currency.as_deref(), Some("S$"));
        // No sleep watts: asleep counts as 0 W and the report says so.
        let r = report(
            &v,
            1,
            now,
            &zone,
            &EnergySettings {
                watts_on: Some(100.0),
                ..Default::default()
            },
        );
        assert!(!r.sleep_energy_counted);
        assert!((r.totals.kwh.unwrap() - r.totals.on_secs as f64 * 100.0 / 3_600_000.0).abs() < 1e-9);
        assert_eq!(r.totals.cost, None);
    }

    #[test]
    fn totals_add_the_days_and_the_range_is_capped() {
        let zone = FixedZone(480);
        let now = T0.plus(10 * DAY);
        let r = report(&[e(0, AgentStarted)], 1000, now, &zone, &EnergySettings::default());
        assert_eq!(r.days.len(), MAX_DAYS as usize);
        assert_eq!(
            r.totals.on_secs + r.totals.sleep_secs + r.totals.unknown_secs,
            r.days.iter().map(|d| d.on_secs + d.sleep_secs + d.unknown_secs).sum::<u64>()
        );
        assert_eq!(
            report(&[], 0, now, &zone, &EnergySettings::default()).days.len(),
            1,
            "at least today"
        );
        assert_eq!(DAY_SECS, 86_400);
        let _ = SECOND;
    }

    #[test]
    fn settings_drop_numbers_out_of_range_and_tidy_the_currency() {
        let s = EnergySettings {
            watts_on: Some(-1.0),
            watts_sleep: Some(f32::NAN),
            tariff_per_kwh: Some(5000.0),
            co2_g_per_kwh: Some(400.0),
            currency: Some("  \u{7}  ".into()),
            ..Default::default()
        }
        .normalised();
        assert_eq!(
            (s.watts_on, s.watts_sleep, s.tariff_per_kwh, s.co2_g_per_kwh, s.currency),
            (None, None, None, Some(400.0), None)
        );
        let long = EnergySettings {
            currency: Some("ABCDEFGHIJKL".into()),
            watts_on: Some(65.0),
            ..Default::default()
        }
        .normalised();
        assert_eq!(long.currency.as_deref(), Some("ABCDEFGH"));
        assert_eq!(long.watts_on, Some(65.0));
        assert!(EnergySettings::default().enabled);
        let j: EnergySettings = serde_json::from_str(r#"{"wattsOn":50}"#).unwrap();
        assert_eq!((j.watts_on, j.enabled), (Some(50.0), true));
    }

    // ---- measured CPU energy (helper wiring, WP-CNT) ----

    fn mw(label: &str, milliwatts: u32) -> EnergyDomain {
        EnergyDomain {
            label: label.into(),
            microjoules: None,
            wraps_at_microjoules: None,
            milliwatts: Some(milliwatts),
        }
    }
    fn uj(label: &str, microjoules: u64, wraps: Option<u64>) -> EnergyDomain {
        EnergyDomain {
            label: label.into(),
            microjoules: Some(microjoules),
            wraps_at_microjoules: wraps,
            milliwatts: None,
        }
    }
    const UTC: crate::time::FixedZone = crate::time::FixedZone(0);

    #[test]
    fn only_the_cpu_package_counts_so_nothing_is_counted_twice() {
        let all = [
            mw("CPU Power", 1),
            mw("GPU Power", 2),
            mw("ANE Power", 3),
            mw("Combined Power (CPU + GPU + ANE)", 6),
            uj("package-0", 5, None),
            uj("core", 4, None),
        ];
        let labels: Vec<String> = cpu_package_domains(&all).into_iter().map(|d| d.label).collect();
        assert_eq!(labels, ["CPU Power", "package-0"]);
    }

    #[test]
    fn measured_power_is_the_mean_of_two_readings_times_the_time_between_them() {
        let mut m = CpuEnergyMeter::default();
        let t = T0.plus(HOUR);
        m.feed(t, &[mw("CPU Power", 2_000)], &UTC);
        assert!(m.days().is_empty(), "a first reading has no time behind it");
        m.feed(t.plus(30 * SECOND), &[mw("CPU Power", 4_000)], &UTC);
        // (2 W + 4 W) / 2 * 30 s = 90 J.
        assert_eq!(m.days().get(&T0.0).copied(), Some(90.0));
    }

    #[test]
    fn a_counter_adds_its_difference_and_survives_a_wrap_but_not_a_reset() {
        let mut m = CpuEnergyMeter::default();
        let t = T0.plus(HOUR);
        m.feed(t, &[uj("package-0", 900_000_000, Some(1_000_000_000))], &UTC);
        m.feed(t.plus(10 * SECOND), &[uj("package-0", 50_000_000, Some(1_000_000_000))], &UTC);
        // 100 J to the wrap point and 50 J after it.
        assert_eq!(m.days().get(&T0.0).copied(), Some(150.0));
        m.feed(t.plus(20 * SECOND), &[uj("package-0", 10, None)], &UTC);
        m.feed(t.plus(30 * SECOND), &[uj("package-0", 5, None)], &UTC);
        assert_eq!(
            m.days().get(&T0.0).copied(),
            Some(150.0),
            "a counter that went back with no wrap point adds nothing"
        );
    }

    #[test]
    fn time_the_helper_was_not_asked_is_not_guessed() {
        let mut m = CpuEnergyMeter::default();
        let t = T0.plus(HOUR);
        m.feed(t, &[mw("CPU Power", 5_000)], &UTC);
        m.feed(t.plus(CPU_MAX_GAP_MS + SECOND), &[mw("CPU Power", 5_000)], &UTC);
        assert!(m.days().is_empty());
        m.feed(t.plus(CPU_MAX_GAP_MS + 10 * SECOND), &[mw("CPU Power", 5_000)], &UTC);
        assert_eq!(m.days().len(), 1);
        m.lose_thread();
        m.feed(t.plus(CPU_MAX_GAP_MS + 20 * SECOND), &[mw("CPU Power", 5_000)], &UTC);
        assert_eq!(m.days().values().copied().sum::<f64>(), 45.0, "a failed read is not bridged");
    }

    #[test]
    fn the_meter_keeps_a_bounded_number_of_days() {
        let days: BTreeMap<i64, f64> = (0..(CPU_MAX_DAYS as i64 + 5)).map(|d| (d * DAY, 1.0)).collect();
        let mut m = CpuEnergyMeter::new(days);
        let t = Timestamp((CPU_MAX_DAYS as i64 + 10) * DAY);
        m.feed(t, &[mw("CPU Power", 1_000)], &UTC);
        m.feed(t.plus(SECOND), &[mw("CPU Power", 1_000)], &UTC);
        assert_eq!(m.days().len(), CPU_MAX_DAYS);
        assert!(
            m.days().contains_key(&t.0.div_euclid(DAY).saturating_mul(DAY)),
            "the newest day stays"
        );
    }

    #[test]
    fn measured_cpu_energy_is_reported_beside_the_estimate() {
        let log = [e(0, AgentStarted), e(HOUR, Alive)];
        let s = EnergySettings {
            watts_on: Some(60.0),
            ..Default::default()
        };
        let now = T0.plus(2 * HOUR);
        let base = report(&log, 1, now, &UTC, &s);
        let estimate = base.totals.kwh;
        // 3.6 MJ is exactly 1 kWh.
        let measured = base
            .clone()
            .with_cpu_energy(&BTreeMap::from([(T0.0, 3_600_000.0)]), ReadVia::Helper);
        assert_eq!(measured.days[0].cpu_kwh, Some(1.0));
        assert_eq!(measured.totals.cpu_kwh, Some(1.0));
        assert_eq!(measured.totals.kwh, estimate, "the estimate stays");
        assert_eq!(measured.cpu_via, ReadVia::Helper);
        let none = base.with_cpu_energy(&BTreeMap::new(), ReadVia::needs_admin("CPU energy needs it."));
        assert_eq!(none.totals.cpu_kwh, None);
        assert!(matches!(none.cpu_via, ReadVia::NeedsHelper { .. }));
    }
}
