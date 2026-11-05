//! Debrief's DuckDB store (ADR 0014): one store folder per project. Events are zstd Parquet segments written
//! by the parse threads in parallel ([`segments`], `writer`); DuckDB keeps the catalog (sources,
//! diagnostics, the segment manifest, time-order positions) and runs every query over the `events` view.
//! Queries compile to SQL with a batch-scan fallback for what SQL cannot express yet.
//!
//! Operations not offered yet answer [`StoreError::Unsupported`] naming the phase that adds them.

mod analysis;
mod anomalies;
mod catalogue;
mod compact;
mod console;
mod context;
mod diag;
mod encodings;
mod fts;
mod hits;
mod inspect;
mod lanes;
mod mining;
mod parsers;
mod pos;
mod profiles;
mod progress;
mod project;
pub mod promote;
mod qlang;
mod read;
mod redaction;
mod reuse;
mod schema;
pub mod segments;
mod shutdown;
mod skew;
mod spans;
mod sql;
pub mod stats;
mod tail;
mod turns;
mod writer;

pub use compact::Compaction;
pub use fts::WriterBudget;
pub use schema::FORMAT_VERSION;

use debrief_core::api::{UnrecognizedFile, WorkspaceState};
use debrief_core::batch::{EventId, EventSchema, SourceId};
use debrief_core::ingest::{self, ImportEnv, Outcome, StreamOptions, StreamSpec};
use debrief_core::parser::{ParserSet, ParserSetup};
use debrief_core::ports::{
    ImportReport, ImportSession, IndexStatus, Ingest, Search, SearchReply, SearchRequest, StoreError, StoreResult, TailAppend,
};
use debrief_core::profile::{CompiledProfile, Profile};
use debrief_core::workspace::{ImportProgress, NOT_IMPORTED};
use debrief_core::PluginRegistry;
use duckdb::{params, Config, Connection};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// DuckDB's memory limit (ADR 0014 memory budget); it spills to the store's `tmp` folder beyond it.
pub const MEMORY_LIMIT: &str = "192MB";

/// DuckDB's memory limit while an import compacts and orders the events (the import memory budget: parsing is over, so
/// the import may use it; the limit goes back to [`MEMORY_LIMIT`] afterwards).
pub const IMPORT_MEMORY_LIMIT: &str = "768MB";

/// Allocations DuckDB's allocator may keep cached after a task (`allocator_flush_threshold`).
pub const ALLOCATOR_FLUSH: &str = "32MiB";

/// DuckDB's query threads by default: each holds row groups being read, so more threads need more of
/// the memory limit (a sort over 24 threads ran out of 256 MB in the bake-off).
pub const DUCK_THREADS: usize = 4;

/// Limits of a store (ADR 0014 §2: configurable in Settings for larger machines).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreOptions {
    /// Parse threads of an import (see [`ingest::default_threads`]).
    pub parse_threads: usize,
    /// DuckDB query threads.
    pub duck_threads: usize,
    /// DuckDB memory limit, e.g. `256MB`.
    pub memory_limit: String,
    /// The full-text index writer's heap and threads (applies when the store opens).
    pub index: fts::WriterBudget,
    /// Sources the catalogue builds at the same time (applies when the store opens); 0 is automatic.
    pub catalogue_workers: usize,
    /// DuckDB's memory limit while an import compacts and orders the events ([`IMPORT_MEMORY_LIMIT`]).
    pub import_memory_limit: String,
    /// Where DuckDB spills beyond its memory limit: a folder per store below this one (see [`spill_folder`]);
    /// `None`: the store folder's `tmp` (applies when the store opens).
    pub spill_dir: Option<PathBuf>,
    /// The most DuckDB may spill (`max_temp_directory_size`, e.g. `10GB`); `None`: DuckDB's default (90% of the free
    /// disk space).
    pub max_spill: Option<String>,
    /// Freed memory DuckDB's allocator may keep cached (`allocator_flush_threshold`).
    pub allocator_flush: String,
    /// DuckDB's allocator background threads (`allocator_background_threads`).
    pub allocator_background_threads: bool,
}

impl Default for StoreOptions {
    fn default() -> Self {
        StoreOptions {
            parse_threads: ingest::default_threads(),
            duck_threads: DUCK_THREADS,
            memory_limit: MEMORY_LIMIT.to_string(),
            index: fts::WriterBudget::default(),
            catalogue_workers: 0,
            import_memory_limit: IMPORT_MEMORY_LIMIT.to_string(),
            spill_dir: None,
            max_spill: None,
            allocator_flush: ALLOCATOR_FLUSH.to_string(),
            allocator_background_threads: true,
        }
    }
}

/// DuckDB's memory limit raised while it lives; dropping it (also when the work fails or panics) sets the limit to what
/// `normal` returns then, so a `configure` that ran meanwhile is not undone. Such a `configure` sets its limit at once,
/// so the rest of the raised work runs with it (lower than the import memory, as the user asked).
struct RaisedMemory<'c, N: Fn() -> String> {
    conn: &'c Connection,
    normal: N,
}

fn set_memory_limit(conn: &Connection, limit: &str) -> duckdb::Result<()> {
    conn.execute_batch(&format!("SET memory_limit = '{}'", limit.replace('\'', "''")))
}

impl<'c, N: Fn() -> String> RaisedMemory<'c, N> {
    fn new(conn: &'c Connection, raised: &str, normal: N) -> Self {
        if let Err(e) = set_memory_limit(conn, raised) {
            log::warn!("could not raise DuckDB's memory limit to {raised}: {e}");
        }
        RaisedMemory { conn, normal }
    }
}

impl<N: Fn() -> String> Drop for RaisedMemory<'_, N> {
    fn drop(&mut self) {
        let normal = (self.normal)();
        if let Err(e) = set_memory_limit(self.conn, &normal) {
            log::error!("could not set DuckDB's memory limit back to {normal}: {e}");
        }
    }
}

/// Runs `f` with DuckDB's memory limit at `raised`, then sets it to what `normal` returns at that moment, also when `f`
/// fails or panics (the limit is the database's, so every connection sees it while `f` runs).
pub(crate) fn with_memory<T>(conn: &Connection, raised: &str, normal: impl Fn() -> String, f: impl FnOnce(&Connection) -> T) -> T {
    let _raised = RaisedMemory::new(conn, raised, normal);
    f(conn)
}

/// Runs `f` (the ordering) while the import's stage reads "Ordering events by time (N%)" from DuckDB's progress
/// estimate of the statement on `conn` (the Store pane's ordering job gets it through the run's control instead).
fn with_ordering_percent<T>(conn: &Connection, progress: &ImportProgress, f: impl FnOnce() -> T) -> T {
    if conn.set_query_progress_tracking(true).is_err() {
        return f();
    }
    let handle = conn.progress_handle();
    let (stop, stopped) = std::sync::mpsc::channel::<()>();
    let out = std::thread::scope(|s| {
        s.spawn(move || {
            // Never backwards: DuckDB's estimate starts again with each statement (and a retry).
            let mut best = 0;
            while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(crate::progress::POLL) {
                if let Some(p) = crate::progress::from_duckdb(&handle.query_progress()) {
                    best = ordering_percent(best, p.done, p.total);
                    progress.set_stage(Some(&format!("Ordering events by time ({best}%)")));
                }
            }
        });
        let out = f();
        drop(stop);
        out
    });
    let _ = conn.set_query_progress_tracking(false);
    out
}

/// The ordering's percentage from `done` of `total`, never below `best` and never above 100.
fn ordering_percent(best: u64, done: u64, total: u64) -> u64 {
    best.max(done.min(total).saturating_mul(100) / total.max(1))
}

/// What the user is told when the events could not be put in time order.
pub(crate) fn reorder_notice(e: &dyn std::fmt::Display) -> String {
    format!(
        "Debrief could not put the events in time order ({e}), so the event list may be incomplete. It tries again \
         when the project opens and after the next import."
    )
}

/// The spill folder of the store in `dir` below the chosen spill folder `base`: one folder per store (stores never
/// share DuckDB's temporary files), named by a stable hash of the store folder.
pub fn spill_folder(base: &Path, dir: &Path) -> PathBuf {
    // FNV-1a, 64 bits: stable across runs, so a store reuses (and cleans) its own folder.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in dir.to_string_lossy().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    base.join(format!("debrief-spill-{h:016x}"))
}

/// Parse threads in low memory mode (each holds a file's chunk and its batches in flight; 2 halved the speed).
pub const LOW_MEMORY_PARSE_THREADS: usize = 4;
/// DuckDB's memory limit in low memory mode.
pub const LOW_MEMORY_LIMIT: &str = "128MB";

