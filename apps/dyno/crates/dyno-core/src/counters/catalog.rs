//! The catalog of counters an OS offers, bounded so a machine with thousands of instances cannot flood the UI.

use super::{CounterPath, MAX_COUNTERS_PER_OBJECT, MAX_INSTANCES, MAX_OBJECTS};
use serde::{Deserialize, Serialize};

/// What a counter's value measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum CounterUnit {
    /// 0 to 100.
    Percent,
    /// A size in bytes.
    Bytes,
    /// Bytes per second.
    BytesPerSec,
    /// Events per second.
    PerSec,
    /// A plain number of things.
    Count,
    /// Milliseconds.
    Milliseconds,
    /// Seconds.
    Seconds,
    /// No unit is known.
    #[default]
    Number,
}

impl CounterUnit {
    /// The unit a counter's name suggests (for adapters whose OS gives names but no units, such as Windows PDH).
    pub fn guess(counter: &str) -> CounterUnit {
        let n = counter.to_lowercase();
        if n.contains('%') || n.contains("percent") {
            CounterUnit::Percent
        } else if n.contains("bytes") && n.contains("/sec") {
            CounterUnit::BytesPerSec
        } else if n.contains("/sec") {
            CounterUnit::PerSec
        } else if n.contains("mbytes") || n.contains("kbytes") {
            CounterUnit::Number
        } else if n.ends_with("bytes") || n.contains("bytes ") || n.contains("(bytes)") {
            CounterUnit::Bytes
        } else if n.contains("(ms)") || n.contains("milliseconds") {
            CounterUnit::Milliseconds
        } else if n.contains("seconds") || n.contains("(s)") || n.contains("(sec)") {
            CounterUnit::Seconds
        } else if n.contains("count")
            || n.contains("length")
            || n.starts_with("number of")
            || n.contains("threads")
            || n.contains("handles")
        {
            CounterUnit::Count
        } else {
            CounterUnit::Number
        }
    }
}

/// How a counter's raw value behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CounterKind {
    /// The value is the reading (a size, a level, a percentage, a rate the OS already computed).
    Gauge,
    /// The value only grows; the reading is its change per second between two looks.
    Cumulative,
}

/// One counter of an object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterInfo {
    /// Its name.
    pub name: String,
    /// What it measures (after any rate conversion).
    pub unit: CounterUnit,
    /// How the raw value behaves.
    pub kind: CounterKind,
    /// A sentence about it, when the OS gives one.
    pub help: Option<String>,
}

/// An object (a group of counters, such as `Memory` or `Network Interface`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterObject {
    /// Its name.
    pub name: String,
    /// Its counters.
    pub counters: Vec<CounterInfo>,
    /// Its instances (empty when the object has none), at most [`MAX_INSTANCES`].
    pub instances: Vec<String>,
    /// How many instances the OS has (more than `instances` when the list was cut).
    pub instances_total: u32,
}

/// An object in the browser's first level: its name and how much is inside (the counters load on opening it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterObjectSummary {
    /// Its name.
    pub name: String,
    /// How many counters it has.
    pub counters: u32,
    /// How many instances the OS has.
    pub instances_total: u32,
}

/// The browser's first level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CounterObjectList {
    /// The objects, sorted by name.
    pub objects: Vec<CounterObjectSummary>,
    /// Whether anything was left out to keep the catalog small.
    pub truncated: bool,
}

/// Everything the OS offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CounterCatalog {
    /// The objects, sorted by name.
    pub objects: Vec<CounterObject>,
    /// Whether objects, counters or instances were left out to keep the catalog small.
    pub truncated: bool,
}

