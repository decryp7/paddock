//! The events-grid page query reads only what its time range needs (WP-1, Debrief 1.2 foundation).
//!
//! Regression: the time range compiled to `coalesce(e.ts >= F AND e.ts <= T, FALSE)`, which DuckDB cannot push into the
//! Parquet scan, so a 45 s window of a 56M-event store read every segment's `ts` column (384 MB, 480 ms for the page)
//! instead of the row groups the segments' min/max statistics select (7.8 MB, 55 ms). The test reads DuckDB's own plan of
//! the page's `WHERE`, so it asserts the mechanism (the scan carries the time filter) and not a timing (ER-12).

use super::*;
use debrief_core::ports::{Filter, Ingest, Query};
use debrief_core::{PluginRegistry, Profile};

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("debrief-page-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn samples() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples");
    vec![root.join("serilog-text"), root.join("serilog-clef")]
}

/// DuckDB's physical plan of the statement the page runs to count the matches of `query`.
fn count_plan(store: &DuckStore, query: &Filter) -> String {
    let conn = store.inner.conn().unwrap();
    let ctx = store.inner.ctx(&conn).unwrap();
    let (f, _) = store.inner.compile(&ctx, query).unwrap();
    assert!(f.exact, "SQL expresses this filter completely");
    let sql = format!("EXPLAIN SELECT count(*) FROM {} WHERE {}", sql::FROM, f.all_sql());
    let mut stmt = conn.prepare(&sql).unwrap();
    let rows = stmt.query_map([], |r| r.get::<_, String>(1)).unwrap();
    rows.map(Result::unwrap).collect::<Vec<_>>().join("\n")
}

#[test]
fn a_time_range_is_pushed_into_the_segment_scan() {
    let dir = Dir::new("time-pushdown");
    let store = DuckStore::create(&dir.0, Profile::default_profile(), PluginRegistry::new()).unwrap();
    store.open(&samples()).unwrap().wait().unwrap();
    let summary = store.summary().unwrap();
    let (from, to) = (summary.from.unwrap(), summary.to.unwrap());
    let plan = count_plan(
        &store,
        &Filter {
            from: Some(from + (to - from) / 3),
            to: Some(from + (to - from) / 2),
            ..Filter::default()
        },
    );
    // The scan's box in the plan: from its label to its closing line (labels are matched ignoring case).
    let lines: Vec<&str> = plan.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.to_ascii_lowercase().contains("read parquet") || l.to_ascii_lowercase().contains("read_parquet"))
        .unwrap_or_else(|| panic!("the plan has no Parquet scan, so the probe cannot see the filter:\n{plan}"));
    let scan = lines[start..]
        .iter()
        .take_while(|l| !l.contains('╰'))
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(scan.contains("ts >="), "the scan has no time filter at all:\n{plan}");
    assert!(
        !scan.to_ascii_lowercase().contains("coalesce"),
        "the time range is wrapped in an expression (COALESCE), so DuckDB filters rows instead of pruning row groups by the \
         segments' min/max statistics and reads every segment's ts column:\n{plan}"
    );
}

/// The instance key's per-source counts are scanned once per source (WP-1: the grouped count over every event was
/// 1.2 s at 56M events on the first page after each change). Another import scans only its own sources, and the
/// key equals the one a fresh store computes from scratch.
#[test]
fn the_instance_key_scans_only_new_sources_and_equals_a_full_recount() {
    let dir = Dir::new("key-incremental");
    let store = DuckStore::create(&dir.0, Profile::default_profile(), PluginRegistry::new()).unwrap();
    let scans = |s: &DuckStore| lock(&s.inner.presence).scans();
    let key = |s: &DuckStore| {
        let conn = s.inner.conn().unwrap();
        let ctx = s.inner.ctx(&conn).unwrap();
        (ctx.default_group_by.clone(), format!("{:?}", ctx.key_scopes))
    };
    store.open(&samples()[..1]).unwrap().wait().unwrap();
    let _ = key(&store);
    let after_first = scans(&store);
    assert!(after_first > 0, "the first page counts the sources it has");
    let _ = key(&store);
    assert_eq!(scans(&store), after_first, "the same sources are not counted twice");
    store.open(&samples()[1..]).unwrap().wait().unwrap();
    let both = key(&store);
    let fresh_dir = Dir::new("key-fresh");
    let fresh = DuckStore::create(&fresh_dir.0, Profile::default_profile(), PluginRegistry::new()).unwrap();
    fresh.open(&samples()).unwrap().wait().unwrap();
    let from_scratch = key(&fresh);
    assert!(scans(&store) > after_first, "the second import's sources were counted");
    assert_eq!(
        scans(&store),
        scans(&fresh),
        "every source and candidate was counted exactly once, as often as a store that counts them in one go"
    );
    assert_eq!(
        from_scratch, both,
        "the incremental key equals the key of a store counted from scratch"
    );
}

/// Removing a source drops its counts: a source that later takes the same id is counted again.
#[test]
fn a_removed_sources_counts_are_dropped() {
    let dir = Dir::new("key-forget");
    let store = DuckStore::create(&dir.0, Profile::default_profile(), PluginRegistry::new()).unwrap();
    store.open(&samples()).unwrap().wait().unwrap();
    {
        let conn = store.inner.conn().unwrap();
        store.inner.ctx(&conn).unwrap();
    }
    let before = lock(&store.inner.presence).scans();
    store.inner.forget_events().unwrap();
    store.open(&samples()).unwrap().wait().unwrap();
    {
        let conn = store.inner.conn().unwrap();
        store.inner.ctx(&conn).unwrap();
    }
    assert_eq!(
        lock(&store.inner.presence).scans(),
        2 * before,
        "after forgetting every event the sources are counted again"
    );
}

/// The import job prepares the instance key's counts itself, so the first page after it counts nothing (WP-1: the
/// first page after an import took 1.3 s at 56M events).
#[test]
fn the_import_prepares_the_instance_key_before_the_first_page() {
    let dir = Dir::new("key-prepared");
    let store = DuckStore::create(&dir.0, Profile::default_profile(), PluginRegistry::new()).unwrap();
    store.open(&samples()).unwrap().wait().unwrap();
    let after_import = lock(&store.inner.presence).scans();
    assert!(after_import > 0, "the import counted the sources before any page asked");
    let conn = store.inner.conn().unwrap();
    store.inner.ctx(&conn).unwrap();
    assert_eq!(lock(&store.inner.presence).scans(), after_import, "the first page counted nothing");
}
