//! Throttling and hardware life in the agent (WP-23, WP-24): holds the three hardware readers, feeds the throttling
//! tracker from the sampler, reads SSD wear and battery capacity on a slow background cycle, stores the readings and
//! answers the Throttling and Lifespan requests. The judging is in `dyno_core::{throttle, lifespan}`; this module only
//! gathers inputs, persists and raises alarms. Slow reads (external tools) never run on the sampling thread or under the
//! core lock.

use crate::agent::{lock, now, Shared};
use crate::store::{hardware_kind, Store};
use dyno_core::alarms::feed::{from_lifespan, from_throttle, Observation};
use dyno_core::diagnosis::ThrottleContext;
use dyno_core::hardware::{BatteryCapacity, Confidence, DiskWear, OsThrottle, NO_BATTERY};
use dyno_core::lifespan::{self, BatteryFacts, CapacityPoint, LifeSubject, LifespanOverview, SsdFacts, WearPoint};
use dyno_core::metrics::{Reading, SystemSample};
use dyno_core::ports::{BatteryCapacitySource, DiskWearSource, ThrottleSignals};
use dyno_core::throttle::{self, InputStatus, ThrottleInput, ThrottleOverview, ThrottleTracker, TrackerEvent, MAX_EPISODES};
use dyno_core::time::{Timestamp, DAY, HOUR};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The OS's speed limit is read at most this often, and only while the CPU is busy or was limited.
pub const OS_SIGNAL_EVERY: Duration = Duration::from_secs(10);
/// CPU load from which the OS's limit is worth reading.
pub const BUSY_FOR_SIGNAL: f32 = 40.0;
/// A cached OS reading older than this is not used for a new input.
const OS_SIGNAL_FRESH: Duration = Duration::from_secs(30);
/// Slow readings (SSD wear, battery) are taken this often.
pub const SLOW_EVERY: Duration = Duration::from_secs(3 * 3600);
/// The first slow reading waits this long after the agent starts (the sampler comes first).
pub const SLOW_FIRST_AFTER: Duration = Duration::from_secs(20);
/// A reading is stored when the previous one is at least this old.
pub const STORE_EVERY_MS: i64 = 12 * HOUR;
/// Devices with no reading for this long are no longer shown.
pub const DEVICE_STALE_MS: i64 = 14 * DAY;
/// The Throttling page lists episodes of this many days.
pub const EPISODE_DAYS: i64 = 30;

struct State {
    tracker: ThrottleTracker,
    os: Reading<OsThrottle>,
    os_at: Option<Instant>,
    wear: Reading<Vec<DiskWear>>,
    battery: Reading<BatteryCapacity>,
    power_saver: Option<bool>,
}

/// The hardware readers and what they last said.
pub struct HardwareCentre {
    signals: Mutex<Box<dyn ThrottleSignals>>,
    wear: Mutex<Box<dyn DiskWearSource>>,
    battery: Mutex<Box<dyn BatteryCapacitySource>>,
    state: Mutex<State>,
}

impl HardwareCentre {
    /// A centre over these readers.
    pub fn new(
        signals: Box<dyn ThrottleSignals>,
        wear: Box<dyn DiskWearSource>,
        battery: Box<dyn BatteryCapacitySource>,
    ) -> HardwareCentre {
        HardwareCentre {
            signals: Mutex::new(signals),
            wear: Mutex::new(wear),
            battery: Mutex::new(battery),
            state: Mutex::new(State {
                tracker: ThrottleTracker::new(),
                os: Reading::unavailable("Not read yet"),
                os_at: None,
                wear: Reading::unavailable("Not read yet"),
                // Until the first slow read there is nothing to say about a battery.
                battery: Reading::unavailable(NO_BATTERY),
                power_saver: None,
            }),
        }
    }