impl StoreOptions {
    /// Low memory mode (a user setting, off by default): fewer parse threads and so fewer batches in flight, a
    /// smaller DuckDB limit with fewer threads, a smaller index writer. Imports are slower (20-40%); the worker
    /// also gives freed memory back at once (`debrief_alloc::purge_at_once`).
    pub fn low_memory() -> StoreOptions {
        StoreOptions {
            parse_threads: LOW_MEMORY_PARSE_THREADS.min(ingest::default_threads()),
            duck_threads: 2,
            memory_limit: LOW_MEMORY_LIMIT.to_string(),
            index: fts::WriterBudget {
                heap: 16 << 20,
                threads: 1,
            },
            import_memory_limit: LOW_MEMORY_LIMIT.to_string(),
            ..StoreOptions::default()
        }
    }
}

/// Stores with at least this many events answer timelines from the pre-aggregated histogram.
pub const PREAGGREGATE_ABOVE: u64 = 1_000_000;

/// File name of the database in a store folder.
pub const DB_FILE: &str = "data.duckdb";

/// The DuckDB storage version new catalogs are written in (`storage_compatibility_version`, set when the database is
/// opened: the main file is created in it, a later `SET` does not change it). DuckDB 2.0 would otherwise write a
/// format DuckDB 1.x cannot open, so a project shared from this Debrief would not open in the previous release. The
/// catalog is small and needs none of the newer storage features. Lift it only with a `FORMAT_VERSION` bump that
/// rebuilds the catalog (`COPY FROM DATABASE`, then swap).
pub(crate) const STORAGE_VERSION: &str = "v1.5.0";

/// Idle connections kept for reuse.
const POOL: usize = 8;

pub(crate) fn engine(e: impl std::fmt::Display) -> StoreError {
    classify(e.to_string())
}

/// A database failure as the typed error it is: a lock held by another process, a damaged data file, or an
/// engine failure.
fn classify(message: String) -> StoreError {
    let lower = message.to_lowercase();
    // Linux/macOS (a POSIX lock) and Windows (a sharing violation).
    if lower.contains("could not set lock on file") || lower.contains("conflicting lock") || lower.contains("file is already open in") {
        StoreError::Locked(format!("this project is open in another Debrief window or program ({message})"))
    } else if lower.contains("database file with version number") || lower.contains("created with a newer version of duckdb") {
        // DuckDB fails before `meta` is read, so the format-version check of `open_with` never runs: say it the same way.
        let found = lower
            .split("version number ")
            .nth(1)
            .map(|rest| rest.chars().take_while(char::is_ascii_digit).collect::<String>())
            .filter(|n| !n.is_empty())
            .map_or(String::new(), |n| format!(" (database format {n})"));
        StoreError::Invalid(format!(
            "this project was saved by a newer Debrief{found}; update Debrief to open it"
        ))
    } else if lower.contains("no magic bytes found")
        || lower.contains("too small to be a parquet file")
        || (lower.contains("parquet") && (lower.contains("corrupt") || lower.contains("invalid footer")))
    {
        StoreError::Damaged(format!("a data file of this project is damaged ({message})"))
    } else {
        StoreError::Engine(message)
    }
}

/// A segment the catalog lists is not in the folder.
#[derive(Debug)]
struct MissingSegment(String);

impl std::fmt::Display for MissingSegment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the data file {} of this project is missing", self.0)
    }
}

impl std::error::Error for MissingSegment {}

/// What the store was opened with and what was opened in it.
pub(crate) struct State {
    /// The profile with the generic parser (format-independent analysis).
    pub(crate) profile: Arc<CompiledProfile>,
    /// The parser library compiled with the profile, with this store's per-source choices (ADR 0026).
    pub(crate) parsers: Arc<ParserSet>,
    pub(crate) roots: Vec<PathBuf>,
    removed: HashSet<PathBuf>,
    unrecognized: Vec<UnrecognizedFile>,
}

pub(crate) struct Inner {
    /// The store folder.
    pub(crate) dir: PathBuf,
    /// The connection the database was opened with; every other connection is cloned from it.
    db: Mutex<Connection>,
    pool: Mutex<Vec<Connection>>,
    registry: PluginRegistry,
    pub(crate) schema: Arc<EventSchema>,
    pub(crate) state: RwLock<State>,
    /// Bumped by every change to the events (keys read caches).
    pub(crate) generation: Arc<AtomicU64>,
    pub(crate) importing: AtomicBool,
    /// A bulk import runs (not a live refresh, which also sets `importing` for its brief hold of the store).
    pub(crate) bulk_import: Arc<AtomicBool>,
    /// The time window later imports read (ADR 0047).
    pub(crate) import_window: RwLock<debrief_core::ingest::ImportWindow>,
    /// Held by whatever rewrites the catalog (an import, compaction, positions, removal).
    pub(crate) write_lock: Arc<Mutex<()>>,
    /// Held by the heavy DuckDB work that must not share the memory limit: an import's compaction and ordering, and the
    /// catalogue's per-source aggregate, so the catalogue never starves the ordering of the memory limit.
    pub(crate) heavy: Arc<Mutex<()>>,
    /// The full-text index and its indexing thread (`None` when the index cannot be opened: search then
    /// fails with the reason, everything else works).
    fts: Option<fts::FtsService>,
    /// The catalogue of names and its building thread (`None` when the thread cannot start: lookups then fail).
    catalogue: Option<catalogue::CatalogueService>,
    fts_error: Option<String>,
    /// Word-search hits handed to SQL (smart search).
    pub(crate) hits: hits::HitsCache,
    /// The SQL console's sandboxed database and its kept results (ADR 0030).
    pub(crate) console: console::ConsoleState,
    options: RwLock<StoreOptions>,
    pub(crate) progress: Arc<ImportProgress>,
    pub(crate) derived: Mutex<Option<(u64, Arc<read::Derived>)>>,
    /// Analyses being computed now, by kind and query: a second identical request waits for the first and reads its
    /// cached result instead of computing the same thing again (Home, the health check and the headline ask together).
    pub(crate) flights: Mutex<std::collections::HashMap<String, Arc<Mutex<()>>>>,
    /// From this many events on, timelines use the pre-aggregated histogram (approximate at bucket edges)
    /// instead of grouping the events.
    pub(crate) preaggregate_above: AtomicU64,
    /// Tests: seed of the streaming import's random delays (0 = none), see `StreamOptions::jitter`.
    pub(crate) import_jitter: AtomicU64,
}

/// The catalog tables that hold the imported data; a reload keeps their rows aside until it completes.
const ASIDE_TABLES: [&str; 9] = [
    "segments",
    "segment_fields",
    "histogram",
    "ord",
    "diagnostics",
    "tail_offsets",
    "source_encodings",
    "source_fingerprints",
    "sources",
];

/// What a reload keeps until the new data is complete: the old segment files and the unrecognised list.
pub(crate) struct Aside {
    files: Vec<String>,
    unrecognized: Vec<debrief_core::api::UnrecognizedFile>,
}

/// A connection borrowed from the pool, returned on drop.
pub(crate) struct Conn<'a> {
    inner: &'a Inner,
    conn: Option<Connection>,
    /// Interrupts this connection's statement when the thread's current control (`QueryControl::scope`) is cancelled.
    _interrupt: Option<debrief_core::ql::HookGuard>,
    /// Relays DuckDB's query progress to the same control while the connection is borrowed.
    relay: Option<progress::Relay>,
    /// Counts this connection as running, for the exit handler (`shutdown`).
    _active: shutdown::Active,
}

impl std::ops::Deref for Conn<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn.as_ref().unwrap_or_else(|| unreachable!("taken only on drop"))
    }
}

impl Drop for Conn<'_> {
    fn drop(&mut self) {
        if let Some(c) = self.conn.take() {
            if let Some(relay) = self.relay.take() {
                relay.finish(&c);
            }
            let mut pool = lock(&self.inner.pool);
            if pool.len() < POOL {
                pool.push(c);
            }
        }
    }
}

/// Locks recover from poisoning: a panicking reader must not take the store down.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

pub(crate) fn read_lock<T>(m: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    m.read().unwrap_or_else(|p| p.into_inner())
}

pub(crate) fn write_lock<T>(m: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    m.write().unwrap_or_else(|p| p.into_inner())
}

