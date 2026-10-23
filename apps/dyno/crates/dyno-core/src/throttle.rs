//! The throttling detector (WP-23): is the CPU running slower than it could under load, why, and how much does it cost?
//!
//! Pure functions over a short window of [`ThrottleInput`]s (the sampler builds them from each tick plus the few facts
//! the OS reports). [`detect`] judges a window; [`ThrottleTracker`] turns a stream of inputs into episodes with a
//! start, an end, a cause and the share of speed lost. An unavailable input gives "cannot tell", never a guess, and a
//! cause is only named when something measured supports it.

use crate::anomaly::Evidence;
use crate::hardware::{Confidence, OsThrottle};
use crate::metrics::SystemSample;
use crate::time::{Timestamp, MINUTE, SECOND};
use crate::units::duration;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Below this average load the CPU is allowed to run slowly; a low clock then proves nothing.
pub const BUSY_LOAD: f32 = 50.0;
/// A clock this many percent under the expected speed (while busy) counts as throttled.
pub const MIN_LOSS_PERCENT: f32 = 15.0;
/// Temperatures within this many degrees of the sensor's critical value count as hot.
pub const HOT_MARGIN_C: f32 = 8.0;
/// Without a reported critical value, this is hot.
pub const HOT_DEFAULT_C: f32 = 92.0;
/// How long the tracker looks back to judge.
pub const WINDOW_MS: i64 = 10 * SECOND;
/// Throttling must last this long before an episode starts (a one-second dip is not an episode).
pub const SUSTAIN_MS: i64 = 20 * SECOND;
/// It must be gone this long before an episode ends.
pub const CLEAR_MS: i64 = 20 * SECOND;
/// A silence this long (or time running backwards) ends the window and any open episode.
pub const GAP_MS: i64 = 2 * MINUTE;
/// Fewer samples than this make a finding less certain.
pub const MIN_SAMPLES: usize = 5;

/// Why the CPU is slowed down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThrottleCause {
    /// It is too hot.
    Thermal,
    /// It is limited by power: running on battery or an adapter that cannot supply enough.
    Power,
    /// A battery-saving mode caps the speed.
    BatterySaver,
    /// Slower than expected, and nothing measured explains it.
    Unexplained,
}

impl ThrottleCause {
    /// A short label.
    pub fn label(self) -> &'static str {
        match self {
            ThrottleCause::Thermal => "Heat",
            ThrottleCause::Power => "Power limit",
            ThrottleCause::BatterySaver => "Battery saver",
            ThrottleCause::Unexplained => "Unknown cause",
        }
    }
}

/// What the detector concluded about a window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Verdict {
    /// The CPU runs below the speed it could.
    Throttled {
        /// Why.
        cause: ThrottleCause,
        /// How much of the expected speed is lost, 0–100.
        lost_percent: f32,
    },
    /// Busy, and running at the speed it should.
    Fine,
    /// Not busy enough to judge: a slow clock is normal when there is nothing to do.
    NotBusy,
    /// The machine does not report what is needed.
    CannotTell,
}

/// One tick's worth of what the detector needs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThrottleInput {
    /// When.
    pub at: Timestamp,
    /// CPU load, 0–100.
    pub load: f32,
    /// Current clock in MHz (0 when unknown).
    pub frequency_mhz: u32,
    /// The speed the CPU should reach under load, in MHz (0 when unknown).
    pub expected_mhz: u32,
    /// The hottest temperature, °C.
    pub temperature_c: Option<f32>,
    /// That sensor's critical temperature, °C.
    pub critical_c: Option<f32>,
    /// Running on battery, when known.
    pub on_battery: Option<bool>,
    /// A battery-saving mode is on, when known.
    pub power_saver: Option<bool>,
    /// What the OS says about its own limits.
    pub os: OsThrottle,
}

impl ThrottleInput {
    /// The input for one system sample plus the facts that are not part of it.
    pub fn from_sample(s: &SystemSample, power_saver: Option<bool>, os: OsThrottle) -> ThrottleInput {
        let hottest = s.hottest();
        ThrottleInput {
            at: s.at,
            load: s.cpu.total,
            frequency_mhz: s.cpu.frequency_mhz,
            expected_mhz: s.cpu.base_frequency_mhz,
            temperature_c: hottest.map(|t| t.celsius),
            critical_c: hottest.and_then(|t| t.critical),
            on_battery: s.battery.value().map(|b| b.source == crate::metrics::PowerSource::Battery),
            power_saver,
            os,
        }
    }
}

