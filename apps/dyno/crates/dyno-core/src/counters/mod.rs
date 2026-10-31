//! The performance counters browser (WP-12): what the OS can measure, found by name, read on request, charted with
//! the app's events marked and pinned to the dashboard. Pure: the OS adapters behind
//! [`crate::ports::CounterSource`] enumerate and read; this module parses paths, bounds the catalog, searches it,
//! turns cumulative counts into rates, keeps bounded history and validates the saved sets.
//!
//! One path syntax on every OS, taken from Windows PDH: `\Object(Instance)\Counter` (the instance is optional). On
//! Windows the names are the OS's own, in the user's language; on macOS and Linux they are Dyno's names for the
//! host's statistics (`\Memory\MemAvailable`, `\Network Interface(en0)\Bytes Received`).

mod catalog;
mod path;
mod rate;
mod ring;
mod search;
mod settings;

pub use catalog::{CounterCatalog, CounterInfo, CounterKind, CounterObject, CounterObjectList, CounterObjectSummary, CounterUnit};
pub use path::{CounterError, CounterPath};
pub use rate::{RateTracker, RawCounter, RawValue};
pub use ring::{thin, CounterHistory, CounterPoint, CounterRing, CounterSeries, MAX_POINTS, MAX_SENT};
pub use search::{search, CounterHit};
pub use settings::{CounterSet, CounterSettings, MAX_NAME, MAX_PINNED, MAX_SETS};

use crate::metrics::Reading;
use serde::{Deserialize, Serialize};

/// Longest counter path, in characters.
pub const MAX_PATH: usize = 200;
/// Most paths one read, history or set may name.
pub const MAX_READ: usize = 32;
/// Most objects in a catalog.
pub const MAX_OBJECTS: usize = 400;
/// Most counters kept per object.
pub const MAX_COUNTERS_PER_OBJECT: usize = 300;
/// Most instances kept per object (the rest are counted, not listed).
pub const MAX_INSTANCES: usize = 100;
/// Most search results.
pub const MAX_HITS: usize = 200;

/// One counter read now, as the UI and the CLI show it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterReading {
    /// The counter's path as asked.
    pub path: String,
    /// What the value measures.
    pub unit: CounterUnit,
    /// The value, or why there is none (the counter disappeared, needs a second reading, the OS refused...).
    pub value: Reading<f64>,
    /// 0 to 100 (clamped) for a percent counter that has a value, else none. The engine decides; the UI draws it.
    pub percent: Option<f32>,
    /// Against `COUNTER_THRESHOLDS` for a percent counter, `ok` otherwise.
    pub tone: crate::widgets::dashboard::ValueTone,
}

impl CounterReading {
    /// A reading with its percent and tone worked out.
    pub fn new(path: String, unit: CounterUnit, value: Reading<f64>) -> CounterReading {
        let number = value.value().copied().filter(|v| v.is_finite());
        let percent = if unit == CounterUnit::Percent {
            number.map(|v| v.clamp(0.0, 100.0) as f32)
        } else {
            None
        };
        let tone = match percent {
            Some(p) => crate::widgets::popover_values::COUNTER_THRESHOLDS.tone(Some(f64::from(p))),
            None => crate::widgets::dashboard::ValueTone::Ok,
        };
        CounterReading {
            path,
            unit,
            value,
            percent,
            tone,
        }
    }
}

#[cfg(test)]
mod reading_tests {
    use super::*;
    use crate::widgets::dashboard::ValueTone;

    fn read(unit: CounterUnit, v: f64) -> CounterReading {
        CounterReading::new("\\A\\B".into(), unit, Reading::Available(v))
    }

    #[test]
    fn a_percent_counter_is_clamped_and_toned_by_the_engine() {
        let r = read(CounterUnit::Percent, 120.0);
        assert_eq!((r.percent, r.tone), (Some(100.0), ValueTone::Bad));
        assert_eq!(read(CounterUnit::Percent, -5.0).percent, Some(0.0));
        assert_eq!(read(CounterUnit::Percent, 80.0).tone, ValueTone::Warn);
        assert_eq!(read(CounterUnit::Percent, 10.0).tone, ValueTone::Ok);
    }

    #[test]
    fn other_units_and_missing_values_have_no_percent_and_an_ok_tone() {
        let b = read(CounterUnit::Bytes, 9e12);
        assert_eq!((b.percent, b.tone), (None, ValueTone::Ok));
        let gone = CounterReading::new("\\A\\B".into(), CounterUnit::Percent, Reading::Unavailable("x".into()));
        assert_eq!((gone.percent, gone.tone), (None, ValueTone::Ok));
        assert_eq!(read(CounterUnit::Percent, f64::NAN).percent, None);
    }
}
