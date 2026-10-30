//! Time-order positions (ADR 0014 R2b): `ord(pos, id)` numbers every visible event in time order
//! (`coalesce(ts, i64::MAX), source, line, id`, untimed last), stored sorted by `pos` so DuckDB's zone maps
//! turn a position range into a short scan. It is rebuilt at the end of every import and after a source is
//! removed; events imported since have no position until then ([`Positions::positioned`] tells).

use crate::schema::{canonical_columns, parquet_from, READY};
use crate::segments::SEGMENTS_DIR;
use crate::{engine, lock, DuckStore};
use debrief_core::batch::{EventBatch, EventId, BATCH_ROWS};
use debrief_core::ports::{Positions, StoreError, StoreResult, MAX_PAGE_ROWS};
use duckdb::arrow::array::{RecordBatch, StructArray};
use duckdb::{params, Connection};

/// The sort key of the last positioned event, kept in `meta` so a later append can tell whether new events sort
/// after it (`ts,source,line`; untimed events sort last).
const LAST_KEY: &str = "ord_last";

/// `meta` key set to 1 while the positions are missing or stale because a rebuild failed: the store rebuilds them when
/// it opens and at the next maintenance (they must never stay missing).
pub(crate) const STALE: &str = "ord_stale";

/// Attempts of a rebuild before it is reported as failed.
pub(crate) const ATTEMPTS: usize = 2;

/// [`rebuild`], tried [`ATTEMPTS`] times; marks the positions [`STALE`] when every attempt failed (and clears the mark
/// when one succeeded). The error is the last attempt's. Between attempts a `CHECKPOINT` writes out what the catalog
/// holds in memory, so its blocks can be evicted and the second attempt has the memory the first ran out of (the caller
/// already gave it the import memory, `Inner::reorder`).
pub(crate) fn rebuild_or_mark(conn: &Connection) -> duckdb::Result<()> {
    let mut last = None;
    for attempt in 1..=ATTEMPTS {
        match rebuild(conn) {
            Ok(()) => {
                if let Err(e) = crate::writer::set_meta(conn, STALE, 0) {
                    log::warn!("could not clear {STALE}: {e:#}");
                }
                return Ok(());
            }
            Err(e) => {
                log::warn!("time-order positions could not be rebuilt (attempt {attempt} of {ATTEMPTS}): {e}");
                last = Some(e);
                if attempt < ATTEMPTS {
                    if let Err(e) = conn.execute_batch("CHECKPOINT") {
                        log::warn!("checkpoint before the next ordering attempt failed: {e}");
                    }
                }
            }
        }
    }
    if let Err(e) = crate::writer::set_meta(conn, STALE, 1) {
        log::error!("could not mark the positions stale: {e:#}");
    }
    Err(last.unwrap_or(duckdb::Error::QueryReturnedNoRows))
}

/// Whether there are as many positions as visible events (rows rewritten out by compaction leave more positions than
/// events; missing positions leave fewer). A count, not a check of every id: cheap enough after each compaction.
pub(crate) fn complete(conn: &Connection) -> duckdb::Result<bool> {
    conn.query_row("SELECT (SELECT count(*) FROM ord) = (SELECT count(*) FROM events)", [], |r| {
        r.get(0)
    })
}

/// Whether a failed rebuild left the positions stale.
pub(crate) fn is_stale(conn: &Connection) -> bool {
    crate::writer::meta_u64(conn, STALE) == 1
}

/// Renumbers the visible events.
pub(crate) fn rebuild(conn: &Connection) -> duckdb::Result<()> {
    conn.execute_batch(
        "BEGIN TRANSACTION;
DELETE FROM ord;
INSERT INTO ord SELECT (row_number() OVER (ORDER BY coalesce(ts, 9223372036854775807), source, line, id) - 1)::UBIGINT AS pos, id FROM events ORDER BY pos;
COMMIT;",
    )
    .inspect_err(|_| {
        let _ = conn.execute_batch("ROLLBACK");
    })?;
    remember_last(conn)
}