impl Inner {
    /// After the events changed: compaction, positions and a checkpoint. Failures are logged (the data is
    /// complete without them; the next change retries).
    ///
    /// `progress` names each stage and is polled for Cancel: a cancelled import skips the compaction (only an
    /// optimisation; the next change retries) but still orders the events, so what was read stays usable.
    fn maintain(&self, conn: &Connection, progress: Option<&ImportProgress>) -> Vec<String> {
        let stage = |label: &str| {
            if let Some(p) = progress {
                p.set_stage(Some(label));
            }
        };
        let mut notices = Vec::new();
        let tc = std::time::Instant::now();
        // No catalogue query runs beside the compaction and the ordering, which get the import's memory: the ordering
        // ran out of memory beside eight catalogue workers once, leaving the events unordered.
        let _heavy = lock(&self.heavy);
        // Cancel may have come while waiting for the lock: then only the ordering runs (what was read stays usable).
        if progress.is_some_and(|p| p.cancel_flag().load(Ordering::SeqCst)) {
            log::info!("import cancelled: segment compaction skipped");
        } else {
            stage("Compacting segments");
            let raised = read_lock(&self.options).import_memory_limit.clone();
            let compacted = with_memory(
                conn,
                &raised,
                || read_lock(&self.options).memory_limit.clone(),
                |conn| compact::compact(conn, &self.dir, &self.schema),
            );
            match compacted {
                Ok(c) if c.replaced > 0 => log::info!("compacted {} segments into {}", c.replaced, c.written),
                Ok(_) => {}
                Err(e) => log::warn!("segment compaction failed: {e:#}"),
            }
        }
        stage("Ordering events by time");
        log::debug!("import: compact {:?}", tc.elapsed());
        notices.extend(match progress {
            Some(p) => with_ordering_percent(conn, p, || self.reorder_holding_heavy(conn)),
            None => self.reorder_holding_heavy(conn),
        });
        log::debug!("import: pos {:?}", tc.elapsed());
        stage("Saving the index");
        // Write the catalog into the database file now, so a reopen does not replay the WAL.
        if let Err(e) = conn.execute_batch("CHECKPOINT") {
            log::warn!("checkpoint failed: {e}");
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(f) = &self.fts {
            f.wake();
        }
        if let Some(c) = &self.catalogue {
            c.wake();
        }
        // The burst's free pages go back to the OS now: the process may idle afterwards.
        debrief_alloc::release();
        notices
    }

    /// Puts the events in time order again: the one way the positions are rebuilt (imports, live refreshes,
    /// compaction, removals, skew corrections, the Store pane). Under the `heavy` lock, with the import memory as
    /// DuckDB's limit, tried twice and marked stale when both fail ([`pos::rebuild_or_mark`]). `None` when the
    /// positions are complete, else what to tell the user.
    pub(crate) fn reorder(&self, conn: &Connection) -> Option<String> {
        let _heavy = lock(&self.heavy);
        self.reorder_holding_heavy(conn)
    }

    /// [`Inner::reorder`] for a caller that holds the `heavy` lock.
    fn reorder_holding_heavy(&self, conn: &Connection) -> Option<String> {
        let raised = read_lock(&self.options).import_memory_limit.clone();
        match with_memory(
            conn,
            &raised,
            || read_lock(&self.options).memory_limit.clone(),
            pos::rebuild_or_mark,
        ) {
            Ok(()) => None,
            Err(e) => {
                log::error!("time-order positions could not be rebuilt: {e}");
                Some(reorder_notice(&e))
            }
        }
    }

    pub(crate) fn conn(&self) -> StoreResult<Conn<'_>> {
        let active = shutdown::Active::enter().ok_or(StoreError::Cancelled)?;
        let pooled = lock(&self.pool).pop();
        let conn = match pooled {
            Some(c) => c,
            None => lock(&self.db).try_clone().map_err(engine)?,
        };
        let mut relay = None;
        let interrupt = match debrief_core::ql::QueryControl::current() {
            Some(control) if control.is_cancelled() => return Err(StoreError::Cancelled),
            Some(control) => {
                let handle = conn.interrupt_handle();
                relay = progress::Relay::start(&conn, control.clone(), progress::POLL);
                Some(control.on_cancel(Box::new(move || handle.interrupt())))
            }
            None => None,
        };
        Ok(Conn {
            inner: self,
            conn: Some(conn),
            _interrupt: interrupt,
            relay,
            _active: active,
        })
    }

    pub(crate) fn profile(&self) -> Arc<CompiledProfile> {
        read_lock(&self.state).profile.clone()
    }

    pub(crate) fn registry(&self) -> &PluginRegistry {
        &self.registry
    }

    pub(crate) fn unrecognized(&self) -> Vec<UnrecognizedFile> {
        read_lock(&self.state).unrecognized.clone()
    }

    pub(crate) fn is_importing(&self) -> bool {
        self.importing.load(Ordering::SeqCst)
    }

    /// Bumps the persisted data version (cached analyses of older versions are dropped).
    pub(crate) fn bump_version(&self, conn: &Connection) -> StoreResult<()> {
        let current: u64 = conn
            .query_row("SELECT value FROM meta WHERE key = 'data_version'", [], |r| r.get::<_, String>(0))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let next = current + 1;
        conn.execute("INSERT OR REPLACE INTO meta VALUES ('data_version', ?)", params![next.to_string()])
            .map_err(engine)?;
        conn.execute("DELETE FROM analysis_cache WHERE version < ?", params![next])
            .map_err(engine)?;
        conn.execute_batch(&lanes::prune_sql(next)).map_err(engine)?;
        // The background loops wait for news, not for the clock: every change of the data is news.
        if let Some(f) = &self.fts {
            f.wake();
        }
        if let Some(c) = &self.catalogue {
            c.wake();
        }
        Ok(())
    }

    /// Forgets every event, source and problem like [`Inner::forget_events`] but keeps the old catalog rows in
    /// `aside_*` tables and the old segment files on disk, so [`Inner::restore_aside`] can bring them back.
    pub(crate) fn set_aside_events(&self) -> StoreResult<Aside> {
        let conn = self.conn()?;
        let _writing = lock(&self.write_lock);
        let files = writer::segment_files(&conn, &self.dir).map_err(|e| engine(format!("{e:#}")))?;
        let mut sql = String::from("BEGIN TRANSACTION;");
        for t in ASIDE_TABLES {
            sql.push_str(&format!(
                "DROP TABLE IF EXISTS aside_{t}; CREATE TABLE aside_{t} AS SELECT * FROM {t}; DELETE FROM {t};"
            ));
        }
        sql.push_str("COMMIT;");
        conn.execute_batch(&sql).map_err(engine)?;
        writer::refresh_view(&conn, &self.dir, &self.schema).map_err(|e| engine(format!("{e:#}")))?;
        if let Some(f) = &self.fts {
            if let Err(e) = f.clear() {
                log::warn!("could not clear the full-text index: {e:#}");
            }
        }
        let unrecognized = std::mem::take(&mut write_lock(&self.state).unrecognized);
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.bump_version(&conn)?;
        Ok(Aside { files, unrecognized })
    }

    /// The new data is complete: drops the kept rows and deletes the old segment files.
    pub(crate) fn discard_aside(&self, aside: Aside) {
        if let Ok(conn) = self.conn() {
            for t in ASIDE_TABLES {
                if let Err(e) = conn.execute_batch(&format!("DROP TABLE IF EXISTS aside_{t}")) {
                    log::warn!("could not drop aside_{t}: {e}");
                }
            }
            for f in aside.files {
                if std::fs::remove_file(&f).is_err() && Path::new(&f).exists() {
                    let _ = conn.execute("INSERT OR IGNORE INTO trash VALUES (?)", params![f]);
                }
            }
        }
    }

    /// The new import did not complete: removes what it wrote and puts the kept rows and files back.
    pub(crate) fn restore_aside(&self, aside: Aside) -> StoreResult<()> {
        let conn = self.conn()?;
        let _writing = lock(&self.write_lock);
        let new_files = writer::segment_files(&conn, &self.dir).map_err(|e| engine(format!("{e:#}")))?;
        let mut sql = String::from("BEGIN TRANSACTION;");
        for t in ASIDE_TABLES {
            sql.push_str(&format!(
                "DELETE FROM {t}; INSERT INTO {t} SELECT * FROM aside_{t}; DROP TABLE aside_{t};"
            ));
        }
        // The full-text index was emptied: the restored segments are indexed again in the background.
        sql.push_str("UPDATE segments SET indexed = FALSE; COMMIT;");
        conn.execute_batch(&sql).map_err(engine)?;
        writer::refresh_view(&conn, &self.dir, &self.schema).map_err(|e| engine(format!("{e:#}")))?;
        for f in new_files {
            if !aside.files.contains(&f) && std::fs::remove_file(&f).is_err() && Path::new(&f).exists() {
                let _ = conn.execute("INSERT OR IGNORE INTO trash VALUES (?)", params![f]);
            }
        }
        write_lock(&self.state).unrecognized = aside.unrecognized;
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(f) = &self.fts {
            f.wake();
        }
        self.bump_version(&conn)
    }

