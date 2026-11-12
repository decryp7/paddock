//! The persistent catalogue of names ([`debrief_core::catalogue`]): field values and the entities found in message
//! and exception text, kept in the store's database so a lookup is a query over a few tables, not a scan of events.
//!
//! - Tables: `catalogue` (one row per source, scope and name: events, first and last seen), `catalogue_trigrams`
//!   (the three-letter groups of each name, for typo-tolerant lookups) and `catalogue_sources` (the sources
//!   catalogued, with the layout version they were built with).
//! - Built per source in the background (`debrief-catalogue` thread, one source per turn, its events read once through
//!   the `events` view): text scanned by [`debrief_core::entities::extract`], field values from the `extra` map. The
//!   rows of a source are written in one transaction under the store's write lock, and only while the source is still
//!   ready, so a lookup sees a source's names exactly when it sees its events.
//! - Lifecycle: rows are joined with the ready sources, so a re-import's old sources (still ready) keep answering until
//!   the new ones are ready; rows of sources that no longer exist are deleted on the next turn, and eagerly on clear,
//!   remove and refresh. A source built with another [`VERSION`] is built again.
//! - Bounds: see [`debrief_core::catalogue`]; one thread, one source in memory at a time.

use crate::turns::{wait_after, Gate, Turn};
use crate::{engine, lock};
use anyhow::Result;
use debrief_core::catalogue::*;
use debrief_core::entities::extract;
use debrief_core::ports::StoreResult;
use duckdb::{params, params_from_iter, Connection};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Layout and extraction version; sources built with another are built again (2: the tail mark, the left-out fields and
/// the GUID bound; stores made before are catalogued again, as no data is migrated before v1).
pub const VERSION: u32 = 2;

/// Rows of the catalogue tables of `source` are removed with this (the mark of live tail's appends is a separate
/// statement: a build replaces the rows but must keep a mark it has not read past, see [`settle_mark`]).
pub(crate) const DELETE_ROWS_SQL: [&str; 4] = [
    "DELETE FROM catalogue WHERE source = ?",
    "DELETE FROM catalogue_trigrams WHERE source = ?",
    "DELETE FROM catalogue_sources WHERE source = ?",
    "DELETE FROM catalogue_skipped WHERE source = ?",
];

/// The mark of the rows live tail appended to `source` since it was catalogued.
pub(crate) const DELETE_MARK_SQL: &str = "DELETE FROM catalogue_tail WHERE source = ?";

/// Everything the catalogue keeps of `source`, for a removal or a give-up: its rows and its mark.
pub(crate) const DELETE_SQL: [&str; 5] = [
    DELETE_MARK_SQL,
    DELETE_ROWS_SQL[0],
    DELETE_ROWS_SQL[1],
    DELETE_ROWS_SQL[2],
    DELETE_ROWS_SQL[3],
];

/// Removes the rows of the sources in `list` (comma-separated ids) or all rows with `None`.
pub(crate) fn forget(conn: &Connection, list: Option<&str>) -> duckdb::Result<()> {
    for t in [
        "catalogue",
        "catalogue_trigrams",
        "catalogue_sources",
        "catalogue_skipped",
        "catalogue_tail",
    ] {
        match list {
            Some(l) => conn.execute_batch(&format!("DELETE FROM {t} WHERE source IN ({l})"))?,
            None => conn.execute_batch(&format!("DELETE FROM {t}"))?,
        }
    }
    Ok(())
}

/// Sources that are ready and not catalogued with the current version.
fn pending(conn: &Connection) -> Result<Vec<u32>> {
    let mut stmt = conn
        .prepare("SELECT id FROM sources WHERE ready AND id NOT IN (SELECT source FROM catalogue_sources WHERE version = ?) ORDER BY id")?;
    let v = stmt
        .query_map(params![VERSION], |r| r.get::<_, u32>(0))?
        .collect::<duckdb::Result<Vec<_>>>()?;
    Ok(v)
}

/// What one read of a source's events found.
#[derive(Default)]
struct Read {
    entries: Vec<Entry>,
    /// Names the tally refused because it was full.
    dropped: u64,
    guids: u64,
    /// Events scanned, and the id of the last one.
    rows: u64,
    last_id: Option<u64>,
    /// The scan stopped at its bound: events after `last_id` are unread.
    truncated: bool,
    /// Fields left out because they have too many distinct values.
    skipped: Vec<String>,
}

/// Reads the events of `source` with an id from `from` to `upto` (none: the source has no events) into a tally: entities
/// of the text of the events, then the values of their low-cardinality fields. `ordered` reads them by id (a tail read
/// that may stop at the scan bound must stop at a known id).
fn build_from(conn: &Connection, source: u32, from: u64, upto: Option<u64>, ordered: bool, heavy: &Mutex<()>) -> Result<Read> {
    let Some(upto) = upto else { return Ok(Read::default()) };
    let mut tally = Tally::default();
    let mut out = Read::default();
    let limit = scan_limit();
    {
        let order = if ordered { " ORDER BY id" } else { "" };
        let mut stmt = conn.prepare(&format!(
            "SELECT id, ts, message, exception FROM events WHERE source = ? AND id >= ? AND id <= ?{order} LIMIT ?"
        ))?;
        let mut rows = stmt.query(params![source, from, upto, limit])?;
        while let Some(r) = rows.next()? {
            out.rows += 1;
            // The process is exiting: stop reading (the source is catalogued again next time).
            if crate::shutdown::down() {
                anyhow::bail!("the process is exiting");
            }
            let (id, ts): (u64, Option<i64>) = (r.get(0)?, r.get(1)?);
            out.last_id = Some(id);
            let message: String = r.get(2)?;
            let exception: Option<String> = r.get(3)?;
            let mut texts = vec![message.as_str()];
            if let Some(e) = exception.as_deref() {
                texts.push(e);
            }
            let found = extract(&texts);
            tally.add_event(&found, id, ts);
        }
    }
    out.truncated = out.rows >= limit;
    // The field values are read over the rows the scan above reached, not beyond: the rest is the next read's.
    let upto = if out.truncated { out.last_id.unwrap_or(upto) } else { upto };
    let sql = format!(
        "WITH kv AS (SELECT unnest(map_keys(extra)) AS k, unnest(map_values(extra)) AS v FROM events WHERE source = ? AND id >= ? AND id <= ?),
g AS (SELECT k, v, count(*) AS n FROM kv WHERE v IS NOT NULL AND length(v) BETWEEN 1 AND {} AND TRY_CAST(v AS DOUBLE) IS NULL GROUP BY k, v),
d AS (SELECT k FROM g GROUP BY k HAVING count(*) <= {MAX_FIELD_DISTINCT}),
r AS (SELECT g.k, g.v, g.n, row_number() OVER (PARTITION BY g.k ORDER BY g.n DESC, g.v) AS rk FROM g JOIN d USING (k))
SELECT k, v, n, FALSE FROM r WHERE rk <= {MAX_VALUES_PER_FIELD}
UNION ALL
SELECT k, NULL, 0, TRUE FROM g GROUP BY k HAVING count(*) > {MAX_FIELD_DISTINCT}",
        debrief_core::entities::MAX_NAME_CHARS
    );
    // The aggregate holds a hash table of the source's fields: never beside an import's ordering (the store's `heavy`
    // lock), so the catalogue cannot starve it of the memory limit.
    let _heavy = lock(heavy);
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![source, from, upto])?;
    while let Some(r) = rows.next()? {
        let (k, v, n, left_out): (String, Option<String>, u64, bool) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
        match v {
            Some(v) if !left_out => tally.add(Scope::Field(k), &v, 0, None, n),
            _ => out.skipped.push(k),
        }
    }
    (out.dropped, out.guids) = (tally.dropped, tally.guids);
    out.entries = tally.finish();
    Ok(out)
}

/// Rows (names and trigrams) committed since the last checkpoint that end a run of builds early: the store's own
/// checkpoint threshold is 1 GB (`open_db`), so without this every row of a long build stayed in memory, counted against
/// DuckDB's memory limit, until the build ended (about 60 bytes a row: 56 MB for 346 sources, over 190 MB at 7.6 GB).
const CHECKPOINT_AFTER_ROWS: u64 = 100_000;