    /// Replaces the readers (tests, and a future helper-backed reader).
    pub fn replace_sources(
        &self,
        signals: Box<dyn ThrottleSignals>,
        wear: Box<dyn DiskWearSource>,
        battery: Box<dyn BatteryCapacitySource>,
    ) {
        *lock(&self.signals) = signals;
        *lock(&self.wear) = wear;
        *lock(&self.battery) = battery;
        let mut s = lock(&self.state);
        s.os_at = None;
        s.os = Reading::unavailable("Not read yet");
    }

    /// Remembers whether a battery-saving mode is on (from the facts, refreshed by the background cycle).
    pub fn set_power_saver(&self, on: Option<bool>) {
        lock(&self.state).power_saver = on;
    }

    /// What the diagnosis needs to name a throttling cause.
    pub fn context(&self) -> ThrottleContext {
        let s = lock(&self.state);
        ThrottleContext {
            os: s.os.value().copied().unwrap_or_default(),
            power_saver: s.power_saver,
        }
    }

    /// Feeds one sample to the tracker; returns the episodes that started or ended. Reads the OS's speed limit (an
    /// external tool on macOS) at most every [`OS_SIGNAL_EVERY`] and only while it can matter.
    pub fn on_sample(&self, sample: &SystemSample) -> (Vec<TrackerEvent>, Option<Confidence>) {
        let (due, power_saver) = {
            let s = lock(&self.state);
            let limited = s.os.value().is_some_and(OsThrottle::limited);
            let stale = s.os_at.is_none_or(|t| t.elapsed() >= OS_SIGNAL_EVERY);
            (stale && (sample.cpu.total >= BUSY_FOR_SIGNAL || limited), s.power_saver)
        };
        if due {
            let reading = lock(&self.signals).read();
            let mut s = lock(&self.state);
            s.os = reading;
            s.os_at = Some(Instant::now());
        }
        let mut s = lock(&self.state);
        let os = match (&s.os, s.os_at) {
            (Reading::Available(o), Some(t)) if t.elapsed() < OS_SIGNAL_FRESH => *o,
            _ => OsThrottle::default(),
        };
        let events = s.tracker.push(ThrottleInput::from_sample(sample, power_saver, os));
        let confidence = s.tracker.reading().map(|r| r.confidence);
        (events, confidence)
    }

    /// The Throttling page.
    pub fn throttle_overview(&self, store: &Store, sample: Option<&SystemSample>, t: Timestamp) -> ThrottleOverview {
        let (reading, current, os, power_saver) = {
            let s = lock(&self.state);
            (s.tracker.reading().cloned(), s.tracker.current(), s.os.clone(), s.power_saver)
        };
        let episodes = store
            .throttle_episodes(t.minus(EPISODE_DAYS * DAY), MAX_EPISODES as u32)
            .unwrap_or_default();
        let mut all = episodes.clone();
        all.extend(current.clone());
        ThrottleOverview {
            at: t,
            reading: reading.unwrap_or_else(|| throttle::detect(&[])),
            current,
            minutes_30d: throttle::total_minutes(&all, t),
            episodes,
            inputs: inputs(sample, &os, power_saver),
        }
    }

