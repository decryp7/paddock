//! Clock-skew correction on the store ([`debrief_core::ports::Skew`], [`debrief_core::skew`]).
//!
//! A source file's offset is kept in `clock_offsets` (by path) and already applied to its events: files read
//! later take it as they are written ([`crate::writer::SegmentSink`]), and changing it rewrites the segments that
//! hold the file's events with their `ts` moved by the difference. The rewritten segments keep their id blocks (an
//! event keeps its id when it moves in time; their `segment_blocks` rows move to the new files) and are swapped for
//! the old ones, with the offset, the sources' time ranges and the histogram, in one catalog transaction: a reader
//! sees the old times or the new ones. The time-order positions are renumbered afterwards. The full-text index holds
//! each event's time and orders hits by it (ADR 0062, defect 3), so the rewritten segments are marked unindexed and
//! their blocks' documents deleted, still under the write lock: the indexer adds them again with the new times.

use crate::compact::delete_or_trash;
use crate::schema::{parquet_from, READY};
use crate::segments::{Segment, SegmentWriter, SEGMENTS_DIR};
use crate::sql::ident;
use crate::writer::{self, delete_segment, insert_segment, meta_u64, move_blocks, refresh_view, set_meta};
use crate::{engine, lock, DuckStore};
use anyhow::Result;
use debrief_core::batch::{EventBatch, EventId, EventSchema, BATCH_ROWS};
use debrief_core::ports::{EventsRequest, Query, Skew, StoreError, StoreResult};
use debrief_core::skew::{
    align_offset, checked_offset, suggest_over, ClockOffsets, OffsetChange, OffsetSpec, SetOffsetRequest, SkewError, SuggestRequest,
    SuggestResult,
};
use duckdb::arrow::array::{RecordBatch, StructArray};
use duckdb::{params, Connection};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

fn invalid(e: SkewError) -> StoreError {
    StoreError::Invalid(e.to_string())
}

/// `col + delta` in SQL, saturating at the ends of `BIGINT` (a NULL stays NULL).
fn saturating(col: &str, delta: i64) -> String {
    format!("CAST(least(greatest(CAST({col} AS HUGEINT) + {delta}, -9223372036854775808), 9223372036854775807) AS BIGINT)")
}

