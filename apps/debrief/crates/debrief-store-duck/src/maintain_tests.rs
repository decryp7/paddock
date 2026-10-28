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