    /// The Lifespan page: one forecast per SSD and battery, from the stored readings.
    pub fn lifespan_overview(&self, store: &Store, t: Timestamp) -> LifespanOverview {
        let (wear_now, battery_now) = {
            let s = lock(&self.state);
            (s.wear.clone(), s.battery.clone())
        };
        let mut forecasts = Vec::new();
        let ssds = store.hardware_devices(hardware_kind::SSD).unwrap_or_default();
        for device in ssds {
            let rows: Vec<(Timestamp, DiskWear)> = store.hardware_readings(hardware_kind::SSD, &device).unwrap_or_default();
            let Some((last_at, latest)) = rows.last().cloned() else { continue };
            if t.since(last_at) > DEVICE_STALE_MS {
                continue;
            }
            let points: Vec<WearPoint> = rows
                .iter()
                .map(|(at, w)| WearPoint {
                    at: *at,
                    percent_used: w.percent_used,
                    bytes_written: w.bytes_written,
                })
                .collect();
            let facts = SsdFacts {
                name: &latest.name,
                rated_bytes: latest.rated_bytes,
                power_on_hours: latest.power_on_hours,
            };
            forecasts.push(lifespan::ssd_life(&facts, &points, t));
        }
        if forecasts.iter().all(|f| f.subject != LifeSubject::Ssd) {
            let reason = match &wear_now {
                Reading::Unavailable(r) => r.clone(),
                Reading::Available(_) => "Dyno has not stored an SSD wear reading yet; the first one arrives within minutes.".into(),
            };
            forecasts.push(lifespan::unavailable(LifeSubject::Ssd, "SSD", &reason));
        }
        let batteries = store.hardware_devices(hardware_kind::BATTERY).unwrap_or_default();
        let mut any_battery = false;
        for device in batteries {
            let rows: Vec<(Timestamp, BatteryCapacity)> = store.hardware_readings(hardware_kind::BATTERY, &device).unwrap_or_default();
            let Some((last_at, latest)) = rows.last().cloned() else { continue };
            if t.since(last_at) > DEVICE_STALE_MS {
                continue;
            }
            let points: Vec<CapacityPoint> = rows
                .iter()
                .filter_map(|(at, c)| {
                    Some(CapacityPoint {
                        at: *at,
                        health_percent: c.health_percent()?,
                        cycles: c.cycles,
                    })
                })
                .collect();
            let facts = BatteryFacts {
                name: "Battery",
                rated_cycles: latest.rated_cycles,
            };
            forecasts.push(lifespan::battery_life(&facts, &points, t));
            any_battery = true;
        }
        if !any_battery {
            match &battery_now {
                // A desktop has no battery: say nothing about one.
                Reading::Unavailable(r) if r.as_str() == NO_BATTERY => {}
                Reading::Unavailable(r) => forecasts.push(lifespan::unavailable(LifeSubject::Battery, "Battery", r)),
                Reading::Available(_) => {}
            }
        }
        LifespanOverview { at: t, forecasts }
    }

    /// Reads SSD wear and battery capacity now (slow: external tools), remembering the answers.
    fn read_slow(&self) -> (Reading<Vec<DiskWear>>, Reading<BatteryCapacity>) {
        let wear = lock(&self.wear).read();
        let battery = lock(&self.battery).read();
        let mut s = lock(&self.state);
        s.wear = wear.clone();
        s.battery = battery.clone();
        (wear, battery)
    }
}

fn inputs(sample: Option<&SystemSample>, os: &Reading<OsThrottle>, power_saver: Option<bool>) -> Vec<InputStatus> {
    let status = |label: &str, ok: bool, yes: String, no: &str| InputStatus {
        label: label.into(),
        available: ok,
        detail: if ok { yes } else { no.into() },
    };
    let clock = sample.filter(|s| s.cpu.frequency_mhz > 0 && s.cpu.base_frequency_mhz > 0);
    let temp = sample.and_then(|s| s.hottest());
    vec![
        status(
            "CPU clock speed",
            clock.is_some(),
            clock.map_or(String::new(), |s| {
                format!("{} MHz of {} MHz", s.cpu.frequency_mhz, s.cpu.base_frequency_mhz)
            }),
            "This machine does not report the CPU's clock speed.",
        ),
        status(
            "Temperature",
            temp.is_some(),
            temp.map_or(String::new(), |t| format!("{} at {:.0} °C", t.label, t.celsius)),
            "This machine does not report a CPU temperature to apps.",
        ),
        match os {
            Reading::Available(o) => InputStatus {
                label: "The system's own speed limit".into(),
                available: true,
                detail: o.speed_limit_percent.map_or_else(
                    || "Reported: no limit recorded".to_string(),
                    |p| format!("Reported: {p}% of full speed allowed"),
                ),
            },
            Reading::Unavailable(r) => InputStatus {
                label: "The system's own speed limit".into(),
                available: false,
                detail: r.clone(),
            },
        },
        status(
            "Battery-saving mode",
            power_saver.is_some(),
            if power_saver == Some(true) { "On".into() } else { "Off".into() },
            "Not known yet.",
        ),
    ]
}