/// The event columns of a segment with `ts` moved by each source's change.
fn shifted_columns(schema: &EventSchema, shifts: &HashMap<u32, i64>) -> String {
    let mut moves: Vec<_> = shifts.iter().collect();
    moves.sort();
    let whens: String = moves
        .iter()
        .map(|(source, delta)| format!(" WHEN source = {source} THEN {}", saturating("ts", **delta)))
        .collect();
    schema
        .arrow()
        .fields()
        .iter()
        .map(|f| {
            if f.name() == "ts" {
                format!("CASE{whens} ELSE ts END AS \"ts\"")
            } else {
                ident(f.name())
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A segment as listed.
struct Listed {
    file: String,
    first_id: u64,
    end_id: u64,
    indexed: bool,
}

/// The id blocks of the segments in `listed` that have them, by file.
fn blocks_of(conn: &Connection, listed: &[Listed]) -> Result<HashMap<String, Vec<(u64, u64)>>> {
    let mut out: HashMap<String, Vec<(u64, u64)>> = HashMap::new();
    let mut stmt = conn.prepare("SELECT first_id, end_id FROM segment_blocks WHERE file = ?")?;
    for l in listed {
        let rows = stmt.query_map(params![l.file], |r| Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?)))?;
        let found = rows.collect::<duckdb::Result<Vec<_>>>()?;
        if !found.is_empty() {
            out.insert(l.file.clone(), found);
        }
    }
    Ok(out)
}

/// Writes the live rows of `from` with the sources' times moved, into a new segment with the same id block.
fn rewrite(
    conn: &Connection,
    seg_dir: &Path,
    schema: &Arc<EventSchema>,
    from: &Listed,
    shifts: &HashMap<u32, i64>,
    generation: u64,
) -> Result<Segment> {
    let file = seg_dir.join(&from.file).to_string_lossy().into_owned();
    let sql = format!(
        "SELECT {} FROM {} WHERE {READY} ORDER BY id",
        shifted_columns(schema, shifts),
        parquet_from(&[file])
    );
    let mut w = SegmentWriter::create(seg_dir, schema, from.first_id, from.end_id, generation)?;
    let written = (|| -> Result<()> {
        let mut stmt = conn.prepare(&sql)?;
        let _ = stmt.stream_arrow([])?;
        while let Some(chunk) = stmt.step()? {
            let rb = RecordBatch::from(&chunk as &StructArray);
            let mut offset = 0;
            while offset < rb.num_rows() {
                let len = (rb.num_rows() - offset).min(BATCH_ROWS);
                w.write_existing(&EventBatch::try_new(schema.clone(), rb.slice(offset, len))?)?;
                offset += len;
            }
        }
        Ok(())
    })();
    match written {
        Ok(()) => w.close(),
        Err(e) => {
            w.discard();
            Err(e.context("moving events in time"))
        }
    }
}

impl DuckStore {
    /// Sets the offsets of the files `changes` names (`(path, new offset)`; 0 clears) and moves their events.
    fn apply_offsets(&self, changes: &[(String, i64)]) -> StoreResult<()> {
        self.idle()?;
        let inner = &self.inner;
        let conn = inner.conn()?;
        let _writing = lock(&inner.write_lock);
        let current = writer::clock_offsets(&conn).map_err(|e| engine(format!("{e:#}")))?;
        let mut shifts: HashMap<u32, i64> = HashMap::new();
        for (path, new) in changes {
            let delta = new.saturating_sub(current.get(path).copied().unwrap_or(0));
            if delta == 0 {
                continue;
            }
            let mut stmt = conn.prepare("SELECT id FROM sources WHERE path = ? AND ready").map_err(engine)?;
            let ids = stmt
                .query_map(params![path], |r| r.get::<_, u32>(0))
                .map_err(engine)?
                .collect::<duckdb::Result<Vec<_>>>()
                .map_err(engine)?;
            shifts.extend(ids.into_iter().map(|id| (id, delta)));
        }
        let seg_dir = inner.dir.join(SEGMENTS_DIR);
        // The segments holding events of the moved sources: the histogram names each segment's sources.
        let mut affected: Vec<Listed> = Vec::new();
        if !shifts.is_empty() {
            let list = shifts.keys().map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT file, first_id, end_id, indexed FROM segments WHERE file IN (SELECT DISTINCT file FROM histogram WHERE source IN ({list})) ORDER BY first_id"
                ))
                .map_err(engine)?;
            affected = stmt
                .query_map([], |r| {
                    Ok(Listed {
                        file: r.get(0)?,
                        first_id: r.get(1)?,
                        end_id: r.get(2)?,
                        indexed: r.get(3)?,
                    })
                })
                .map_err(engine)?
                .collect::<duckdb::Result<Vec<_>>>()
                .map_err(engine)?;
        }
        // The id blocks of each affected segment: their documents are replaced (see the module docs).
        let blocks = blocks_of(&conn, &affected).map_err(|e| engine(format!("{e:#}")))?;
        if crate::trial_lake::native() {
            // Trial 2, native: the rows move in the lake; the store files that held them are read through the lake from
            // now on and their search entries are added again.
            let files: Vec<String> = affected.iter().map(|l| l.file.clone()).collect();
            crate::trial_lake::shift_lake(&conn, &shifts).map_err(|e| engine(format!("{e:#}")))?;
            conn.execute_batch("BEGIN TRANSACTION").map_err(engine)?;
            let done = (|| -> Result<()> {
                crate::trial_lake::shift(&conn, &shifts, &files)?;
                for (id, delta) in &shifts {
                    conn.execute(
                        &format!(
                            "UPDATE sources SET ts_from = {}, ts_to = {} WHERE id = ?",
                            saturating("ts_from", *delta),
                            saturating("ts_to", *delta)
                        ),
                        params![id],
                    )?;
                }
                for (path, new) in changes {
                    if *new == 0 {
                        conn.execute("DELETE FROM clock_offsets WHERE path = ?", params![path])?;
                    } else {
                        conn.execute("INSERT OR REPLACE INTO clock_offsets VALUES (?, ?)", params![path, new])?;
                    }
                }
                for f in files.iter().filter(|f| blocks.contains_key(*f)) {
                    conn.execute("UPDATE segments SET indexed = FALSE WHERE file = ?", params![f])?;
                }
                Ok(())
            })();
            match done {
                Ok(()) => conn.execute_batch("COMMIT").map_err(engine)?,
                Err(e) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(engine(format!("{e:#}")));
                }
            }
            if let Some(fts) = &inner.fts {
                let ranges: Vec<(u64, u64)> = affected.iter().filter_map(|l| blocks.get(&l.file)).flatten().copied().collect();
                if !ranges.is_empty() {
                    match fts.delete_blocks(&ranges) {
                        Ok(()) => fts.wake(),
                        Err(e) => log::warn!("the moved events' search entries keep their old times: {e:#}"),
                    }
                }
            }
        } else {
        let mut generation = meta_u64(&conn, "file_generation");
        let mut written: Vec<Segment> = Vec::new();
        for l in &affected {
            generation += 1;
            match rewrite(&conn, &seg_dir, &inner.schema, l, &shifts, generation) {
                Ok(seg) => written.push(seg),
                Err(e) => {
                    for seg in &written {
                        let _ = std::fs::remove_file(seg_dir.join(&seg.file));
                    }
                    return Err(engine(format!("{e:#}")));
                }
            }
        }
        conn.execute_batch("BEGIN TRANSACTION").map_err(engine)?;
        let swapped = (|| -> Result<()> {
            for (l, seg) in affected.iter().zip(&written) {
                move_blocks(&conn, &l.file, &seg.file)?;
                delete_segment(&conn, &l.file)?;
                // Indexed again from the new file, unless its blocks are not known yet (a segment whose range
                // overlaps another's, from before `segment_blocks`): those keep their documents and old times.
                let keep = l.indexed && !blocks.contains_key(&l.file);
                insert_segment(&conn, seg, seg.rows, keep)?;
            }
            for (id, delta) in &shifts {
                conn.execute(
                    &format!(
                        "UPDATE sources SET ts_from = {}, ts_to = {} WHERE id = ?",
                        saturating("ts_from", *delta),
                        saturating("ts_to", *delta)
                    ),
                    params![id],
                )?;
            }
            for (path, new) in changes {
                if *new == 0 {
                    conn.execute("DELETE FROM clock_offsets WHERE path = ?", params![path])?;
                } else {
                    conn.execute("INSERT OR REPLACE INTO clock_offsets VALUES (?, ?)", params![path, new])?;
                }
            }
            set_meta(&conn, "file_generation", generation)?;
            if !affected.is_empty() {
                refresh_view(&conn, &inner.dir, &inner.schema)?;
            }
            Ok(())
        })();
        match swapped {
            Ok(()) => conn.execute_batch("COMMIT").map_err(engine)?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                for seg in &written {
                    let _ = std::fs::remove_file(seg_dir.join(&seg.file));
                }
                return Err(engine(format!("{e:#}")));
            }
        }
        for l in &affected {
            if let Err(e) = delete_or_trash(&conn, &seg_dir.join(&l.file)) {
                log::warn!("an old data file could not be removed: {e:#}");
            }
        }
        self.reindex_moved(&conn, &affected, &written, &blocks);
        }
        if !shifts.is_empty() {
            // A failed ordering is marked stale and done again by the next open or import; the correction itself holds.
            let notice = inner.reorder(&conn);
            inner.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            inner.bump_version(&conn)?;
            if let Some(notice) = notice {
                return Err(StoreError::Engine(format!("The clock correction is applied. {notice}")));
            }
        }
        Ok(())
    }

    /// Deletes the documents of the rewritten segments' blocks, which hold the old times; the indexer (kept out by the
    /// write lock the caller holds) adds them again from the new files. This covers segments not marked indexed too,
    /// whose documents may already be committed. When the delete fails (it is rolled back), each new file gets its old
    /// segment's flag back: indexed ones keep the documents with the old times (as before ADR 0062) rather than appear
    /// twice.
    fn reindex_moved(&self, conn: &Connection, affected: &[Listed], written: &[Segment], blocks: &HashMap<String, Vec<(u64, u64)>>) {
        let Some(fts) = &self.inner.fts else {
            return;
        };
        let moved: Vec<(&Listed, &Segment)> = affected
            .iter()
            .zip(written)
            // Also unindexed ones: their documents may already be committed.
            .filter(|(l, _)| blocks.contains_key(&l.file))
            .collect();
        if moved.is_empty() {
            return;
        }
        let ranges: Vec<(u64, u64)> = moved.iter().flat_map(|(l, _)| blocks[&l.file].iter().copied()).collect();
        match fts.delete_blocks(&ranges) {
            Ok(()) => fts.wake(),
            Err(e) => {
                log::warn!("the moved events' search entries keep their old times: {e:#}");
                for (_, seg) in moved.into_iter().filter(|(l, _)| l.indexed) {
                    if let Err(e) = conn.execute("UPDATE segments SET indexed = TRUE WHERE file = ?", params![seg.file]) {
                        log::warn!("a moved segment could not be marked indexed: {e:#}");
                    }
                }
            }
        }
    }

    /// The file path of a source.
    fn source_path(&self, source: u32) -> StoreResult<String> {
        let conn = self.inner.conn()?;
        conn.query_row("SELECT path FROM sources WHERE id = ? AND ready", params![source], |r| {
            r.get::<_, String>(0)
        })
        .map_err(|_| invalid(SkewError::UnknownSource(source)))
    }

    /// Timed events by id: (source, time as shown).
    fn timed(&self, id: u32) -> StoreResult<(u32, i64)> {
        let found = self.events(&EventsRequest {
            ids: vec![EventId(u64::from(id))],
            group_by: Vec::new(),
        })?;
        let e = found.first().ok_or_else(|| invalid(SkewError::UnknownRecord(id)))?;
        Ok((e.source.0, e.ts.ok_or_else(|| invalid(SkewError::Untimed(id)))?))
    }
}