    /// Forgets every event, source and problem (the opened roots stay): the manifest is emptied, the views
    /// refreshed and the segment files deleted (or moved to `trash` while a reader holds them).
    pub(crate) fn forget_events(&self) -> StoreResult<()> {
        let conn = self.conn()?;
        let _writing = lock(&self.write_lock);
        let files = writer::segment_files(&conn, &self.dir).map_err(|e| engine(format!("{e:#}")))?;
        conn.execute_batch(
            "BEGIN TRANSACTION; DELETE FROM segments; DELETE FROM segment_fields; DELETE FROM segment_blocks; DELETE FROM histogram; DELETE FROM ord; DELETE FROM diagnostics; DELETE FROM tail_offsets; DELETE FROM source_encodings; DELETE FROM source_fingerprints; DELETE FROM sources; COMMIT;",
        )
        .map_err(engine)?;
        catalogue::forget(&conn, None).map_err(engine)?;
        writer::refresh_view(&conn, &self.dir, &self.schema).map_err(|e| engine(format!("{e:#}")))?;
        if let Some(f) = &self.fts {
            if let Err(e) = f.clear() {
                log::warn!("could not clear the full-text index: {e:#}");
            }
        }
        for f in files {
            if std::fs::remove_file(&f).is_err() && Path::new(&f).exists() {
                let _ = conn.execute("INSERT OR IGNORE INTO trash VALUES (?)", params![f]);
            }
        }
        write_lock(&self.state).unrecognized.clear();
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.bump_version(&conn)
    }

    /// Reads `paths` (canonical) into the database.
    pub(crate) fn import(&self, paths: &[PathBuf], progress: &ImportProgress) -> StoreResult<ImportReport> {
        self.import_keeping(paths, progress, std::collections::HashMap::new())
    }

    /// [`Inner::import`] where the sources at the paths of `keep` keep the given ids (a reload, see `reuse`).
    pub(crate) fn import_keeping(
        &self,
        paths: &[PathBuf],
        progress: &ImportProgress,
        keep: std::collections::HashMap<String, u32>,
    ) -> StoreResult<ImportReport> {
        let started = std::time::Instant::now();
        // From here the progress reads "finding files" with live counts; it ends with this function, however.
        let _run = progress.begin();
        let (profile, parser_set, roots, removed) = {
            let s = read_lock(&self.state);
            (s.profile.clone(), s.parsers.clone(), s.roots.clone(), s.removed.clone())
        };
        let conn = self.conn()?;
        let mut known: HashSet<String> = HashSet::new();
        {
            let mut stmt = conn.prepare("SELECT path FROM sources").map_err(engine)?;
            let paths = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(engine)?;
            for p in paths {
                let p = p.map_err(engine)?;
                known.insert(p.split(debrief_core::archive::ENTRY_SEPARATOR).next().unwrap_or(&p).to_string());
            }
        }
        known.extend(self.unrecognized().into_iter().map(|u| u.path));
        let encodings = self.encoding_policy(&conn);
        let env = ImportEnv {
            importers: self.registry.importers(),
            parsers: &parser_set,
            schema: self.schema.clone(),
            encodings: &encodings,
            cancel: progress.cancel_flag(),
            window: {
                let w = *read_lock(&self.import_window);
                (w.from.is_some() || w.to.is_some()).then_some(w)
            },
        };
        // Parsing is CPU bound (the parse threads); the folder walkers mostly wait for the disk or the network, so
        // they may outnumber them.
        let threads = read_lock(&self.options).parse_threads.max(1);
        let opts = StreamOptions {
            threads,
            walkers: (threads * 2).clamp(4, 16),
            jitter: Some(self.import_jitter.load(Ordering::Relaxed)).filter(|s| *s != 0),
            ..StreamOptions::default()
        };
        let _writing = lock(&self.write_lock);
        let sink = writer::SegmentSink::new(
            conn.try_clone().map_err(engine)?,
            &self.dir,
            self.schema.clone(),
            opts.threads,
            self.generation.clone(),
            None,
        )
        .map_err(|e| engine(format!("{e:#}")))?;
        sink.keep_ids(keep);
        let t0 = std::time::Instant::now();
        let spec = StreamSpec {
            paths,
            roots: &roots,
            profile: &profile,
            removed: &removed,
            known: &known,
        };
        let outcomes = ingest::import_stream(&env, &spec, &removed, progress, &opts, &sink);
        let t1 = t0.elapsed();
        progress.set_stage(Some("Writing the last segments"));
        let fingerprints = sink.take_fingerprints();
        sink.finish().map_err(|e| engine(format!("{e:#}")))?;
        // Each source recorded its parser; the catalog keeps a copy of every parser's TOML (ADR 0026).
        parsers::record(&conn, &parser_set).map_err(engine)?;
        record_fingerprints(&conn, &fingerprints);
        let t2 = t0.elapsed();
        let maintenance_notices = self.maintain(&conn, Some(progress));
        self.bump_version(&conn)?;
        log::debug!("import: parse {t1:?} finish {t2:?} maintained {:?}", t0.elapsed());
        let mut report = ImportReport {
            notices: maintenance_notices,
            ..ImportReport::default()
        };
        let mut unrecognized = Vec::new();
        for o in outcomes {
            match o {
                Outcome::Imported(r) => {
                    report.events += r.events;
                    report.bytes += r.bytes;
                    report.sources.push(read::report_source(&r));
                }
                Outcome::Unrecognized(u) => unrecognized.push(u),
                Outcome::Notice(n) => report.notices.push(n),
            }
        }
        {
            let mut s = write_lock(&self.state);
            s.unrecognized.extend(unrecognized.iter().cloned());
            s.unrecognized.sort_by(|a, b| a.name.cmp(&b.name));
        }
        report.unrecognized = unrecognized;
        report.elapsed_ms = started.elapsed().as_millis() as u64;
        inspect::record_import(&conn, &report);
        Ok(report)
    }
}

/// Keeps the content fingerprints of the files read (best effort: a rename is then only not recognised).
fn record_fingerprints(conn: &Connection, fingerprints: &[(u32, debrief_core::ingest::Fingerprint)]) {
    for (id, fp) in fingerprints {
        if let Err(e) = conn.execute(
            "INSERT OR REPLACE INTO source_fingerprints VALUES (?, ?, ?)",
            params![id, fp.size, fp.hash],
        ) {
            log::warn!("could not keep the fingerprint of source {id}: {e}");
            return;
        }
    }
}

/// A DuckDB store in a folder (`data.duckdb` plus a `tmp` spill folder). Cheap to share: clones use
/// the same database.
#[derive(Clone)]
pub struct DuckStore {
    inner: Arc<Inner>,
}

fn open_db(dir: &Path, opts: &StoreOptions) -> StoreResult<Connection> {
    shutdown::install();
    let tmp = match &opts.spill_dir {
        Some(base) => spill_folder(base, dir),
        None => dir.join("tmp"),
    };
    std::fs::create_dir_all(&tmp)?;
    let background = if opts.allocator_background_threads { "true" } else { "false" };
    let config = Config::default()
        .max_memory(&opts.memory_limit)
        .and_then(|c| c.threads(opts.duck_threads.max(1) as i64))
        // DuckDB 2.0 reads Parquet through a pool of I/O threads that defaults to four per core and does not follow
        // `threads`; bound it by the same Performance setting (rule 13) so the worker's memory budget holds.
        .and_then(|c| c.with("async_threads", opts.duck_threads.max(1).to_string()))
        .and_then(|c| c.with("storage_compatibility_version", STORAGE_VERSION))
        .and_then(|c| c.with("temp_directory", tmp.to_string_lossy()))
        .and_then(|c| c.enable_autoload_extension(false))
        // The catalog is small; a checkpoint at the end of each change is enough.
        .and_then(|c| c.with("checkpoint_threshold", "1GB"))
        // Freed memory goes back to the OS after each task above this (DuckDB's jemalloc on Linux keeps up to
        // 128 MB by default, which the worker's 300 MB query budget cannot spare).
        .and_then(|c| c.with("allocator_flush_threshold", &opts.allocator_flush))
        // DuckDB's own jemalloc (Debrief's Rust code uses mimalloc) returns freed pages through background threads, off by
        // default: without them a Linux store kept anomaly detection's freed memory (+165 MB over a 64 MB limit in
        // CI). No effect where DuckDB uses the system allocator.
        .and_then(|c| c.with("allocator_background_threads", background))
        .and_then(|c| match &opts.max_spill {
            Some(max) => c.with("max_temp_directory_size", max),
            None => Ok(c),
        })
        .map_err(engine)?;
    let conn = Connection::open_with_flags(dir.join(DB_FILE), config).map_err(engine)?;
    // Segment footers are parsed once, not per query (a setting of the Parquet reader, so set after opening).
    if let Err(e) = conn.execute_batch("SET GLOBAL parquet_metadata_cache = true") {
        log::warn!("Parquet metadata cache unavailable: {e}");
    }
    Ok(conn)
}

