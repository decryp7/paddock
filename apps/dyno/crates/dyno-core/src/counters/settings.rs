//! What the user keeps of the counters browser: pinned counters and saved sets. Parsed and bounded at the edge, so a
//! damaged or oversized settings file can never give the UI something it cannot draw.

use super::{CounterPath, MAX_READ};
use serde::{Deserialize, Serialize};

/// Most pinned counters.
pub const MAX_PINNED: usize = 16;
/// Most saved sets.
pub const MAX_SETS: usize = 12;
/// Longest set name, in characters.
pub const MAX_NAME: usize = 48;

/// A named group of counters to chart together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterSet {
    /// Its name (unique among the sets).
    pub name: String,
    /// Its counters' paths.
    pub paths: Vec<String>,
}

/// Pinned counters and saved sets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CounterSettings {
    /// Counters kept in view (and sampled in the background so their history is there when the page opens).
    pub pinned: Vec<String>,
    /// Saved sets.
    pub sets: Vec<CounterSet>,
}

/// Keeps the valid, distinct paths in their canonical spelling, at most `max`.
fn clean(paths: Vec<String>, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in paths {
        if let Ok(parsed) = p.parse::<CounterPath>() {
            let canonical = parsed.to_string();
            if !out.contains(&canonical) {
                out.push(canonical);
            }
        }
    }
    out.truncate(max);
    out
}

impl CounterSettings {
    /// The settings with every entry valid and within its limit; sets without a usable name or counter are dropped.
    pub fn normalised(self) -> CounterSettings {
        let mut sets: Vec<CounterSet> = Vec::new();
        for s in self.sets {
            let name = s.name.trim().to_string();
            let paths = clean(s.paths, MAX_READ.min(MAX_PINNED));
            if name.is_empty() || name.chars().count() > MAX_NAME || paths.is_empty() || sets.iter().any(|x| x.name == name) {
                continue;
            }
            sets.push(CounterSet { name, paths });
        }
        sets.truncate(MAX_SETS);
        CounterSettings {
            pinned: clean(self.pinned, MAX_PINNED),
            sets,
        }
    }

    /// Every distinct path the user keeps an eye on (pinned ones), for the background sampler.
    pub fn watched(&self) -> &[String] {
        &self.pinned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(name: &str, paths: &[&str]) -> CounterSet {
        CounterSet {
            name: name.into(),
            paths: paths.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn pinned_paths_are_validated_canonical_distinct_and_capped() {
        let mut pinned: Vec<String> = vec![
            " \\Memory\\Pages/sec ".into(),
            "\\Memory\\Pages/sec".into(),
            "nonsense".into(),
            "".into(),
        ];
        pinned.extend((0..40).map(|i| format!("\\Object\\Counter {i}")));
        let n = CounterSettings { pinned, sets: vec![] }.normalised();
        assert_eq!(n.pinned[0], "\\Memory\\Pages/sec");
        assert_eq!(n.pinned.len(), MAX_PINNED);
        assert_eq!(n.watched(), n.pinned.as_slice());
    }

    #[test]
    fn sets_need_a_unique_name_and_a_valid_counter() {
        let n = CounterSettings {
            pinned: vec![],
            sets: vec![
                set(" Disk ", &["\\PhysicalDisk(0)\\Disk Reads/sec", "bad"]),
                set("Disk", &["\\a\\b"]),
                set("", &["\\a\\b"]),
                set("Empty", &["bad"]),
                set(&"x".repeat(MAX_NAME + 1), &["\\a\\b"]),
            ],
        }
        .normalised();
        assert_eq!(n.sets, vec![set("Disk", &["\\PhysicalDisk(0)\\Disk Reads/sec"])]);
    }

    #[test]
    fn at_most_the_limit_of_sets_survive() {
        let sets = (0..MAX_SETS + 4).map(|i| set(&format!("s{i}"), &["\\a\\b"])).collect();
        assert_eq!(CounterSettings { pinned: vec![], sets }.normalised().sets.len(), MAX_SETS);
    }

    #[test]
    fn missing_keys_default_and_the_json_is_camel_case() {
        let s: CounterSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, CounterSettings::default());
        let s: CounterSettings = serde_json::from_str(r#"{"pinned":["\\a\\b"],"sets":[{"name":"n","paths":["\\a\\b"]}]}"#).unwrap();
        assert_eq!(s.sets[0].name, "n");
    }
}