/// The detector's answer for a window, with its reasoning in words.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThrottleReading {
    /// The verdict.
    pub verdict: Verdict,
    /// How sure.
    pub confidence: Confidence,
    /// One line in plain words.
    pub headline: String,
    /// What it means and what could help.
    pub explanation: String,
    /// The numbers behind it.
    pub evidence: Vec<Evidence>,
}

fn mean(values: impl Iterator<Item = f32>) -> Option<f32> {
    let (sum, n) = values.fold((0.0f32, 0u32), |(s, n), v| (s + v, n + 1));
    (n > 0).then(|| sum / n as f32)
}

fn is_hot(t: f32, critical: Option<f32>) -> bool {
    t >= critical.map_or(HOT_DEFAULT_C, |c| c - HOT_MARGIN_C)
}

/// Judges a window of inputs (oldest first).
pub fn detect(window: &[ThrottleInput]) -> ThrottleReading {
    let Some(last) = window.last() else {
        return reading(
            Verdict::CannotTell,
            Confidence::Low,
            "Nothing measured yet",
            "Dyno has not taken a measurement yet.",
            vec![],
        );
    };
    let n = window.len();
    let load = mean(window.iter().map(|i| i.load)).unwrap_or(0.0);
    let busy = load >= BUSY_LOAD;
    let freq = mean(window.iter().filter(|i| i.frequency_mhz > 0).map(|i| i.frequency_mhz as f32));
    let expected = mean(window.iter().filter(|i| i.expected_mhz > 0).map(|i| i.expected_mhz as f32));
    let share = |f: fn(&OsThrottle) -> bool| window.iter().filter(|i| f(&i.os)).count() as f32 / n as f32;
    let os_limited = share(OsThrottle::limited) >= 0.5;
    let os_limit = window.iter().filter_map(|i| i.os.speed_limit_percent).min().filter(|p| *p < 100);
    let hot = window
        .iter()
        .filter(|i| i.temperature_c.is_some_and(|t| is_hot(t, i.critical_c)))
        .count()
        * 2
        >= n;
    let os_thermal = share(|o| o.thermal == Some(true)) >= 0.5;
    let os_power = share(|o| o.power == Some(true)) >= 0.5;
    let battery = window.iter().filter(|i| i.on_battery == Some(true)).count() * 2 >= n;
    let saver = window.iter().filter(|i| i.power_saver == Some(true)).count() * 2 >= n;

    let mut evidence = vec![Evidence::new("Load", format!("{load:.0}%"))];
    if let (Some(f), Some(e)) = (freq, expected) {
        evidence.push(Evidence::new("Clock", format!("{f:.0} MHz of {e:.0} MHz")));
    }
    if let Some(t) = last.temperature_c {
        evidence.push(Evidence::new("Temperature", format!("{t:.0} °C")));
    }
    if let Some(p) = os_limit {
        evidence.push(Evidence::new("Limit set by the system", format!("{p}% of full speed")));
    }

    // How much is lost: the system's own limit when it reports one, else the clock against what it should reach.
    let clock_loss = match (freq, expected) {
        (Some(f), Some(e)) if busy && e > 0.0 => Some(((1.0 - f / e) * 100.0).clamp(0.0, 100.0)),
        _ => None,
    };
    let lost = match (os_limit, clock_loss) {
        (Some(p), _) if os_limited => Some(100.0 - f32::from(p)),
        (_, Some(l)) if l >= MIN_LOSS_PERCENT => Some(l),
        _ => None,
    };

    let Some(lost) = lost.or_else(|| (os_limited && busy).then_some(0.0)) else {
        let can_judge_clock = freq.is_some() && expected.is_some();
        return if !busy {
            reading(
                Verdict::NotBusy,
                Confidence::High,
                "Not busy, nothing to judge",
                "A processor that has little to do slows down on purpose to save power. Dyno judges its speed only while it is busy.",
                evidence,
            )
        } else if can_judge_clock {
            reading(
                Verdict::Fine,
                confidence_for(n, true),
                "Running at full speed",
                "The CPU is busy and its clock is where it should be.",
                evidence,
            )
        } else {
            reading(
                Verdict::CannotTell,
                Confidence::Low,
                "Cannot tell if the CPU is slowed down",
                "This machine does not report the CPU's clock speed or a speed limit, so Dyno cannot compare it with what it should be.",
                evidence,
            )
        };
    };

    let (cause, mut confidence, why) = if os_thermal || (os_limited && hot) {
        (
            ThrottleCause::Thermal,
            Confidence::High,
            "the system reports a heat limit".to_string(),
        )
    } else if os_power {
        (
            ThrottleCause::Power,
            Confidence::High,
            "the system reports a power limit".to_string(),
        )
    } else if hot {
        let t = last.temperature_c.map_or(String::new(), |t| format!(" at {t:.0} °C"));
        (ThrottleCause::Thermal, Confidence::Medium, format!("it is running hot{t}"))
    } else if saver {
        (
            ThrottleCause::BatterySaver,
            Confidence::Medium,
            "a battery-saving mode is on".to_string(),
        )
    } else if battery {
        (ThrottleCause::Power, Confidence::Medium, "it runs on battery".to_string())
    } else {
        (ThrottleCause::Unexplained, Confidence::Low, String::new())
    };
    if os_limited && matches!(cause, ThrottleCause::Unexplained) {
        confidence = Confidence::Medium;
    }
    confidence = confidence.min(confidence_for(n, os_limited || !matches!(cause, ThrottleCause::Unexplained)));

    let cost = format!("about {lost:.0}% of its speed");
    let (headline, explanation) = match cause {
        ThrottleCause::Thermal => (
            format!("Slowed down by heat: {lost:.0}% slower"),
            format!(
                "The CPU is giving up {cost} because {why}. Free the vents, move the machine off soft surfaces, and check for a \
                 fan that has stopped or is clogged with dust."
            ),
        ),
        ThrottleCause::Power => (
            format!("Slowed down by a power limit: {lost:.0}% slower"),
            format!(
                "The CPU is giving up {cost} because {why}. Plug in the original charger, or choose a power mode that favours performance."
            ),
        ),
        ThrottleCause::BatterySaver => (
            format!("Slowed down by battery saver: {lost:.0}% slower"),
            format!("The CPU is giving up {cost} because {why}. Turn the mode off (or plug in) when you need full speed."),
        ),
        ThrottleCause::Unexplained => (
            format!("Running {lost:.0}% slower than it should"),
            format!(
                "The CPU is giving up {cost} while busy, but nothing Dyno can measure explains it (it is not hot, not on battery and \
                 no saving mode is on). {}",
                if last.temperature_c.is_none() {
                    "This machine does not report its temperature, so heat cannot be ruled out."
                } else {
                    "It may be a firmware or vendor power limit."
                }
            ),
        ),
    };
    reading(
        Verdict::Throttled { cause, lost_percent: lost },
        confidence,
        &headline,
        &explanation,
        evidence,
    )
}

