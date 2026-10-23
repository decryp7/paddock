//! One counter value as an adapter found it, and how entries become a catalog and readings.

use dyno_core::counters::{CounterCatalog, CounterInfo, CounterKind, CounterObject, CounterPath, CounterUnit, RawCounter, RawValue};
use std::collections::BTreeMap;

/// One counter (of one instance) with its value right now.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The object.
    pub object: String,
    /// The instance, when the object has instances.
    pub instance: Option<String>,
    /// The counter.
    pub counter: String,
    /// What the value measures (a rate when `kind` is cumulative).
    pub unit: CounterUnit,
    /// Whether the value is a level or a running total.
    pub kind: CounterKind,
    /// The value.
    pub value: f64,
}

impl Entry {
    /// An entry.
    pub fn new(object: &str, instance: Option<&str>, counter: &str, unit: CounterUnit, kind: CounterKind, value: f64) -> Entry {
        Entry {
            object: object.into(),
            instance: instance.map(str::to_string),
            counter: counter.into(),
            unit,
            kind,
            value,
        }
    }

    fn path(&self) -> Option<String> {
        CounterPath::new(&self.object, self.instance.as_deref(), &self.counter)
            .ok()
            .map(|p| p.to_string())
    }
}

/// The catalog the entries make up (bounded by the core).
pub fn catalog_of(entries: &[Entry]) -> CounterCatalog {
    let mut objects: BTreeMap<&str, CounterObject> = BTreeMap::new();
    for e in entries {
        let o = objects.entry(e.object.as_str()).or_insert_with(|| CounterObject {
            name: e.object.clone(),
            counters: Vec::new(),
            instances: Vec::new(),
            instances_total: 0,
        });
        if !o.counters.iter().any(|c| c.name == e.counter) {
            o.counters.push(CounterInfo {
                name: e.counter.clone(),
                // A running total is shown as the rate it becomes.
                unit: match e.kind {
                    CounterKind::Gauge => e.unit,
                    CounterKind::Cumulative => e.unit.per_second(),
                },
                kind: e.kind,
                help: None,
            });
        }
        if let Some(i) = &e.instance {
            if !o.instances.contains(i) {
                o.instances.push(i.clone());
            }
        }
    }
    CounterCatalog::bounded(
        objects
            .into_values()
            .map(|mut o| {
                o.instances_total = u32::try_from(o.instances.len()).unwrap_or(u32::MAX);
                o
            })
            .collect(),
    )
}

/// One raw reading per path, in order; a path with no entry fails alone with a sentence saying so.
pub fn read_paths(entries: &[Entry], paths: &[CounterPath]) -> Vec<RawCounter> {
    let by_path: BTreeMap<String, &Entry> = entries.iter().filter_map(|e| e.path().map(|p| (p, e))).collect();
    paths
        .iter()
        .map(|p| {
            let path = p.to_string();
            match by_path.get(&path) {
                Some(e) => RawCounter {
                    path,
                    unit: e.unit,
                    value: Ok(RawValue {
                        value: e.value,
                        kind: e.kind,
                    }),
                },
                None => RawCounter {
                    unit: CounterUnit::Number,
                    value: Err(format!(
                        "{path} is not available on this computer (it may have been removed or renamed)."
                    )),
                    path,
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Entry> {
        vec![
            Entry::new(
                "Network Interface",
                Some("en0"),
                "Bytes Received",
                CounterUnit::Bytes,
                CounterKind::Cumulative,
                100.0,
            ),
            Entry::new(
                "Network Interface",
                Some("lo0"),
                "Bytes Received",
                CounterUnit::Bytes,
                CounterKind::Cumulative,
                7.0,
            ),
            Entry::new("Memory", None, "Free", CounterUnit::Bytes, CounterKind::Gauge, 3.0),
        ]
    }

    #[test]
    fn the_catalog_groups_counters_and_instances_and_shows_cumulative_counters_as_rates() {
        let c = catalog_of(&sample());
        assert_eq!(c.objects.len(), 2);
        let n = c.object("Network Interface").unwrap();
        assert_eq!(n.instances, ["en0", "lo0"]);
        assert_eq!(n.instances_total, 2);
        assert_eq!(n.counters.len(), 1);
        assert_eq!(n.counters[0].unit, CounterUnit::BytesPerSec);
        assert_eq!(c.object("Memory").unwrap().counters[0].unit, CounterUnit::Bytes);
    }

    #[test]
    fn reading_returns_one_result_per_path_and_a_missing_one_fails_alone() {
        let paths: Vec<CounterPath> = [
            "\\Memory\\Free",
            "\\Network Interface(en9)\\Bytes Received",
            "\\Network Interface(lo0)\\Bytes Received",
        ]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
        let r = read_paths(&sample(), &paths);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].value.as_ref().unwrap().value, 3.0);
        assert!(r[1].value.as_ref().unwrap_err().contains("en9"));
        assert_eq!(r[2].value.as_ref().unwrap().kind, CounterKind::Cumulative);
        assert_eq!(r[2].path, "\\Network Interface(lo0)\\Bytes Received");
    }

    #[test]
    fn an_entry_that_cannot_be_a_path_is_never_readable() {
        let bad = vec![Entry::new("a\\b", None, "c", CounterUnit::Count, CounterKind::Gauge, 1.0)];
        assert!(catalog_of(&bad).objects.is_empty());
        assert!(bad[0].path().is_none());
    }
}
