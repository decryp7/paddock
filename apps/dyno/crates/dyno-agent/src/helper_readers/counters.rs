//! Temperatures, fans and CPU energy on macOS, through the helper (helper-wiring plan section 2.7, decision D2).
//!
//! macOS gives a program without administrator rights no temperatures, no fans and no CPU power. The helper reads what the OS
//! offers through `powermetrics`: CPU power on every Mac, fans on Intel Macs only, and **no temperatures** (macOS reports a
//! thermal pressure level, which Dyno shows elsewhere). So the card reads "CPU energy (fans on Intel Macs)", and a temperature
//! widget on a Mac says why it has no number rather than staying empty.
//!
//! `powermetrics` takes about a second, so [`HelperClimate`] reads on its own worker every [`EVERY`], never on the sampler; the
//! sampler copies the last answer ([`HelperSensors`]). The CPU power is added up per local day ([`CpuEnergyMeter`]) and kept in
//! the database so a restart continues the day. Everything is a [`HelperReading`] on the seam: the helper answers or the reading
//! says why nobody can.

use crate::helper_first::{HelperCalls, HelperFirst, HelperPort, HelperReading, ReadClass};
use crate::helper_link::CallError;
use dyno_core::helper_counters::{CounterAnswer, CounterValue, EnergyDomain};
use dyno_core::helper_requests::{CounterId, CounterQuery, RequestClass};
use dyno_core::metrics::{Fan, Reading, SystemSample, Temperature};
use dyno_core::ports::SystemSensors;
use dyno_core::power::energy::{cpu_package_domains, CpuEnergyMeter};
use dyno_core::read_via::{sentence, ReadVia};
use dyno_core::time::{LocalZone, Timestamp};
use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

/// How often the helper is asked (powermetrics takes about a second).
pub const EVERY: i64 = 30 * dyno_core::time::SECOND;
/// The key the day totals are kept under in the database's `meta` table.
pub const META_KEY: &str = "cpuEnergyJoules";

/// What one `Counters` request came to: one outcome per reading, so a Mac without temperatures still gives its CPU power.
#[derive(Debug, Clone, PartialEq)]
pub struct CounterSet {
    /// The temperatures, or why there are none.
    pub temperatures: Result<Vec<Temperature>, String>,
    /// The fans, or why there are none.
    pub fans: Result<Vec<Fan>, String>,
    /// The CPU package power or energy counter, or why there is none.
    pub cpu: Result<Vec<EnergyDomain>, String>,
}

/// The macOS readings.
pub struct MacCounters;

fn outcome<T>(answers: &[CounterAnswer], id: CounterId, pick: impl FnOnce(&CounterValue) -> Option<T>) -> Result<T, String> {
    match answers.iter().find(|a| a.id == id) {
        None => Err("The helper did not answer this reading.".into()),
        Some(a) => match &a.value {
            CounterValue::Unavailable(why) => Err(why.clone()),
            v => pick(v).ok_or_else(|| "The helper answered with another kind of reading.".into()),
        },
    }
}

impl HelperReading for MacCounters {
    type Value = CounterSet;

    fn name(&self) -> &'static str {
        "temperatures, fans and CPU energy"
    }

    fn class(&self) -> ReadClass {
        // Read by the 30 s worker, never by the sampler, and `powermetrics` takes about 2 s per id: a live deadline would time out on a
        // real Mac and put the helper into backoff. A call gets no deadline of its own (the request kind's wait applies).
        ReadClass::Bulk
    }

    fn via_helper(&self, calls: &dyn HelperCalls) -> Result<CounterSet, CallError> {
        let answers = calls.counters(CounterQuery {
            ids: vec![CounterId::Thermal, CounterId::Fans, CounterId::CpuEnergy],
            class: RequestClass::Background,
        })?;
        Ok(CounterSet {
            temperatures: outcome(&answers, CounterId::Thermal, |v| match v {
                CounterValue::Thermal(z) => Some(
                    z.iter()
                        .map(|z| Temperature {
                            label: z.label.clone(),
                            celsius: z.milli_celsius as f32 / 1000.0,
                            critical: None,
                        })
                        .collect(),
                ),
                _ => None,
            }),
            fans: outcome(&answers, CounterId::Fans, |v| match v {
                CounterValue::Fans(f) => Some(
                    f.iter()
                        .map(|f| Fan {
                            label: f.label.clone(),
                            rpm: f.rpm,
                        })
                        .collect(),
                ),
                _ => None,
            }),
            cpu: outcome(&answers, CounterId::CpuEnergy, |v| match v {
                CounterValue::CpuEnergy(e) => Some(e.clone()),
                _ => None,
            }),
        })
    }

    fn local(&mut self) -> Reading<CounterSet> {
        // The reason is a clause: the seam makes the sentence and adds what to do.
        Reading::unavailable("this computer gives Dyno no temperatures, fan speeds or CPU power without the helper")
    }
}