impl CounterCatalog {
    /// Builds a catalog from what an adapter found: sorted, deduplicated, with every limit applied.
    pub fn bounded(mut objects: Vec<CounterObject>) -> CounterCatalog {
        let mut truncated = false;
        objects.retain(|o| !o.name.trim().is_empty() && !o.name.contains('\\'));
        objects.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name)));
        objects.dedup_by(|a, b| a.name == b.name);
        if objects.len() > MAX_OBJECTS {
            objects.truncate(MAX_OBJECTS);
            truncated = true;
        }
        for o in &mut objects {
            o.counters.retain(|c| !c.name.trim().is_empty() && !c.name.contains('\\'));
            o.counters.sort_by(|a, b| a.name.cmp(&b.name));
            o.counters.dedup_by(|a, b| a.name == b.name);
            if o.counters.len() > MAX_COUNTERS_PER_OBJECT {
                o.counters.truncate(MAX_COUNTERS_PER_OBJECT);
                truncated = true;
            }
            o.instances.retain(|i| !i.trim().is_empty() && !i.contains('\\'));
            o.instances.sort();
            o.instances.dedup();
            o.instances_total = o.instances_total.max(u32::try_from(o.instances.len()).unwrap_or(u32::MAX));
            if o.instances.len() > MAX_INSTANCES {
                o.instances.truncate(MAX_INSTANCES);
                truncated = true;
            }
        }
        CounterCatalog { objects, truncated }
    }

    /// The first level of the browser: names and sizes only, so the reply stays small however big the catalog is.
    pub fn summaries(&self) -> CounterObjectList {
        CounterObjectList {
            objects: self
                .objects
                .iter()
                .map(|o| CounterObjectSummary {
                    name: o.name.clone(),
                    counters: u32::try_from(o.counters.len()).unwrap_or(u32::MAX),
                    instances_total: o.instances_total,
                })
                .collect(),
            truncated: self.truncated,
        }
    }

    /// One object with its counters and instances.
    pub fn object(&self, name: &str) -> Option<&CounterObject> {
        self.objects.iter().find(|o| o.name == name)
    }

    /// The counter a path names, when the catalog lists it (the instance is not checked: instances come and go).
    pub fn find(&self, path: &CounterPath) -> Option<(&CounterObject, &CounterInfo)> {
        let o = self.objects.iter().find(|o| o.name == path.object())?;
        o.counters.iter().find(|c| c.name == path.counter()).map(|c| (o, c))
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    pub fn info(name: &str, unit: CounterUnit) -> CounterInfo {
        CounterInfo {
            name: name.into(),
            unit,
            kind: CounterKind::Gauge,
            help: None,
        }
    }

    pub fn object(name: &str, counters: &[(&str, CounterUnit)], instances: &[&str]) -> CounterObject {
        CounterObject {
            name: name.into(),
            counters: counters.iter().map(|(n, u)| info(n, *u)).collect(),
            instances: instances.iter().map(|s| s.to_string()).collect(),
            instances_total: 0,
        }
    }

    pub fn sample() -> CounterCatalog {
        CounterCatalog::bounded(vec![
            object(
                "Network Interface",
                &[
                    ("Bytes Received/sec", CounterUnit::BytesPerSec),
                    ("Bytes Sent/sec", CounterUnit::BytesPerSec),
                ],
                &["en0", "lo0"],
            ),
            object(
                "Memory",
                &[("Available MBytes", CounterUnit::Bytes), ("Pages/sec", CounterUnit::PerSec)],
                &[],
            ),
            object("Processor", &[("% Processor Time", CounterUnit::Percent)], &["_Total", "0", "1"]),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn it_sorts_dedups_and_drops_unnamed_entries() {
        let c = CounterCatalog::bounded(vec![
            object(
                "b",
                &[
                    ("z", CounterUnit::Count),
                    ("a", CounterUnit::Count),
                    ("a", CounterUnit::Count),
                    (" ", CounterUnit::Count),
                ],
                &["y", "x", "x", ""],
            ),
            object("A", &[], &[]),
            object("b", &[], &[]),
            object("", &[], &[]),
            object("bad\\name", &[], &[]),
        ]);
        let names: Vec<_> = c.objects.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["A", "b"]);
        let b = &c.objects[1];
        assert_eq!(b.counters.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a", "z"]);
        assert_eq!(b.instances, ["x", "y"]);
        assert_eq!(b.instances_total, 2);
        assert!(!c.truncated);
    }

    #[test]
    fn every_limit_marks_the_catalog_truncated() {
        let many_objects = (0..MAX_OBJECTS + 5).map(|i| object(&format!("o{i:04}"), &[], &[])).collect();
        let c = CounterCatalog::bounded(many_objects);
        assert_eq!(c.objects.len(), MAX_OBJECTS);
        assert!(c.truncated);

        let counters: Vec<(String, CounterUnit)> = (0..MAX_COUNTERS_PER_OBJECT + 1)
            .map(|i| (format!("c{i:04}"), CounterUnit::Count))
            .collect();
        let refs: Vec<(&str, CounterUnit)> = counters.iter().map(|(n, u)| (n.as_str(), *u)).collect();
        let c = CounterCatalog::bounded(vec![object("o", &refs, &[])]);
        assert_eq!(c.objects[0].counters.len(), MAX_COUNTERS_PER_OBJECT);
        assert!(c.truncated);

        let inst: Vec<String> = (0..MAX_INSTANCES + 7).map(|i| format!("i{i:04}")).collect();
        let refs: Vec<&str> = inst.iter().map(String::as_str).collect();
        let c = CounterCatalog::bounded(vec![object("o", &[], &refs)]);
        assert_eq!(c.objects[0].instances.len(), MAX_INSTANCES);
        assert_eq!(c.objects[0].instances_total as usize, MAX_INSTANCES + 7);
        assert!(c.truncated);
    }

    #[test]
    fn units_are_guessed_from_the_counter_name() {
        let g = CounterUnit::guess;
        assert_eq!(g("% Processor Time"), CounterUnit::Percent);
        assert_eq!(g("Bytes Total/sec"), CounterUnit::BytesPerSec);
        assert_eq!(g("Pages/sec"), CounterUnit::PerSec);
        assert_eq!(g("Available MBytes"), CounterUnit::Number);
        assert_eq!(g("Committed Bytes"), CounterUnit::Bytes);
        assert_eq!(g("Idle Time (ms)"), CounterUnit::Milliseconds);
        assert_eq!(g("Elapsed Seconds"), CounterUnit::Seconds);
        assert_eq!(g("Current Disk Queue Length"), CounterUnit::Count);
        assert_eq!(g("Thread Count"), CounterUnit::Count);
        assert_eq!(g("Frobnication"), CounterUnit::Number);
    }

    #[test]
    fn summaries_and_objects_are_looked_up_by_name() {
        let c = sample();
        let s = c.summaries();
        assert_eq!(s.objects.len(), 3);
        assert_eq!(s.objects[0].name, "Memory");
        assert_eq!((s.objects[0].counters, s.objects[2].instances_total), (2, 3));
        assert!(c.object("Memory").is_some() && c.object("Nope").is_none());
    }

    #[test]
    fn find_matches_object_and_counter_but_not_the_instance() {
        let c = sample();
        let p: CounterPath = "\\Processor(7)\\% Processor Time".parse().unwrap();
        assert_eq!(c.find(&p).unwrap().1.unit, CounterUnit::Percent);
        assert!(c.find(&"\\Processor\\Nope".parse().unwrap()).is_none());
        assert!(c.find(&"\\Nope\\x".parse().unwrap()).is_none());
    }
}