fn confidence_for(samples: usize, evidence_is_strong: bool) -> Confidence {
    match (samples >= MIN_SAMPLES, evidence_is_strong) {
        (true, true) => Confidence::High,
        (true, false) | (false, true) => Confidence::Medium,
        (false, false) => Confidence::Low,
    }
}

fn reading(verdict: Verdict, confidence: Confidence, headline: &str, explanation: &str, evidence: Vec<Evidence>) -> ThrottleReading {
    ThrottleReading {
        verdict,
        confidence,
        headline: headline.into(),
        explanation: explanation.into(),
        evidence,
    }
}

/// One stretch of throttling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThrottleEpisode {
    /// Stable across refreshes and restarts: it names the episode by when it began (`throttle-<ms>`), so the UI can key rows on it.
    #[serde(default)]
    pub id: String,
    /// When it began.
    pub start: Timestamp,
    /// When it ended; `None` while it continues.
    pub end: Option<Timestamp>,
    /// The cause most of the time.
    pub cause: ThrottleCause,
    /// Average share of speed lost, 0–100.
    pub avg_lost_percent: f32,
    /// The largest share lost.
    pub peak_lost_percent: f32,
    /// The hottest temperature seen during it, °C.
    pub peak_temperature_c: Option<f32>,
}

impl ThrottleEpisode {
    /// The id of the episode that began at `start`.
    pub fn id_for(start: Timestamp) -> String {
        format!("throttle-{}", start.millis())
    }

    /// How long it lasted (to `now` while it continues).
    pub fn duration_ms(&self, now: Timestamp) -> i64 {
        self.end.unwrap_or(now).since(self.start).max(0)
    }