/// What the sampler and the energy report show of the last read.
#[derive(Debug, Clone)]
struct View {
    temperatures: Reading<Vec<Temperature>>,
    temperatures_via: ReadVia,
    fans: Reading<Vec<Fan>>,
    cpu_via: ReadVia,
}

struct State {
    view: Option<View>,
    at: Option<Timestamp>,
    epoch: u64,
    meter: CpuEnergyMeter,
    /// Where the temperatures of the last sample came from (what `LiveUpdate.via.temperatures` says).
    live_via: ReadVia,
}

/// The helper's temperatures, fans and CPU energy, kept between reads. `active` is false where the helper has nothing to add (not
/// macOS, or a test agent that must not touch the machine): then nothing is read and nothing is claimed.
pub struct HelperClimate {
    active: bool,
    reading: Mutex<HelperFirst<MacCounters>>,
    state: Mutex<State>,
}

fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The day totals saved in the database; corrupt or missing means none (new persistent state falls back to defaults).
pub fn parse_saved(text: Option<&str>) -> BTreeMap<i64, f64> {
    text.and_then(|t| serde_json::from_str::<BTreeMap<i64, f64>>(t).ok())
        .map(|m| m.into_iter().filter(|(_, j)| j.is_finite() && *j >= 0.0).collect())
        .unwrap_or_default()
}

impl HelperClimate {
    /// A climate over `port`; `saved` is what the database held.
    pub fn new(port: Box<dyn HelperPort>, active: bool, saved: BTreeMap<i64, f64>) -> HelperClimate {
        HelperClimate {
            active,
            reading: Mutex::new(HelperFirst::new(MacCounters, port)),
            state: Mutex::new(State {
                view: None,
                at: None,
                epoch: 0,
                meter: CpuEnergyMeter::new(saved),
                live_via: ReadVia::Local,
            }),
        }
    }

    /// Whether this climate reads at all.
    pub fn active(&self) -> bool {
        self.active
    }

    /// Whether a read is due at `now`: never read yet, [`EVERY`] passed, or the helper changed since (it was turned on or off).
    pub fn due(&self, now: Timestamp) -> bool {
        if !self.active {
            return false;
        }
        let epoch = locked(&self.reading).epoch();
        let s = locked(&self.state);
        s.epoch != epoch || s.at.is_none_or(|at| now.since(at) >= EVERY || now < at)
    }