/// Records the sort key of the last positioned event (or forgets it when nothing is positioned).
fn remember_last(conn: &Connection) -> duckdb::Result<()> {
    let key = conn
        .query_row(
            "SELECT coalesce(e.ts, 9223372036854775807), e.source, e.line FROM events e WHERE e.id = (SELECT id FROM ord ORDER BY pos DESC LIMIT 1)",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, u32>(1)?, r.get::<_, u32>(2)?)),
        )
        .ok();
    match key {
        Some((ts, source, line)) => conn.execute(
            "INSERT OR REPLACE INTO meta VALUES (?, ?)",
            params![LAST_KEY, format!("{ts},{source},{line}")],
        ),
        None => conn.execute("DELETE FROM meta WHERE key = ?", params![LAST_KEY]),
    }
    .map(drop)
}

fn last_key(conn: &Connection) -> Option<(i64, u32, u32)> {
    let text: String = conn
        .query_row("SELECT value FROM meta WHERE key = ?", params![LAST_KEY], |r| r.get(0))
        .ok()?;
    let mut parts = text.split(',');
    Some((
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ))
}

/// Positions the events with ids from `first_new` on after the ones already positioned, when they all sort after
/// them (the usual case when files grow). `before` is the number of events that were visible before the new ones:
/// positions are only extended when exactly that many are positioned. False when they cannot be extended and every
/// event must be renumbered (the caller does that through `Inner::reorder`, never here).
pub(crate) fn append(conn: &Connection, first_new: u64, before: u64) -> duckdb::Result<bool> {
    let positioned = conn.query_row("SELECT count(*) FROM ord", [], |r| r.get::<_, u64>(0))?;
    let smallest = conn.query_row(
        "SELECT coalesce(ts, 9223372036854775807), source, line FROM events WHERE id >= ? ORDER BY 1, 2, 3 LIMIT 1",
        params![first_new],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, u32>(1)?, r.get::<_, u32>(2)?)),
    );
    let smallest = match smallest {
        Ok(k) => k,
        // The new ids hold no visible events.
        Err(duckdb::Error::QueryReturnedNoRows) => return Ok(true),
        Err(e) => return Err(e),
    };
    match last_key(conn) {
        Some(last) if positioned == before && smallest > last => {}
        _ => return Ok(false),
    }
    conn.execute(
        "INSERT INTO ord SELECT (? + row_number() OVER (ORDER BY coalesce(ts, 9223372036854775807), source, line, id) - 1)::UBIGINT, id FROM events WHERE id >= ? ORDER BY 1",
        params![positioned, first_new],
    )?;
    remember_last(conn)?;
    // The positions are complete again (they were before, and the new events follow them).
    if let Err(e) = crate::writer::set_meta(conn, STALE, 0) {
        log::warn!("could not clear {STALE}: {e:#}");
    }
    Ok(true)
}

impl Positions for DuckStore {
    fn ids_at(&self, from: u64, len: usize) -> StoreResult<Vec<EventId>> {
        let conn = self.inner.conn()?;
        let len = len.min(MAX_PAGE_ROWS) as u64;
        let mut stmt = conn
            .prepare("SELECT id FROM ord WHERE pos >= ? AND pos < ? ORDER BY pos")
            .map_err(engine)?;
        let rows = stmt
            .query_map(params![from, from.saturating_add(len)], |r| r.get::<_, u64>(0))
            .map_err(engine)?;
        rows.map(|r| r.map(EventId).map_err(engine)).collect()
    }