/// Writes the rows of a source (replacing older ones) if it is still ready; the rows written, or `None` when it is not.
/// The mark of live tail's appends stays (rows appended while the build ran); `seen_to` is the newest id the build read.
fn commit(conn: &Connection, source: u32, read: &Read, seen_to: Option<u64>) -> Result<Option<u64>> {
    let ready: bool = conn.query_row("SELECT count(*) > 0 FROM sources WHERE id = ? AND ready", params![source], |r| {
        r.get(0)
    })?;
    if !ready {
        return Ok(None);
    }
    let entries = &read.entries;
    conn.execute_batch("BEGIN TRANSACTION")?;
    let written = (|| -> Result<u64> {
        let mut rows = entries.len() as u64;
        for sql in DELETE_ROWS_SQL {
            conn.execute(sql, params![source])?;
        }
        let mut app = conn.appender("catalogue")?;
        for e in entries {
            app.append_row(params![
                source,
                e.scope.encode(),
                e.name,
                squash(&e.name),
                e.count,
                e.first_ts,
                e.last_ts,
                e.first_id,
                e.last_id
            ])?;
        }
        app.flush()?;
        let mut app = conn.appender("catalogue_trigrams")?;
        for e in entries {
            for t in trigram_keys(&squash(&e.name)) {
                app.append_row(params![source, t, e.scope.encode(), e.name])?;
                rows += 1;
            }
        }
        app.flush()?;
        for field in &read.skipped {
            conn.execute("INSERT INTO catalogue_skipped VALUES (?, ?)", params![source, field])?;
        }
        conn.execute(
            "INSERT INTO catalogue_sources VALUES (?, ?, ?, ?, ?)",
            params![source, entries.len() as u64, read.dropped, read.guids, VERSION],
        )?;
        // Rows appended while the source was built are marked and read by the tail step; those the build read are not.
        settle_mark(conn, source, seen_to)?;
        Ok(rows)
    })();
    match written {
        Ok(rows) => {
            conn.execute_batch("COMMIT")?;
            Ok(Some(rows))
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

// Test seams: a callback run between reading the newest id of a source and reading its rows (a live append landing at
// that moment), and a smaller bound on the events a read scans.
#[cfg(test)]
type AppendHook = Box<dyn FnOnce(&Connection)>;

#[cfg(test)]
thread_local! {
    static AFTER_NEWEST: std::cell::RefCell<Option<AppendHook>> = const { std::cell::RefCell::new(None) };
    static SCAN_LIMIT: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// The most events one read scans.
fn scan_limit() -> u64 {
    #[cfg(test)]
    if let Some(l) = SCAN_LIMIT.with(|c| c.get()) {
        return l;
    }
    MAX_EVENTS_SCANNED as u64
}

/// The newest event id of `source`, read before a build so the rows appended while it runs stay marked.
// Relies on new rows becoming visible as a prefix by id: a row with an id above this one is not read, and is marked.
fn newest_id(conn: &Connection, source: u32) -> Result<Option<u64>> {
    let newest = conn.query_row("SELECT max(id) FROM events WHERE source = ?", params![source], |r| r.get(0))?;
    #[cfg(test)]
    if let Some(hook) = AFTER_NEWEST.with(|h| h.borrow_mut().take()) {
        hook(conn);
    }
    Ok(newest)
}

/// After a read of the rows up to `seen_to`: the source's mark moves past them, and goes when no newer row exists.
fn settle_mark(conn: &Connection, source: u32, seen_to: Option<u64>) -> Result<()> {
    if let Some(hw) = seen_to {
        conn.execute(
            "UPDATE catalogue_tail SET from_id = ? WHERE source = ? AND from_id <= ?",
            params![hw + 1, source, hw],
        )?;
    }
    conn.execute(
        "DELETE FROM catalogue_tail WHERE source = ? AND from_id > (SELECT coalesce(max(id), 0) FROM events WHERE source = ?)",
        params![source, source],
    )?;
    Ok(())
}

/// Adds the names of the rows appended to `source` (`read`) to its rows, as a build would have: a name it already has
/// counts more events and widens its first and last seen, a new name is added (with its trigrams) within the bounds of
/// a build (names of the source, values of one field, GUIDs; those refused count as dropped), and a field the build
/// left out stays out. Set-based: the names go into the staging table once, then one UPDATE and one INSERT per table.
/// The mark moves past what was read (`read_to`) and goes if nothing newer exists; `None` when the source is not ready.
fn commit_tail(conn: &Connection, source: u32, read_to: Option<u64>, read: &Read) -> Result<Option<u64>> {
    // The source is ready and catalogued with the current version: a give-up or a rebuild since the read leaves nothing
    // to add to.
    let current: bool = conn.query_row(
        "SELECT count(*) > 0 FROM sources s WHERE s.id = ? AND s.ready
           AND EXISTS (SELECT 1 FROM catalogue_sources c WHERE c.source = s.id AND c.version = ?)",
        params![source, VERSION],
        |r| r.get(0),
    )?;
    if !current {
        return Ok(None);
    }
    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS catalogue_stage (
           source UINTEGER NOT NULL, scope VARCHAR NOT NULL, name VARCHAR NOT NULL, n UBIGINT NOT NULL, first_ts BIGINT,
           last_ts BIGINT, first_id UBIGINT NOT NULL, last_id UBIGINT NOT NULL)",
    )?;
    conn.execute_batch("BEGIN TRANSACTION")?;
    let written = (|| -> Result<u64> {
        let mut dropped = read.dropped;
        for field in &read.skipped {
            conn.execute(
                "INSERT INTO catalogue_skipped SELECT ?, ? WHERE NOT EXISTS (SELECT 1 FROM catalogue_skipped WHERE source = ? AND field = ?)",
                params![source, field, source, field],
            )?;
        }
        let left_out: HashSet<String> = {
            let mut stmt = conn.prepare("SELECT field FROM catalogue_skipped WHERE source = ?")?;
            let fields = stmt
                .query_map(params![source], |r| r.get::<_, String>(0))?
                .collect::<duckdb::Result<_>>()?;
            fields
        };
        let mut staged = 0u64;
        {
            let mut app = conn.appender("catalogue_stage")?;
            for e in &read.entries {
                if matches!(&e.scope, Scope::Field(k) if left_out.contains(k)) {
                    // `dropped` counts these values on tail reads only (a full build never reads a left-out field's
                    // values into the tally); it is a diagnostic of what the catalogue refused, not a bound.
                    dropped += 1;
                    continue;
                }
                app.append_row(params![
                    source,
                    e.scope.encode(),
                    e.name,
                    e.count,
                    e.first_ts,
                    e.last_ts,
                    e.first_id,
                    e.last_id
                ])?;
                staged += 1;
            }
            app.flush()?;
        }
        let mut rows = 0u64;
        let mut added = 0u64;
        if staged > 0 {
            rows += conn.execute(
                "UPDATE catalogue AS c SET n = c.n + t.n,
                   first_ts = CASE WHEN t.first_ts IS NULL THEN c.first_ts WHEN c.first_ts IS NULL THEN t.first_ts ELSE least(c.first_ts, t.first_ts) END,
                   last_ts = CASE WHEN t.last_ts IS NULL THEN c.last_ts WHEN c.last_ts IS NULL THEN t.last_ts ELSE greatest(c.last_ts, t.last_ts) END,
                   first_id = least(c.first_id, t.first_id), last_id = greatest(c.last_id, t.last_id)
                 FROM catalogue_stage t
                 WHERE c.source = ? AND t.source = c.source AND c.scope = t.scope AND lower(c.name) = lower(t.name)",
                params![source],
            )? as u64;
            // The names it did not have, most events first (so a bound keeps the commonest).
            let fresh: Vec<Entry> = {
                let mut stmt = conn.prepare(
                    "SELECT t.scope, t.name, t.n, t.first_ts, t.last_ts, t.first_id, t.last_id FROM catalogue_stage t
                     WHERE t.source = ? AND NOT EXISTS (
                       SELECT 1 FROM catalogue c WHERE c.source = t.source AND c.scope = t.scope AND lower(c.name) = lower(t.name))
                     ORDER BY t.n DESC, t.name, t.scope",
                )?;
                let rows = stmt.query_map(params![source], |r| {
                    let scope: String = r.get(0)?;
                    Ok((scope, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
                })?;
                let mut v = Vec::new();
                for row in rows {
                    let (scope, name, count, first_ts, last_ts, first_id, last_id) = row?;
                    if let Some(scope) = Scope::decode(&scope) {
                        v.push(Entry {
                            scope,
                            name,
                            count,
                            first_ts,
                            last_ts,
                            first_id,
                            last_id,
                        });
                    }
                }
                v
            };
            if !fresh.is_empty() {
                let mut per_scope: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
                {
                    let mut stmt = conn.prepare("SELECT scope, count(*) FROM catalogue WHERE source = ? GROUP BY scope")?;
                    let counts = stmt.query_map(params![source], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))?;
                    for c in counts {
                        let (scope, n) = c?;
                        per_scope.insert(scope, n);
                    }
                }
                let mut names: u64 = per_scope.values().sum();
                let guid_scope = Scope::Entity(debrief_core::entities::EntityKind::Guid).encode();
                let mut names_app = conn.appender("catalogue")?;
                let mut tri_app = conn.appender("catalogue_trigrams")?;
                for e in &fresh {
                    let scope = e.scope.encode();
                    let have = per_scope.get(&scope).copied().unwrap_or(0);
                    let cap = match &e.scope {
                        Scope::Field(_) => MAX_VALUES_PER_FIELD as u64,
                        _ if scope == guid_scope => MAX_GUIDS as u64,
                        _ => u64::MAX,
                    };
                    if names >= MAX_NAMES as u64 || have >= cap {
                        dropped += 1;
                        continue;
                    }
                    names_app.append_row(params![
                        source,
                        scope,
                        e.name,
                        squash(&e.name),
                        e.count,
                        e.first_ts,
                        e.last_ts,
                        e.first_id,
                        e.last_id
                    ])?;
                    for t in trigram_keys(&squash(&e.name)) {
                        tri_app.append_row(params![source, t, scope, e.name])?;
                        rows += 1;
                    }
                    per_scope.insert(scope, have + 1);
                    names += 1;
                    added += 1;
                    rows += 1;
                }
                names_app.flush()?;
                tri_app.flush()?;
            }
            conn.execute("DELETE FROM catalogue_stage WHERE source = ?", params![source])?;
        }
        conn.execute(
            "UPDATE catalogue_sources SET names = names + ?, dropped = dropped + ?, guids = guids + ? WHERE source = ?",
            params![added, dropped, read.guids, source],
        )?;
        settle_mark(conn, source, read_to)?;
        Ok(rows)
    })();
    match written {
        Ok(rows) => {
            conn.execute_batch("COMMIT")?;
            Ok(Some(rows))
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// A counter that moves whenever the catalogue's state may have changed (news, a commit, a checkpoint), and that callers
/// can block on: [`crate::Inner::catalogue_status`] waits here instead of being asked again and again.
struct Changes {
    count: Mutex<u64>,
    moved: Condvar,
}

impl Changes {
    fn new() -> Changes {
        Changes {
            count: Mutex::new(0),
            moved: Condvar::new(),
        }
    }

    fn bump(&self) {
        *lock(&self.count) += 1;
        self.moved.notify_all();
    }

    fn get(&self) -> u64 {
        *lock(&self.count)
    }

    /// Waits until the counter plus the store's `generation` is not `seen` any more, at most `max`.
    fn wait_past(&self, seen: u64, max: Duration, generation: &AtomicU64) {
        let guard = lock(&self.count);
        let _ = self
            .moved
            .wait_timeout_while(guard, max, |n| *n + generation.load(Ordering::SeqCst) == seen)
            .unwrap_or_else(|p| p.into_inner());
    }
}

struct Shared {
    stop: AtomicBool,
    wake: (Mutex<bool>, Condvar),
    /// Sources not catalogued yet, as last seen.
    pending: AtomicU64,
    /// Sources whose build failed (not retried until the store is reopened).
    failed: Mutex<HashSet<u32>>,
    /// Rows committed since the last checkpoint (see [`settle`]); the catalogue counts as built only once they are saved.
    unsaved: AtomicBool,
    /// How many rows they are ([`CHECKPOINT_AFTER_ROWS`]).
    unsaved_rows: AtomicU64,
    /// The most unsaved rows there were at any moment, the rows ever committed and the largest single commit (the hook
    /// of [`RowStats`]).
    rows_high: AtomicU64,
    rows_committed: AtomicU64,
    rows_largest: AtomicU64,
    /// The store's lock of heavy DuckDB work (`Inner::heavy`).
    heavy: Arc<Mutex<()>>,
    /// Checkpoints run under / outside an [`crate::shutdown::Active`] guard (tests: the exit handler must count each).
    #[cfg(test)]
    checkpoints: (AtomicU64, AtomicU64),
    /// Whether the thread dropped its connection after the process began to exit (tests).
    #[cfg(test)]
    dropped_while_exiting: AtomicBool,
    /// Whether anything changed since the last look; without news a turn runs no statement.
    gate: Gate,
    /// What status callers wait on.
    changes: Changes,
}

/// What the catalogue's builds did with their rows, kept by the service so a test can check the bound on the unsaved
/// rows without sampling the store's memory from outside (a sample can miss the peak).
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowStats {
    /// The most rows committed and not yet checkpointed at any moment.
    pub high_water: u64,
    /// Rows committed by all builds.
    pub committed: u64,
    /// The rows of the largest single commit.
    pub largest_commit: u64,
    /// The unsaved rows that make a build checkpoint.
    pub checkpoint_after: u64,
}

/// The catalogue thread of a store; stopped and joined on drop.
pub(crate) struct CatalogueService {
    shared: Arc<Shared>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl CatalogueService {
    pub(crate) fn start(
        conn: Connection,
        write_lock: Arc<Mutex<()>>,
        heavy: Arc<Mutex<()>>,
        generation: Arc<AtomicU64>,
    ) -> Result<CatalogueService> {
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            wake: (Mutex::new(true), Condvar::new()),
            pending: AtomicU64::new(0),
            failed: Mutex::new(HashSet::new()),
            unsaved: AtomicBool::new(false),
            unsaved_rows: AtomicU64::new(0),
            rows_high: AtomicU64::new(0),
            rows_committed: AtomicU64::new(0),
            rows_largest: AtomicU64::new(0),
            heavy,
            #[cfg(test)]
            checkpoints: (AtomicU64::new(0), AtomicU64::new(0)),
            #[cfg(test)]
            dropped_while_exiting: AtomicBool::new(false),
            gate: Gate::new(generation),
            changes: Changes::new(),
        });
        crate::shutdown::interrupt_on_exit(&conn);
        let s = shared.clone();
        let for_exit = shared.clone();
        // Listed before it starts, so the exit drain and the tests' guard always see it; the thread drops it last.
        let alive = crate::shutdown::register_thread(
            "debrief-catalogue",
            Arc::new(move || {
                for_exit.stop.store(true, Ordering::SeqCst);
                for_exit.gate.news();
                for_exit.changes.bump();
                *lock(&for_exit.wake.0) = true;
                for_exit.wake.1.notify_all();
            }),
        );
        let handle = std::thread::Builder::new().name("debrief-catalogue".into()).spawn(move || {
            let _alive = alive;
            run(&s, &conn, &write_lock);
            if crate::shutdown::down() {
                // `run` returns at exit too. Dropping the connection then would call duckdb_disconnect (and
                // duckdb_close for the last handle of the database) while C++ destroys DuckDB's statics: the crash
                // the exit handler exists to prevent. The process is exiting, so the OS reclaims what is leaked here.
                std::mem::forget(conn);
                return;
            }
            // A drop that still meets an exit begun since the check above (tests record it).
            #[cfg(test)]
            if crate::shutdown::down() {
                s.dropped_while_exiting.store(true, Ordering::SeqCst);
            }
            drop(conn);
        })?;
        Ok(CatalogueService {
            shared,
            handle: Mutex::new(Some(handle)),
        })
    }

    /// The row counters of the builds so far.
    pub(crate) fn row_stats(&self) -> RowStats {
        RowStats {
            high_water: self.shared.rows_high.load(Ordering::SeqCst),
            committed: self.shared.rows_committed.load(Ordering::SeqCst),
            largest_commit: self.shared.rows_largest.load(Ordering::SeqCst),
            checkpoint_after: CHECKPOINT_AFTER_ROWS,
        }
    }

    /// How many of the `todo` sources a build failed on (they are not tried again until the store is reopened).
    pub(crate) fn failed_among(&self, todo: &[u32]) -> u64 {
        let failed = lock(&self.shared.failed);
        todo.iter().filter(|id| failed.contains(id)).count() as u64
    }

    /// Whether rows committed by the last builds still wait for their checkpoint.
    pub(crate) fn saving(&self) -> bool {
        self.shared.unsaved.load(Ordering::SeqCst)
    }

    /// The counter [`crate::Inner::catalogue_status`] answers with.
    pub(crate) fn generation(&self) -> u64 {
        self.shared.changes.get()
    }

    /// Waits until the catalogue's state, or the store's data (`generation`), changed after `seen`, at most `max`. A
    /// change of the data alone does not wake the wait (the loops are told by [`wake`](Self::wake)); the next call sees it.
    pub(crate) fn wait_for_change(&self, seen: u64, max: Duration, generation: &AtomicU64) {
        self.shared.changes.wait_past(seen, max, generation);
    }

    pub(crate) fn wake(&self) {
        self.shared.gate.news();
        self.shared.changes.bump();
        *lock(&self.shared.wake.0) = true;
        self.shared.wake.1.notify_all();
    }
}

impl CatalogueService {
    /// Whether the thread was joined (tests).
    #[cfg(test)]
    pub(crate) fn joined(&self) -> bool {
        lock(&self.handle).is_none()
    }

    /// Asks the thread to end, without waiting.
    pub(crate) fn request_stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.wake();
    }

    /// Stops the thread and waits for it to end (idempotent; the caller bounds the wait, see [`crate::DuckStore::stop_for_exit`]).
    pub(crate) fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.wake();
        let handle = lock(&self.handle).take();
        if let Some(h) = handle {
            let _ = h.join();
        }
    }
}

impl Drop for CatalogueService {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One turn: tidy, then build the next source. `running` proves the exit handler counts its statements.
fn step(running: &crate::shutdown::Active, s: &Shared, conn: &Connection, write_lock: &Mutex<()>) -> Result<Turn> {
    // Nothing changed since the last look: nothing to tidy and nothing new to build (and no statement to run).
    if !s.gate.take() {
        return Ok(Turn::Idle);
    }
    let todo = {
        let Ok(_writing) = write_lock.try_lock() else {
            s.gate.news();
            return Ok(Turn::Blocked);
        };
        conn.execute_batch(
            "DELETE FROM catalogue WHERE source NOT IN (SELECT id FROM sources);
             DELETE FROM catalogue_trigrams WHERE source NOT IN (SELECT id FROM sources);
             DELETE FROM catalogue_sources WHERE source NOT IN (SELECT id FROM sources);
             DELETE FROM catalogue_skipped WHERE source NOT IN (SELECT id FROM sources);
             DELETE FROM catalogue_tail WHERE source NOT IN (SELECT id FROM sources);",
        )?;
        pending(conn)?
    };
    let failed = lock(&s.failed).clone();
    let todo: Vec<u32> = todo.into_iter().filter(|id| !failed.contains(id)).collect();
    s.pending.store(todo.len() as u64, Ordering::SeqCst);
    let Some(&source) = todo.first() else {
        return step_tail(running, s, conn, write_lock);
    };
    // What the build reads: events up to this id. Rows appended meanwhile stay marked for the tail step.
    let seen_to = newest_id(conn, source)?;
    let read = match build_from(conn, source, 0, seen_to, false, &s.heavy) {
        Ok(b) => b,
        Err(e) => {
            // A source whose events cannot be read now (compaction swapped its files) is tried again later.
            lock(&s.failed).insert(source);
            return Err(e.context(format!("cataloguing source {source}")));
        }
    };
    // The write lock is taken without waiting for an import: the next turn tries again.
    let Ok(writing) = write_lock.try_lock() else {
        s.gate.news();
        return Ok(Turn::Blocked);
    };
    s.unsaved.store(true, Ordering::SeqCst);
    if crate::shutdown::down() {
        return Ok(Turn::Idle);
    }
    let written = commit(conn, source, &read, seen_to)?;
    drop(writing);
    s.pending.store(todo.len() as u64 - 1, Ordering::SeqCst);
    s.changes.bump();
    Ok(finish_turn(running, s, conn, write_lock, written))
}

/// The turn that follows a commit: its rows count toward the checkpoint.
fn finish_turn(running: &crate::shutdown::Active, s: &Shared, conn: &Connection, write_lock: &Mutex<()>, written: Option<u64>) -> Turn {
    // A long run of builds does not keep its rows in memory until it ends.
    let unsaved = s.unsaved_rows.fetch_add(written.unwrap_or(0), Ordering::SeqCst) + written.unwrap_or(0);
    s.rows_high.fetch_max(unsaved, Ordering::SeqCst);
    s.rows_committed.fetch_add(written.unwrap_or(0), Ordering::SeqCst);
    s.rows_largest.fetch_max(written.unwrap_or(0), Ordering::SeqCst);
    if unsaved >= CHECKPOINT_AFTER_ROWS && settle(running, s, conn, write_lock) {
        s.unsaved_rows.store(0, Ordering::SeqCst);
        s.unsaved.store(false, Ordering::SeqCst);
        s.changes.bump();
    }
    Turn::Worked
}

/// A tail read of at least this many rows is logged at `info`; smaller ones at `debug` (a live log appends constantly).
const LOG_INFO_ROWS: u64 = 10_000;

/// Reads again the rows live tail appended to one catalogued source (the first marked in `catalogue_tail`) and adds
/// their names to its rows; nothing else of the source is read. Idle when no source is marked. A read or a commit that
/// fails is logged and the mark stays: the loop goes idle and tries again on the next change of the store, not at once.
fn step_tail(running: &crate::shutdown::Active, s: &Shared, conn: &Connection, write_lock: &Mutex<()>) -> Result<Turn> {
    let marked: Option<(u32, u64)> = {
        let mut stmt = conn.prepare(
            "SELECT t.source, t.from_id FROM catalogue_tail t
             WHERE t.source IN (SELECT source FROM catalogue_sources WHERE version = ?) ORDER BY t.source LIMIT 1",
        )?;
        let mut rows = stmt.query(params![VERSION])?;
        rows.next()?.map(|r| Ok::<_, duckdb::Error>((r.get(0)?, r.get(1)?))).transpose()?
    };
    let Some((source, from_id)) = marked else { return Ok(Turn::Idle) };
    let started = std::time::Instant::now();
    let read = newest_id(conn, source).and_then(|seen_to| Ok((seen_to, build_from(conn, source, from_id, seen_to, true, &s.heavy)?)));
    let (seen_to, read) = match read {
        Ok(r) => r,
        Err(e) => {
            log::warn!("catalogue: the rows appended to source {source} could not be read, tried again at the next change: {e:#}");
            return Ok(Turn::Idle);
        }
    };
    // A read that stopped at the scan bound goes on from the last row it read.
    let read_to = if read.truncated { read.last_id } else { seen_to };
    let Ok(writing) = write_lock.try_lock() else {
        s.gate.news();
        return Ok(Turn::Blocked);
    };
    s.unsaved.store(true, Ordering::SeqCst);
    if crate::shutdown::down() {
        return Ok(Turn::Idle);
    }
    let written = match commit_tail(conn, source, read_to, &read) {
        Ok(w) => w,
        Err(e) => {
            log::warn!(
                "catalogue: the names of the rows appended to source {source} could not be saved, tried again at the next change: {e:#}"
            );
            return Ok(Turn::Idle);
        }
    };
    drop(writing);
    s.changes.bump();
    let summary = format!(
        "catalogue: source {source} re-catalogued from its appended rows: {} rows read{}, {} names, {} rows written, {} ms",
        read.rows,
        if read.truncated { " (more follow)" } else { "" },
        read.entries.len(),
        written.unwrap_or(0),
        started.elapsed().as_millis()
    );
    if read.rows >= LOG_INFO_ROWS {
        log::info!("{summary}");
    } else {
        log::debug!("{summary}");
    }
    Ok(finish_turn(running, s, conn, write_lock, written))
}

/// Appends a line to the file `DEBRIEF_EXIT_HOLD` names (test hook; see [`hold_for_exit_tests`]).
#[cfg(all(feature = "exit-hold", debug_assertions))]
pub(crate) fn note_exit_hold(text: &str) {
    use std::io::Write;
    let Some(path) = std::env::var_os("DEBRIEF_EXIT_HOLD") else {
        return;
    };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{text}");
    }
}

/// Test hook (feature `exit-hold` in a debug build; a release build never contains it): when the environment names a file in `DEBRIEF_EXIT_HOLD`, the
/// thread stays inside DuckDB (a statement that fails in the binder, outside the exit handler's count) until it is
/// stopped, and records `held` when it starts and `stopped` when it ends. A worker whose `main` does not stop and join
/// this thread before it exits never writes `stopped`; one that exits with the thread live can crash in DuckDB's static
/// destructors (the store worker's exit crash, ADR 0076 amendment WP-X0).
#[cfg(all(feature = "exit-hold", debug_assertions))]
fn hold_for_exit_tests(s: &Shared, conn: &Connection) {
    use std::io::Write;
    let Some(path) = std::env::var_os("DEBRIEF_EXIT_HOLD") else {
        return;
    };
    let note = |text: &str| {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(f, "{text}");
        }
    };
    note("held");
    while !s.stop.load(Ordering::SeqCst) {
        let _ = conn.execute_batch("SELECT i FROM range(1)");
    }
    note("stopped");
}

fn run(s: &Shared, conn: &Connection, write_lock: &Mutex<()>) {
    #[cfg(all(feature = "exit-hold", debug_assertions))]
    hold_for_exit_tests(s, conn);
    while !s.stop.load(Ordering::SeqCst) {
        // Every statement of a turn (the step and the checkpoint after it) runs under one guard the exit handler counts
        // (see `fts::step`): a checkpoint outside it ran on while C++ destroyed DuckDB's statics at exit and crashed
        // the process with SIGSEGV (CI 7837). None once the process is exiting: the thread ends, no statement may start.
        let Some(running) = crate::shutdown::Active::enter() else {
            return;
        };
        let turn = match step(&running, s, conn, write_lock) {
            Ok(t) => t,
            Err(e) => {
                log::warn!("catalogue: {e:#}");
                Turn::Blocked
            }
        };
        s.gate.after(turn);
        let Some(wait) = wait_after(turn) else { continue };
        if s.unsaved.load(Ordering::SeqCst) && settle(&running, s, conn, write_lock) {
            s.unsaved.store(false, Ordering::SeqCst);
            s.unsaved_rows.store(0, Ordering::SeqCst);
            s.changes.bump();
        }
        // Not held while waiting: the exit handler waits for statements, never for an idle thread.
        drop(running);
        let mut woken = lock(&s.wake.0);
        if !*woken {
            woken = s.wake.1.wait_timeout(woken, wait).map(|r| r.0).unwrap_or_else(|p| p.into_inner().0);
        }
        *woken = false;
    }
}

/// Ends a run of builds like every other change to the store (see `open_db`): a checkpoint writes the new rows into
/// the database file, then the run's freed pages go back to the OS. Without it a worker that had catalogued 20 copies
/// of the samples idled at about 440 MB (130 MB once reopened), DuckDB holding 145 MB of the uncheckpointed rows
/// (docs/performance.md, 2026-10-31).
/// False when an import holds the write lock (it checkpoints when it ends; the next idle turn tries again), or when the
/// process is exiting (no new statement; `_running` keeps the exit handler waiting for one already running).
#[cfg_attr(not(test), allow(unused_variables))] // `s` only records the checkpoints for the tests
fn settle(_running: &crate::shutdown::Active, s: &Shared, conn: &Connection, write_lock: &Mutex<()>) -> bool {
    let Ok(_writing) = write_lock.try_lock() else { return false };
    if crate::shutdown::down() {
        return false;
    }
    #[cfg(test)]
    if crate::shutdown::held_here() {
        &s.checkpoints.0
    } else {
        &s.checkpoints.1
    }
    .fetch_add(1, Ordering::SeqCst);
    if let Err(e) = conn.execute_batch("CHECKPOINT") {
        log::warn!("catalogue: checkpoint failed: {e}");
    }
    debrief_alloc::release();
    true
}

impl crate::Inner {
    /// How far the catalogue is, waiting for news when `req` asks to (see [`CatalogueStatusRequest`]).
    pub(crate) fn catalogue_status(&self, req: &CatalogueStatusRequest) -> StoreResult<CatalogueStatus> {
        if let (Some(after), Some(c)) = (req.after, self.catalogue.as_ref()) {
            c.wait_for_change(after, Duration::from_millis(req.wait_ms.min(MAX_STATUS_WAIT_MS)), &self.generation);
        }
        // Moves with the catalogue's own changes and with every change of the store's data (a removal, a refresh, a
        // live-follow step), so a table built from it is stale exactly when the workspace changed. Read before the
        // state: a change after it makes the next call return at once.
        let generation = self.catalogue.as_ref().map_or(0, |c| c.generation()) + self.generation.load(Ordering::SeqCst);
        let conn = self.conn()?;
        let (pending, failed) = self.outstanding(&conn)?;
        Ok(CatalogueStatus {
            built: pending == 0 && !self.catalogue.as_ref().is_some_and(|c| c.saving()),
            pending,
            failed,
            generation,
        })
    }

    /// The ready sources still to catalogue and the ones a build gave up on (those count as done: waiting for them
    /// would never end).
    fn outstanding(&self, conn: &Connection) -> StoreResult<(u64, u64)> {
        let todo = pending(conn).map_err(|e| engine(format!("{e:#}")))?;
        let failed = self.catalogue.as_ref().map_or(0, |c| c.failed_among(&todo));
        Ok((todo.len() as u64 - failed, failed))
    }

    /// Names that look like `req.text`, from the catalogue.
    pub(crate) fn catalogue(&self, req: &CatalogueRequest) -> StoreResult<CatalogueReply> {
        let conn = self.conn()?;
        let (pending, _failed) = self.outstanding(&conn)?;
        let text: String = req.text.trim().chars().take(debrief_core::find::MAX_TEXT_CHARS).collect();
        let keys = search_keys(&text);
        let squashed = squash(&text);
        let mut reply = CatalogueReply {
            built: pending == 0 && !self.catalogue.as_ref().is_some_and(|c| c.saving()),
            pending,
            candidates: Vec::new(),
        };
        if squashed.is_empty() && !req.all {
            return Ok(reply);
        }
        let (sql, args): (String, Vec<String>) = if req.all {
            (
                // The totals first, with nothing but numbers per name (a hash table of up to a million names must fit the
                // query budget); only the names that make the cut get their sources listed, from the same rows.
                format!(
                    "WITH t AS (SELECT e.scope, e.name, sum(e.n) AS n, min(e.first_ts) AS f, max(e.last_ts) AS l
                         FROM catalogue e JOIN sources s ON s.id = e.source AND s.ready
                         GROUP BY e.scope, e.name ORDER BY n DESC, e.name LIMIT {})
                     SELECT t.scope, t.name, t.n, t.f, t.l, list(DISTINCT s.name)
                     FROM t JOIN catalogue e ON e.scope = t.scope AND e.name = t.name
                     JOIN sources s ON s.id = e.source AND s.ready
                     GROUP BY t.scope, t.name, t.n, t.f, t.l ORDER BY t.n DESC, t.name",
                    debrief_core::catalogue::MAX_NAMES
                ),
                Vec::new(),
            )
        } else if keys.is_empty() {
            (
                format!(
                    "SELECT e.scope, e.name, sum(e.n), min(e.first_ts), max(e.last_ts), list(DISTINCT s.name)
                     FROM catalogue e JOIN sources s ON s.id = e.source AND s.ready
                     WHERE e.norm LIKE ? ESCAPE '\\' GROUP BY e.scope, e.name ORDER BY sum(e.n) DESC, e.name LIMIT {MAX_CANDIDATES}"
                ),
                vec![format!(
                    "{}%",
                    squashed.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
                )],
            )
        } else {
            let marks = vec!["?"; keys.len()].join(", ");
            (
                format!(
                    "WITH c AS (SELECT scope, name, count(DISTINCT tri) AS ov FROM catalogue_trigrams
                         WHERE source IN (SELECT id FROM sources WHERE ready) AND tri IN ({marks})
                         GROUP BY scope, name ORDER BY ov DESC, name LIMIT {MAX_CANDIDATES})
                     SELECT e.scope, e.name, sum(e.n), min(e.first_ts), max(e.last_ts), list(DISTINCT s.name)
                     FROM c JOIN catalogue e ON e.scope = c.scope AND e.name = c.name
                     JOIN sources s ON s.id = e.source AND s.ready
                     GROUP BY e.scope, e.name, c.ov ORDER BY c.ov DESC, sum(e.n) DESC, e.name"
                ),
                keys,
            )
        };
        let mut stmt = conn.prepare(&sql).map_err(engine)?;
        let mut rows = stmt.query(params_from_iter(args.iter())).map_err(engine)?;
        while let Some(r) = rows.next().map_err(engine)? {
            let scope: String = r.get(0).map_err(engine)?;
            let Some(scope) = Scope::decode(&scope) else { continue };
            let seen: duckdb::types::Value = r.get(5).map_err(engine)?;
            let mut seen_in = match seen {
                duckdb::types::Value::List(l) => l
                    .into_iter()
                    .filter_map(|v| if let duckdb::types::Value::Text(t) = v { Some(t) } else { None })
                    .collect(),
                _ => Vec::new(),
            };
            seen_in.sort();
            seen_in.truncate(MAX_SEEN_IN);
            reply.candidates.push(Candidate {
                scope,
                name: r.get(1).map_err(engine)?,
                count: r.get::<_, f64>(2).map_err(engine)? as u64,
                first_ts: r.get(3).map_err(engine)?,
                last_ts: r.get(4).map_err(engine)?,
                seen_in,
            });
        }
        Ok(reply)
    }
}

impl crate::DuckStore {
    /// Waits until every ready source is catalogued (tests and benchmarks), at most `timeout`; true when it is.
    /// Makes `source` look like one whose catalogue build failed: its rows are removed and the service has given up on it
    /// (as after an error in its build). A hook for tests.
    #[doc(hidden)]
    pub fn catalogue_give_up_on(&self, source: u32) -> StoreResult<()> {
        let service = self.inner.catalogue.as_ref().ok_or_else(|| engine("the store has no catalogue"))?;
        let conn = self.inner.conn()?;
        // Under the write lock and in one transaction, so it cannot interleave with a catalogue commit.
        let _writing = lock(&self.inner.write_lock);
        conn.execute_batch("BEGIN TRANSACTION").map_err(engine)?;
        let deleted = DELETE_SQL.iter().try_for_each(|sql| conn.execute(sql, params![source]).map(|_| ()));
        match deleted {
            Ok(()) => conn.execute_batch("COMMIT").map_err(engine)?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(engine(e));
            }
        }
        lock(&service.shared.failed).insert(source);
        Ok(())
    }

    /// The row counters of the catalogue builds so far (none without a catalogue service); a hook for tests.
    #[doc(hidden)]
    pub fn catalogue_row_stats(&self) -> Option<RowStats> {
        self.inner.catalogue.as_ref().map(|c| c.row_stats())
    }

    pub fn wait_for_catalogue(&self, timeout: Duration) -> StoreResult<bool> {
        let started = std::time::Instant::now();
        loop {
            let reply = self.inner.catalogue(&CatalogueRequest::default())?;
            if reply.built {
                return Ok(true);
            }
            if started.elapsed() > timeout {
                return Ok(false);
            }
            if let Some(c) = &self.inner.catalogue {
                c.wake();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turns::statement_log;
    use crate::DuckStore;
    use debrief_core::ports::Ingest;
    use debrief_core::{PluginRegistry, Profile};

    /// The store's own stop for the process's end (`DuckStore::stop_for_exit`, called by the store worker's `main`): both
    /// background threads are stopped and joined before it returns, so none of them runs DuckDB code when the process
    /// exits, and it says which thread it is waiting for (the caller names it if the bound runs out).
    #[test]
    fn stop_for_exit_joins_the_background_threads() {
        let dir = debrief_ipc::testing::scratch_dir("stop-for-exit");
        let store = crate::DuckStore::create(&dir, Profile::default_profile(), PluginRegistry::new()).unwrap();
        let samples = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/serilog-text");
        store.open(&[samples]).unwrap().wait().unwrap();
        let catalogue = store.inner.catalogue.as_ref().expect("a catalogue thread");
        let fts = store.inner.fts.as_ref().expect("an indexing thread");
        assert!(!catalogue.joined() && !fts.joined(), "both threads run");
        let said = Mutex::new(Vec::new());
        store.stop_for_exit(&|thread| lock(&said).push(thread));
        assert_eq!(*lock(&said), ["debrief-catalogue", "debrief-fts"]);
        assert!(catalogue.joined() && fts.joined(), "a background thread was not joined");
        // Again is harmless (a store whose threads are gone).
        store.stop_for_exit(&|_| {});
    }

    /// Regression (CI 7837, `debrief-app` on macOS: SIGSEGV after the test's own "ok", on the `debrief-catalogue`
    /// thread in `settle` -> `CHECKPOINT`, the main thread in the exit handlers): the catalogue's checkpoint ran
    /// outside the statements the exit handler counts, so the handler did not wait for it, and it ran on while C++
    /// destroyed DuckDB's statics (and the test helpers removed the store's folder).
    #[test]
    fn the_exit_handler_waits_for_the_catalogue_checkpoint() {
        let dir = debrief_ipc::testing::scratch_dir("catalogue-checkpoint-exit");
        let store = crate::DuckStore::create(&dir, Profile::default_profile(), PluginRegistry::new()).unwrap();
        let samples = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/serilog-text");
        store.open(&[samples]).unwrap().wait().unwrap();
        assert!(store.wait_for_catalogue(Duration::from_secs(120)).unwrap(), "catalogue built");
        let c = &store.inner.catalogue.as_ref().expect("a catalogue thread").shared;
        let (counted, uncounted) = (c.checkpoints.0.load(Ordering::SeqCst), c.checkpoints.1.load(Ordering::SeqCst));
        assert!(counted + uncounted > 0, "the probe saw no checkpoint");
        assert_eq!(
            uncounted, 0,
            "{uncounted} catalogue checkpoint(s) ran outside the statements the exit handler waits for ({counted} inside)"
        );
        drop(store);
        crate::test_scratch::remove_or_defer(&dir);
    }

    const EXIT_CHILD: &str = "DEBRIEF_CATALOGUE_EXIT_CHILD";

    /// Runs in the child process only (setting the process's exit state cannot be undone): a store with catalogued
    /// rows, then the exit handler, then one more turn of the catalogue thread.
    #[test]
    fn a_turn_after_exit_began_child() {
        let Some(dir) = std::env::var_os(EXIT_CHILD) else { return };
        let store = crate::DuckStore::create(std::path::Path::new(&dir), Profile::default_profile(), PluginRegistry::new()).unwrap();
        let samples = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/serilog-text");
        store.open(&[samples]).unwrap().wait().unwrap();
        assert!(store.wait_for_catalogue(Duration::from_secs(120)).unwrap(), "catalogue built");
        let service = store.inner.catalogue.as_ref().expect("a catalogue thread");
        let c = service.shared.clone();
        let before = c.checkpoints.0.load(Ordering::SeqCst) + c.checkpoints.1.load(Ordering::SeqCst);
        // Rows to checkpoint, as when the process exits right after a build.
        c.unsaved.store(true, Ordering::SeqCst);
        crate::shutdown::begin_exit();
        service.wake();
        assert!(
            paddock_testkit::poll_until_within(Duration::from_secs(60), || {
                lock(&service.handle).as_ref().is_some_and(std::thread::JoinHandle::is_finished)
            }),
            "the catalogue thread did not end once exit began"
        );
        let after = c.checkpoints.0.load(Ordering::SeqCst) + c.checkpoints.1.load(Ordering::SeqCst);
        assert_eq!(after, before, "a checkpoint ran after exit began");
        assert!(
            !c.dropped_while_exiting.load(Ordering::SeqCst),
            "the catalogue thread dropped its connection after exit began (duckdb_disconnect/duckdb_close during exit)"
        );
        // The store is not dropped either: the process is exiting.
        std::mem::forget(store);
    }

    /// Regression (review of the CI 7837 fix): once exit has begun, the catalogue thread runs no checkpoint and ends
    /// without tearing DuckDB down (dropping its connection would disconnect, and close the database if it held the last
    /// handle, while C++ destroys DuckDB's statics).
    #[test]
    fn a_turn_after_exit_began_runs_no_statement_and_keeps_the_connection() {
        let dir = debrief_ipc::testing::scratch_dir("catalogue-exit-turn");
        let out = debrief_core::process::quiet_command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "catalogue::tests::a_turn_after_exit_began_child",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(EXIT_CHILD, &dir)
            .output()
            .unwrap();
        crate::test_scratch::remove_or_defer(&dir);
        assert!(
            out.status.success(),
            "{:?}\n{}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// H6 (docs/research/2026-10-28-cpu-memory.md): a catalogue loop with nothing to do and no news ran three deletes and
    /// a query every turn. An idle turn runs no statement at all.
    #[test]
    fn a_hundred_idle_turns_run_no_statement() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&crate::schema::ddl(&debrief_core::batch::EventSchema::plain()))
            .unwrap();
        let shared = Shared {
            stop: AtomicBool::new(false),
            wake: (Mutex::new(false), Condvar::new()),
            pending: AtomicU64::new(0),
            failed: Mutex::new(HashSet::new()),
            unsaved: AtomicBool::new(false),
            unsaved_rows: AtomicU64::new(0),
            rows_high: AtomicU64::new(0),
            rows_committed: AtomicU64::new(0),
            rows_largest: AtomicU64::new(0),
            heavy: Arc::new(Mutex::new(())),
            checkpoints: (AtomicU64::new(0), AtomicU64::new(0)),
            dropped_while_exiting: AtomicBool::new(false),
            gate: Gate::new(Arc::new(AtomicU64::new(0))),
            changes: Changes::new(),
        };
        let write_lock = Mutex::new(());
        let running = crate::shutdown::Active::enter().expect("the process is not exiting");
        // The first turn looks (it has never looked); after it the store is idle.
        assert_eq!(step(&running, &shared, &conn, &write_lock).unwrap(), Turn::Idle);
        statement_log::start(&conn);
        for _ in 0..100 {
            assert_eq!(step(&running, &shared, &conn, &write_lock).unwrap(), Turn::Idle);
        }
        let ran = statement_log::count(&conn);
        assert!(
            ran.is_empty(),
            "100 idle catalogue turns ran {} statements, the first: {:?}",
            ran.len(),
            ran.first()
        );
    }

    // ---- The tail step (rows live tail appended to a catalogued source), on an in-memory store ----

    fn test_shared() -> Shared {
        Shared {
            stop: AtomicBool::new(false),
            wake: (Mutex::new(false), Condvar::new()),
            pending: AtomicU64::new(0),
            failed: Mutex::new(HashSet::new()),
            unsaved: AtomicBool::new(false),
            unsaved_rows: AtomicU64::new(0),
            rows_high: AtomicU64::new(0),
            rows_committed: AtomicU64::new(0),
            rows_largest: AtomicU64::new(0),
            heavy: Arc::new(Mutex::new(())),
            checkpoints: (AtomicU64::new(0), AtomicU64::new(0)),
            dropped_while_exiting: AtomicBool::new(false),
            gate: Gate::new(Arc::new(AtomicU64::new(0))),
            changes: Changes::new(),
        }
    }

    /// A store with the tables and an `events` view over the empty events table, which the tests fill directly.
    fn rig() -> (Connection, Shared, Mutex<()>) {
        let conn = Connection::open_in_memory().unwrap();
        let plain = debrief_core::batch::EventSchema::plain();
        conn.execute_batch(&crate::schema::ddl(&plain)).unwrap();
        conn.execute_batch(&crate::schema::events_view(&plain, &[])).unwrap();
        (conn, test_shared(), Mutex::new(()))
    }

    fn add_source(conn: &Connection, id: u32) {
        conn.execute(
            "INSERT INTO sources (id, path, name, ready) VALUES (?, 'p', 'n', TRUE)",
            params![id],
        )
        .unwrap();
    }

    /// Appends an event to `source` as live tail does: its row, and the mark for the catalogue.
    fn append(conn: &Connection, source: u32, id: u64, message: &str, extra: &[(&str, &str)]) {
        let keys: Vec<String> = extra.iter().map(|(k, _)| format!("'{k}'")).collect();
        let values: Vec<String> = extra.iter().map(|(_, v)| format!("'{v}'")).collect();
        let map = if extra.is_empty() {
            "map([]::VARCHAR[], []::VARCHAR[])".to_string()
        } else {
            format!("map([{}], [{}])", keys.join(","), values.join(","))
        };
        conn.execute(
            &format!(
                "INSERT INTO events_empty (id, ts, ts_state, level, source, line, byte_offset, message, markers, raw, extra)
                 VALUES (?, ?, 0, 0, ?, 0, 0, ?, 0, '', {map})"
            ),
            params![id, 1_000 + id as i64, source, message],
        )
        .unwrap();
        mark(conn, source, id);
    }

    fn mark(conn: &Connection, source: u32, from_id: u64) {
        conn.execute(
            "INSERT INTO catalogue_tail VALUES (?, ?) ON CONFLICT (source) DO UPDATE SET from_id = least(from_id, excluded.from_id)",
            params![source, from_id],
        )
        .unwrap();
    }

    fn marks(conn: &Connection, source: u32) -> Vec<u64> {
        let mut stmt = conn.prepare("SELECT from_id FROM catalogue_tail WHERE source = ?").unwrap();
        stmt.query_map(params![source], |r| r.get::<_, u64>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn count_of(conn: &Connection, source: u32, name: &str) -> Option<u64> {
        conn.query_row(
            "SELECT n FROM catalogue WHERE source = ? AND lower(name) = lower(?)",
            params![source, name],
            |r| r.get(0),
        )
        .ok()
    }

    fn rows_in(conn: &Connection, table: &str, source: u32) -> u64 {
        conn.query_row(&format!("SELECT count(*) FROM {table} WHERE source = ?"), params![source], |r| {
            r.get(0)
        })
        .unwrap()
    }

    /// Runs turns, each with news, until one is idle; returns how many did work.
    fn drain(s: &Shared, conn: &Connection, write_lock: &Mutex<()>) -> usize {
        let running = crate::shutdown::Active::enter().expect("the process is not exiting");
        let mut worked = 0;
        for _ in 0..200 {
            s.gate.news();
            match step(&running, s, conn, write_lock).unwrap() {
                Turn::Worked => worked += 1,
                Turn::Idle => return worked,
                Turn::Blocked => panic!("a turn was blocked"),
            }
        }
        panic!("the catalogue never went idle");
    }

    /// A source with `events` already catalogued (`ran <name>` per event, ids from 1).
    fn catalogued(conn: &Connection, s: &Shared, write_lock: &Mutex<()>, source: u32, events: &[&str]) {
        add_source(conn, source);
        for (i, m) in events.iter().enumerate() {
            conn.execute(
                "INSERT INTO events_empty (id, ts, ts_state, level, source, line, byte_offset, message, markers, raw, extra)
                 VALUES (?, 1000, 0, 0, ?, 0, 0, ?, 0, '', map([]::VARCHAR[], []::VARCHAR[]))",
                params![i as u64 + 1, source, m],
            )
            .unwrap();
        }
        drain(s, conn, write_lock);
        assert_eq!(rows_in(conn, "catalogue_sources", source), 1, "the source is catalogued");
    }

    /// P1-1 and P1-2: rows that live tail appends while a build runs (after it read the newest id) stay marked, and the
    /// build does not read them: they are counted by the tail turn, once.
    #[test]
    fn rows_appended_while_a_source_is_built_are_counted_once_by_the_tail_turn() {
        let (conn, s, write_lock) = rig();
        add_source(&conn, 1);
        for id in 1..=3 {
            append(&conn, 1, id, "ran alpha.exe", &[]);
        }
        conn.execute("DELETE FROM catalogue_tail", []).unwrap();
        AFTER_NEWEST.with(|h| {
            *h.borrow_mut() = Some(Box::new(|c: &Connection| append(c, 1, 4, "ran alpha.exe and beta.dll", &[])));
        });
        let running = crate::shutdown::Active::enter().unwrap();
        s.gate.news();
        assert_eq!(step(&running, &s, &conn, &write_lock).unwrap(), Turn::Worked, "the build");
        assert_eq!(marks(&conn, 1), vec![4], "the row appended during the build is still marked");
        assert_eq!(
            count_of(&conn, 1, "alpha.exe"),
            Some(3),
            "the build read up to the newest id it saw, not beyond"
        );
        drop(running);
        drain(&s, &conn, &write_lock);
        assert_eq!(count_of(&conn, 1, "alpha.exe"), Some(4), "counted once more by the tail turn");
        assert_eq!(count_of(&conn, 1, "beta.dll"), Some(1));
        assert!(marks(&conn, 1).is_empty());
    }

    /// P1-3: the GUIDs of a source are listed up to MAX_GUIDS however many tail reads bring them, so they cannot fill the
    /// source's names and keep a real name out; the names refused are counted.
    #[test]
    fn guids_appended_over_many_tail_reads_stay_within_their_cap() {
        let (conn, s, write_lock) = rig();
        catalogued(&conn, &s, &write_lock, 1, &["ran first.exe"]);
        let guid = |i: u64| format!("request {:08x}-0000-4000-8000-{:012x} done", i, i);
        let mut id = 1;
        for _round in 0..4 {
            for _ in 0..120 {
                id += 1;
                append(&conn, 1, id, &guid(id), &[]);
            }
            drain(&s, &conn, &write_lock);
        }
        let guids = Scope::Entity(debrief_core::entities::EntityKind::Guid).encode();
        let listed: u64 = conn
            .query_row(
                "SELECT count(*) FROM catalogue WHERE source = 1 AND scope = ?",
                params![guids],
                |r| r.get(0),
            )
            .unwrap();
        assert!(listed > 0 && listed <= MAX_GUIDS as u64, "{listed} GUIDs listed");
        id += 1;
        append(&conn, 1, id, "ran realname.exe", &[]);
        drain(&s, &conn, &write_lock);
        assert_eq!(count_of(&conn, 1, "realname.exe"), Some(1), "a real new name still gets in");
        let dropped: u64 = conn
            .query_row("SELECT dropped FROM catalogue_sources WHERE source = 1", [], |r| r.get(0))
            .unwrap();
        assert!(dropped > 0, "the refused GUIDs are counted");
    }

    /// P2-1: a field the full build left out (over MAX_FIELD_DISTINCT values) is not catalogued through a small tail;
    /// a field it catalogues, and one it never saw, take new values.
    #[test]
    fn a_field_the_build_left_out_stays_out_when_a_tail_adds_values() {
        let (conn, s, write_lock) = rig();
        add_source(&conn, 1);
        for id in 1..=(MAX_FIELD_DISTINCT as u64 + 100) {
            append(&conn, 1, id, "served", &[("Tenant", &format!("tenant{id}")), ("Env", "prod")]);
        }
        conn.execute("DELETE FROM catalogue_tail", []).unwrap();
        drain(&s, &conn, &write_lock);
        assert_eq!(count_of(&conn, 1, "prod"), Some(MAX_FIELD_DISTINCT as u64 + 100));
        assert_eq!(count_of(&conn, 1, "tenant7"), None, "the build left Tenant out");
        let id = MAX_FIELD_DISTINCT as u64 + 101;
        append(
            &conn,
            1,
            id,
            "served",
            &[("Tenant", "newtenant"), ("Env", "staging"), ("Region", "eu")],
        );
        drain(&s, &conn, &write_lock);
        assert_eq!(count_of(&conn, 1, "newtenant"), None, "a tail does not put a left-out field in");
        assert_eq!(count_of(&conn, 1, "staging"), Some(1), "a catalogued field takes the new value");
        assert_eq!(count_of(&conn, 1, "eu"), Some(1), "a field the source never had is added");
    }

    /// P2-2: a tail read that fails is logged and left marked; it is not tried again until the store changes (the loop
    /// goes idle, it does not retry every half second).
    #[test]
    fn a_failing_tail_read_does_not_keep_the_loop_busy() {
        let (conn, s, write_lock) = rig();
        catalogued(&conn, &s, &write_lock, 1, &["ran alpha.exe"]);
        append(&conn, 1, 2, "ran beta.dll", &[]);
        // The events can no longer be read.
        conn.execute_batch("DROP VIEW events_wide; DROP VIEW events;").unwrap();
        let running = crate::shutdown::Active::enter().unwrap();
        s.gate.news();
        assert_eq!(step(&running, &s, &conn, &write_lock).unwrap(), Turn::Idle);
        assert_eq!(marks(&conn, 1), vec![2], "the mark stays for the next try");
        statement_log::start(&conn);
        for _ in 0..100 {
            assert_eq!(step(&running, &s, &conn, &write_lock).unwrap(), Turn::Idle);
        }
        let ran = statement_log::count(&conn);
        assert!(
            ran.is_empty(),
            "{} statements after a failed tail read, the first: {:?}",
            ran.len(),
            ran.first()
        );
    }

    /// P2-4: a read that stops at the scan bound moves the mark only to the last row it read: the rest is read by the
    /// next turns, nothing is lost or counted twice.
    #[test]
    fn a_tail_read_that_hits_the_scan_bound_leaves_the_rest_marked() {
        let (conn, s, write_lock) = rig();
        catalogued(&conn, &s, &write_lock, 1, &["ran first.exe"]);
        for (i, name) in ["a1", "b2", "c3", "d4", "e5"].iter().enumerate() {
            append(&conn, 1, i as u64 + 2, &format!("ran {name}.exe"), &[("Env", "prod")]);
        }
        SCAN_LIMIT.with(|c| c.set(Some(2)));
        let worked = drain(&s, &conn, &write_lock);
        SCAN_LIMIT.with(|c| c.set(None));
        assert!(worked >= 3, "{worked} turns for 5 rows at 2 a turn");
        for name in ["a1", "b2", "c3", "d4", "e5"] {
            assert_eq!(count_of(&conn, 1, &format!("{name}.exe")), Some(1), "{name}");
        }
        assert!(marks(&conn, 1).is_empty());
        // The field values of the rows a read did not reach are not counted by it as well: once each.
        assert_eq!(count_of(&conn, 1, "prod"), Some(5), "a field value is counted once per row");
    }

    /// A tail commit for a source whose rows were given up on or rebuilt (no current catalogue_sources row) writes nothing.
    #[test]
    fn a_tail_commit_for_a_source_without_a_current_row_writes_nothing() {
        let (conn, _s, _w) = rig();
        add_source(&conn, 1);
        let entries = vec![Entry {
            scope: Scope::Entity(debrief_core::entities::EntityKind::Executable),
            name: "late.exe".into(),
            count: 1,
            first_ts: None,
            last_ts: None,
            first_id: 1,
            last_id: 1,
        }];
        let read = Read {
            entries,
            ..Read::default()
        };
        assert_eq!(commit_tail(&conn, 1, None, &read).unwrap(), None);
        assert_eq!(rows_in(&conn, "catalogue", 1), 0);
        // An older layout version counts as no row.
        conn.execute("INSERT INTO catalogue_sources VALUES (1, 0, 0, 0, ?)", params![VERSION - 1])
            .unwrap();
        assert_eq!(commit_tail(&conn, 1, None, &read).unwrap(), None);
        assert_eq!(rows_in(&conn, "catalogue", 1), 0);
    }

    /// Giving up on a source waits for the write lock, so it cannot interleave with a commit that holds it.
    #[test]
    fn giving_up_on_a_source_waits_for_the_write_lock() {
        let dir = debrief_ipc::testing::scratch_dir("catalogue-give-up-lock");
        let samples = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples");
        let store = DuckStore::create(&dir, Profile::default_profile(), PluginRegistry::new()).unwrap();
        store.open(&[samples.join("serilog-text")]).unwrap().wait().unwrap();
        assert!(store.wait_for_catalogue(Duration::from_secs(60)).unwrap());
        let id: u32 = store
            .inner
            .conn()
            .unwrap()
            .query_row("SELECT min(id) FROM sources", [], |r| r.get(0))
            .unwrap();
        let started = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let held = lock(&store.inner.write_lock);
        let worker = {
            let (store, started, finished) = (store.clone(), started.clone(), finished.clone());
            std::thread::spawn(move || {
                started.store(true, Ordering::SeqCst);
                store.catalogue_give_up_on(id).unwrap();
                finished.store(true, Ordering::SeqCst);
            })
        };
        assert!(paddock_testkit::poll_until_within(Duration::from_secs(30), || started.load(Ordering::SeqCst)));
        // Bounded look: with the lock held the call must not finish (it finishes within milliseconds without one).
        assert!(
            !paddock_testkit::poll_until_within(Duration::from_millis(500), || finished.load(Ordering::SeqCst)),
            "the give-up ran while the write lock was held"
        );
        drop(held);
        assert!(paddock_testkit::poll_until_within(Duration::from_secs(30), || finished.load(Ordering::SeqCst)));
        worker.join().unwrap();
    }

    /// P2-5: the mark and the left-out fields go with their source (clear, refresh, removal, the orphan sweep).
    #[test]
    fn the_marks_and_left_out_fields_go_with_their_source() {
        let (conn, s, write_lock) = rig();
        let fill = |conn: &Connection| {
            for id in [1u32, 2] {
                mark(conn, id, 7);
                conn.execute("INSERT INTO catalogue_skipped VALUES (?, 'Tenant')", params![id])
                    .unwrap();
            }
        };
        let left = |conn: &Connection, id: u32| rows_in(conn, "catalogue_tail", id) + rows_in(conn, "catalogue_skipped", id);
        fill(&conn);
        forget(&conn, Some("1")).unwrap();
        assert_eq!((left(&conn, 1), left(&conn, 2)), (0, 2), "forget of a list");
        forget(&conn, None).unwrap();
        assert_eq!(left(&conn, 2), 0, "forget of all");
        fill(&conn);
        for sql in DELETE_SQL {
            conn.execute(sql, params![1u32]).unwrap();
        }
        assert_eq!((left(&conn, 1), left(&conn, 2)), (0, 2), "removal of a source");
        // Source 2 is not in `sources`: the next turn's sweep removes what is left of it.
        let running = crate::shutdown::Active::enter().unwrap();
        s.gate.news();
        step(&running, &s, &conn, &write_lock).unwrap();
        assert_eq!(left(&conn, 2), 0, "the orphan sweep");
    }

    /// P2-5: giving up on a source takes its mark and left-out fields too.
    #[test]
    fn giving_up_on_a_source_removes_its_mark() {
        let dir = debrief_ipc::testing::scratch_dir("catalogue-give-up-mark");
        let samples = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples");
        let store = DuckStore::create(&dir, Profile::default_profile(), PluginRegistry::new()).unwrap();
        store.open(&[samples.join("serilog-text")]).unwrap().wait().unwrap();
        assert!(store.wait_for_catalogue(Duration::from_secs(60)).unwrap());
        let conn = store.inner.conn().unwrap();
        let id: u32 = conn.query_row("SELECT min(id) FROM sources", [], |r| r.get(0)).unwrap();
        mark(&conn, id, 1);
        conn.execute("INSERT INTO catalogue_skipped VALUES (?, 'Tenant')", params![id])
            .unwrap();
        store.catalogue_give_up_on(id).unwrap();
        assert_eq!(rows_in(&conn, "catalogue_tail", id) + rows_in(&conn, "catalogue_skipped", id), 0);
    }

    /// Rule 48: what a tail commit costs per name (the numbers are in docs/performance.md).
    #[test]
    fn a_tail_commit_of_many_names_is_timed() {
        let (conn, _s, _w) = rig();
        add_source(&conn, 1);
        conn.execute("INSERT INTO catalogue_sources VALUES (1, 0, 0, 0, ?)", params![VERSION])
            .unwrap();
        for names in [1_000u64, 10_000] {
            let entries: Vec<Entry> = (0..names)
                .map(|i| Entry {
                    scope: Scope::Entity(debrief_core::entities::EntityKind::Executable),
                    name: format!("tool{names}x{i}.exe"),
                    count: 1,
                    first_ts: Some(1),
                    last_ts: Some(2),
                    first_id: i,
                    last_id: i,
                })
                .collect();
            let started = std::time::Instant::now();
            tail_commit_for_test(&conn, 1, &entries);
            eprintln!("commit_tail of {names} new names: {} ms", started.elapsed().as_millis());
            let again = std::time::Instant::now();
            tail_commit_for_test(&conn, 1, &entries);
            eprintln!(
                "commit_tail of the same {names} names again (updates): {} ms",
                again.elapsed().as_millis()
            );
            assert_eq!(count_of(&conn, 1, &format!("tool{names}x0.exe")), Some(2));
        }
    }

    fn tail_commit_for_test(conn: &Connection, source: u32, entries: &[Entry]) {
        let read = Read {
            entries: entries.to_vec(),
            ..Read::default()
        };
        commit_tail(conn, source, None, &read).unwrap();
    }

    /// P3: a status call that asks for news returns when the catalogue commits, not when its wait is over.
    #[test]
    fn a_status_call_wakes_when_the_catalogue_commits() {
        let dir = debrief_ipc::testing::scratch_dir("catalogue-status");
        let samples = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples");
        let store = DuckStore::create(&dir, Profile::default_profile(), PluginRegistry::new()).unwrap();
        store
            .open(&[samples.join("serilog-text"), samples.join("serilog-clef")])
            .unwrap()
            .wait()
            .unwrap();
        assert!(store.wait_for_catalogue(Duration::from_secs(60)).unwrap());
        // Heavy work holds the catalogue back while its rows are forgotten: it has to be built again.
        let held = lock(&store.inner.heavy);
        forget(&store.inner.conn().unwrap(), None).unwrap();
        store.inner.catalogue.as_ref().unwrap().wake();
        let first = store.inner.catalogue_status(&CatalogueStatusRequest::default()).unwrap();
        assert!(!first.built && first.pending > 0, "the catalogue has to be built again: {first:?}");
        let inner = store.inner.clone();
        let waiter = std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let mut seen = first.generation;
            loop {
                let status = inner
                    .catalogue_status(&CatalogueStatusRequest {
                        after: Some(seen),
                        wait_ms: MAX_STATUS_WAIT_MS,
                    })
                    .unwrap();
                if status.built {
                    return started.elapsed();
                }
                seen = status.generation;
            }
        });
        drop(held);
        let waited = waiter.join().unwrap();
        assert!(
            waited < Duration::from_millis(MAX_STATUS_WAIT_MS),
            "the status call waited {waited:?}, a whole wait; a commit has to wake it"
        );
        drop(store);
        crate::test_scratch::remove_or_defer(&dir);
    }
}
