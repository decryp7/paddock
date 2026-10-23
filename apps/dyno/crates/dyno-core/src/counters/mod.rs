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
}