    fn position_of(&self, id: EventId) -> StoreResult<Option<u64>> {
        let conn = self.inner.conn()?;
        match conn.query_row("SELECT pos FROM ord WHERE id = ?", params![id.0], |r| r.get::<_, u64>(0)) {
            Ok(p) => Ok(Some(p)),
            Err(duckdb::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(engine(e)),
        }
    }

    fn positioned(&self) -> StoreResult<u64> {
        let conn = self.inner.conn()?;
        conn.query_row("SELECT count(*) FROM ord", [], |r| r.get::<_, u64>(0))
            .map_err(engine)
    }
}

impl DuckStore {
    /// The sorted ids (at most [`MAX_PAGE_ROWS`]) and the segment files that hold them; `None` for none.
    fn files_for(&self, conn: &Connection, ids: &[EventId]) -> StoreResult<Option<(Vec<String>, Vec<u64>)>> {
        let mut ids: Vec<u64> = ids.iter().map(|i| i.0).take(MAX_PAGE_ROWS).collect();
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            return Ok(None);
        }
        let seg_dir = self.inner.dir.join(SEGMENTS_DIR);
        let mut files = Vec::new();
        let mut stmt = conn
            .prepare("SELECT file, first_id, end_id FROM segments ORDER BY first_id")
            .map_err(engine)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?, r.get::<_, u64>(2)?)))
            .map_err(engine)?;
        for row in rows {
            let (file, first, end) = row.map_err(engine)?;
            let at = ids.partition_point(|i| *i < first);
            if ids.get(at).is_some_and(|i| *i < end) {
                files.push(seg_dir.join(file).to_string_lossy().into_owned());
            }
        }
        Ok((!files.is_empty()).then_some((files, ids)))
    }

    /// The lower-cased text (message, exception and template) of the visible events with these ids, for
    /// checking phrases; at most [`MAX_PAGE_ROWS`] ids.
    pub(crate) fn texts(&self, ids: &[EventId]) -> StoreResult<Vec<(EventId, String)>> {
        let conn = self.inner.conn()?;
        let Some((files, ids)) = self.files_for(&conn, ids)? else {
            return Ok(Vec::new());
        };
        let list: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
        let sql = format!(
            "SELECT id, lower(concat_ws(chr(10), message, exception, template)) FROM {} WHERE id IN ({}) AND {READY}",
            parquet_from(&files),
            list.join(", ")
        );
        let mut stmt = conn.prepare(&sql).map_err(engine)?;
        let rows = stmt
            .query_map([], |r| Ok((EventId(r.get::<_, u64>(0)?), r.get::<_, String>(1)?)))
            .map_err(engine)?;
        rows.map(|r| r.map_err(engine)).collect()
    }

    /// The visible events with these ids, in id order, as batches (the second phase of a page: positions
    /// or search give the ids, this fetches the rows). Only the segments whose id blocks hold the ids are
    /// read; at most [`MAX_PAGE_ROWS`] ids.
    pub fn rows(&self, ids: &[EventId]) -> StoreResult<Vec<EventBatch>> {
        let conn = self.inner.conn()?;
        let Some((files, ids)) = self.files_for(&conn, ids)? else {
            return Ok(Vec::new());
        };
        let list: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
        let sql = format!(
            "SELECT {} FROM {} WHERE id IN ({}) AND {READY} ORDER BY id",
            canonical_columns(&self.inner.schema),
            parquet_from(&files),
            list.join(", ")
        );
        let mut stmt = conn.prepare(&sql).map_err(engine)?;
        let _ = stmt.stream_arrow([]).map_err(engine)?;
        let mut out = Vec::new();
        while let Some(chunk) = stmt.step().map_err(engine)? {
            let rb = RecordBatch::from(&chunk as &StructArray);
            let mut offset = 0;
            while offset < rb.num_rows() {
                let len = (rb.num_rows() - offset).min(BATCH_ROWS);
                out.push(EventBatch::try_new(self.inner.schema.clone(), rb.slice(offset, len)).map_err(engine)?);
                offset += len;
            }
        }
        Ok(out)
    }

    /// Runs a read-only SQL statement and counts its rows (benchmarks of queries not yet behind a port).
    #[doc(hidden)]
    pub fn bench_sql(&self, sql: &str) -> StoreResult<u64> {
        let conn = self.inner.conn()?;
        let mut stmt = conn.prepare(sql).map_err(engine)?;
        let mut rows = stmt.query([]).map_err(engine)?;
        let mut n = 0;
        while rows.next().map_err(engine)?.is_some() {
            n += 1;
        }
        Ok(n)
    }

    /// Renumbers the events now (normally done by imports and removals), through `Inner::reorder`.
    pub fn rebuild_positions(&self) -> StoreResult<()> {
        let _writing = lock(&self.inner.write_lock);
        let conn = self.inner.conn()?;
        match self.inner.reorder(&conn) {
            None => Ok(()),
            Some(notice) => Err(StoreError::Engine(notice)),
        }
    }
}
