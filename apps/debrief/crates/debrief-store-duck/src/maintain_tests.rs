//! The tail of an import (`Inner::maintain`): a cancelled import skips compaction but still orders the events, and
//! the store stays consistent and queryable. Compaction's own Cancel and progress are covered here too.

use super::*;
use debrief_core::ports::{Ingest, PageQuery, Positions, Query};
use debrief_core::ql::{QueryControl, QueryStage};
use debrief_core::{PluginRegistry, Profile};

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("debrief-maintain-{tag}-{}", std::process::id()));
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

fn segment_files(conn: &Connection) -> Vec<String> {
    let mut stmt = conn.prepare("SELECT file FROM segments ORDER BY file").unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().map(Result::unwrap).collect()
}

/// A store with the sample logs, and one of its sources dropped from the catalog the way `remove_source` does it, but
/// without the compaction that follows: its rows are garbage the next compaction would rewrite.
fn with_garbage(dir: &Dir) -> DuckStore {
    let store = DuckStore::create(&dir.0, Profile::default_profile(), PluginRegistry::new()).unwrap();
    store.open(&samples()).unwrap().wait().unwrap();
    {
        let conn = store.inner.conn().unwrap();
        let sources: u32 = conn.query_row("SELECT count(*) FROM sources", [], |r| r.get(0)).unwrap();
        assert!(sources > 1, "the fixture has several sources");
        let victim: u32 = conn.query_row("SELECT min(id) FROM sources", [], |r| r.get(0)).unwrap();
        conn.execute("DELETE FROM sources WHERE id = ?", params![victim]).unwrap();
    }
    store
}

#[test]
fn a_cancelled_import_skips_compaction_but_still_orders_the_events() {
    let dir = Dir::new("cancelled-tail");
    let store = with_garbage(&dir);
    let conn = store.inner.conn().unwrap();
    let files_before = segment_files(&conn);
    // The positions are stale (as when events were just added): maintain must renumber them.
    conn.execute_batch("DELETE FROM ord").unwrap();
    assert_eq!(store.positioned().unwrap(), 0);
    let generation = store.inner.generation.load(Ordering::SeqCst);

    let progress = ImportProgress::default();
    progress.cancel();
    store.inner.maintain(&conn, Some(&progress));

    assert_eq!(segment_files(&conn), files_before, "compaction was skipped");
    let events = store.summary().unwrap().total as u64;
    assert!(events > 0);
    assert_eq!(store.positioned().unwrap(), events, "every visible event has a position");
    let ids = store.ids_at(0, 10_000).unwrap();
    assert_eq!(ids.len() as u64, events.min(debrief_core::ports::MAX_PAGE_ROWS as u64));
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(store.position_of(*id).unwrap(), Some(i as u64));
    }
    assert!(
        store.inner.generation.load(Ordering::SeqCst) > generation,
        "readers are told the data changed"
    );
    let page = store
        .page(&PageQuery {
            limit: 5,
            ..PageQuery::default()
        })
        .unwrap();
    assert_eq!(page.rows.len(), 5, "the store stays queryable");

    // The next change (here: an import that is not cancelled) retries the compaction.
    store.inner.maintain(&conn, Some(&ImportProgress::default()));
    assert_ne!(segment_files(&conn), files_before, "the garbage segments were rewritten");
    assert_eq!(store.positioned().unwrap(), store.summary().unwrap().total as u64);
}

#[test]
fn compaction_reports_progress_and_stops_between_groups_when_cancelled() {
    let dir = Dir::new("compact-cancel");
    let store = with_garbage(&dir);
    let conn = store.inner.conn().unwrap();
    let files_before = segment_files(&conn);

    // Cancelled before it starts: nothing is rewritten, and the error says so.
    let control = QueryControl::new();
    control.cancel();
    let err = control
        .scope(|| compact::compact_with(&conn, &store.inner.dir, &store.inner.schema, true))
        .unwrap_err();
    assert!(err.is::<compact::CompactCancelled>(), "{err:#}");
    assert_eq!(segment_files(&conn), files_before);
    assert!(store.summary().unwrap().total > 0, "the store still reads");

    // Not cancelled: the groups are counted until done equals total.
    let control = QueryControl::new();
    let done = control
        .scope(|| compact::compact_with(&conn, &store.inner.dir, &store.inner.schema, true))
        .unwrap();
    assert!(done.replaced > 0);
    let p = control.progress();
    assert_eq!(p.stage, QueryStage::Evaluating);
    assert!(p.total > 0 && p.done == p.total, "{p:?}");
    assert_ne!(segment_files(&conn), files_before);
}