/// Raises or returns one observation: stores the alarm change and tells the connected apps.
fn observe(shared: &Arc<Shared>, o: Observation, t: Timestamp) {
    let mut core = lock(&shared.core);
    let core = &mut *core;
    let changes = core.alarms.observe(&core.store, o, t);
    for c in &changes {
        let _ = core.recorder.on_alarm(&core.store, c, t);
    }
    crate::sampler::push_alarm_changes(shared, changes);
}

/// What the sampler does with the tracker's events: persists finished episodes and raises or returns the alarm.
pub fn handle_events(shared: &Arc<Shared>, events: Vec<TrackerEvent>, confidence: Option<Confidence>, t: Timestamp) {
    let (alarms, zone) = {
        let core = lock(&shared.core);
        (core.settings.hardware_alarms.throttling, core.zone.clone())
    };
    for e in events {
        match e {
            TrackerEvent::Started(ep) => {
                // Only with evidence: a hint (low confidence) is shown on the page but never raises an alarm.
                let c = confidence.unwrap_or(Confidence::Low);
                if alarms && c >= Confidence::Medium {
                    observe(shared, from_throttle(&ep, true, c, zone.as_ref()), t);
                }
            }
            TrackerEvent::Ended(ep) => {
                let _ = lock(&shared.core).store.put_throttle_episode(&ep);
                observe(shared, from_throttle(&ep, false, Confidence::Medium, zone.as_ref()), t);
            }
        }
    }
}

/// One slow cycle: reads the hardware, stores the readings that are due and raises or returns the lifespan alarms.
pub fn slow_cycle(shared: &Arc<Shared>) {
    let (wear, battery) = shared.hardware.read_slow();
    let t = now();
    {
        let core = lock(&shared.core);
        if let Reading::Available(disks) = &wear {
            for d in disks {
                store_if_due(&core.store, hardware_kind::SSD, &d.id, t, d);
            }
        }
        if let Reading::Available(b) = &battery {
            store_if_due(&core.store, hardware_kind::BATTERY, &b.id, t, b);
        }
    }
    let (forecasts, alarms) = {
        let core = lock(&shared.core);
        (
            shared.hardware.lifespan_overview(&core.store, t).forecasts,
            core.settings.hardware_alarms.lifespan,
        )
    };
    for f in forecasts {
        observe(shared, from_lifespan(&f, alarms && f.needs_attention()), t);
    }
}

fn store_if_due<T: serde::Serialize>(store: &Store, kind: &str, device: &str, t: Timestamp, doc: &T) {
    let due = store
        .last_hardware_reading(kind, device)
        .ok()
        .flatten()
        .is_none_or(|last| t.since(last) >= STORE_EVERY_MS);
    if due {
        if let Err(e) = store.put_hardware_reading(kind, device, t, doc) {
            log::error!("dyno-agent: could not store a hardware reading: {e}");
        }
    }
}