    /// Reads once at `at`. Returns the day totals to save when they changed.
    pub fn refresh(&self, at: Timestamp, zone: &dyn LocalZone) -> Option<BTreeMap<i64, f64>> {
        if !self.active {
            return None;
        }
        let (got, epoch) = {
            let mut r = locked(&self.reading);
            (r.read(), r.epoch())
        };
        let mut s = locked(&self.state);
        s.at = Some(at);
        s.epoch = epoch;
        let before = s.meter.days().clone();
        let view = match (got.via, got.value) {
            (ReadVia::Helper, Reading::Available(set)) => {
                let unavailable = |why: &str| (Reading::Unavailable(sentence(why)), ReadVia::unavailable(why));
                let (temperatures, temperatures_via) = match set.temperatures {
                    Ok(t) => (Reading::Available(t), ReadVia::Helper),
                    Err(why) => unavailable(&why),
                };
                let fans = match set.fans {
                    Ok(f) => Reading::Available(f),
                    Err(why) => Reading::Unavailable(sentence(&why)),
                };
                let packages = set.cpu.map(|all| cpu_package_domains(&all)).and_then(|p| {
                    if p.is_empty() {
                        Err("The helper listed no CPU power, as in a virtual machine.".to_string())
                    } else {
                        Ok(p)
                    }
                });
                let cpu_via = match packages {
                    Ok(p) => {
                        s.meter.feed(at, &p, zone);
                        ReadVia::Helper
                    }
                    Err(why) => {
                        s.meter.lose_thread();
                        ReadVia::unavailable(&why)
                    }
                };
                View {
                    temperatures,
                    temperatures_via,
                    fans,
                    cpu_via,
                }
            }
            (via, value) => {
                // Nobody could read: every part says why, and the time until the next read is not counted.
                s.meter.lose_thread();
                let why = match value {
                    Reading::Unavailable(why) => why,
                    Reading::Available(_) => String::new(),
                };
                View {
                    temperatures: Reading::Unavailable(why.clone()),
                    temperatures_via: via.clone(),
                    fans: Reading::Unavailable(why),
                    cpu_via: via,
                }
            }
        };
        log::info!(
            target: "dyno::helper",
            "temperatures, fans and CPU energy: temperatures {}, fans {}, CPU energy {}",
            if view.temperatures.value().is_some() { "read" } else { "not available" },
            if view.fans.value().is_some() { "read" } else { "not available" },
            if view.cpu_via == ReadVia::Helper { "read" } else { "not available" },
        );
        s.view = Some(view);
        (s.meter.days() != &before).then(|| s.meter.days().clone())
    }

    /// Puts the last read into `sample`: the helper's values when it gave them, else what the sensors already found, else the
    /// reason. Returns where the temperatures came from.
    pub fn apply(&self, sample: &mut SystemSample) -> ReadVia {
        let mut s = locked(&self.state);
        let Some(v) = s.view.clone() else { return ReadVia::Local };
        let via = if v.temperatures_via == ReadVia::Helper {
            sample.temperatures = v.temperatures;
            ReadVia::Helper
        } else if sample.temperatures.value().is_some() {
            ReadVia::Local
        } else {
            sample.temperatures = v.temperatures;
            v.temperatures_via
        };
        if v.fans.value().is_some() || sample.fans.value().is_none() {
            sample.fans = v.fans;
        }
        s.live_via = via.clone();
        via
    }

    /// Where the temperatures of the last sample came from.
    pub fn live_via(&self) -> ReadVia {
        locked(&self.state).live_via.clone()
    }

    /// The measured joules by local day and who measured them, for the energy report.
    pub fn cpu_energy(&self) -> (BTreeMap<i64, f64>, ReadVia) {
        let s = locked(&self.state);
        let via = s.view.as_ref().map_or(ReadVia::Local, |v| v.cpu_via.clone());
        (s.meter.days().clone(), via)
    }
}