    /// "for 4 min" style text.
    pub fn duration_text(&self, now: Timestamp) -> String {
        duration((self.duration_ms(now) / SECOND) as u64)
    }
}

/// What Dyno can and cannot see on this machine, so "cannot tell" is explained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputStatus {
    /// What it is ("CPU clock speed", "Temperature", "The system's own speed limit").
    pub label: String,
    /// Whether this machine reports it.
    pub available: bool,
    /// The value in words when available, else why not.
    pub detail: String,
}

/// Everything the Throttling page shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThrottleOverview {
    /// When it was computed.
    pub at: Timestamp,
    /// Right now.
    pub reading: ThrottleReading,
    /// The running episode, if one has lasted long enough to count.
    pub current: Option<ThrottleEpisode>,
    /// Episodes of the last 30 days, newest first (at most [`MAX_EPISODES`]).
    pub episodes: Vec<ThrottleEpisode>,
    /// Minutes spent throttled in the last 30 days.
    pub minutes_30d: u32,
    /// What this machine reports.
    pub inputs: Vec<InputStatus>,
}

/// Most episodes listed.
pub const MAX_EPISODES: usize = 100;

/// Totals the minutes of `episodes` (open ones counted to `now`).
pub fn total_minutes(episodes: &[ThrottleEpisode], now: Timestamp) -> u32 {
    let ms: i64 = episodes.iter().map(|e| e.duration_ms(now)).sum();
    u32::try_from(ms / MINUTE).unwrap_or(u32::MAX)
}

/// What the tracker reports after an input.
#[derive(Debug, Clone, PartialEq)]
pub enum TrackerEvent {
    /// An episode began (it was throttled for [`SUSTAIN_MS`]).
    Started(ThrottleEpisode),
    /// The episode ended.
    Ended(ThrottleEpisode),
}

#[derive(Debug, Clone)]
struct Open {
    start: Timestamp,
    last: Timestamp,
    causes: [u32; 4],
    lost_sum: f64,
    lost_n: u32,
    peak: f32,
    peak_temp: Option<f32>,
    announced: bool,
}

impl Open {
    fn episode(&self, end: Option<Timestamp>) -> ThrottleEpisode {
        let causes = [
            ThrottleCause::Thermal,
            ThrottleCause::Power,
            ThrottleCause::BatterySaver,
            ThrottleCause::Unexplained,
        ];
        let cause = causes
            .iter()
            .zip(self.causes)
            .max_by_key(|(_, n)| *n)
            .map_or(ThrottleCause::Unexplained, |(c, _)| *c);
        ThrottleEpisode {
            id: ThrottleEpisode::id_for(self.start),
            start: self.start,
            end,
            cause,
            avg_lost_percent: if self.lost_n == 0 {
                0.0
            } else {
                (self.lost_sum / f64::from(self.lost_n)) as f32
            },
            peak_lost_percent: self.peak,
            peak_temperature_c: self.peak_temp,
        }
    }
}

fn cause_slot(c: ThrottleCause) -> usize {
    match c {
        ThrottleCause::Thermal => 0,
        ThrottleCause::Power => 1,
        ThrottleCause::BatterySaver => 2,
        ThrottleCause::Unexplained => 3,
    }
}

/// Turns a stream of inputs into episodes.
#[derive(Debug, Default)]
pub struct ThrottleTracker {
    window: VecDeque<ThrottleInput>,
    open: Option<Open>,
    clear_since: Option<Timestamp>,
    latest: Option<ThrottleReading>,
}

impl ThrottleTracker {
    /// A tracker with nothing seen.
    pub fn new() -> ThrottleTracker {
        ThrottleTracker::default()
    }

    /// The judgement of the latest window.
    pub fn reading(&self) -> Option<&ThrottleReading> {
        self.latest.as_ref()
    }

    /// The running episode, once it has lasted long enough to count.
    pub fn current(&self) -> Option<ThrottleEpisode> {
        self.open.as_ref().filter(|o| o.announced).map(|o| o.episode(None))
    }