/// Rewrites a segment in place the way Debrief 1.1 (Arrow 58) wrote it: the `extra` map's entry fields are `keys` and
/// `values` where Arrow 60 says `key` and `value`. Only the field names differ; the rows and the catalog entry stay.
fn rewrite_as_arrow_58(path: &Path) -> usize {
    use duckdb::arrow::array::{Array, ArrayRef, MapArray, StructArray};
    use duckdb::arrow::datatypes::{DataType, Field, Fields, Schema};
    use duckdb::arrow::record_batch::RecordBatch;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use parquet::arrow::ArrowWriter;
    let file = std::fs::File::open(path).unwrap();
    let reader = ParquetRecordBatchReaderBuilder::try_new(file).unwrap().build().unwrap();
    let old_entries = Field::new(
        "entries",
        DataType::Struct(Fields::from(vec![
            Field::new("keys", DataType::Utf8, false),
            Field::new("values", DataType::Utf8, true),
        ])),
        false,
    );
    let mut batches = Vec::new();
    let mut rows = 0;
    for batch in reader {
        let batch = batch.unwrap();
        rows += batch.num_rows();
        let at = batch.schema().index_of("extra").unwrap();
        let map = batch.column(at).as_any().downcast_ref::<MapArray>().unwrap();
        let entries = map.entries();
        let old_struct = StructArray::try_new(
            Fields::from(vec![
                Field::new("keys", DataType::Utf8, false),
                Field::new("values", DataType::Utf8, true),
            ]),
            entries.columns().to_vec(),
            None,
        )
        .unwrap();
        let old_map = MapArray::try_new(
            Arc::new(old_entries.clone()),
            map.offsets().clone(),
            old_struct,
            map.nulls().cloned(),
            false,
        )
        .unwrap();
        let mut columns: Vec<ArrayRef> = batch.columns().to_vec();
        columns[at] = Arc::new(old_map);
        let mut fields: Vec<Field> = batch.schema().fields().iter().map(|f| f.as_ref().clone()).collect();
        fields[at] = Field::new(
            "extra",
            DataType::Map(Arc::new(old_entries.clone()), false),
            fields[at].is_nullable(),
        );
        batches.push(RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap());
    }
    let out = std::fs::File::create(path).unwrap();
    let mut writer = ArrowWriter::try_new(out, batches[0].schema(), None).unwrap();
    for b in &batches {
        writer.write(b).unwrap();
    }
    writer.close().unwrap();
    rows
}