impl Skew for DuckStore {
    fn clock_offsets(&self) -> StoreResult<ClockOffsets> {
        let conn = self.inner.conn()?;
        let rows = writer::clock_offsets(&conn).map_err(|e| engine(format!("{e:#}")))?;
        let mut offsets = ClockOffsets::default();
        for (path, ms) in rows {
            // Stored values were checked when they were set.
            let _ = offsets.set(&path, ms);
        }
        Ok(offsets)
    }

    fn set_clock_offsets(&self, offsets: &ClockOffsets) -> StoreResult<()> {
        let wanted = offsets.clone().sanitized();
        let current = self.clock_offsets()?;
        let paths: BTreeSet<&String> = wanted.iter().chain(current.iter()).map(|(p, _)| p).collect();
        let changes: Vec<(String, i64)> = paths
            .into_iter()
            .filter(|p| wanted.get(p) != current.get(p))
            .map(|p| (p.clone(), wanted.get(p)))
            .collect();
        if changes.is_empty() {
            return Ok(());
        }
        self.apply_offsets(&changes)
    }

    fn set_source_offset(&self, req: &SetOffsetRequest) -> StoreResult<OffsetChange> {
        let (source, offset) = match req.spec {
            OffsetSpec::Manual { source, offset_ms } => (source, checked_offset(offset_ms).map_err(invalid)?),
            OffsetSpec::Align { reference, target } => {
                let (r, t) = (self.timed(reference)?, self.timed(target)?);
                if r.0 == t.0 {
                    return Err(invalid(SkewError::SameSource(r.0)));
                }
                let current = self.clock_offsets()?.get(&self.source_path(t.0)?);
                (t.0, align_offset(r.1, t.1.saturating_sub(current)).map_err(invalid)?)
            }
        };
        let path = self.source_path(source)?;
        let previous = self.clock_offsets()?.get(&path);
        self.apply_offsets(&[(path, offset)])?;
        // Events keep their ids when they move in time.
        Ok(OffsetChange {
            source,
            offset_ms: offset,
            previous_ms: previous,
            ids_changed_from: None,
        })
    }

    fn suggest_offsets(&self, req: &SuggestRequest) -> StoreResult<SuggestResult> {
        suggest_over(self, req)
    }
}
