//! Turning what an adapter read into readings: gauges pass through, cumulative counts become a rate per second.

use super::catalog::{CounterKind, CounterUnit};
use super::CounterReading;
use crate::metrics::Reading;
use crate::time::Timestamp;
use std::collections::HashMap;

/// Most counters whose previous value is remembered.
const MAX_TRACKED: usize = 128;

/// What an adapter read for one counter.
#[derive(Debug, Clone, PartialEq)]
pub struct RawValue {
    /// The number.
    pub value: f64,
    /// Whether it is a level or a running total.
    pub kind: CounterKind,
}

/// One counter as an adapter read it.
#[derive(Debug, Clone, PartialEq)]
pub struct RawCounter {
    /// The path asked for.
    pub path: String,
    /// What the value measures before any rate conversion.
    pub unit: CounterUnit,
    /// The value, or why the OS could not give it (the counter is gone, access was refused...).
    pub value: Result<RawValue, String>,
}

impl CounterUnit {
    /// The unit of a rate computed from a running total in this unit.
    pub fn per_second(self) -> CounterUnit {
        match self {
            CounterUnit::Bytes | CounterUnit::BytesPerSec => CounterUnit::BytesPerSec,
            _ => CounterUnit::PerSec,
        }
    }
}

/// Remembers the last value of every cumulative counter to compute rates. Bounded and forgiving: a counter that
/// restarts, repeats a timestamp or disappears yields an unavailable reading with the reason, never a wrong number.
#[derive(Debug, Default)]
pub struct RateTracker {
    last: HashMap<String, (f64, Timestamp)>,
}

impl RateTracker {
    /// An empty tracker.
    pub fn new() -> RateTracker {
        RateTracker::default()
    }

    /// The reading for `raw` read at `at`.
    pub fn apply(&mut self, raw: RawCounter, at: Timestamp) -> CounterReading {
        let RawCounter { path, unit, value } = raw;
        let (unit, value) = match value {
            Err(why) => {
                self.last.remove(&path);
                (unit, Reading::Unavailable(why))
            }
            Ok(RawValue { value, .. }) if !value.is_finite() => {
                (unit, Reading::unavailable("The OS returned a value that is not a number."))
            }
            Ok(RawValue {
                value,
                kind: CounterKind::Gauge,
            }) => (unit, Reading::Available(value)),
            Ok(RawValue {
                value,
                kind: CounterKind::Cumulative,
            }) => (unit.per_second(), self.rate(&path, value, at)),
        };
        CounterReading { path, unit, value }
    }

    fn rate(&mut self, path: &str, value: f64, at: Timestamp) -> Reading<f64> {
        if self.last.len() >= MAX_TRACKED && !self.last.contains_key(path) {
            // Never grow without bound: drop the oldest remembered counter.
            if let Some(oldest) = self.last.iter().min_by_key(|(_, (_, t))| t.0).map(|(k, _)| k.clone()) {
                self.last.remove(&oldest);
            }
        }
        let previous = self.last.insert(path.to_string(), (value, at));
        match previous {
            None => Reading::unavailable("A rate needs two readings; the next one will show it."),
            Some((_, t)) if at.0 <= t.0 => Reading::unavailable("Two readings arrived at the same moment."),
            Some((v, _)) if value < v => Reading::unavailable("The counter restarted; the next reading will show the rate."),
            Some((v, t)) => Reading::Available((value - v) / ((at.0 - t.0) as f64 / 1000.0)),
        }
    }

    /// Forgets every counter not in `keep` (what is no longer watched).
    pub fn retain(&mut self, keep: &[String]) {
        self.last.retain(|k, _| keep.contains(k));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(path: &str, v: f64, kind: CounterKind) -> RawCounter {
        RawCounter {
            path: path.into(),
            unit: CounterUnit::Bytes,
            value: Ok(RawValue { value: v, kind }),
        }
    }

    #[test]
    fn gauges_pass_through_with_their_unit() {
        let mut t = RateTracker::new();
        let r = t.apply(raw("\\a\\b", 42.0, CounterKind::Gauge), Timestamp(0));
        assert_eq!((r.value, r.unit), (Reading::Available(42.0), CounterUnit::Bytes));
    }

    #[test]
    fn cumulative_counters_need_two_readings_and_then_give_a_rate() {
        let mut t = RateTracker::new();
        let first = t.apply(raw("\\a\\b", 1000.0, CounterKind::Cumulative), Timestamp(0));
        assert!(matches!(first.value, Reading::Unavailable(ref w) if w.contains("two readings")));
        assert_eq!(first.unit, CounterUnit::BytesPerSec);
        let second = t.apply(raw("\\a\\b", 3000.0, CounterKind::Cumulative), Timestamp(2000));
        assert_eq!(second.value, Reading::Available(1000.0));
    }

    #[test]
    fn a_restart_or_repeated_timestamp_is_unavailable_then_recovers() {
        let mut t = RateTracker::new();
        t.apply(raw("p", 500.0, CounterKind::Cumulative), Timestamp(1000));
        assert!(matches!(
            t.apply(raw("p", 10.0, CounterKind::Cumulative), Timestamp(2000)).value,
            Reading::Unavailable(ref w) if w.contains("restarted")
        ));
        assert!(matches!(
            t.apply(raw("p", 20.0, CounterKind::Cumulative), Timestamp(2000)).value,
            Reading::Unavailable(ref w) if w.contains("same moment")
        ));
        assert_eq!(
            t.apply(raw("p", 30.0, CounterKind::Cumulative), Timestamp(3000)).value,
            Reading::Available(10.0)
        );
    }

    #[test]
    fn a_counter_that_disappears_says_why_and_is_forgotten() {
        let mut t = RateTracker::new();
        t.apply(raw("p", 1.0, CounterKind::Cumulative), Timestamp(0));
        let gone = t.apply(
            RawCounter {
                path: "p".into(),
                unit: CounterUnit::Count,
                value: Err("The interface en9 is gone.".into()),
            },
            Timestamp(1000),
        );
        assert_eq!(gone.value, Reading::Unavailable("The interface en9 is gone.".into()));
        assert!(t.last.is_empty());
    }

    #[test]
    fn non_finite_values_are_unavailable() {
        let mut t = RateTracker::new();
        assert!(matches!(
            t.apply(raw("p", f64::NAN, CounterKind::Gauge), Timestamp(0)).value,
            Reading::Unavailable(_)
        ));
    }

    #[test]
    fn memory_is_bounded_and_retain_forgets_unwatched_counters() {
        let mut t = RateTracker::new();
        for i in 0..MAX_TRACKED + 10 {
            t.apply(raw(&format!("p{i}"), 1.0, CounterKind::Cumulative), Timestamp(i as i64));
        }
        assert_eq!(t.last.len(), MAX_TRACKED);
        t.retain(&["p100".to_string()]);
        assert_eq!(t.last.len(), 1);
    }

    #[test]
    fn units_become_per_second() {
        assert_eq!(CounterUnit::Bytes.per_second(), CounterUnit::BytesPerSec);
        assert_eq!(CounterUnit::Count.per_second(), CounterUnit::PerSec);
        assert_eq!(CounterUnit::Number.per_second(), CounterUnit::PerSec);
    }
}