#[test]
fn segments_written_by_arrow_58_and_60_read_and_compact_together() {
    let dir = Dir::new("arrow-58-60");
    let store = with_garbage(&dir);
    let conn = store.inner.conn().unwrap();
    let files = segment_files(&conn);
    assert!(files.len() >= 2, "{files:?}");
    let seg_dir = store.inner.dir.join(crate::segments::SEGMENTS_DIR);
    // The first segment becomes a 1.x one, the others stay new ones.
    let old_rows = rewrite_as_arrow_58(&seg_dir.join(&files[0]));
    assert!(old_rows > 0);
    let total = |conn: &Connection| -> i64 {
        conn.query_row("SELECT count(*) FROM events WHERE source IN (SELECT id FROM sources)", [], |r| {
            r.get(0)
        })
        .unwrap()
    };
    let with_extra = |conn: &Connection| -> i64 {
        conn.query_row(
            "SELECT count(*) FROM events WHERE source IN (SELECT id FROM sources) AND cardinality(extra) > 0",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    let (before, extra_before) = (total(&conn), with_extra(&conn));
    assert!(before > 0 && extra_before > 0, "the fixture has events with extra fields");
    // Compaction rewrites segments of both shapes into new ones, losing no event and no extra field.
    store.inner.maintain(&conn, Some(&ImportProgress::default()));
    assert_ne!(segment_files(&conn), files, "the garbage segments were rewritten");
    assert_eq!((total(&conn), with_extra(&conn)), (before, extra_before));
    assert_eq!(store.positioned().unwrap(), before as u64);
}

/// A store with the sample logs whose positions can no longer be written: every ordering fails, as one that runs out
/// of memory does.
fn unorderable(dir: &Dir) -> DuckStore {
    let store = DuckStore::create(&dir.0, Profile::default_profile(), PluginRegistry::new()).unwrap();
    store.open(&samples()).unwrap().wait().unwrap();
    assert_eq!(store.positioned().unwrap(), store.summary().unwrap().total as u64);
    store.inner.conn().unwrap().execute_batch("DROP TABLE ord").unwrap();
    store
}

fn stale(store: &DuckStore) -> u64 {
    crate::writer::meta_u64(&store.inner.conn().unwrap(), pos::STALE)
}

#[test]
fn a_failed_time_ordering_is_reported_kept_and_done_again_by_the_ordering_job() {
    let dir = Dir::new("ordering-fails");
    let store = unorderable(&dir);
    let events = store.summary().unwrap().total as u64;
    let notices = {
        let conn = store.inner.conn().unwrap();
        store.inner.maintain(&conn, Some(&ImportProgress::default()))
    };
    assert!(
        notices.iter().any(|n| n.contains("time order") && n.contains("again")),
        "the import says so: {notices:?}"
    );
    assert_eq!(stale(&store), 1, "and the store remembers it");
    drop(store);
    // Opening does not order inside the open call; the app sees the gap and runs the ordering job.
    let store = DuckStore::open(&dir.0, Profile::default_profile(), PluginRegistry::new()).unwrap();
    assert!(store.positioned().unwrap() < events, "the gap the app checks");
    use debrief_core::ports::{Maintain, MaintenanceAction};
    store.maintain(MaintenanceAction::OrderEvents).unwrap();
    assert_eq!(store.positioned().unwrap(), events, "ordered again by the job");
    assert_eq!(stale(&store), 0);
}

#[test]
fn every_path_that_reorders_reports_a_failure_and_marks_the_positions_stale() {
    use debrief_core::ports::{Maintain, MaintenanceAction};
    // rebuild_positions
    let dir = Dir::new("reorder-api");
    let store = unorderable(&dir);
    let e = store.rebuild_positions().unwrap_err().to_string();
    assert!(e.contains("time order"), "{e}");
    assert_eq!(stale(&store), 1, "rebuild_positions");
    drop(store);
    // Store pane: Compact (with rows of a removed source to rewrite)
    let dir = Dir::new("reorder-compact");
    let store = with_garbage(&dir);
    store.inner.conn().unwrap().execute_batch("DROP TABLE ord").unwrap();
    let e = store.maintain(MaintenanceAction::Compact).unwrap_err().to_string();
    assert!(e.contains("Compacted") && e.contains("time order"), "{e}");
    assert_eq!(stale(&store), 1, "Compact");
    drop(store);
    // Removing a source
    let dir = Dir::new("reorder-remove");
    let store = unorderable(&dir);
    let source = store
        .inner
        .conn()
        .unwrap()
        .query_row("SELECT min(id) FROM sources", [], |r| r.get::<_, u32>(0))
        .unwrap();
    let e = store.remove_source(SourceId(source)).unwrap_err().to_string();
    assert!(e.contains("removed") && e.contains("time order"), "{e}");
    assert_eq!(stale(&store), 1, "remove_source");
    drop(store);
    // A clock correction
    use debrief_core::ports::Skew;
    let dir = Dir::new("reorder-skew");
    let store = unorderable(&dir);
    let source = store
        .inner
        .conn()
        .unwrap()
        .query_row("SELECT min(id) FROM sources", [], |r| r.get::<_, u32>(0))
        .unwrap();
    let e = store
        .set_source_offset(&debrief_core::skew::SetOffsetRequest {
            spec: debrief_core::skew::OffsetSpec::Manual {
                source,
                offset_ms: 3_600_000,
            },
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("clock correction") && e.contains("time order"), "{e}");
    assert_eq!(stale(&store), 1, "clock correction");
}

#[test]
fn a_stale_order_stays_marked_through_live_refreshes_until_the_ordering_job_repairs_it() {
    let dir = Dir::new("stale-live");
    let (store, file) = followed(&dir, &[]);
    // A failed ordering (a clock correction's, say) left positions with the right count in the wrong order.
    crate::writer::set_meta(&store.inner.conn().unwrap(), pos::STALE, 1).unwrap();
    assert!(!store.order_complete().unwrap(), "the count matches, the order is not complete");
    let r = store.append_tail(std::slice::from_ref(&file)).unwrap();
    assert_eq!(r.appended, 2, "{r:?}");
    assert_eq!(stale(&store), 1, "an append does not clear the mark");
    assert!(r.notices.iter().any(|n| n.contains("time order")), "the refresh says so: {r:?}");
    assert!(!store.order_complete().unwrap());
    use debrief_core::ports::{Maintain, MaintenanceAction};
    store.maintain(MaintenanceAction::OrderEvents).unwrap();
    assert!(store.order_complete().unwrap(), "the job repairs it");
    assert_eq!(stale(&store), 0);
}

/// A store following one CLEF file (one line per event, so nothing is held back), and reading `more` folders.
fn followed(dir: &Dir, more: &[PathBuf]) -> (DuckStore, PathBuf) {
    let logs = dir.0.join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let file = logs.join("a.clef");
    let line = |i: u32| format!("{{\"@t\":\"2026-10-14T10:00:{i:02}Z\",\"@mt\":\"event {i}\",\"@l\":\"Information\"}}\n");
    std::fs::write(&file, (0..3).map(line).collect::<String>()).unwrap();
    let store = DuckStore::create(&dir.0.join("store"), Profile::default_profile(), PluginRegistry::new()).unwrap();
    let mut roots = vec![logs];
    roots.extend_from_slice(more);
    store.open(&roots).unwrap().wait().unwrap();
    let mut f = std::fs::OpenOptions::new().append(true).open(&file).unwrap();
    std::io::Write::write_all(&mut f, (3..5).map(line).collect::<String>().as_bytes()).unwrap();
    (store, file)
}

#[test]
fn a_live_refresh_that_cannot_order_the_new_events_says_so_and_marks_the_positions_stale() {
    let dir = Dir::new("reorder-tail");
    let (store, file) = followed(&dir, &[]);
    store.inner.conn().unwrap().execute_batch("DROP TABLE ord").unwrap();
    let r = store.append_tail(std::slice::from_ref(&file)).unwrap();
    assert_eq!(r.appended, 2, "{r:?}");
    assert!(r.notices.iter().any(|n| n.contains("time order")), "{r:?}");
    assert_eq!(stale(&store), 1, "the append's fallback goes through the one ordering");
}

#[test]
fn the_catalogue_waits_while_heavy_work_holds_the_store() {
    let dir = Dir::new("heavy-catalogue");
    let store = DuckStore::create(&dir.0, Profile::default_profile(), PluginRegistry::new()).unwrap();
    store.open(&samples()).unwrap().wait().unwrap();
    assert!(store.wait_for_catalogue(std::time::Duration::from_secs(60)).unwrap());
    // Heavy work holds the store (an import's ordering) while the catalogue has to be built again.
    let held = lock(&store.inner.heavy);
    catalogue::forget(&store.inner.conn().unwrap(), None).unwrap();
    if let Some(c) = &store.inner.catalogue {
        c.wake();
    }
    // It cannot run its per-source aggregate meanwhile...
    assert!(
        !store.wait_for_catalogue(std::time::Duration::from_millis(1500)).unwrap(),
        "built beside heavy work"
    );
    drop(held);
    // ...and finishes once the lock is free.
    assert!(store.wait_for_catalogue(std::time::Duration::from_secs(60)).unwrap());
}

#[test]
fn maintenance_runs_with_the_import_memory_and_gives_it_back() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("SET memory_limit = '64MB'").unwrap();
    let limit = |c: &Connection| {
        c.query_row("SELECT current_setting('memory_limit')", [], |r| r.get::<_, String>(0))
            .unwrap()
    };
    let normal = limit(&conn);
    let inside = with_memory(&conn, "512MB", || "64MB".to_string(), |c| limit(c));
    assert_ne!(inside, normal, "raised while it runs");
    assert_eq!(limit(&conn), normal, "given back");
    let failed = with_memory(
        &conn,
        "512MB",
        || "64MB".to_string(),
        |c| c.execute_batch("SELECT * FROM no_such_table"),
    );
    assert!(failed.is_err());
    assert_eq!(limit(&conn), normal, "given back after a failure too");
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_memory(&conn, "512MB", || "64MB".to_string(), |_| panic!("the work panics"))
    }));
    assert!(panicked.is_err());
    assert_eq!(limit(&conn), normal, "given back after a panic too");
    // A limit configured while the work runs is the one set back, not the one from before.
    let configured = std::sync::Mutex::new("64MB".to_string());
    with_memory(
        &conn,
        "512MB",
        || configured.lock().unwrap().clone(),
        |_| *configured.lock().unwrap() = "96MB".into(),
    );
    let expected = {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("SET memory_limit = '96MB'").unwrap();
        limit(&c)
    };
    assert_eq!(limit(&conn), expected);
}
