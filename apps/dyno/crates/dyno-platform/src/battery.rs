//! The battery through `starship-battery`: charge, power source, wear, cycles and time to empty.

use dyno_core::metrics::{BatterySample, PowerSource, Reading};
use starship_battery::units::ratio::percent;
use starship_battery::units::time::second;
use starship_battery::{Battery, Manager, State};

/// The reason shown on machines without a battery.
pub const NO_BATTERY: &str = "No battery";

/// What one battery reports, in plain numbers (so the mapping is testable without hardware).
#[derive(Debug, Clone, PartialEq)]
pub struct BatteryFacts {
    /// Charge, 0–100.
    pub charge: f32,
    /// The battery's state.
    pub state: State,
    /// Full-charge energy / design energy, in percent.
    pub health: f32,
    /// Charge cycles.
    pub cycles: Option<u32>,
    /// Seconds to empty.
    pub seconds_to_empty: Option<f32>,
}

/// Maps a battery's facts to a sample. Charging, full and paused (charge limit) mean the charger is plugged in.
pub fn to_sample(f: &BatteryFacts) -> BatterySample {
    let source = match f.state {
        State::Discharging | State::Empty => PowerSource::Battery,
        State::Charging | State::Full | State::Paused | State::Unknown => PowerSource::Ac,
    };
    let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
    BatterySample {
        charge: finite(f.charge).clamp(0.0, 100.0),
        source,
        health: finite(f.health).max(0.0),
        cycles: f.cycles,
        seconds_to_empty: match source {
            PowerSource::Battery => f.seconds_to_empty.filter(|s| s.is_finite() && *s > 0.0).map(|s| s as u64),
            PowerSource::Ac => None,
        },
    }
}

fn facts_of(b: &Battery) -> BatteryFacts {
    BatteryFacts {
        charge: b.state_of_charge().get::<percent>(),
        state: b.state(),
        health: b.state_of_health().get::<percent>(),
        cycles: b.cycle_count(),
        seconds_to_empty: b.time_to_empty().map(|t| t.get::<second>()),
    }
}

/// Reads the first battery; refreshes at most every `every` calls (batteries change slowly). The OS handles
/// (`starship-battery` is not `Send` on macOS) live only inside one read, so the reader can move between threads.
pub struct BatteryReader {
    calls: u64,
    every: u64,
    last: Reading<BatterySample>,
}

impl BatteryReader {
    /// A reader that refreshes on every `every`-th call (at least 1).
    pub fn new(every: u64) -> Self {
        BatteryReader {
            calls: 0,
            every: every.max(1),
            last: Reading::unavailable(NO_BATTERY),
        }
    }

    /// The battery reading.
    pub fn read(&mut self) -> Reading<BatterySample> {
        let due = self.calls.is_multiple_of(self.every);
        self.calls = self.calls.wrapping_add(1);
        if due {
            self.last = read_first();
        }
        self.last.clone()
    }
}

fn read_first() -> Reading<BatterySample> {
    let battery = Manager::new()
        .ok()
        .and_then(|m| m.batteries().ok().and_then(|mut it| it.find_map(Result::ok)));
    match battery {
        Some(b) => Reading::Available(to_sample(&facts_of(&b))),
        None => Reading::unavailable(NO_BATTERY),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(state: State) -> BatteryFacts {
        BatteryFacts {
            charge: 55.5,
            state,
            health: 91.0,
            cycles: Some(120),
            seconds_to_empty: Some(3600.4),
        }
    }

    #[test]
    fn maps_states_to_power_sources() {
        let s = to_sample(&facts(State::Discharging));
        assert_eq!(s.source, PowerSource::Battery);
        assert_eq!(s.seconds_to_empty, Some(3600));
        assert_eq!((s.charge, s.health, s.cycles), (55.5, 91.0, Some(120)));
        for st in [State::Charging, State::Full, State::Paused, State::Unknown] {
            let s = to_sample(&facts(st));
            assert_eq!(s.source, PowerSource::Ac);
            assert_eq!(s.seconds_to_empty, None);
        }
        assert_eq!(to_sample(&facts(State::Empty)).source, PowerSource::Battery);
    }

    #[test]
    fn clamps_odd_values() {
        let mut f = facts(State::Discharging);
        f.charge = f32::NAN;
        f.health = -3.0;
        f.seconds_to_empty = Some(f32::INFINITY);
        let s = to_sample(&f);
        assert_eq!((s.charge, s.health, s.seconds_to_empty), (0.0, 0.0, None));
        f.charge = 140.0;
        assert_eq!(to_sample(&f).charge, 100.0);
    }

    #[test]
    fn reader_never_panics_and_caches() {
        let mut r = BatteryReader::new(0);
        let first = r.read();
        let again = r.read();
        // A desktop has no battery; a laptop has one. Either way the reading is stable between calls.
        assert_eq!(first.value().is_some(), again.value().is_some());
        if let Reading::Unavailable(why) = first {
            assert_eq!(why, NO_BATTERY);
        }
    }
}