/// Starts the worker that asks the helper every [`EVERY`] (and when the helper is turned on or off) and keeps the day totals in the
/// database. It lives as long as the agent.
///
/// # Errors
/// The thread could not be started.
pub fn spawn_worker(shared: &std::sync::Arc<crate::agent::Shared>) -> std::io::Result<std::thread::JoinHandle<()>> {
    let shared = shared.clone();
    std::thread::Builder::new().name("dyno-climate".into()).spawn(move || {
        while !shared.stopped() {
            let now = crate::agent::now();
            if shared.climate.due(now) {
                let zone = crate::agent::lock(&shared.core).zone.clone();
                let days = shared.climate.refresh(now, zone.as_ref());
                if let Some(days) = days {
                    match serde_json::to_string(&days) {
                        // The agent may have stopped during the read (a stop does not wait for it): then nothing is written.
                        Ok(json) => match shared.write_unless_stopped(|core| core.store.set_meta(META_KEY, &json)) {
                            None => break,
                            Some(Err(e)) => log::warn!(target: "dyno::helper", "could not keep the CPU energy totals: {e}"),
                            Some(Ok(_)) => {}
                        },
                        Err(e) => log::warn!(target: "dyno::helper", "could not write the CPU energy totals: {e}"),
                    }
                }
            }
            // Not `Shared::sleep`: its wake flag belongs to the sampler. One-second steps so a stopping agent is not kept waiting.
            for _ in 0..5 {
                if shared.stopped() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    })
}

/// The sensors with the helper's readings added; the common sensors still run and still answer when the helper cannot.
pub struct HelperSensors {
    inner: Box<dyn SystemSensors>,
    climate: std::sync::Arc<HelperClimate>,
}

impl HelperSensors {
    /// `inner` with `climate`'s last read added to every sample.
    pub fn new(inner: Box<dyn SystemSensors>, climate: std::sync::Arc<HelperClimate>) -> HelperSensors {
        HelperSensors { inner, climate }
    }
}

impl SystemSensors for HelperSensors {
    fn sample(&mut self, now: Timestamp) -> SystemSample {
        let mut sample = self.inner.sample(now);
        self.climate.apply(&mut sample);
        sample
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::helper_counters::{FanSpeed, ThermalZone};
    use dyno_core::time::FixedZone;

    fn answer(thermal: CounterValue, fans: CounterValue, cpu: CounterValue) -> Vec<CounterAnswer> {
        vec![
            CounterAnswer {
                id: CounterId::Thermal,
                value: thermal,
            },
            CounterAnswer {
                id: CounterId::Fans,
                value: fans,
            },
            CounterAnswer {
                id: CounterId::CpuEnergy,
                value: cpu,
            },
        ]
    }

    fn power(mw: u32) -> CounterValue {
        CounterValue::CpuEnergy(vec![EnergyDomain {
            label: "CPU Power".into(),
            microjoules: None,
            wraps_at_microjoules: None,
            milliwatts: Some(mw),
        }])
    }

    const APPLE_SILICON_NO_TEMPS: &str = "macOS reports a thermal pressure level, not temperatures, to software.";

    fn rig(answers: Vec<Result<dyno_core::helper_wire::HelperReply, crate::helper_link::TransportError>>, up: bool) -> HelperClimate {
        rig_net(answers, up).0
    }

    fn rig_net(
        answers: Vec<Result<dyno_core::helper_wire::HelperReply, crate::helper_link::TransportError>>,
        up: bool,
    ) -> (HelperClimate, std::sync::Arc<crate::helper_first::testkit::Net>) {
        use crate::helper_first::testkit::{centre, port, Net};
        let net = Net::new(up, answers);
        let c = centre(&net, &paddock_jobs::time::FakeClock::new());
        (HelperClimate::new(port(&c), true, BTreeMap::new()), net)
    }

    #[test]
    fn the_climate_call_gets_no_live_deadline_but_the_readiness_probe_does() {
        let (c, net) = rig_net(
            vec![reply(answer(
                CounterValue::Unavailable("n".into()),
                CounterValue::Unavailable("n".into()),
                power(1_000),
            ))],
            true,
        );
        c.refresh(T0, &UTC);
        let maxes = net.maxes.lock().unwrap().clone();
        assert_eq!(
            maxes.last().copied().flatten(),
            None,
            "the Counters call has no live deadline: {maxes:?}"
        );
        assert!(
            maxes[..maxes.len() - 1].iter().all(|m| *m == Some(crate::helper_link::LIVE_WAIT)),
            "the readiness probe is always live: {maxes:?}"
        );
        assert_eq!(MacCounters.class(), ReadClass::Bulk);
    }

    fn reply(a: Vec<CounterAnswer>) -> Result<dyno_core::helper_wire::HelperReply, crate::helper_link::TransportError> {
        Ok(dyno_core::helper_wire::HelperReply::CounterValues(a))
    }

    fn blank() -> SystemSample {
        dyno_platform::fake::quiet_sample(T0)
    }

    const T0: Timestamp = Timestamp(1_700_000_000_000);
    const UTC: FixedZone = FixedZone(0);

    #[test]
    fn on_apple_silicon_cpu_energy_is_measured_and_temperatures_say_why_not() {
        let c = rig(
            vec![
                reply(answer(
                    CounterValue::Unavailable(APPLE_SILICON_NO_TEMPS.into()),
                    CounterValue::Unavailable("No fans.".into()),
                    power(2_000),
                )),
                reply(answer(
                    CounterValue::Unavailable(APPLE_SILICON_NO_TEMPS.into()),
                    CounterValue::Unavailable("No fans.".into()),
                    power(4_000),
                )),
            ],
            true,
        );
        assert!(c.due(T0));
        c.refresh(T0, &UTC);
        assert!(!c.due(T0.plus(EVERY - 1)) && c.due(T0.plus(EVERY)));
        let saved = c.refresh(T0.plus(EVERY), &UTC).expect("the day total changed");
        // (2 W + 4 W) / 2 * 30 s.
        assert_eq!(saved.values().copied().sum::<f64>(), 90.0);
        assert_eq!(c.cpu_energy().1, ReadVia::Helper);
        let mut sample = blank();
        let via = c.apply(&mut sample);
        assert_eq!(via, ReadVia::unavailable(APPLE_SILICON_NO_TEMPS));
        assert_eq!(sample.temperatures, Reading::Unavailable(APPLE_SILICON_NO_TEMPS.into()));
        assert_eq!(c.live_via(), via);
    }

    #[test]
    fn an_intel_mac_gives_fans_and_the_helper_is_named_as_the_source_of_temperatures_it_read() {
        let c = rig(
            vec![reply(answer(
                CounterValue::Thermal(vec![ThermalZone {
                    label: "CPU".into(),
                    milli_celsius: 70_000,
                }]),
                CounterValue::Fans(vec![FanSpeed {
                    label: "Fan 0".into(),
                    rpm: 2_150,
                }]),
                power(1_000),
            ))],
            true,
        );
        c.refresh(T0, &UTC);
        let mut sample = blank();
        assert_eq!(c.apply(&mut sample), ReadVia::Helper);
        assert_eq!(sample.temperatures.value().map(|t| t[0].celsius), Some(70.0));
        assert_eq!(sample.fans.value().map(|f| f[0].rpm), Some(2_150));
    }

    #[test]
    fn without_the_helper_the_reason_asks_for_it_and_no_time_is_counted() {
        let c = rig(vec![], false);
        c.refresh(T0, &UTC);
        let mut sample = blank();
        let via = c.apply(&mut sample);
        assert!(matches!(via, ReadVia::NeedsHelper { .. }), "{via:?}");
        assert!(sample.temperatures.value().is_none());
        let (days, cpu_via) = c.cpu_energy();
        assert!(days.is_empty() && matches!(cpu_via, ReadVia::NeedsHelper { .. }));
    }

    #[test]
    fn temperatures_the_sensors_already_found_are_kept_when_the_helper_has_none() {
        let c = rig(vec![], false);
        c.refresh(T0, &UTC);
        let mut sample = blank();
        sample.temperatures = Reading::Available(vec![Temperature {
            label: "SoC".into(),
            celsius: 50.0,
            critical: None,
        }]);
        assert_eq!(c.apply(&mut sample), ReadVia::Local);
        assert_eq!(sample.temperatures.value().map(|t| t.len()), Some(1));
    }

    #[test]
    fn an_inactive_climate_reads_nothing_and_claims_nothing() {
        use crate::helper_first::OffPort;
        let c = HelperClimate::new(Box::new(OffPort), false, BTreeMap::new());
        assert!(!c.due(T0));
        assert!(c.refresh(T0, &UTC).is_none());
        let mut sample = blank();
        let before = sample.clone();
        assert_eq!(c.apply(&mut sample), ReadVia::Local);
        assert_eq!(sample, before);
        assert_eq!(c.cpu_energy().1, ReadVia::Local);
    }

    #[test]
    fn saved_totals_that_are_corrupt_or_absurd_fall_back_to_none() {
        assert!(parse_saved(None).is_empty());
        assert!(parse_saved(Some("not json")).is_empty());
        assert_eq!(parse_saved(Some(r#"{"100": 5.5, "200": -1.0}"#)), BTreeMap::from([(100, 5.5)]));
    }
}
