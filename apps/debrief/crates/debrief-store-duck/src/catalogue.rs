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

/// Sources built at the same time when the option says automatic: one. Four workers built the catalogue of `large10`
/// about 2.4 times sooner than one (performance.md, "catalogue workers"), but with more than one connection the
/// regression test `building_the_catalogue_keeps_its_unsaved_rows_bounded` measures 20 to 24 MB of unsaved rows against
/// its 16 MB bound (12 to 16 MB with one), whatever the round size, so the workers wait for a decision on that bound: there is
/// no option for them yet ([`initial_workers`]).
const AUTO_WORKERS: usize = 1;

/// Workers of the catalogue service a store starts: automatic (0), except in tests, which may ask for more on the
/// current thread to check that several workers build the same catalogue. There is no option for it until the D-0 bound
/// is decided.
pub(crate) fn initial_workers() -> usize {
    #[cfg(test)]
    {
        TEST_WORKERS.with(|w| w.get())
    }
    #[cfg(not(test))]
    {
        0
    }
}

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_WORKERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Layout and extraction version; sources built with another are built again.
pub const VERSION: u32 = 1;

/// Rows of the catalogue tables of `source` are removed with this.
pub(crate) const DELETE_SQL: [&str; 3] = [
    "DELETE FROM catalogue WHERE source = ?",
    "DELETE FROM catalogue_trigrams WHERE source = ?",
    "DELETE FROM catalogue_sources WHERE source = ?",
];