/// The background thread's body: primes the facts (for the battery-saving flag), then runs the slow cycle every
/// [`SLOW_EVERY`], the facts every few minutes, and the baseline evaluation every minute. Stops with the agent.
pub fn run_background(shared: &Arc<Shared>) {
    let started = Instant::now();
    let (mut last_slow, mut last_facts, mut last_baselines): (Option<Instant>, Option<Instant>, Option<Instant>) = (None, None, None);
    while !shared.stopped() {
        std::thread::sleep(Duration::from_millis(500));
        if shared.stopped() {
            break;
        }
        let guarded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if last_facts.is_none_or(|t| t.elapsed() >= crate::agent::FACTS_TTL) && started.elapsed() >= Duration::from_secs(5) {
                last_facts = Some(Instant::now());
                let f = crate::handlers::facts(shared);
                shared.hardware.set_power_saver(f.power_plan.value().map(|p| p.power_saver));
            }
            let slow_due = match last_slow {
                None => started.elapsed() >= SLOW_FIRST_AFTER,
                Some(t) => t.elapsed() >= SLOW_EVERY,
            };
            if slow_due {
                last_slow = Some(Instant::now());
                slow_cycle(shared);
            }
            if last_baselines.is_none_or(|t| t.elapsed() >= crate::baselines::EVALUATE_EVERY)
                && started.elapsed() >= Duration::from_secs(30)
            {
                last_baselines = Some(Instant::now());
                crate::baselines::evaluate_alarms(shared);
            }
        }));
        if guarded.is_err() {
            log::error!("dyno-agent: a hardware background cycle panicked; continuing");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::hardware::OsThrottle;
    use dyno_core::metrics::{BatterySample, PowerSource, Temperature};
    use dyno_core::time::SECOND;
    use dyno_platform::hardware::{FakeBattery, FakeThrottle, FakeWear};

    fn sample(at_s: i64, load: f32, mhz: u32) -> SystemSample {
        let mut platform = dyno_platform::fake::Fake::new().platform();
        let mut s = platform.sensors.sample(Timestamp(at_s * SECOND));
        s.at = Timestamp(at_s * SECOND);
        s.cpu.total = load;
        s.cpu.frequency_mhz = mhz;
        s.cpu.base_frequency_mhz = 3_000;
        s.battery = Reading::Available(BatterySample {
            charge: 80.0,
            source: PowerSource::Ac,
            health: 90.0,
            cycles: None,
            seconds_to_empty: None,
        });
        s.temperatures = Reading::Available(vec![Temperature {
            label: "CPU".into(),
            celsius: 98.0,
            critical: Some(100.0),
        }]);
        s
    }

    fn centre(os: Reading<OsThrottle>) -> HardwareCentre {
        HardwareCentre::new(
            Box::new(FakeThrottle(vec![os])),
            Box::new(FakeWear(Reading::unavailable("no smartctl"))),
            Box::new(FakeBattery(Reading::unavailable(NO_BATTERY))),
        )
    }

    #[test]
    fn busy_and_slow_for_long_enough_is_an_episode_with_the_systems_word_for_it() {
        let c = centre(Reading::Available(OsThrottle {
            thermal: Some(true),
            speed_limit_percent: Some(55),
            power: None,
        }));
        let mut started = None;
        for s in 0..60 {
            let (events, conf) = c.on_sample(&sample(s, 95.0, 1_650));
            if let Some(TrackerEvent::Started(e)) = events.into_iter().next() {
                started = Some((e, conf));
            }
        }
        let (ep, conf) = started.expect("an episode started");
        assert_eq!(ep.cause, throttle::ThrottleCause::Thermal);
        assert_eq!(conf, Some(Confidence::High));
        let store = Store::memory().unwrap();
        let o = c.throttle_overview(&store, Some(&sample(60, 95.0, 1_650)), Timestamp(60 * SECOND));
        assert!(o.current.is_some() && matches!(o.reading.verdict, throttle::Verdict::Throttled { .. }));
        assert_eq!(c.context().os.speed_limit_percent, Some(55));
        let labels: Vec<&str> = o.inputs.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "CPU clock speed",
                "Temperature",
                "The system's own speed limit",
                "Battery-saving mode"
            ]
        );
        assert!(o.inputs[2].detail.contains("55%") && o.inputs.iter().take(3).all(|i| i.available));
        assert!(!o.inputs[3].available);
    }

    #[test]
    fn an_idle_machine_never_asks_the_system_and_is_not_throttled() {
        let c = centre(Reading::unavailable("never read"));
        for s in 0..40 {
            let (events, _) = c.on_sample(&sample(s, 3.0, 800));
            assert!(events.is_empty());
        }
        let store = Store::memory().unwrap();
        let o = c.throttle_overview(&store, None, Timestamp(40 * SECOND));
        assert_eq!(o.reading.verdict, throttle::Verdict::NotBusy);
        assert_eq!(o.minutes_30d, 0);
        assert!(o.inputs.iter().all(|i| !i.available), "{:?}", o.inputs);
        assert_eq!(c.context().os, OsThrottle::default());
    }

    #[test]
    fn a_missing_sensor_is_cannot_tell_not_a_guess() {
        let c = centre(Reading::unavailable("Windows does not say"));
        let mut s = sample(0, 90.0, 0);
        s.cpu.base_frequency_mhz = 0;
        s.temperatures = Reading::unavailable("none");
        for i in 0..20 {
            s.at = Timestamp(i * SECOND);
            assert!(c.on_sample(&s).0.is_empty());
        }
        let o = c.throttle_overview(&Store::memory().unwrap(), Some(&s), Timestamp(20 * SECOND));
        assert_eq!(o.reading.verdict, throttle::Verdict::CannotTell);
        assert!(o.inputs[2].detail.contains("Windows does not say"));
    }

    #[test]
    fn lifespan_comes_from_stored_readings_and_says_why_when_there_are_none() {
        let c = centre(Reading::unavailable("x"));
        let store = Store::memory().unwrap();
        // Nothing stored and no reader: the SSD says why, the desktop has no battery card.
        let o = c.lifespan_overview(&store, Timestamp(0));
        assert_eq!(o.forecasts.len(), 1);
        assert!(
            matches!(&o.forecasts[0].outcome, lifespan::LifeOutcome::Unavailable { reason } if reason == "no smartctl")
                || o.forecasts[0].explanation.contains("Not read yet")
        );
        // 80 days of twice-daily battery readings, falling 0.03 points a day.
        let t0 = 1_000 * DAY;
        for i in 0..160 {
            let at = Timestamp(t0 + i * 12 * HOUR);
            let full = 5_000 - (i as u32) * 3 / 2 / 2;
            store
                .put_hardware_reading(
                    hardware_kind::BATTERY,
                    "b1",
                    at,
                    &BatteryCapacity {
                        id: "b1".into(),
                        design: 5_500,
                        full,
                        cycles: Some(100 + i as u32 / 4),
                        rated_cycles: Some(1_000),
                    },
                )
                .unwrap();
        }
        let now = Timestamp(t0 + 160 * 12 * HOUR);
        let o = c.lifespan_overview(&store, now);
        let battery = o
            .forecasts
            .iter()
            .find(|f| f.subject == LifeSubject::Battery)
            .expect("a battery forecast");
        assert!(battery.readings >= 100, "{battery:?}");
        assert!(battery.mid_days().is_some(), "{battery:?}");
        // A device not seen for weeks drops out.
        let later = c.lifespan_overview(&store, now.plus(40 * DAY));
        assert!(later.forecasts.iter().all(|f| f.subject != LifeSubject::Battery));
    }

    #[test]
    fn slow_readings_are_stored_at_most_twice_a_day() {
        let store = Store::memory().unwrap();
        let b = BatteryCapacity {
            id: "b".into(),
            design: 100,
            full: 90,
            cycles: None,
            rated_cycles: None,
        };
        store_if_due(&store, hardware_kind::BATTERY, "b", Timestamp(0), &b);
        store_if_due(&store, hardware_kind::BATTERY, "b", Timestamp(HOUR), &b);
        store_if_due(&store, hardware_kind::BATTERY, "b", Timestamp(13 * HOUR), &b);
        let rows: Vec<(Timestamp, BatteryCapacity)> = store.hardware_readings(hardware_kind::BATTERY, "b").unwrap();
        assert_eq!(rows.len(), 2);
    }
}