/// After opening: drops what a crash left (unlisted segment files, the trash) and points the `events`
/// view at this folder's segments (the store may have been moved or unpacked elsewhere).
fn recover(conn: &Connection, dir: &Path, schema: &EventSchema) -> anyhow::Result<()> {
    let listed: std::collections::BTreeSet<String> = {
        let mut stmt = conn.prepare("SELECT file FROM segments")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<duckdb::Result<_>>()?
    };
    let seg_dir = dir.join(segments::SEGMENTS_DIR);
    std::fs::create_dir_all(&seg_dir)?;
    for f in &listed {
        if !seg_dir.join(f).is_file() {
            return Err(MissingSegment(f.clone()).into());
        }
    }
    for left in segments::sweep(&seg_dir, &listed) {
        log::warn!("could not delete {}", left.display());
    }
    compact::empty_trash(conn)?;
    writer::refresh_view(conn, dir, schema)
}

pub(crate) fn invalid(e: anyhow::Error) -> StoreError {
    StoreError::Invalid(format!("{e:#}"))
}

impl DuckStore {
    /// A new, empty store in `dir` (an existing database there is replaced) reading with `profile`
    /// and the plugins in `registry`.
    pub fn create(dir: &Path, profile: Profile, registry: PluginRegistry) -> StoreResult<DuckStore> {
        Self::create_with(dir, profile, registry, StoreOptions::default())
    }