/// Removes the rows of the sources in `list` (comma-separated ids) or all rows with `None`.
pub(crate) fn forget(conn: &Connection, list: Option<&str>) -> duckdb::Result<()> {
    for t in ["catalogue", "catalogue_trigrams", "catalogue_sources"] {
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

/// Reads one source into a tally: entities of the text of its events, then the values of its low-cardinality fields.
fn build(conn: &Connection, source: u32, heavy: &Mutex<()>) -> Result<(Vec<Entry>, u64, u64)> {
    let mut tally = Tally::default();
    {
        use duckdb::arrow::array::{Array, AsArray, RecordBatch, StructArray};
        use duckdb::arrow::datatypes::{Int64Type, UInt64Type};
        let mut stmt = conn.prepare("SELECT id, ts, message, exception FROM events WHERE source = ? LIMIT ?")?;
        // `stream_arrow` starts the statement's streaming execution. Its iterator is dropped unused on purpose: it panics when a
        // fetch fails, so the chunks are pulled with `step()` below, which returns the error.
        let _ = stmt.stream_arrow(params![source, MAX_EVENTS_SCANNED as u64])?;
        // Batches, not rows: the texts are borrowed from the Arrow arrays instead of copied into a String each.
        while let Some(chunk) = stmt.step()? {
            // The process is exiting: stop reading (the source is catalogued again next time).
            if crate::shutdown::down() {
                anyhow::bail!("the process is exiting");
            }
            let rb = RecordBatch::from(&chunk as &StructArray);
            let ids = rb
                .column(0)
                .as_primitive_opt::<UInt64Type>()
                .ok_or_else(|| anyhow::anyhow!("id type"))?;
            let ts = rb
                .column(1)
                .as_primitive_opt::<Int64Type>()
                .ok_or_else(|| anyhow::anyhow!("ts type"))?;
            let msg = rb.column(2).as_string_opt::<i32>().ok_or_else(|| anyhow::anyhow!("message type"))?;
            let exc = rb
                .column(3)
                .as_string_opt::<i32>()
                .ok_or_else(|| anyhow::anyhow!("exception type"))?;
            for i in 0..rb.num_rows() {
                let mut texts = [""; 2];
                let mut n = 1;
                if !msg.is_null(i) {
                    texts[0] = msg.value(i);
                }
                if !exc.is_null(i) {
                    texts[1] = exc.value(i);
                    n = 2;
                }
                let found = extract(&texts[..n]);
                tally.add_event(&found, ids.value(i), (!ts.is_null(i)).then(|| ts.value(i)));
            }
        }
    }
    let sql = format!(
        "WITH kv AS (SELECT unnest(map_keys(extra)) AS k, unnest(map_values(extra)) AS v FROM events WHERE source = ?),
g AS (SELECT k, v, count(*) AS n FROM kv WHERE v IS NOT NULL AND length(v) BETWEEN 1 AND {} AND TRY_CAST(v AS DOUBLE) IS NULL GROUP BY k, v),
d AS (SELECT k FROM g GROUP BY k HAVING count(*) <= {MAX_FIELD_DISTINCT}),
r AS (SELECT g.k, g.v, g.n, row_number() OVER (PARTITION BY g.k ORDER BY g.n DESC, g.v) AS rk FROM g JOIN d USING (k))
SELECT k, v, n FROM r WHERE rk <= {MAX_VALUES_PER_FIELD} ORDER BY k, rk",
        debrief_core::entities::MAX_NAME_CHARS
    );
    // The aggregate holds a hash table of the source's fields: never beside an import's ordering (the store's `heavy`
    // lock), so the catalogue cannot starve it of the memory limit.
    let _heavy = lock(heavy);
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![source])?;
    while let Some(r) = rows.next()? {
        let (k, v, n): (String, String, u64) = (r.get(0)?, r.get(1)?, r.get(2)?);
        tally.add(Scope::Field(k), &v, 0, None, n);
    }
    let (dropped, guids) = (tally.dropped, tally.guids);
    Ok((tally.finish(), dropped, guids))
}

/// Rows (names and trigrams) committed since the last checkpoint that end a run of builds early: the store's own
/// checkpoint threshold is 1 GB (`open_db`), so without this every row of a long build stayed in memory, counted against
/// DuckDB's memory limit, until the build ended (about 60 bytes a row: 56 MB for 346 sources, over 190 MB at 7.6 GB).
const CHECKPOINT_AFTER_ROWS: u64 = 100_000;

/// Writes the rows of a source (replacing older ones) if it is still ready; the rows written, or `None` when it is not.
fn commit(conn: &Connection, source: u32, entries: &[Entry], dropped: u64, guids: u64) -> Result<Option<u64>> {
    let ready: bool = conn.query_row("SELECT count(*) > 0 FROM sources WHERE id = ? AND ready", params![source], |r| {
        r.get(0)
    })?;
    if !ready {
        return Ok(None);
    }
    conn.execute_batch("BEGIN TRANSACTION")?;
    let written = (|| -> Result<u64> {
        let mut rows = entries.len() as u64;
        for sql in DELETE_SQL {
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
        conn.execute(
            "INSERT INTO catalogue_sources VALUES (?, ?, ?, ?, ?)",
            params![source, entries.len() as u64, dropped, guids, VERSION],
        )?;
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
    /// The store's lock of heavy DuckDB work (`Inner::heavy`).
    heavy: Arc<Mutex<()>>,
    /// Sources built at the same time (at least 1).
    workers: usize,
    /// Whether anything changed since the last look; without news a turn runs no statement.
    gate: Gate,
    /// What status callers wait on.
    changes: Changes,
}

/// The catalogue thread of a store; stopped and joined on drop.
pub(crate) struct CatalogueService {
    shared: Arc<Shared>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl CatalogueService {
    pub(crate) fn start(
        conn: Connection,
        write_lock: Arc<Mutex<()>>,
        heavy: Arc<Mutex<()>>,
        generation: Arc<AtomicU64>,
        workers: usize,
    ) -> Result<CatalogueService> {
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            wake: (Mutex::new(true), Condvar::new()),
            pending: AtomicU64::new(0),
            failed: Mutex::new(HashSet::new()),
            unsaved: AtomicBool::new(false),
            unsaved_rows: AtomicU64::new(0),
            heavy,
            workers: if workers == 0 { AUTO_WORKERS } else { workers },
            gate: Gate::new(generation),
            changes: Changes::new(),
        });
        crate::shutdown::interrupt_on_exit(&conn);
        let s = shared.clone();
        let handle = std::thread::Builder::new()
            .name("debrief-catalogue".into())
            .spawn(move || run(&s, &conn, &write_lock))?;
        Ok(CatalogueService {
            shared,
            handle: Some(handle),
        })
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

impl Drop for CatalogueService {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.wake();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// One turn: tidy, then build the next source.
fn step(s: &Shared, conn: &Connection, write_lock: &Mutex<()>) -> Result<Turn> {
    // Counted by the exit handler like a borrowed connection (see `fts::step`).
    let Some(_running) = crate::shutdown::Active::enter() else {
        return Ok(Turn::Idle);
    };
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
             DELETE FROM catalogue_sources WHERE source NOT IN (SELECT id FROM sources);",
        )?;
        pending(conn)?
    };
    let failed = lock(&s.failed).clone();
    let todo: Vec<u32> = todo.into_iter().filter(|id| !failed.contains(id)).collect();
    s.pending.store(todo.len() as u64, Ordering::SeqCst);
    if todo.is_empty() {
        return Ok(Turn::Idle);
    }
    let began = std::time::Instant::now();
    let round = build_round(s, conn, write_lock, &todo);
    s.pending.store(todo.len() as u64 - round.committed, Ordering::SeqCst);
    if round.committed > 0 {
        log::debug!(
            "catalogue: {} of {} pending sources built by {} workers in {:.2?}, {} rows",
            round.committed,
            todo.len(),
            s.workers,
            began.elapsed(),
            round.rows
        );
    }
    // A long run of builds does not keep its rows in memory until it ends.
    let unsaved = s.unsaved_rows.fetch_add(round.rows, Ordering::SeqCst) + round.rows;
    if unsaved >= round_limit(s.workers) && settle(conn, write_lock) {
        s.unsaved_rows.store(0, Ordering::SeqCst);
        s.unsaved.store(false, Ordering::SeqCst);
        s.changes.bump();
    }
    if let Some(e) = round.error {
        return Err(e);
    }
    Ok(if round.blocked { Turn::Blocked } else { Turn::Worked })
}

/// Rows a round writes, and that end a run of builds with a checkpoint: each worker's share of [`CHECKPOINT_AFTER_ROWS`],
/// since the sources already being built when the limit is reached still write theirs.
fn round_limit(workers: usize) -> u64 {
    (CHECKPOINT_AFTER_ROWS / workers.max(1) as u64).max(1)
}

/// What one round of builds did.
#[derive(Default)]
struct Round {
    /// Sources written.
    committed: u64,
    /// Rows written.
    rows: u64,
    /// An import held the write lock: the next turn tries again.
    blocked: bool,
    error: Option<anyhow::Error>,
}

/// Builds sources of `todo` on `s.workers` threads, each with its own connection, claiming the next source when it is
/// free (so one large source does not hold the others back), until about [`CHECKPOINT_AFTER_ROWS`] rows are written (a share of it for each worker)
/// (the caller then checkpoints, so a long run keeps its rows out of memory). Reading and extraction run in parallel;
/// the aggregate holds the store's `heavy` lock, and the writes go one at a time under the store's write lock, which
/// is taken without waiting for an import. The rows are those of building one source after the other.
fn build_round(s: &Shared, conn: &Connection, write_lock: &Mutex<()>, todo: &[u32]) -> Round {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let round = Mutex::new(Round::default());
    let committing = Mutex::new(());
    // Sources already being built when the limit is reached still write their rows: the share keeps the total near the limit.
    let round_rows = round_limit(s.workers);
    let work = |c: &Connection| loop {
        {
            let r = lock(&round);
            if r.blocked || r.error.is_some() || r.rows >= round_rows {
                return;
            }
        }
        if s.stop.load(Ordering::SeqCst) || crate::shutdown::down() {
            return;
        }
        let Some(&source) = todo.get(next.fetch_add(1, Ordering::SeqCst)) else {
            return;
        };
        let (entries, dropped, guids) = match build(c, source, &s.heavy) {
            Ok(b) => b,
            Err(e) => {
                // A source whose events cannot be read now (compaction swapped its files) is tried again later.
                lock(&s.failed).insert(source);
                lock(&round).error.get_or_insert(e.context(format!("cataloguing source {source}")));
                return;
            }
        };
        let _one = lock(&committing);
        // The write lock is taken without waiting for an import: the next turn tries again.
        let Ok(writing) = write_lock.try_lock() else {
            s.gate.news();
            lock(&round).blocked = true;
            return;
        };
        s.unsaved.store(true, Ordering::SeqCst);
        if crate::shutdown::down() {
            return;
        }
        match commit(c, source, &entries, dropped, guids) {
            Ok(written) => {
                drop(writing);
                s.changes.bump();
                let mut r = lock(&round);
                r.committed += 1;
                r.rows += written.unwrap_or(0);
            }
            Err(e) => {
                lock(&round).error.get_or_insert(e);
                return;
            }
        }
    };
    if s.workers <= 1 {
        work(conn);
    } else {
        std::thread::scope(|sc| {
            for i in 1..s.workers {
                let clone = match conn.try_clone() {
                    Ok(c) => c,
                    Err(e) => {
                        log::warn!("catalogue: no connection for worker {i}: {e}");
                        break;
                    }
                };
                crate::shutdown::interrupt_on_exit(&clone);
                let work = &work;
                let spawned = std::thread::Builder::new()
                    .name(format!("debrief-catalogue-{i}"))
                    .spawn_scoped(sc, move || {
                        debrief_core::threads::lower_priority();
                        work(&clone)
                    });
                if let Err(e) = spawned {
                    log::warn!("catalogue: worker {i} did not start: {e}");
                    break;
                }
            }
            // This thread is the first worker, so a round never waits for a thread that could not start.
            work(conn);
        });
    }
    round.into_inner().unwrap_or_else(|p| p.into_inner())
}

fn run(s: &Shared, conn: &Connection, write_lock: &Mutex<()>) {
    while !s.stop.load(Ordering::SeqCst) {
        let turn = match step(s, conn, write_lock) {
            Ok(t) => t,
            Err(e) => {
                log::warn!("catalogue: {e:#}");
                Turn::Blocked
            }
        };
        s.gate.after(turn);
        let Some(wait) = wait_after(turn) else { continue };
        if s.unsaved.load(Ordering::SeqCst) && settle(conn, write_lock) {
            s.unsaved.store(false, Ordering::SeqCst);
            s.unsaved_rows.store(0, Ordering::SeqCst);
            s.changes.bump();
        }
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
/// False when an import holds the write lock (it checkpoints when it ends; the next idle turn tries again).
fn settle(conn: &Connection, write_lock: &Mutex<()>) -> bool {
    let Ok(_writing) = write_lock.try_lock() else { return false };
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
        let pending = pending(&conn).map_err(|e| engine(format!("{e:#}")))?.len() as u64;
        Ok(CatalogueStatus {
            built: pending == 0 && !self.catalogue.as_ref().is_some_and(|c| c.saving()),
            pending,
            generation,
        })
    }

    /// Names that look like `req.text`, from the catalogue.
    pub(crate) fn catalogue(&self, req: &CatalogueRequest) -> StoreResult<CatalogueReply> {
        let conn = self.conn()?;
        let pending = pending(&conn).map_err(|e| engine(format!("{e:#}")))?.len() as u64;
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
            heavy: Arc::new(Mutex::new(())),
            workers: 1,
            gate: Gate::new(Arc::new(AtomicU64::new(0))),
            changes: Changes::new(),
        };
        let write_lock = Mutex::new(());
        // The first turn looks (it has never looked); after it the store is idle.
        assert_eq!(step(&shared, &conn, &write_lock).unwrap(), Turn::Idle);
        statement_log::start(&conn);
        for _ in 0..100 {
            assert_eq!(step(&shared, &conn, &write_lock).unwrap(), Turn::Idle);
        }
        let ran = statement_log::count(&conn);
        assert!(
            ran.is_empty(),
            "100 idle catalogue turns ran {} statements, the first: {:?}",
            ran.len(),
            ran.first()
        );
    }

    /// P3: a status call that asks for news returns when the catalogue commits, not when its wait is over.
    #[test]
    fn a_status_call_wakes_when_the_catalogue_commits() {
        let dir = std::env::temp_dir().join(format!("debrief-catalogue-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
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
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_store_asks_for_the_automatic_workers_unless_a_test_says_otherwise() {
        assert_eq!(initial_workers(), 0);
        TEST_WORKERS.with(|w| w.set(3));
        assert_eq!(initial_workers(), 3);
        TEST_WORKERS.with(|w| w.set(0));
    }

    /// SR3: several workers write the rows of building the sources one after the other, whatever the order they finish in.
    /// The event ids are left out: two imports of the same files hand out their id blocks in a different order.
    #[test]
    fn four_workers_build_the_same_catalogue_as_one() {
        let samples = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples");
        let rows = |workers: usize| {
            let dir = std::env::temp_dir().join(format!("debrief-catalogue-workers-{workers}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TEST_WORKERS.with(|w| w.set(workers));
            let opts = crate::StoreOptions::default();
            let store = DuckStore::create_with(&dir, Profile::default_profile(), PluginRegistry::new(), opts).unwrap();
            TEST_WORKERS.with(|w| w.set(0));
            store.open(std::slice::from_ref(&samples)).unwrap().wait().unwrap();
            assert!(store.wait_for_catalogue(Duration::from_secs(120)).unwrap());
            let conn = store.inner.conn().unwrap();
            let mut stmt = conn
                .prepare(
                    "SELECT source, scope, name, n, first_ts, last_ts FROM catalogue
                     ORDER BY source, scope, name",
                )
                .unwrap();
            let out: Vec<String> = stmt
                .query_map([], |r| {
                    Ok(format!(
                        "{:?}",
                        (
                            r.get::<_, u32>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, u64>(3)?,
                            r.get::<_, Option<i64>>(4)?,
                            r.get::<_, Option<i64>>(5)?
                        )
                    ))
                })
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            drop(stmt);
            drop(conn);
            drop(store);
            let _ = std::fs::remove_dir_all(&dir);
            out
        };
        let one = rows(1);
        assert!(one.len() > 100, "the samples have names: {}", one.len());
        assert_eq!(one, rows(4));
    }
}
