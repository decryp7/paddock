//! Finding a counter by typing a few words.

use super::catalog::{CounterCatalog, CounterObject, CounterUnit};
use super::{CounterPath, MAX_HITS};
use serde::{Deserialize, Serialize};

/// One match: a counter (of one instance, when the object has instances).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterHit {
    /// The path to read.
    pub path: String,
    /// The object.
    pub object: String,
    /// The counter.
    pub counter: String,
    /// The instance.
    pub instance: Option<String>,
    /// What it measures.
    pub unit: CounterUnit,
}

/// How well a counter matches (lower is better), or `None` when some word is missing. Every word of the query must
/// appear in the object, counter or instance name, in any order, ignoring case.
fn score(words: &[String], object: &str, counter: &str, instance: Option<&str>) -> Option<u32> {
    let (o, c) = (object.to_lowercase(), counter.to_lowercase());
    let i = instance.map(str::to_lowercase).unwrap_or_default();
    let mut total = 0;
    for w in words {
        total += if c == *w {
            0
        } else if c.starts_with(w.as_str()) {
            1
        } else if c.contains(w.as_str()) {
            2
        } else if o.starts_with(w.as_str()) {
            3
        } else if o.contains(w.as_str()) {
            4
        } else if i.contains(w.as_str()) {
            5
        } else {
            return None;
        };
    }
    Some(total)
}

/// The best matches for `query`, best first, at most `limit` (and never more than [`MAX_HITS`]). An empty query
/// matches nothing: the browser lists the objects instead.
pub fn search(catalog: &CounterCatalog, query: &str, limit: usize) -> Vec<CounterHit> {
    let words: Vec<String> = query.split_whitespace().take(8).map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(u32, CounterHit)> = Vec::new();
    for o in &catalog.objects {
        for c in &o.counters {
            for instance in instances_of(o) {
                let Some(s) = score(&words, &o.name, &c.name, instance) else {
                    continue;
                };
                let Ok(path) = CounterPath::new(&o.name, instance, &c.name) else {
                    continue;
                };
                found.push((
                    s,
                    CounterHit {
                        path: path.to_string(),
                        object: o.name.clone(),
                        counter: c.name.clone(),
                        instance: instance.map(str::to_string),
                        unit: c.unit,
                    },
                ));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.path.cmp(&b.1.path)));
    found.into_iter().take(limit.min(MAX_HITS)).map(|(_, h)| h).collect()
}

fn instances_of(o: &CounterObject) -> Vec<Option<&str>> {
    if o.instances.is_empty() {
        vec![None]
    } else {
        o.instances.iter().map(|i| Some(i.as_str())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::catalog::fixtures::sample;
    use super::*;

    fn paths(q: &str, limit: usize) -> Vec<String> {
        search(&sample(), q, limit).into_iter().map(|h| h.path).collect()
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        assert!(paths("   ", 10).is_empty());
    }

    #[test]
    fn every_word_must_match_in_any_order_ignoring_case() {
        assert_eq!(paths("MEMORY pages", 10), ["\\Memory\\Pages/sec"]);
        assert_eq!(paths("pages memory", 10), ["\\Memory\\Pages/sec"]);
        assert!(paths("pages nothing", 10).is_empty());
    }

    #[test]
    fn one_hit_per_instance_and_the_instance_is_searchable() {
        assert_eq!(
            paths("bytes received", 10),
            [
                "\\Network Interface(en0)\\Bytes Received/sec",
                "\\Network Interface(lo0)\\Bytes Received/sec"
            ]
        );
        assert_eq!(paths("received lo0", 10), ["\\Network Interface(lo0)\\Bytes Received/sec"]);
    }

    #[test]
    fn counter_name_matches_rank_before_object_matches() {
        let hits = search(&sample(), "memory", 10);
        // "Memory" is an object here (both counters match through it); "Available MBytes" has no "memory" itself.
        assert_eq!(hits.len(), 2);
        let c = search(&sample(), "processor", 10);
        assert_eq!(c[0].counter, "% Processor Time");
        assert_eq!(c[0].instance.as_deref(), Some("0"));
        assert_eq!(c[0].unit, CounterUnit::Percent);
    }

    #[test]
    fn results_are_capped() {
        assert_eq!(paths("e", 2).len(), 2);
        assert_eq!(paths("e", 0).len(), 0);
        let big = CounterCatalog::bounded(vec![super::super::catalog::fixtures::object(
            "o",
            &(0..300).map(|_| ("x", CounterUnit::Count)).collect::<Vec<_>>(),
            &(0..100)
                .map(|i| Box::leak(format!("i{i}").into_boxed_str()) as &str)
                .collect::<Vec<_>>(),
        )]);
        assert!(search(&big, "x", 10_000).len() <= MAX_HITS);
    }
}