    /// Takes the next input (time order). Time running backwards or a long silence ends the window and any episode.
    pub fn push(&mut self, input: ThrottleInput) -> Vec<TrackerEvent> {
        let mut events = Vec::new();
        if let Some(prev) = self.window.back() {
            let gap = input.at.since(prev.at);
            if !(0..=GAP_MS).contains(&gap) {
                let end = prev.at;
                self.window.clear();
                self.clear_since = None;
                if let Some(o) = self.open.take() {
                    if o.announced {
                        events.push(TrackerEvent::Ended(o.episode(Some(end))));
                    }
                }
            }
        }
        self.window.push_back(input);
        while self.window.front().is_some_and(|f| input.at.since(f.at) > WINDOW_MS) {
            self.window.pop_front();
        }
        let (a, b) = self.window.as_slices();
        let all: Vec<ThrottleInput> = a.iter().chain(b).copied().collect();
        let reading = detect(&all);
        let verdict = reading.verdict;
        self.latest = Some(reading);
        match verdict {
            Verdict::Throttled { cause, lost_percent } => {
                self.clear_since = None;
                let slot = self.open.get_or_insert(Open {
                    start: input.at,
                    last: input.at,
                    causes: [0; 4],
                    lost_sum: 0.0,
                    lost_n: 0,
                    peak: 0.0,
                    peak_temp: None,
                    announced: false,
                });
                slot.last = input.at;
                slot.causes[cause_slot(cause)] += 1;
                slot.lost_sum += f64::from(lost_percent);
                slot.lost_n += 1;
                slot.peak = slot.peak.max(lost_percent);
                if let Some(t) = input.temperature_c {
                    slot.peak_temp = Some(slot.peak_temp.map_or(t, |p| p.max(t)));
                }
                if !slot.announced && input.at.since(slot.start) >= SUSTAIN_MS {
                    slot.announced = true;
                    events.push(TrackerEvent::Started(slot.episode(None)));
                }
            }
            _ => {
                if let Some(o) = &self.open {
                    if !o.announced {
                        // Throttled for less than SUSTAIN_MS: a dip, not an episode.
                        self.open = None;
                    } else {
                        let since = *self.clear_since.get_or_insert(input.at);
                        if input.at.since(since) >= CLEAR_MS {
                            if let Some(o) = self.open.take() {
                                events.push(TrackerEvent::Ended(o.episode(Some(o.last))));
                            }
                            self.clear_since = None;
                        }
                    }
                }
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(at_s: i64, load: f32, mhz: u32) -> ThrottleInput {
        ThrottleInput {
            at: Timestamp(at_s * SECOND),
            load,
            frequency_mhz: mhz,
            expected_mhz: 3_000,
            temperature_c: None,
            critical_c: None,
            on_battery: None,
            power_saver: None,
            os: OsThrottle::default(),
        }
    }

    fn window(f: impl Fn(&mut ThrottleInput)) -> Vec<ThrottleInput> {
        (0..10)
            .map(|i| {
                let mut x = input(i, 90.0, 1_800);
                f(&mut x);
                x
            })
            .collect()
    }

    fn cause(r: &ThrottleReading) -> Option<(ThrottleCause, f32)> {
        match r.verdict {
            Verdict::Throttled { cause, lost_percent } => Some((cause, lost_percent)),
            _ => None,
        }
    }

    #[test]
    fn heat_is_named_from_the_temperature() {
        let r = detect(&window(|i| {
            i.temperature_c = Some(98.0);
            i.critical_c = Some(105.0);
        }));
        let (c, lost) = cause(&r).unwrap();
        assert_eq!(c, ThrottleCause::Thermal);
        assert!((lost - 40.0).abs() < 0.1);
        assert_eq!(r.confidence, Confidence::Medium);
        assert!(r.headline.contains("heat") && r.explanation.contains("vents"), "{r:?}");
    }

    #[test]
    fn the_system_saying_so_is_the_strongest_evidence() {
        let r = detect(&window(|i| {
            i.os = OsThrottle {
                thermal: Some(true),
                speed_limit_percent: Some(60),
                power: None,
            }
        }));
        assert_eq!(cause(&r), Some((ThrottleCause::Thermal, 40.0)));
        assert_eq!(r.confidence, Confidence::High);
        let r = detect(&window(|i| {
            i.os = OsThrottle {
                power: Some(true),
                ..Default::default()
            }
        }));
        assert_eq!(cause(&r).map(|c| c.0), Some(ThrottleCause::Power));
        assert_eq!(r.confidence, Confidence::High);
    }

    #[test]
    fn the_system_limit_counts_even_without_a_clock_reading() {
        let r = detect(&window(|i| {
            i.frequency_mhz = 0;
            i.expected_mhz = 0;
            i.os = OsThrottle {
                speed_limit_percent: Some(75),
                ..Default::default()
            };
        }));
        assert_eq!(cause(&r), Some((ThrottleCause::Unexplained, 25.0)));
        assert_eq!(r.confidence, Confidence::Medium);
    }

    #[test]
    fn battery_saver_and_battery_power_are_told_apart() {
        let r = detect(&window(|i| i.power_saver = Some(true)));
        assert_eq!(cause(&r).map(|c| c.0), Some(ThrottleCause::BatterySaver));
        let r = detect(&window(|i| {
            i.on_battery = Some(true);
            i.power_saver = Some(false);
        }));
        assert_eq!(cause(&r).map(|c| c.0), Some(ThrottleCause::Power));
        assert!(r.explanation.contains("battery"));
    }

    #[test]
    fn heat_wins_over_battery_and_unknown_is_admitted() {
        let r = detect(&window(|i| {
            i.temperature_c = Some(100.0);
            i.on_battery = Some(true);
        }));
        assert_eq!(cause(&r).map(|c| c.0), Some(ThrottleCause::Thermal));
        let r = detect(&window(|_| {}));
        assert_eq!(cause(&r).map(|c| c.0), Some(ThrottleCause::Unexplained));
        assert_eq!(r.confidence, Confidence::Low);
        assert!(
            r.explanation.contains("cannot be ruled out"),
            "no temperature sensor: {}",
            r.explanation
        );
        let r = detect(&window(|i| i.temperature_c = Some(60.0)));
        assert!(r.explanation.contains("firmware"), "{}", r.explanation);
    }

    #[test]
    fn idle_machines_and_full_speed_are_not_throttled() {
        let idle = detect(&window(|i| i.load = 5.0));
        assert_eq!(idle.verdict, Verdict::NotBusy);
        let fine = detect(&window(|i| i.frequency_mhz = 3_200));
        assert_eq!(fine.verdict, Verdict::Fine);
        let slight = detect(&window(|i| i.frequency_mhz = 2_700));
        assert_eq!(slight.verdict, Verdict::Fine, "10% under is within normal variation");
    }

    #[test]
    fn missing_inputs_mean_cannot_tell() {
        let r = detect(&window(|i| {
            i.frequency_mhz = 0;
            i.expected_mhz = 0;
        }));
        assert_eq!(r.verdict, Verdict::CannotTell);
        assert_eq!(r.confidence, Confidence::Low);
        assert_eq!(detect(&[]).verdict, Verdict::CannotTell);
        let one = detect(&window(|_| {})[..2]);
        assert_eq!(one.confidence, Confidence::Low, "two samples are thin evidence");
    }

    #[test]
    fn minutes_are_totalled() {
        let ep = |start_min: i64, end_min: Option<i64>| ThrottleEpisode {
            id: ThrottleEpisode::id_for(Timestamp(start_min * MINUTE)),
            start: Timestamp(start_min * MINUTE),
            end: end_min.map(|m| Timestamp(m * MINUTE)),
            cause: ThrottleCause::Power,
            avg_lost_percent: 20.0,
            peak_lost_percent: 25.0,
            peak_temperature_c: None,
        };
        assert_eq!(
            total_minutes(&[ep(0, Some(10)), ep(20, Some(25)), ep(40, None)], Timestamp(50 * MINUTE)),
            25
        );
        assert_eq!(total_minutes(&[], Timestamp(0)), 0);
        assert_eq!(ep(5, Some(3)).duration_ms(Timestamp(0)), 0, "never negative");
        assert_eq!(ThrottleCause::Unexplained.label(), "Unknown cause");
    }

    #[test]
    fn built_from_a_sample() {
        use crate::metrics::{fixtures::sample, BatterySample, PowerSource, Reading, Temperature};
        let mut s = sample(1_000);
        s.cpu.total = 80.0;
        s.cpu.frequency_mhz = 1_000;
        s.battery = Reading::Available(BatterySample {
            charge: 50.0,
            source: PowerSource::Battery,
            health: 90.0,
            cycles: None,
            seconds_to_empty: None,
        });
        s.temperatures = Reading::Available(vec![Temperature {
            label: "CPU".into(),
            celsius: 70.0,
            critical: Some(100.0),
        }]);
        let i = ThrottleInput::from_sample(&s, Some(false), OsThrottle::default());
        assert_eq!((i.load, i.frequency_mhz, i.expected_mhz), (80.0, 1_000, 3_000));
        assert_eq!(
            (i.temperature_c, i.critical_c, i.on_battery, i.power_saver),
            (Some(70.0), Some(100.0), Some(true), Some(false))
        );
        let bare = ThrottleInput::from_sample(&sample(0), None, OsThrottle::default());
        assert_eq!((bare.temperature_c, bare.on_battery), (None, None));
    }

    fn feed(t: &mut ThrottleTracker, from: i64, to: i64, f: impl Fn(&mut ThrottleInput)) -> Vec<TrackerEvent> {
        let mut out = Vec::new();
        for s in from..to {
            let mut x = input(s, 90.0, 3_000);
            f(&mut x);
            out.extend(t.push(x));
        }
        out
    }

    fn slow(i: &mut ThrottleInput) {
        i.frequency_mhz = 1_500;
        i.temperature_c = Some(99.0);
    }

    #[test]
    fn an_episode_needs_to_last_and_ends_after_it_clears() {
        let mut t = ThrottleTracker::new();
        assert!(feed(&mut t, 0, 60, |_| {}).is_empty());
        let started = feed(&mut t, 60, 120, slow);
        assert_eq!(started.len(), 1, "{started:?}");
        let TrackerEvent::Started(e) = &started[0] else { panic!() };
        assert_eq!(e.cause, ThrottleCause::Thermal);
        assert_eq!(e.id, format!("throttle-{}", e.start.millis()));
        let started_id = e.id.clone();
        assert!(e.end.is_none());
        assert!(t.current().is_some());
        let ended = feed(&mut t, 120, 200, |_| {});
        assert_eq!(ended.len(), 1);
        let TrackerEvent::Ended(e) = &ended[0] else { panic!() };
        assert_eq!(e.cause, ThrottleCause::Thermal);
        assert_eq!(e.id, started_id, "the id does not change when the episode ends");
        assert!(e.end.is_some() && e.duration_ms(Timestamp(0)) >= 40 * SECOND);
        assert!(e.avg_lost_percent > 40.0 && e.peak_lost_percent >= e.avg_lost_percent);
        assert_eq!(e.peak_temperature_c, Some(99.0));
        assert!(e.duration_text(Timestamp(0)).contains('s') || e.duration_text(Timestamp(0)).contains("min"));
        assert!(t.current().is_none());
        assert_eq!(t.reading().map(|r| r.verdict), Some(Verdict::Fine));
    }

    #[test]
    fn a_short_dip_is_not_an_episode() {
        let mut t = ThrottleTracker::new();
        let mut events = feed(&mut t, 0, 20, |_| {});
        events.extend(feed(&mut t, 20, 30, slow));
        events.extend(feed(&mut t, 30, 120, |_| {}));
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn a_gap_or_a_clock_change_ends_the_episode() {
        let mut t = ThrottleTracker::new();
        feed(&mut t, 0, 60, slow);
        assert!(t.current().is_some());
        // The clock jumps back an hour: the episode ends at the last time seen and the window starts afresh.
        let mut x = input(0, 90.0, 3_000);
        x.at = Timestamp(30 * SECOND);
        let events = t.push(x);
        assert!(
            matches!(&events[..], [TrackerEvent::Ended(e)] if e.end == Some(Timestamp(59 * SECOND))),
            "{events:?}"
        );
        // A silence of ten minutes does the same.
        feed(&mut t, 100, 160, slow);
        assert!(t.current().is_some());
        let events = t.push(input(1_000, 90.0, 3_000));
        assert!(matches!(&events[..], [TrackerEvent::Ended(_)]), "{events:?}");
        assert!(t.current().is_none());
    }

    #[test]
    fn the_dominant_cause_names_the_episode() {
        let mut t = ThrottleTracker::new();
        feed(&mut t, 0, 25, |i| {
            i.frequency_mhz = 1_500;
            i.on_battery = Some(true);
        });
        let events = feed(&mut t, 25, 120, |i| {
            i.frequency_mhz = 1_500;
            i.temperature_c = Some(99.0);
        });
        assert!(events.is_empty() || matches!(events[0], TrackerEvent::Started(_)));
        let cur = t.current().unwrap();
        assert_eq!(cur.cause, ThrottleCause::Thermal);
    }
}
