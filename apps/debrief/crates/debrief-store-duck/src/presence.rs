//! Per-source counts behind the automatic instance key (ADR 0020), kept between page queries.
//!
//! The key needs, per ready source, its event count and how many of its events carry each candidate field. A ready
//! source's events do not change, so these counts are computed once per source and candidate: after an import only the
//! new sources are scanned (and the older sources only for a candidate field they were not counted for), instead of
//! every event of the store (WP-1: the scan was 1.2 s at 56M events on the first page after each change). The cache is
//! exact, never stale: an entry is used only for the event total the `sources` table states, it is dropped when its
//! source is removed and whenever the profile or the parsers change.

use std::collections::BTreeMap;

/// One source's counts: the `sources.events` value they were computed for, the events counted, and the events with a
/// non-empty value per candidate column (the column's name and SQL expression).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceCounts {
    pub(crate) declared_events: u64,
    pub(crate) events: u64,
    pub(crate) present: BTreeMap<String, u64>,
}

/// What one scan statement must count: these sources, for these columns (indexes into the candidate list).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ScanGroup {
    pub(crate) sources: Vec<u32>,
    pub(crate) columns: Vec<usize>,
}

/// The counts of the sources scanned so far.
#[derive(Default, Debug)]
pub(crate) struct PresenceCache {
    sources: BTreeMap<u32, SourceCounts>,
    /// Source-column counts computed since the store opened (tests and logs).
    scans: u64,
}

impl PresenceCache {
    /// What is still to count for the ready sources (`(id, sources.events)`) and the candidate `columns`: sources
    /// without a valid entry need every column, the others only the columns they lack. Sources with the same need
    /// share a group (one statement). Sources without events need nothing.
    pub(crate) fn missing(&self, ready: &[(u32, u64)], columns: &[String]) -> Vec<ScanGroup> {
        let mut groups: BTreeMap<Vec<usize>, Vec<u32>> = BTreeMap::new();
        for (id, declared) in ready.iter().filter(|(_, declared)| *declared > 0) {
            let valid = self.sources.get(id).filter(|c| c.declared_events == *declared);
            let need: Vec<usize> = (0..columns.len())
                .filter(|k| valid.is_none_or(|c| !c.present.contains_key(&columns[*k])))
                .collect();
            if !need.is_empty() {
                groups.entry(need).or_default().push(*id);
            }
        }
        groups
            .into_iter()
            .map(|(columns, sources)| ScanGroup { sources, columns })
            .collect()
    }

    /// Stores counts for `id`; columns counted before stay when the event total is the same.
    pub(crate) fn insert(&mut self, id: u32, counts: SourceCounts) {
        self.scans += counts.present.len() as u64;
        match self.sources.get_mut(&id) {
            Some(old) if old.declared_events == counts.declared_events => old.present.extend(counts.present),
            _ => {
                self.sources.insert(id, counts);
            }
        }
    }

    pub(crate) fn scans(&self) -> u64 {
        self.scans
    }

    /// Drops the entries of removed sources (a later source may reuse an id).
    pub(crate) fn forget(&mut self, ids: &[u32]) {
        for id in ids {
            self.sources.remove(id);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.sources.clear();
    }

    /// The counts of the ready sources that have scanned events, by source id; sources without an entry (no events)
    /// are left out, as the grouped count leaves them out.
    pub(crate) fn rows(&self, ready: &[(u32, u64)]) -> Vec<(u32, &SourceCounts)> {
        let mut out: Vec<(u32, &SourceCounts)> = ready
            .iter()
            .filter_map(|(id, declared)| {
                self.sources
                    .get(id)
                    .filter(|c| c.declared_events == *declared && c.events > 0)
                    .map(|c| (*id, c))
            })
            .collect();
        out.sort_by_key(|(id, _)| *id);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    fn counts(declared: u64, columns: &[&str]) -> SourceCounts {
        SourceCounts {
            declared_events: declared,
            events: declared,
            present: columns.iter().map(|c| (c.to_string(), 1)).collect(),
        }
    }

    fn group(sources: &[u32], columns: &[usize]) -> ScanGroup {
        ScanGroup {
            sources: sources.to_vec(),
            columns: columns.to_vec(),
        }
    }

    #[test]
    fn only_what_is_not_counted_yet_is_missing() {
        let mut c = PresenceCache::default();
        let a = cols(&["a"]);
        assert_eq!(
            c.missing(&[(1, 10), (2, 5), (3, 0)], &a),
            vec![group(&[1, 2], &[0])],
            "a source without events needs no scan"
        );
        c.insert(1, counts(10, &["a"]));
        c.insert(2, counts(5, &["a"]));
        assert!(c.missing(&[(1, 10), (2, 5), (3, 0)], &a).is_empty());
        assert_eq!(
            c.missing(&[(1, 10), (2, 5), (4, 7)], &a),
            vec![group(&[4], &[0])],
            "a new source is the only one scanned"
        );
        assert_eq!(
            c.missing(&[(1, 11)], &a),
            vec![group(&[1], &[0])],
            "a source whose event total changed is scanned again"
        );
    }

    #[test]
    fn a_new_candidate_is_counted_for_old_sources_and_all_columns_for_new_ones() {
        let mut c = PresenceCache::default();
        c.insert(1, counts(10, &["a"]));
        let ab = cols(&["a", "b"]);
        assert_eq!(c.missing(&[(1, 10), (2, 3)], &ab), vec![group(&[2], &[0, 1]), group(&[1], &[1])]);
        c.insert(1, counts(10, &["b"]));
        assert_eq!(
            c.rows(&[(1, 10)])[0].1.present.len(),
            2,
            "the older column is kept when another is added"
        );
    }

    #[test]
    fn a_clear_or_a_forgotten_source_is_counted_again() {
        let mut c = PresenceCache::default();
        let a = cols(&["a"]);
        c.insert(1, counts(10, &["a"]));
        c.insert(2, counts(10, &["a"]));
        c.forget(&[2]);
        assert_eq!(
            c.missing(&[(1, 10), (2, 10)], &a),
            vec![group(&[2], &[0])],
            "same event total, but another source"
        );
        c.clear();
        assert_eq!(c.missing(&[(1, 10)], &a), vec![group(&[1], &[0])]);
    }

    #[test]
    fn rows_are_ordered_by_source_and_skip_sources_without_events() {
        let mut c = PresenceCache::default();
        c.insert(3, counts(2, &["a"]));
        c.insert(1, counts(4, &["a"]));
        let ids: Vec<u32> = c.rows(&[(3, 2), (1, 4), (2, 0)]).iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, vec![1, 3]);
    }
}