    /// [`DuckStore::create`] with chosen limits.
    pub fn create_with(dir: &Path, profile: Profile, registry: PluginRegistry, opts: StoreOptions) -> StoreResult<DuckStore> {
        registry.resolve();
        let parsers = registry.compile_parsers(&profile, &ParserSetup::builtins()).map_err(invalid)?;
        let compiled = parsers.base().clone();
        std::fs::create_dir_all(dir)?;
        for f in [DB_FILE.to_string(), format!("{DB_FILE}.wal")] {
            match std::fs::remove_file(dir.join(f)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
        match std::fs::remove_dir_all(dir.join(segments::SEGMENTS_DIR)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        let conn = open_db(dir, &opts)?;
        let schema = ingest::schema_for(&compiled);
        conn.execute_batch(&schema::ddl(&schema)).map_err(engine)?;
        conn.execute_batch(&schema::events_view(&schema, &[])).map_err(engine)?;
        conn.execute_batch(schema::CACHE_DDL).map_err(engine)?;
        conn.execute_batch(lanes::DDL).map_err(engine)?;
        for (k, v) in [
            ("format_version", FORMAT_VERSION.to_string()),
            ("declared", schema::encode_declared(&schema)),
            ("profile", compiled.profile.name.clone()),
        ] {
            conn.execute("INSERT INTO meta VALUES (?, ?)", params![k, v]).map_err(engine)?;
        }
        for (bit, m) in compiled.markers.iter().enumerate() {
            conn.execute("INSERT INTO marker_kinds VALUES (0, ?, ?, ?)", params![bit as u8, m.kind, m.owner])
                .map_err(engine)?;
        }
        Ok(Self::with(dir, conn, schema, parsers, registry, opts))
    }

    /// Reopens the store in `dir`. A database written by a newer Debrief is refused with a message.
    pub fn open(dir: &Path, profile: Profile, registry: PluginRegistry) -> StoreResult<DuckStore> {
        Self::open_with(dir, profile, registry, StoreOptions::default())
    }

    /// [`DuckStore::open`] with chosen limits.
    pub fn open_with(dir: &Path, profile: Profile, registry: PluginRegistry, opts: StoreOptions) -> StoreResult<DuckStore> {
        if !dir.join(DB_FILE).is_file() {
            return Err(StoreError::NotFound(format!("no store in {}", dir.display())));
        }
        registry.resolve();
        let parsers = registry.compile_parsers(&profile, &ParserSetup::builtins()).map_err(invalid)?;
        let conn = open_db(dir, &opts)?;
        let meta = |key: &str| -> StoreResult<String> {
            conn.query_row("SELECT value FROM meta WHERE key = ?", params![key], |r| r.get(0))
                .map_err(|e| StoreError::Invalid(format!("not a Debrief store ({key}: {e})")))
        };
        let version: u32 = meta("format_version")?.parse().unwrap_or(u32::MAX);
        if version > FORMAT_VERSION {
            return Err(StoreError::Invalid(format!(
                "this project was saved by a newer Debrief (store format {version}; this Debrief reads up to {FORMAT_VERSION}); update Debrief to open it"
            )));
        }
        let schema =
            schema::decode_declared(&meta("declared")?).ok_or_else(|| StoreError::Invalid("the store's column list is damaged".into()))?;
        if version < FORMAT_VERSION {
            return Err(StoreError::Invalid(format!(
                "this project was saved by an older preview of Debrief (store format {version}); open its log files again"
            )));
        }
        recover(&conn, dir, &schema).map_err(|e| match e.downcast_ref::<MissingSegment>() {
            Some(m) => StoreError::Damaged(m.to_string()),
            None => engine(format!("{e:#}")),
        })?;
        // Added after the format was first written; created on open so older stores gain it.
        conn.execute_batch(schema::CACHE_DDL).map_err(engine)?;
        conn.execute_batch(lanes::DDL).map_err(engine)?;
        conn.execute_batch(schema::ADDED_DDL).map_err(engine)?;
        // Rebuildable: a store whose blocks cannot be filled in still opens (its blockless segments are not compacted
        // and keep their search entries through a clock offset).
        if let Err(e) = conn.execute_batch(schema::BLOCKS_BACKFILL) {
            log::warn!("the id blocks of older segments could not be filled in: {e}");
        }
        // Positions a failed ordering left stale (pos::STALE) or that are missing are not rebuilt here, inside the
        // open call: the app sees `positioned() < total` and runs MaintenanceAction::OrderEvents as a job.
        if pos::is_stale(&conn) {
            log::warn!("the events' time order is stale; it is rebuilt by the next ordering");
        }
        Ok(Self::with(dir, conn, schema, parsers, registry, opts))
    }

    fn with(
        dir: &Path,
        conn: Connection,
        schema: EventSchema,
        parsers: ParserSet,
        registry: PluginRegistry,
        opts: StoreOptions,
    ) -> DuckStore {
        // The store keeps its own "Parse with…" choices (ADR 0026); the library comes from the app.
        let overrides = parsers::overrides(&conn);
        let parsers = if overrides.is_empty() {
            parsers
        } else {
            let mut setup = parsers.setup().clone();
            setup.overrides = overrides;
            match registry.compile_parsers(&parsers.base().profile, &setup) {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("the parser choices of this store cannot be applied: {e:#}");
                    parsers
                }
            }
        };
        let parsers = Arc::new(parsers);
        let write_lock = Arc::new(Mutex::new(()));
        let heavy = Arc::new(Mutex::new(()));
        let bulk_import = Arc::new(AtomicBool::new(false));
        // Bumped by every change of the data; the background loops look at the database only when it moved.
        let generation = Arc::new(AtomicU64::new(0));
        let started = conn
            .try_clone()
            .map_err(anyhow::Error::from)
            .and_then(|c| fts::FtsService::start(dir, c, write_lock.clone(), bulk_import.clone(), generation.clone(), opts.index));
        let (fts, fts_error) = match started {
            Ok(f) => (Some(f), None),
            Err(e) => {
                log::warn!("the full-text index is unavailable: {e:#}");
                (None, Some(format!("the full-text index is unavailable: {e:#}")))
            }
        };
        let catalogue = conn
            .try_clone()
            .map_err(anyhow::Error::from)
            .and_then(|c| {
                catalogue::CatalogueService::start(c, write_lock.clone(), heavy.clone(), generation.clone(), opts.catalogue_workers)
            })
            .map_err(|e| log::warn!("the catalogue is unavailable: {e:#}"))
            .ok();
        DuckStore {
            inner: Arc::new(Inner {
                dir: dir.to_path_buf(),
                db: Mutex::new(conn),
                pool: Mutex::new(Vec::new()),
                registry,
                schema: Arc::new(schema),
                state: RwLock::new(State {
                    profile: parsers.base().clone(),
                    parsers,
                    roots: Vec::new(),
                    removed: HashSet::new(),
                    unrecognized: Vec::new(),
                }),
                generation,
                importing: AtomicBool::new(false),
                bulk_import,
                import_window: RwLock::new(Default::default()),
                write_lock,
                heavy,
                fts,
                catalogue,
                fts_error,
                hits: hits::HitsCache::new(&dir.join("tmp").join("hits")),
                console: console::ConsoleState::default(),
                options: RwLock::new(opts),
                progress: Arc::new(ImportProgress::default()),
                derived: Mutex::new(None),
                flights: Mutex::new(std::collections::HashMap::new()),
                preaggregate_above: AtomicU64::new(PREAGGREGATE_ABOVE),
                import_jitter: AtomicU64::new(0),
            }),
        }
    }

    /// The store folder.
    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    /// Changes the limits; parse threads apply to the next import, DuckDB's settings at once.
    pub fn configure(&self, opts: StoreOptions) -> StoreResult<()> {
        let conn = self.inner.conn()?;
        let limit = sql::lit(&opts.memory_limit).ok_or_else(|| StoreError::Invalid("bad memory limit".into()))?;
        conn.execute_batch(&format!(
            "SET memory_limit = {limit}; SET threads = {n}; SET async_threads = {n};",
            n = opts.duck_threads.max(1)
        ))
        .map_err(|e| StoreError::Invalid(format!("{e}")))?;
        *write_lock(&self.inner.options) = opts;
        Ok(())
    }

    /// Merges small segments and drops rows of removed sources now (normally done after each change), then orders the
    /// events again through the one guarded path when segments were rewritten (as the Store pane's Compact does).
    pub fn compact(&self) -> StoreResult<Compaction> {
        self.idle()?;
        let _writing = lock(&self.inner.write_lock);
        let conn = self.inner.conn()?;
        let c = compact::compact(&conn, &self.inner.dir, &self.inner.schema).map_err(|e| engine(format!("{e:#}")))?;
        let notice = if c.replaced > 0 { self.inner.reorder(&conn) } else { None };
        self.changed();
        match notice {
            None => Ok(c),
            Some(n) => Err(StoreError::Engine(format!(
                "Compacted {} segments into {}. {n}",
                c.replaced, c.written
            ))),
        }
    }

    /// From how many events on timelines are answered from the histogram (tests lower it).
    /// Tests: makes the streaming import sleep random times (derived from `seed`, 0 = off) while listing and reading files.
    #[doc(hidden)]
    pub fn set_import_jitter(&self, seed: u64) {
        self.inner.import_jitter.store(seed, Ordering::Relaxed);
    }

    pub fn set_preaggregate_above(&self, events: u64) {
        self.inner.preaggregate_above.store(events, Ordering::Relaxed);
    }

    /// Runs `f` with `control` interrupting every statement the store runs for it on this thread: a cancelled
    /// analysis, export or preview stops at once and fails with [`StoreError::Cancelled`].
    pub fn with_control<R>(control: &debrief_core::ql::QueryControl, f: impl FnOnce() -> StoreResult<R>) -> StoreResult<R> {
        match control.scope(f) {
            Err(_) if control.is_cancelled() => Err(StoreError::Cancelled),
            other => other,
        }
    }

    /// Progress of the running (or last) import.
    pub fn progress(&self) -> Arc<ImportProgress> {
        self.inner.progress.clone()
    }
}

/// Clears the "import running" flag (and the bulk import's) when the job ends, however it ends.
pub(crate) struct Importing(pub(crate) Arc<Inner>);

impl Importing {
    /// A bulk import (open, reload, open at the end): it also sets `bulk_import`, for which the index writer gives its
    /// heaps back (an import's memory budget and the writer's never add up). The caller set `importing`.
    pub(crate) fn bulk(inner: Arc<Inner>) -> Importing {
        inner.bulk_import.store(true, Ordering::SeqCst);
        Importing(inner)
    }
}

impl Drop for Importing {
    fn drop(&mut self) {
        self.0.bulk_import.store(false, Ordering::SeqCst);
        self.0.importing.store(false, Ordering::SeqCst);
    }
}

impl DuckStore {
    pub(crate) fn idle(&self) -> StoreResult<()> {
        if self.inner.importing.load(Ordering::SeqCst) {
            return Err(StoreError::Busy("an import is running; wait for it or cancel it".into()));
        }
        Ok(())
    }

    fn changed(&self) {
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
        // Not fatal: a stale data version only means cached analyses are recomputed late.
        let bumped = self.inner.conn().and_then(|c| self.inner.bump_version(&c));
        if let Err(e) = bumped {
            log::warn!("the data version could not be bumped: {e}");
        }
    }
}

impl Ingest for DuckStore {
    fn set_import_window(&self, window: &debrief_core::ingest::ImportWindow) -> StoreResult<()> {
        debrief_core::ingest::ImportWindow::new(window.from, window.to).map_err(StoreError::Invalid)?;
        *write_lock(&self.inner.import_window) = *window;
        Ok(())
    }

    fn open(&self, paths: &[PathBuf]) -> StoreResult<ImportSession> {
        if self.inner.importing.swap(true, Ordering::SeqCst) {
            return Err(StoreError::Busy("an import is running; wait for it or cancel it".into()));
        }
        let guard = Importing::bulk(self.inner.clone());
        let canonical = paths
            .iter()
            .map(|p| std::fs::canonicalize(p).map_err(|e| StoreError::Io(format!("{}: {e}", p.display()))))
            .collect::<StoreResult<Vec<_>>>()?;
        {
            let mut s = write_lock(&self.inner.state);
            // Files a cancelled import did not read are read by this one.
            s.unrecognized.retain(|u| u.reason != NOT_IMPORTED);
            for p in &canonical {
                if !s.roots.contains(p) {
                    s.roots.push(p.clone());
                }
                s.removed.remove(p);
            }
        }
        let inner = self.inner.clone();
        ImportSession::spawn(self.inner.progress.clone(), move |progress| {
            let _guard = guard;
            inner.import(&canonical, progress)
        })
    }

    fn reload(&self) -> StoreResult<ImportSession> {
        self.reload_job()
    }

    /// Changed files (and the members of changed archives) are read again, deleted ones dropped, and new
    /// files under the opened folders read.
    fn refresh(&self, changed: &[PathBuf]) -> StoreResult<ImportSession> {
        self.idle()?;
        {
            let _writing = lock(&self.inner.write_lock);
            let conn = self.inner.conn()?;
            let mut gone = Vec::new();
            for p in changed {
                // A deleted file cannot be canonicalised: its folder is.
                let p = std::fs::canonicalize(p).unwrap_or_else(|_| match (p.parent().map(std::fs::canonicalize), p.file_name()) {
                    (Some(Ok(dir)), Some(name)) => dir.join(name),
                    _ => p.clone(),
                });
                let path = p.to_string_lossy().into_owned();
                let member = format!("{path}{}%", debrief_core::archive::ENTRY_SEPARATOR.replace('%', "\\%"));
                let mut stmt = conn
                    .prepare("SELECT id FROM sources WHERE path = ? OR path LIKE ? ESCAPE '\\'")
                    .map_err(engine)?;
                let ids = stmt
                    .query_map(params![path, member], |r| r.get::<_, u32>(0))
                    .map_err(engine)?
                    .collect::<duckdb::Result<Vec<_>>>()
                    .map_err(engine)?;
                gone.extend(ids);
                write_lock(&self.inner.state)
                    .unrecognized
                    .retain(|u| u.path != path && !u.path.starts_with(&format!("{path}{}", debrief_core::archive::ENTRY_SEPARATOR)));
            }
            if !gone.is_empty() {
                let list = gone.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
                conn.execute_batch(&format!(
                    "BEGIN TRANSACTION; DELETE FROM diagnostics WHERE source IN ({list}); DELETE FROM tail_offsets WHERE source IN ({list}); DELETE FROM sources WHERE id IN ({list}); COMMIT;"
                ))
                .map_err(engine)?;
                catalogue::forget(&conn, Some(&list)).map_err(engine)?;
                if let Some(f) = &self.inner.fts {
                    for id in &gone {
                        if let Err(e) = f.delete_source(*id) {
                            log::warn!("could not remove source {id} from the full-text index: {e:#}");
                        }
                    }
                }
                self.changed();
            }
        }
        // Reading the opened roots again reads what is not in the store: the changed and the new files.
        let roots: Vec<PathBuf> = read_lock(&self.inner.state).roots.iter().filter(|r| r.exists()).cloned().collect();
        self.open(&roots)
    }

    fn set_source_encoding(&self, source: SourceId, encoding: Option<&str>) -> StoreResult<ImportSession> {
        self.assign_encoding(source, encoding)
    }

    fn remove_source(&self, id: SourceId) -> StoreResult<()> {
        self.idle()?;
        let conn = self.inner.conn()?;
        let path: Option<String> = conn
            .query_row("SELECT path FROM sources WHERE id = ?", params![id.0], |r| r.get(0))
            .ok();
        let Some(path) = path else {
            return Err(StoreError::NotFound(format!("source {id}")));
        };
        let _writing = lock(&self.inner.write_lock);
        // The view hides the source's rows at once; compaction rewrites its segments.
        for sql in [
            "DELETE FROM diagnostics WHERE source = ?",
            "DELETE FROM tail_offsets WHERE source = ?",
            "DELETE FROM sources WHERE id = ?",
        ] {
            conn.execute(sql, params![id.0]).map_err(engine)?;
        }
        for sql in catalogue::DELETE_SQL {
            conn.execute(sql, params![id.0]).map_err(engine)?;
        }
        if let Some(f) = &self.inner.fts {
            if let Err(e) = f.delete_source(id.0) {
                log::warn!("could not remove source {id} from the full-text index: {e:#}");
            }
        }
        let notices = self.inner.maintain(&conn, None);
        let mut s = write_lock(&self.inner.state);
        let p = PathBuf::from(&path);
        s.roots.retain(|r| *r != p);
        s.removed.insert(p);
        drop(s);
        self.changed();
        match notices.first() {
            None => Ok(()),
            Some(n) => Err(StoreError::Engine(format!("The source was removed. {n}"))),
        }
    }

    fn clear(&self) -> StoreResult<()> {
        self.idle()?;
        self.inner.forget_events()?;
        let mut s = write_lock(&self.inner.state);
        s.roots.clear();
        s.removed.clear();
        Ok(())
    }

    /// Forgets everything and imports the state's roots (files it removed stay out), with the state's clock offsets
    /// applied to the events as they are read.
    fn restore(&self, state: &WorkspaceState) -> StoreResult<ImportSession> {
        self.clear()?;
        debrief_core::ports::Skew::set_clock_offsets(self, &state.offsets)?;
        write_lock(&self.inner.state).removed = state.removed.iter().map(PathBuf::from).collect();
        let roots: Vec<PathBuf> = state.roots.iter().map(PathBuf::from).collect();
        self.open(&roots)
    }

    /// Live tail: reads what was appended to the changed files and the files that appeared under opened folders (see
    /// the `tail` module); with no paths, only what is due. Waits for a running import first (at most
    /// [`tail::TAIL_WAIT`]).
    fn append_tail(&self, changed: &[PathBuf]) -> StoreResult<TailAppend> {
        self.tail_refresh(changed)
    }
}

impl DuckStore {
    fn fts(&self) -> StoreResult<&fts::FtsService> {
        self.inner
            .fts
            .as_ref()
            .ok_or_else(|| StoreError::Engine(self.inner.fts_error.clone().unwrap_or_else(|| "no full-text index".into())))
    }

    /// Waits until the full-text index covers every segment (tests and benchmarks), at most `timeout`.
    pub fn wait_for_index(&self, timeout: std::time::Duration) -> StoreResult<IndexStatus> {
        let started = std::time::Instant::now();
        loop {
            let status = self.index_status()?;
            if status.ready || started.elapsed() > timeout {
                return Ok(status);
            }
            self.fts()?.wake();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

impl Search for DuckStore {
    /// Words of `text` ([`debrief_core::words`]: all required, the last a prefix, case-insensitive, split on
    /// punctuation and inside `camelCase`); the first `limit` hits (default 1,000, at most 10,000) in time
    /// order. A quoted phrase must also appear (ignoring case and separators: `"graphic builder"` finds `Graphic.Builder`) in the message, exception or
    /// template; up to 1,000 candidates are checked, beyond that the total is not exact. Only indexed
    /// segments are searched ([`Search::index_status`] tells how far indexing is).
    fn search(&self, req: &SearchRequest) -> StoreResult<SearchReply> {
        let limit = if req.limit == 0 {
            fts::DEFAULT_HITS
        } else {
            req.limit.min(fts::MAX_HITS)
        };
        let index = &self.fts()?.shared.fts;
        let words = debrief_core::words::WordQuery::parse(&req.text);
        let phrases = &words.phrases;
        if phrases.is_empty() {
            let (total, ids) = index.search(&words, limit).map_err(|e| engine(format!("{e:#}")))?;
            return Ok(SearchReply { total, ids, exact: true });
        }
        // The index has no positions: its hits hold every word; the phrases are checked on the rows.
        let (candidates, ids) = index.search(&words, fts::MAX_VERIFY).map_err(|e| engine(format!("{e:#}")))?;
        let mut matched = Vec::new();
        let mut verified = 0u64;
        for chunk in ids.chunks(debrief_core::ports::MAX_PAGE_ROWS) {
            let ok: HashSet<EventId> = self
                .texts(chunk)?
                .into_iter()
                .filter(|(_, text)| phrases.iter().all(|p| debrief_core::words::phrase_in(p, text)))
                .map(|(id, _)| id)
                .collect();
            for id in chunk.iter().filter(|id| ok.contains(*id)) {
                verified += 1;
                if matched.len() < limit {
                    matched.push(*id);
                }
            }
        }
        let exact = candidates as usize <= ids.len();
        Ok(SearchReply {
            total: if exact { verified } else { candidates },
            ids: matched,
            exact,
        })
    }

    /// Counts the events matching `filter` with its text as a substring but not as words: one SQL count when
    /// SQL expresses the filter, else a scan of at most the analysis limit of events (then `complete` is
    /// false). Interrupted by a cancelled [`debrief_core::ql::QueryControl`].
    fn partial_matches(&self, filter: &debrief_core::ports::Filter) -> StoreResult<debrief_core::ports::PartialMatches> {
        use debrief_core::query::TextMode;
        if filter.text.trim().is_empty() || filter.regex || filter.case_sensitive {
            return Ok(debrief_core::ports::PartialMatches { added: 0, complete: true });
        }
        let with = |mode| debrief_core::ports::Filter {
            text_mode: mode,
            ..filter.clone()
        };
        let (substring, words) = (with(TextMode::Substring), with(TextMode::Words));
        let inner = &self.inner;
        let conn = inner.conn()?;
        let ctx = inner.ctx(&conn)?;
        let (fs, _) = inner.compile(&ctx, &substring)?;
        let (fw, cw) = inner.compile(&ctx, &words)?;
        if fs.exact && fw.exact {
            let sql = format!(
                "SELECT count(*) FROM {} WHERE {} AND NOT coalesce({}, FALSE)",
                sql::FROM,
                fs.all_sql(),
                fw.content_sql()
            );
            let cancelled = || debrief_core::ql::QueryControl::current().is_some_and(|c| c.is_cancelled());
            let added = match read::count(&conn, &sql) {
                Err(_) if cancelled() => return Err(StoreError::Cancelled),
                other => other?,
            };
            return Ok(debrief_core::ports::PartialMatches {
                added: added as u64,
                complete: true,
            });
        }
        drop(conn);
        let mut added = 0u64;
        let coverage = inner.stream_rows(&substring, analysis::MAX_ANALYSED, &mut |r| {
            if !cw.content(&r.rec, &r.source) {
                added += 1;
            }
        })?;
        Ok(debrief_core::ports::PartialMatches {
            added,
            complete: coverage.complete(),
        })
    }

    fn index_status(&self) -> StoreResult<IndexStatus> {
        let f = self.fts()?;
        let conn = self.inner.conn()?;
        let (pending, total): (u64, u64) = conn
            .query_row(
                "SELECT (SELECT count(*) FROM segments WHERE NOT indexed), (SELECT coalesce(sum(events), 0) FROM sources WHERE ready)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(engine)?;
        if let Some(e) = crate::lock(&f.shared.failure).clone() {
            log::debug!("last indexing failure: {e}");
        }
        Ok(IndexStatus {
            indexed: f.shared.fts.docs(),
            total,
            ready: pending == 0,
        })
    }
}

#[cfg(test)]
mod maintain_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_failures_become_typed_errors() {
        let locked = classify("IO Error: Could not set lock on file \"x/catalog.duckdb\": Conflicting lock is held".into());
        assert!(
            matches!(&locked, StoreError::Locked(m) if m.contains("another Debrief window")),
            "{locked:?}"
        );
        assert!(locked.is_retryable());
        let windows = classify(
            "IO Error: Cannot open file \"data.duckdb\": The process cannot access the file because it is being used by another \
             process.\r\n\nFile is already open in \ndebrief-store-worker.exe (PID 1)"
                .into(),
        );
        assert!(matches!(windows, StoreError::Locked(_)), "{windows:?}");
        for damaged in [
            "Invalid Input Error: No magic bytes found at end of file 'seg.parquet'",
            "Invalid Input Error: File 'seg.parquet' too small to be a Parquet file",
            "IO Error: Parquet file is corrupt",
        ] {
            let e = classify(damaged.into());
            assert!(matches!(&e, StoreError::Damaged(m) if m.contains("damaged")), "{e:?}");
            assert!(!e.is_retryable());
        }
        assert!(matches!(classify("Out of Memory Error".into()), StoreError::Engine(_)));
        assert_eq!(
            MissingSegment("seg-1.parquet".into()).to_string(),
            "the data file seg-1.parquet of this project is missing"
        );
    }

    /// The storage version DuckDB wrote into a database file's header (8 bytes of checksum, "DUCK", then the version).
    fn storage_version(file: &Path) -> u64 {
        let bytes = std::fs::read(file).unwrap();
        assert_eq!(&bytes[8..12], b"DUCK", "not a DuckDB file");
        u64::from_le_bytes(bytes[12..20].try_into().unwrap())
    }

    #[test]
    fn a_new_catalog_is_written_in_the_storage_format_the_previous_release_reads() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("pin");
        let conn = open_db(&dir, &StoreOptions::default()).unwrap();
        conn.execute_batch("CREATE TABLE t(x INTEGER); INSERT INTO t VALUES (1); CHECKPOINT")
            .unwrap();
        drop(conn);
        // DuckDB 1.5 reads storage versions 64 to 68; a file created by 2.0 without the pin is version 69.
        let pinned = storage_version(&dir.join(DB_FILE));
        assert!((64..=68).contains(&pinned), "storage version {pinned}");
        // The control: the same file created without the pin is newer, so the test would notice the pin going.
        let unpinned_file = dir.join("unpinned.duckdb");
        let latest = Config::default().with("storage_compatibility_version", "latest").unwrap();
        let unpinned = Connection::open_with_flags(&unpinned_file, latest).unwrap();
        unpinned.execute_batch("CREATE TABLE t(x INTEGER); CHECKPOINT").unwrap();
        drop(unpinned);
        assert!(
            storage_version(&unpinned_file) > 68,
            "the control file is not newer than DuckDB 1.5 reads"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duckdbs_newer_storage_version_error_is_not_an_engine_failure() {
        // The text DuckDB 1.5.6 prints for a catalog that DuckDB 2.0 created without the pin (captured from its CLI).
        let e = classify(
            "IO Error: Trying to read a database file with version number 69, but we can only read versions between 64 and 68.\n\
             The database file was created with a newer version of DuckDB.\n\n\
             Newer DuckDB version might introduce backward incompatible changes (possibly guarded by compatibility settings).\n\
             See the storage page for migration strategy and more information: https://duckdb.org/internals/storage"
                .into(),
        );
        assert!(
            matches!(&e, StoreError::Invalid(m) if m.contains("saved by a newer Debrief") && m.contains("update Debrief") && m.contains("69")),
            "{e:?}"
        );
        assert!(!e.is_retryable());
        // A later DuckDB's own wording of the same failure.
        let e = classify("IO Error: The database file was created with a newer version of DuckDB.".into());
        assert!(
            matches!(&e, StoreError::Invalid(m) if m.contains("saved by a newer Debrief")),
            "{e:?}"
        );
    }

    #[test]
    fn a_borrowed_connection_relays_duckdbs_progress_to_the_runs_control() {
        let tmp = tempfile::tempdir().unwrap();
        let store = DuckStore::create(&tmp.path().join("s"), Profile::default_profile(), PluginRegistry::new()).unwrap();
        let control = debrief_core::ql::QueryControl::new();
        control.scope(|| {
            let conn = store.inner.conn().unwrap();
            conn.execute_batch("CREATE TEMP TABLE big AS SELECT range AS i FROM range(100000000)")
                .unwrap();
            let sum: i64 = conn
                .query_row("SELECT sum(hash(i) % 7) FROM big WHERE i % 3 <> 5", [], |r| r.get(0))
                .unwrap();
            assert!(sum > 0);
        });
        let p = control.progress();
        assert_eq!(p.stage, debrief_core::ql::QueryStage::Evaluating);
        assert!(p.total > 0 && p.done <= p.total, "{p:?}");
        // Pooled connections go back with tracking off, and a run without a control starts no relay.
        let conn = store.inner.conn().unwrap();
        assert!(conn.relay.is_none());
    }

    #[test]
    fn async_threads_follow_the_chosen_threads_instead_of_four_per_core() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("async");
        let opts = StoreOptions {
            duck_threads: 3,
            ..StoreOptions::default()
        };
        let conn = open_db(&dir, &opts).unwrap();
        let setting = |name: &str| -> String {
            conn.query_row(&format!("SELECT CAST(current_setting('{name}') AS VARCHAR)"), [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(setting("async_threads"), "3");
        drop(conn);
        // Changing the limits at run time moves both together.
        let store_dir = dir.join("s");
        let _ = std::fs::remove_dir_all(&dir);
        let store = DuckStore::create_with(&store_dir, Profile::default_profile(), PluginRegistry::new(), opts).unwrap();
        store
            .configure(StoreOptions {
                duck_threads: 5,
                ..StoreOptions::default()
            })
            .unwrap();
        let conn = store.inner.conn().unwrap();
        let value: String = conn
            .query_row("SELECT CAST(current_setting('async_threads') AS VARCHAR)", [], |r| r.get(0))
            .unwrap();
        assert_eq!(value, "5");
        drop(conn);
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_database_gives_freed_memory_back_and_keeps_its_limit() {
        let dir = std::env::temp_dir().join(format!("debrief-open-db-{}", std::process::id()));
        let conn = open_db(&dir, &StoreOptions::default()).unwrap();
        let setting = |name: &str| -> String {
            conn.query_row(&format!("SELECT CAST(current_setting('{name}') AS VARCHAR)"), [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(setting("allocator_flush_threshold"), "32MiB");
        assert_eq!(setting("allocator_background_threads"), "true");
        assert_eq!(setting("threads"), DUCK_THREADS.to_string());
        assert_eq!(setting("temp_directory"), dir.join("tmp").to_string_lossy());
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_database_takes_the_chosen_spill_folder_and_allocator_settings() {
        let root = std::env::temp_dir().join(format!("debrief-open-db-opts-{}", std::process::id()));
        let (dir, spill) = (root.join("store"), root.join("spill"));
        std::fs::create_dir_all(&dir).unwrap();
        let opts = StoreOptions {
            duck_threads: 3,
            memory_limit: "100MB".into(),
            spill_dir: Some(spill.clone()),
            max_spill: Some("2GB".into()),
            allocator_flush: "8MiB".into(),
            allocator_background_threads: false,
            ..StoreOptions::default()
        };
        let conn = open_db(&dir, &opts).unwrap();
        let setting = |name: &str| -> String {
            conn.query_row(&format!("SELECT CAST(current_setting('{name}') AS VARCHAR)"), [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(setting("threads"), "3");
        assert_eq!(setting("allocator_flush_threshold"), "8MiB");
        assert_eq!(setting("allocator_background_threads"), "false");
        assert!(
            setting("max_temp_directory_size").starts_with("1.8 GiB"),
            "{}",
            setting("max_temp_directory_size")
        );
        let own = spill_folder(&spill, &dir);
        assert_eq!(setting("temp_directory"), own.to_string_lossy());
        assert!(own.is_dir() && own.starts_with(&spill));
        assert_ne!(own, spill_folder(&spill, &root.join("other")), "a folder per store");
        assert_eq!(own, spill_folder(&spill, &dir), "stable");
        drop(conn);
        let _ = std::fs::remove_dir_all(&root);
    }
}
