//! The cache database itself, synchronous and single-connection: open, migrate, recreate, save, evict, forget (ADR 0029
//! section 2 and 4). The writer thread owns one [`CacheDb`]; loads use [`load_disk`] on a read-only connection of their own.
//!
//! Everything in the file is rebuildable, so damage is never kept: a newer schema or a file that is not a database is deleted
//! with its `-wal` and `-shm` and started again. A busy or locked file is not damage.

use dyno_core::disk::index::{DiskIndex, RecordBounds, RecordDecoder, RecordError, RecordWriter, RECORD_FORMAT};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::io::Read;
use std::path::{Path, PathBuf};

/// The schema version of the file.
pub const SCHEMA: i64 = 1;
/// Forward steps: `STEPS[n]` takes schema `n + 1` to `n + 2`. None yet; the framework is tested with its own list.
const STEPS: &[&str] = &[];
/// The most uncompressed bytes of one chunk (and so the most one chunk may inflate to).
pub const CHUNK: usize = 4 * 1024 * 1024;
/// The most saved targets.
pub const MAX_TARGETS: i64 = 16;
/// A save leaves at least this many bytes free on the cache volume.
pub const FREE_FLOOR_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// ... and at least this share of the volume (percent).
pub const FREE_FLOOR_PERCENT: u64 = 5;

const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS disk_saved (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  target_key TEXT NOT NULL UNIQUE,
  target TEXT NOT NULL,
  volume_identity TEXT,
  status TEXT NOT NULL,
  limits TEXT NOT NULL,
  scanned_at INTEGER NOT NULL,
  saved_at INTEGER NOT NULL,
  used_bytes_then INTEGER,
  record_format INTEGER NOT NULL,
  nodes INTEGER NOT NULL,
  stored_bytes INTEGER NOT NULL,
  packed_bytes INTEGER NOT NULL,
  last_shown_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS disk_chunks (
  saved INTEGER NOT NULL REFERENCES disk_saved(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  data BLOB NOT NULL,
  PRIMARY KEY (saved, seq)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS view_saved (key TEXT PRIMARY KEY, format INTEGER NOT NULL, saved_at INTEGER NOT NULL, data TEXT NOT NULL);
";

/// Why a cache operation did not happen.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// SQLite failed.
    #[error("cache database: {0}")]
    Sql(#[from] rusqlite::Error),
    /// A file operation failed.
    #[error("cache file: {0}")]
    Io(#[from] std::io::Error),
    /// Another process holds the file: not damage, caching is off for this run.
    #[error("the cache database is in use by another process")]
    Busy,
    /// A saved map whose chunks do not parse.
    #[error("a saved map is damaged: {0}")]
    Damaged(String),
    /// The map alone is larger than the cap.
    #[error("the map is larger than the saved-maps limit")]
    TooBig,
    /// The cache volume would keep less free space than the floor.
    #[error("not enough free space to save the map")]
    NoSpace,
    /// A test-injected failure.
    #[error("injected failure")]
    Injected,
}

fn busy(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked,
                ..
            },
            _
        )
    )
}

/// Free and total bytes of the volume a file is on: a port, so a test can fake a full disk.
pub trait FreeSpace: Send + Sync {
    /// `(total, free)` bytes of the volume holding `dir`; `None` when unknown (then no floor applies).
    fn volume_of(&self, dir: &Path) -> Option<(u64, u64)>;
}

/// The real answer from the OS.
pub struct OsFreeSpace;

impl FreeSpace for OsFreeSpace {
    fn volume_of(&self, dir: &Path) -> Option<(u64, u64)> {
        // The folder may not exist yet on the first save: ask about the nearest folder that does.
        let mut at = dir;
        while !at.exists() {
            at = at.parent()?;
        }
        dyno_platform::volumes::capacity(&at.to_string_lossy())
    }
}

/// What a save writes besides the tree (the columns of `disk_saved`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveParams {
    /// `volume:<id>` or `folder:<path>`.
    pub target_key: String,
    /// `DiskTarget` as JSON.
    pub target: String,
    /// The volume's identity where the OS gives one.
    pub volume_identity: Option<String>,
    /// `DiskScanStatus` at the end of the pass, JSON.
    pub status: String,
    /// `ScanLimits` as JSON.
    pub limits: String,
    /// Wall ms, when the pass ended.
    pub scanned_at: i64,
    /// The volume's used bytes when the pass ended.
    pub used_bytes_then: Option<u64>,
}

/// One row of `disk_saved`, without its chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedRow {
    /// The row id (a scan id's stand-in until loaded).
    pub id: i64,
    /// The columns a save wrote.
    pub params: SaveParams,
    /// Wall ms, when this row was written.
    pub saved_at: i64,
    /// The record layout number.
    pub record_format: u32,
    /// Nodes in the tree.
    pub nodes: u64,
    /// Bytes of the uncompressed stream.
    pub stored_bytes: u64,
    /// Bytes the chunks take in the file.
    pub packed_bytes: u64,
    /// Wall ms, last time the map was shown.
    pub last_shown_at: i64,
}

/// What a save did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// The new row.
    pub id: i64,
    /// Bytes the chunks take.
    pub packed_bytes: u64,
    /// Rows removed to make room (or replaced), by id.
    pub removed: Vec<i64>,
}

/// The limits a save works within.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheLimits {
    /// The total cap on packed bytes (Setting `disk.savedMaps.maxMb`).
    pub max_bytes: u64,
    /// Whether chunks are compressed (decided by WP-5's measurement).
    pub compress: bool,
}

impl Default for CacheLimits {
    fn default() -> Self {
        CacheLimits {
            max_bytes: 1024 * 1024 * 1024,
            compress: COMPRESS,
        }
    }
}

/// Whether chunks are deflated (flate2 level 1): WP-5's bench settled D3, see `docs/performance.md`.
pub const COMPRESS: bool = true;

const TAG_RAW: u8 = 0;
const TAG_DEFLATE: u8 = 1;

/// A chunk as stored: one tag byte (raw or deflate) then the bytes. A chunk that does not shrink is stored raw.
pub fn encode_chunk(raw: &[u8], compress: bool) -> Vec<u8> {
    if compress {
        use flate2::write::DeflateEncoder;
        use std::io::Write;
        let mut e = DeflateEncoder::new(vec![TAG_DEFLATE], flate2::Compression::new(1));
        if e.write_all(raw).is_ok() {
            if let Ok(out) = e.finish() {
                if out.len() <= raw.len() {
                    return out;
                }
            }
        }
    }
    let mut out = Vec::with_capacity(raw.len() + 1);
    out.push(TAG_RAW);
    out.extend_from_slice(raw);
    out
}

/// The bytes of a stored chunk, never more than `cap`: a chunk that would inflate further is an error (the guard against a
/// decompression bomb; there is no ratio limit).
pub fn decode_chunk(stored: &[u8], cap: usize) -> Result<Vec<u8>, CacheError> {
    let bad = |m: &str| CacheError::Damaged(m.to_string());
    let (tag, body) = stored.split_first().ok_or_else(|| bad("empty chunk"))?;
    match *tag {
        TAG_RAW if body.len() <= cap => Ok(body.to_vec()),
        TAG_DEFLATE => {
            let mut out = Vec::new();
            flate2::read::DeflateDecoder::new(body)
                .take(cap as u64 + 1)
                .read_to_end(&mut out)
                .map_err(|_| bad("chunk does not inflate"))?;
            if out.len() > cap {
                return Err(bad("chunk inflates past the chunk size"));
            }
            Ok(out)
        }
        TAG_RAW => Err(bad("chunk larger than the chunk size")),
        _ => Err(bad("unknown chunk coding")),
    }
}

/// The open cache database (the writer's connection).
pub struct CacheDb {
    conn: Connection,
    path: PathBuf,
    /// Test hook: fail the save after this many chunks were inserted.
    fail_after_chunks: Option<usize>,
}

/// How an open went.
pub enum Opened {
    /// Ready, with a note for the activity log when the old file had to be cleared.
    Ready(CacheDb, Option<&'static str>),
    /// Another process holds the file: caching is off for this run, the file stays.
    Busy,
}

/// The note for the activity log when saved maps had to be cleared.
pub const CLEARED_NOTE: &str = "Saved disk maps could not be read and were cleared; scan again to see a map";

/// The files of a database: itself, `-wal`, `-shm`.
pub fn files_of(path: &Path) -> [PathBuf; 3] {
    let with = |ext: &str| {
        let mut o = path.as_os_str().to_owned();
        o.push(ext);
        PathBuf::from(o)
    };
    [path.to_path_buf(), with("-wal"), with("-shm")]
}

/// Removes the database and its `-wal` and `-shm`.
///
/// Every file is tried; the first failure other than "not there" is returned (Windows refuses to delete a file another handle holds
/// open). The caller then empties the database in place ([`wipe_in_place`]) instead of claiming the files are gone.
pub fn delete_files(path: &Path) -> std::io::Result<()> {
    let mut first = Ok(());
    for f in files_of(path) {
        match std::fs::remove_file(f) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound && first.is_ok() => first = Err(e),
            _ => {}
        }
    }
    first
}

/// Empties a database that could not be deleted: every table dropped with `secure_delete` on (freed pages overwritten), the
/// log truncated and the file vacuumed. Works on a file of any schema; fails on a file that is not a database.
pub fn wipe_in_place(path: &Path) -> Result<(), CacheError> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_millis(1500))?;
    conn.execute_batch("PRAGMA secure_delete = ON;")?;
    let tables: Vec<String> = {
        let mut q = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")?;
        let rows = q.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<_, _>>()?
    };
    for t in tables {
        conn.execute_batch(&format!("DROP TABLE IF EXISTS \"{}\";", t.replace('"', "\"\"")))?;
    }
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    conn.execute_batch("VACUUM;")?;
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    Ok(())
}

fn prepare_folder(path: &Path) -> Result<(), CacheError> {
    if let Some(dir) = path.parent() {
        // Owner-only from creation when missing (0700 on Unix, whatever the umask); an existing folder is used as it is (ADR 0092).
        paddock_env::create_private_dir_all(dir)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Created private from the first byte (no chmod after); SQLite gives `-wal` and `-shm` the same mode.
        let f = std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(path)?;
        drop(f);
    }
    Ok(())
}

impl CacheDb {
    /// Opens (or creates) the database at `path`; see the module comment for damage and busy files.
    pub fn open(path: &Path) -> Result<Opened, CacheError> {
        prepare_folder(path)?;
        match Self::try_open(path, STEPS) {
            Ok(db) => Ok(Opened::Ready(db, None)),
            Err(CacheError::Sql(e)) if busy(&e) => Ok(Opened::Busy),
            Err(CacheError::Busy) => Ok(Opened::Busy),
            Err(e) => {
                log::warn!(Cause = e.to_string(); "Saved views database cleared and started again; next step: scan again to see a map");
                if let Err(why) = delete_files(path) {
                    log::warn!(Cause = why.to_string(); "Cache files could not be deleted; emptying the database in place instead");
                    if let Err(e) = wipe_in_place(path) {
                        log::warn!(Cause = e.to_string(); "Cache database could not be emptied either; caching is off for this run");
                        return Ok(Opened::Busy);
                    }
                }
                prepare_folder(path)?;
                Ok(Opened::Ready(Self::try_open(path, STEPS)?, Some(CLEARED_NOTE)))
            }
        }
    }

    fn try_open(path: &Path, steps: &[&str]) -> Result<CacheDb, CacheError> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_millis(1500))?;
        // Before any table exists: freed pages can then be given back (incremental vacuum).
        conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL;")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA secure_delete = ON; PRAGMA foreign_keys = ON;")?;
        let known: Option<String> = conn
            .query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| r.get(0))
            .optional()
            .or_else(|e| match &e {
                rusqlite::Error::SqliteFailure(_, Some(m)) if m.contains("no such table") => Ok(None),
                _ => Err(e),
            })?;
        let version = known.and_then(|v| v.parse::<i64>().ok());
        match version {
            None => {
                conn.execute_batch(SCHEMA_SQL)?;
                conn.execute(
                    "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema', ?1)",
                    [(1 + steps.len() as i64).to_string()],
                )?;
            }
            Some(v) if v > 1 + steps.len() as i64 => {
                return Err(CacheError::Damaged(format!("written by a newer Dyno (schema {v})")));
            }
            Some(v) => migrate(&conn, v, steps)?,
        }
        Ok(CacheDb {
            conn,
            path: path.to_path_buf(),
            fail_after_chunks: None,
        })
    }

    /// The file this database is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every saved map, newest write first.
    pub fn list(&self) -> Result<Vec<SavedRow>, CacheError> {
        list_rows(&self.conn)
    }

    /// Writes a tree: the new row, its chunks and the removal of the old row of the same target (and of the least recently
    /// shown ones that must go to fit the cap and the target count) in one transaction. A failure leaves everything as it was.
    pub fn save_disk(
        &mut self,
        p: &SaveParams,
        index: &DiskIndex,
        now: i64,
        limits: CacheLimits,
        free: &dyn FreeSpace,
    ) -> Result<Saved, CacheError> {
        let mut writer = RecordWriter::new(index);
        let stored = writer.total_len();
        if !limits.compress && stored > limits.max_bytes {
            return Err(CacheError::TooBig);
        }
        if let Some((total, free_now)) = free.volume_of(self.path.parent().unwrap_or(Path::new("."))) {
            let floor = FREE_FLOOR_BYTES.max(total / 100 * FREE_FLOOR_PERCENT);
            // The packed size is not known yet: the uncompressed size is the safe upper bound.
            if free_now.saturating_sub(stored) < floor {
                return Err(CacheError::NoSpace);
            }
        }
        let tx = self.conn.transaction()?;
        let mut removed = Vec::new();
        if let Some(old) = tx
            .query_row("SELECT id FROM disk_saved WHERE target_key = ?1", [&p.target_key], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
        {
            tx.execute("DELETE FROM disk_chunks WHERE saved = ?1", [old])?;
            tx.execute("DELETE FROM disk_saved WHERE id = ?1", [old])?;
            removed.push(old);
        }
        tx.execute(
            "INSERT INTO disk_saved (target_key, target, volume_identity, status, limits, scanned_at, saved_at, used_bytes_then, \
             record_format, nodes, stored_bytes, packed_bytes, last_shown_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,0,?7)",
            params![
                p.target_key,
                p.target,
                p.volume_identity,
                p.status,
                p.limits,
                p.scanned_at,
                now,
                p.used_bytes_then.map(|v| i64::try_from(v).unwrap_or(i64::MAX)),
                RECORD_FORMAT,
                index.len() as i64,
                i64::try_from(stored).unwrap_or(i64::MAX),
            ],
        )?;
        let id = tx.last_insert_rowid();
        let mut packed: u64 = 0;
        let mut seq = 0i64;
        {
            let mut insert = tx.prepare("INSERT INTO disk_chunks (saved, seq, data) VALUES (?1, ?2, ?3)")?;
            while let Some(raw) = writer.next_chunk(CHUNK) {
                let blob = encode_chunk(&raw, limits.compress);
                packed += blob.len() as u64;
                if packed > limits.max_bytes {
                    return Err(CacheError::TooBig);
                }
                insert.execute(params![id, seq, blob])?;
                seq += 1;
                if self.fail_after_chunks.is_some_and(|n| seq as usize >= n) {
                    return Err(CacheError::Injected);
                }
            }
        }
        tx.execute(
            "UPDATE disk_saved SET packed_bytes = ?1 WHERE id = ?2",
            params![i64::try_from(packed).unwrap_or(i64::MAX), id],
        )?;
        // Make room: the least recently shown go until the target count and the cap hold.
        loop {
            let (count, sum): (i64, i64) = tx.query_row(
                "SELECT COUNT(*), COALESCE(SUM(packed_bytes), 0) FROM disk_saved WHERE id <> ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let over = count >= MAX_TARGETS || (sum as u64).saturating_add(packed) > limits.max_bytes;
            if !over {
                break;
            }
            let victim: Option<i64> = tx
                .query_row(
                    "SELECT id FROM disk_saved WHERE id <> ?1 ORDER BY last_shown_at, id LIMIT 1",
                    [id],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(v) = victim else { break };
            tx.execute("DELETE FROM disk_chunks WHERE saved = ?1", [v])?;
            tx.execute("DELETE FROM disk_saved WHERE id = ?1", [v])?;
            removed.push(v);
        }
        tx.commit()?;
        if !removed.is_empty() {
            self.reclaim();
        }
        Ok(Saved {
            id,
            packed_bytes: packed,
            removed,
        })
    }

    /// Removes one saved map (and its chunks); `true` when it existed.
    pub fn forget(&mut self, id: i64) -> Result<bool, CacheError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM disk_chunks WHERE saved = ?1", [id])?;
        let n = tx.execute("DELETE FROM disk_saved WHERE id = ?1", [id])?;
        tx.commit()?;
        if n > 0 {
            self.reclaim();
        }
        Ok(n > 0)
    }

    /// Removes the saved map of a target; its id when there was one.
    pub fn forget_target(&mut self, key: &str) -> Result<Option<i64>, CacheError> {
        let id: Option<i64> = self
            .conn
            .query_row("SELECT id FROM disk_saved WHERE target_key = ?1", [key], |r| r.get(0))
            .optional()?;
        if let Some(id) = id {
            self.forget(id)?;
        }
        Ok(id)
    }

    /// Notes that a map was shown (the order eviction follows).
    pub fn touch(&self, id: i64, now: i64) -> Result<(), CacheError> {
        self.conn
            .execute("UPDATE disk_saved SET last_shown_at = ?1 WHERE id = ?2", params![now, id])?;
        Ok(())
    }

    /// Stores a small "last known" document.
    pub fn put_view(&self, key: &str, format: u32, now: i64, data: &str) -> Result<(), CacheError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO view_saved (key, format, saved_at, data) VALUES (?1, ?2, ?3, ?4)",
            params![key, format, now, data],
        )?;
        Ok(())
    }

    /// Gives freed pages back to the file, with the WAL emptied so no old page lingers there (privacy, section 6).
    fn reclaim(&self) {
        let _ = self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        for _ in 0..4096 {
            let before: i64 = self.conn.query_row("PRAGMA freelist_count", [], |r| r.get(0)).unwrap_or(0);
            if before == 0 {
                break;
            }
            if self.conn.execute_batch("PRAGMA incremental_vacuum(256);").is_err() {
                break;
            }
        }
        let _ = self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    }

    /// Test hook: fail the next saves after `n` chunks.
    #[cfg(test)]
    pub(crate) fn fail_saves_after_chunks(&mut self, n: Option<usize>) {
        self.fail_after_chunks = n;
    }
}

/// Applies the forward steps from schema `from` in one transaction.
fn migrate(conn: &Connection, from: i64, steps: &[&str]) -> Result<(), CacheError> {
    let to = 1 + steps.len() as i64;
    if from >= to {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    for step in &steps[usize::try_from(from - 1).unwrap_or(0)..] {
        tx.execute_batch(step)?;
    }
    tx.execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('schema', ?1)", [to.to_string()])?;
    tx.commit()?;
    Ok(())
}

fn list_rows(conn: &Connection) -> Result<Vec<SavedRow>, CacheError> {
    let mut q = conn.prepare(
        "SELECT id, target_key, target, volume_identity, status, limits, scanned_at, saved_at, used_bytes_then, record_format, nodes, \
         stored_bytes, packed_bytes, last_shown_at FROM disk_saved ORDER BY saved_at DESC, id DESC",
    )?;
    let rows = q.query_map([], |r| {
        Ok(SavedRow {
            id: r.get(0)?,
            params: SaveParams {
                target_key: r.get(1)?,
                target: r.get(2)?,
                volume_identity: r.get(3)?,
                status: r.get(4)?,
                limits: r.get(5)?,
                scanned_at: r.get(6)?,
                used_bytes_then: r.get::<_, Option<i64>>(8)?.and_then(|v| u64::try_from(v).ok()),
            },
            saved_at: r.get(7)?,
            record_format: u32::try_from(r.get::<_, i64>(9)?).unwrap_or(0),
            nodes: u64::try_from(r.get::<_, i64>(10)?).unwrap_or(0),
            stored_bytes: u64::try_from(r.get::<_, i64>(11)?).unwrap_or(0),
            packed_bytes: u64::try_from(r.get::<_, i64>(12)?).unwrap_or(0),
            last_shown_at: r.get(13)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// A read-only connection (WAL lets it read beside the writer); `None` when the file does not exist.
fn read_only(path: &Path) -> Result<Option<Connection>, CacheError> {
    if !path.exists() {
        return Ok(None);
    }
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    c.busy_timeout(std::time::Duration::from_millis(1500))?;
    Ok(Some(c))
}

/// The saved maps, read without the writer (a few rows of JSON).
pub fn list_saved(path: &Path) -> Result<Vec<SavedRow>, CacheError> {
    match read_only(path)? {
        Some(c) => list_rows(&c),
        None => Ok(Vec::new()),
    }
}

/// A saved "last known" document: `(format, saved_at, data)`.
pub fn get_view(path: &Path, key: &str) -> Result<Option<(u32, i64, String)>, CacheError> {
    let Some(c) = read_only(path)? else { return Ok(None) };
    Ok(
        c.query_row("SELECT format, saved_at, data FROM view_saved WHERE key = ?1", [key], |r| {
            Ok((u32::try_from(r.get::<_, i64>(0)?).unwrap_or(0), r.get(1)?, r.get(2)?))
        })
        .optional()?,
    )
}

/// Loads a saved tree. Each chunk inflates into at most [`CHUNK`] bytes and the whole stream into at most the row's
/// `stored_bytes`; the row's columns bound the header before anything is allocated from it.
pub fn load_disk(path: &Path, id: i64) -> Result<DiskIndex, CacheError> {
    let Some(c) = read_only(path)? else {
        return Err(CacheError::Damaged("the cache file is gone".into()));
    };
    let (format, nodes, stored): (i64, i64, i64) = c
        .query_row(
            "SELECT record_format, nodes, stored_bytes FROM disk_saved WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| CacheError::Damaged("no such saved map".into()))?;
    let bad = |e: RecordError| CacheError::Damaged(e.to_string());
    let mut decoder = RecordDecoder::new(RecordBounds {
        format: u32::try_from(format).unwrap_or(0),
        nodes: u64::try_from(nodes).unwrap_or(0),
        stored_bytes: u64::try_from(stored).unwrap_or(0),
    });
    let mut q = c.prepare("SELECT seq, data FROM disk_chunks WHERE saved = ?1 ORDER BY seq")?;
    let mut rows = q.query([id])?;
    let mut expect = 0i64;
    while let Some(r) = rows.next()? {
        let seq: i64 = r.get(0)?;
        if seq != expect {
            return Err(CacheError::Damaged("a chunk is missing".into()));
        }
        expect += 1;
        let blob: Vec<u8> = r.get(1)?;
        let raw = decode_chunk(&blob, CHUNK)?;
        decoder.push(&raw).map_err(bad)?;
    }
    decoder.finish().map_err(bad)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use dyno_core::disk::index::{ScanLimits, Sizes};
    use dyno_core::time::Timestamp;

    pub(crate) struct Roomy;
    impl FreeSpace for Roomy {
        fn volume_of(&self, _: &Path) -> Option<(u64, u64)> {
            Some((1 << 40, 1 << 39))
        }
    }

    pub(crate) fn tree(names: &str, files: u32) -> DiskIndex {
        let mut i = DiskIndex::new(
            names,
            ScanLimits {
                max_nodes: 100_000,
                notable_bytes: 1,
                max_hard_links: 1,
            },
        );
        let r = i.root();
        for n in 0..files {
            let f = i.add_folder(r, format!("{names}-{n}")).unwrap();
            i.add_file(f, format!("{names}-file-{n}"), Sizes::same(1000 + u64::from(n)), Timestamp(5));
            i.finish_listing(f);
        }
        i.finish_listing(r);
        i
    }

    pub(crate) fn params(key: &str) -> SaveParams {
        SaveParams {
            target_key: key.into(),
            target: "{}".into(),
            volume_identity: Some("uuid-1".into()),
            status: "{}".into(),
            limits: "{}".into(),
            scanned_at: 1000,
            used_bytes_then: Some(42),
        }
    }

    pub(crate) fn open_in(dir: &Path) -> CacheDb {
        match CacheDb::open(&dir.join("cache.db")).unwrap() {
            Opened::Ready(db, _) => db,
            Opened::Busy => panic!("busy"),
        }
    }

    #[test]
    fn creates_saves_lists_and_loads_back() {
        let d = tempfile::tempdir().unwrap();
        let folder = d.path().join("cache");
        let mut db = open_in(&folder);
        let t = tree("alpha", 50);
        let saved = db.save_disk(&params("volume:/"), &t, 5000, CacheLimits::default(), &Roomy).unwrap();
        let rows = db.list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, saved.id);
        assert_eq!(rows[0].nodes, t.len() as u64);
        assert_eq!(rows[0].saved_at, 5000);
        assert_eq!(rows[0].params.volume_identity.as_deref(), Some("uuid-1"));
        assert_eq!(rows[0].params.used_bytes_then, Some(42));
        let back = load_disk(db.path(), saved.id).unwrap();
        assert_eq!(back.to_records(), t.to_records());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&folder), 0o700);
            assert_eq!(mode(db.path()), 0o600);
        }
    }

    #[test]
    fn a_save_replaces_the_target_and_reopening_finds_it() {
        let d = tempfile::tempdir().unwrap();
        let mut db = open_in(d.path());
        let first = db
            .save_disk(&params("volume:/"), &tree("a", 5), 1, CacheLimits::default(), &Roomy)
            .unwrap();
        let second = db
            .save_disk(&params("volume:/"), &tree("b", 6), 2, CacheLimits::default(), &Roomy)
            .unwrap();
        assert_eq!(second.removed, vec![first.id]);
        drop(db);
        let db = open_in(d.path());
        let rows = db.list().unwrap();
        assert_eq!(rows.len(), 1, "one saved map per target");
        assert_eq!(rows[0].id, second.id);
    }

    #[test]
    fn migrations_run_forward_in_one_transaction() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.db");
        drop(CacheDb::try_open(&p, &[]).unwrap());
        let steps = ["CREATE TABLE extra (x INTEGER)", "ALTER TABLE extra ADD COLUMN y INTEGER"];
        let db = CacheDb::try_open(&p, &steps).unwrap();
        db.conn.execute("INSERT INTO extra (x, y) VALUES (1, 2)", []).unwrap();
        let v: String = db
            .conn
            .query_row("SELECT value FROM meta WHERE key='schema'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "3");
        // A failing step rolls the whole migration back.
        drop(db);
        let p2 = d.path().join("d.db");
        drop(CacheDb::try_open(&p2, &[]).unwrap());
        assert!(CacheDb::try_open(&p2, &["CREATE TABLE ok (x)", "THIS IS NOT SQL"]).is_err());
        let c = Connection::open(&p2).unwrap();
        let v: String = c.query_row("SELECT value FROM meta WHERE key='schema'", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "1");
        assert!(
            c.query_row("SELECT 1 FROM ok", [], |_| Ok(())).is_err(),
            "the first step was rolled back too"
        );
    }

    #[test]
    fn a_newer_schema_is_refused_and_recreated() {
        let d = tempfile::tempdir().unwrap();
        let mut db = open_in(d.path());
        db.save_disk(&params("volume:/"), &tree("a", 3), 1, CacheLimits::default(), &Roomy)
            .unwrap();
        db.conn.execute("UPDATE meta SET value = '99' WHERE key = 'schema'", []).unwrap();
        drop(db);
        match CacheDb::open(&d.path().join("cache.db")).unwrap() {
            Opened::Ready(db, note) => {
                assert_eq!(note, Some(CLEARED_NOTE));
                assert!(db.list().unwrap().is_empty());
            }
            Opened::Busy => panic!("busy"),
        }
    }

    #[test]
    fn a_file_of_garbage_is_recreated_with_its_wal_and_shm() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("cache.db");
        std::fs::write(&p, vec![0xAB; 5000]).unwrap();
        std::fs::write(files_of(&p)[1].clone(), b"stale wal").unwrap();
        std::fs::write(files_of(&p)[2].clone(), b"stale shm").unwrap();
        match CacheDb::open(&p).unwrap() {
            Opened::Ready(db, note) => {
                assert_eq!(note, Some(CLEARED_NOTE));
                assert!(db.list().unwrap().is_empty());
            }
            Opened::Busy => panic!("busy"),
        }
        assert!(!std::fs::read(files_of(&p)[1].clone()).is_ok_and(|b| b == b"stale wal"));
    }

    #[test]
    fn a_busy_file_is_left_alone() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("cache.db");
        drop(open_in(d.path()));
        // Another connection holds the write lock for the whole open.
        let holder = Connection::open(&p).unwrap();
        holder.execute_batch("BEGIN EXCLUSIVE;").unwrap();
        let before = std::fs::read(&p).unwrap();
        match CacheDb::try_open(&p, &[]) {
            Err(CacheError::Sql(e)) => assert!(busy(&e), "{e}"),
            Err(e) => panic!("{e}"),
            Ok(_) => panic!("opened a locked file"),
        }
        assert!(matches!(CacheDb::open(&p).unwrap(), Opened::Busy));
        holder.execute_batch("ROLLBACK;").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), before, "not cleared");
    }

    #[test]
    fn one_damaged_row_is_dropped_alone() {
        let d = tempfile::tempdir().unwrap();
        let mut db = open_in(d.path());
        let a = db
            .save_disk(&params("volume:/a"), &tree("a", 5), 1, CacheLimits::default(), &Roomy)
            .unwrap();
        let b = db
            .save_disk(&params("volume:/b"), &tree("b", 5), 2, CacheLimits::default(), &Roomy)
            .unwrap();
        db.conn
            .execute("UPDATE disk_chunks SET data = X'0155' WHERE saved = ?1", [a.id])
            .unwrap();
        assert!(matches!(load_disk(db.path(), a.id), Err(CacheError::Damaged(_))));
        assert!(db.forget(a.id).unwrap());
        assert!(load_disk(db.path(), b.id).is_ok());
        assert_eq!(db.list().unwrap().len(), 1);
    }

    #[test]
    fn a_failed_save_keeps_the_old_map() {
        let d = tempfile::tempdir().unwrap();
        let mut db = open_in(d.path());
        let old = tree("old", 20);
        let first = db.save_disk(&params("volume:/"), &old, 1, CacheLimits::default(), &Roomy).unwrap();
        db.fail_saves_after_chunks(Some(1));
        let e = db
            .save_disk(&params("volume:/"), &tree("new", 30), 2, CacheLimits::default(), &Roomy)
            .unwrap_err();
        assert!(matches!(e, CacheError::Injected));
        let rows = db.list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, first.id, "the old row is still the one");
        assert_eq!(load_disk(db.path(), first.id).unwrap().to_records(), old.to_records());
    }

    #[test]
    fn eviction_follows_last_shown_and_the_target_count() {
        let d = tempfile::tempdir().unwrap();
        let mut db = open_in(d.path());
        let mut ids = Vec::new();
        for n in 0..16 {
            let s = db
                .save_disk(
                    &params(&format!("folder:/{n}")),
                    &tree("t", 2),
                    100 + n,
                    CacheLimits::default(),
                    &Roomy,
                )
                .unwrap();
            ids.push(s.id);
        }
        assert_eq!(db.list().unwrap().len(), 16);
        // Map 0 is shown again later than all the others: map 1 is the least recently shown.
        db.touch(ids[0], 9_000).unwrap();
        let s = db
            .save_disk(&params("folder:/new"), &tree("t", 2), 10_000, CacheLimits::default(), &Roomy)
            .unwrap();
        assert_eq!(s.removed, vec![ids[1]]);
        let rows = db.list().unwrap();
        assert_eq!(rows.len(), 16);
        assert!(rows.iter().any(|r| r.id == ids[0]));
        assert!(!rows.iter().any(|r| r.id == ids[1]));
    }

    #[test]
    fn eviction_follows_the_size_cap_and_a_map_over_the_cap_is_not_saved() {
        let d = tempfile::tempdir().unwrap();
        let mut db = open_in(d.path());
        let raw = CacheLimits {
            max_bytes: 1 << 30,
            compress: false,
        };
        let one = db.save_disk(&params("folder:/1"), &tree("t", 400), 1, raw, &Roomy).unwrap();
        db.save_disk(&params("folder:/2"), &tree("t", 400), 2, raw, &Roomy).unwrap();
        // The cap holds two of these, not three.
        let cap = CacheLimits {
            max_bytes: one.packed_bytes * 2 + 100,
            compress: false,
        };
        let third = db.save_disk(&params("folder:/3"), &tree("t", 400), 3, cap, &Roomy).unwrap();
        assert_eq!(third.removed.len(), 1);
        assert_eq!(db.list().unwrap().len(), 2);
        // A map larger than the cap alone is refused, and nothing is lost for it.
        let tiny = CacheLimits {
            max_bytes: 100,
            compress: false,
        };
        assert!(matches!(
            db.save_disk(&params("folder:/4"), &tree("t", 400), 4, tiny, &Roomy),
            Err(CacheError::TooBig)
        ));
        assert_eq!(db.list().unwrap().len(), 2);
    }

    #[test]
    fn no_save_under_the_free_space_floor() {
        struct Tight(u64);
        impl FreeSpace for Tight {
            fn volume_of(&self, _: &Path) -> Option<(u64, u64)> {
                Some((100 * 1024 * 1024 * 1024, self.0))
            }
        }
        let d = tempfile::tempdir().unwrap();
        let mut db = open_in(d.path());
        let t = tree("t", 10);
        let gb = 1024 * 1024 * 1024;
        assert!(matches!(
            db.save_disk(&params("v:/"), &t, 1, CacheLimits::default(), &Tight(gb)),
            Err(CacheError::NoSpace)
        ));
        // 5 % of 100 GB is the floor: 4.9 GB free is too little, 5.1 GB is enough.
        assert!(matches!(
            db.save_disk(&params("v:/"), &t, 1, CacheLimits::default(), &Tight(49 * gb / 10)),
            Err(CacheError::NoSpace)
        ));
        assert!(db
            .save_disk(&params("v:/"), &t, 1, CacheLimits::default(), &Tight(51 * gb / 10))
            .is_ok());
    }

    #[test]
    fn chunks_are_bounded_when_they_inflate() {
        let raw = vec![0u8; 3 * CHUNK];
        let packed = encode_chunk(&raw[..CHUNK], true);
        assert!(packed.len() < 100_000, "zeros pack very well: no ratio limit");
        assert_eq!(decode_chunk(&packed, CHUNK).unwrap().len(), CHUNK);
        let bomb = encode_chunk(&raw, true);
        assert!(
            matches!(decode_chunk(&bomb, CHUNK), Err(CacheError::Damaged(_))),
            "inflates past the chunk size"
        );
        assert!(matches!(decode_chunk(&[], CHUNK), Err(CacheError::Damaged(_))));
        assert!(matches!(decode_chunk(&[7, 1], CHUNK), Err(CacheError::Damaged(_))));
        assert!(
            matches!(decode_chunk(&[1, 255, 255, 255], CHUNK), Err(CacheError::Damaged(_))),
            "not deflate"
        );
        let mut over = vec![TAG_RAW];
        over.extend(vec![1u8; 11]);
        assert!(matches!(decode_chunk(&over, 10), Err(CacheError::Damaged(_))));
        assert_eq!(decode_chunk(&encode_chunk(b"abc", false), 10).unwrap(), b"abc");
    }

    #[test]
    fn a_stream_that_inflates_past_the_rows_stored_bytes_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let mut db = open_in(d.path());
        let s = db
            .save_disk(&params("v:/"), &tree("t", 30), 1, CacheLimits::default(), &Roomy)
            .unwrap();
        db.conn
            .execute("UPDATE disk_saved SET stored_bytes = stored_bytes - 10 WHERE id = ?1", [s.id])
            .unwrap();
        assert!(matches!(load_disk(db.path(), s.id), Err(CacheError::Damaged(_))));
        db.conn
            .execute(
                "UPDATE disk_saved SET stored_bytes = stored_bytes + 10, nodes = nodes + 1 WHERE id = ?1",
                [s.id],
            )
            .unwrap();
        assert!(
            matches!(load_disk(db.path(), s.id), Err(CacheError::Damaged(_))),
            "header count larger than the row's nodes"
        );
    }

    #[test]
    fn a_missing_chunk_is_damage() {
        let d = tempfile::tempdir().unwrap();
        let mut db = open_in(d.path());
        // Many small chunks cannot be made cheaply; delete the only one and the load is cut short.
        let s = db
            .save_disk(&params("v:/"), &tree("t", 30), 1, CacheLimits::default(), &Roomy)
            .unwrap();
        db.conn.execute("DELETE FROM disk_chunks WHERE saved = ?1", [s.id]).unwrap();
        assert!(matches!(load_disk(db.path(), s.id), Err(CacheError::Damaged(_))));
        assert!(matches!(load_disk(db.path(), 9999), Err(CacheError::Damaged(_))));
    }

    #[test]
    fn views_are_kept_and_replaced() {
        let d = tempfile::tempdir().unwrap();
        let db = open_in(d.path());
        db.put_view("counters", 1, 10, "{\"a\":1}").unwrap();
        db.put_view("counters", 1, 20, "{\"a\":2}").unwrap();
        assert_eq!(get_view(db.path(), "counters").unwrap(), Some((1, 20, "{\"a\":2}".into())));
        assert_eq!(get_view(db.path(), "software").unwrap(), None);
    }

    /// Everything on disk that could hold a name: the database and its write-ahead log.
    fn on_disk(db: &Path) -> Vec<u8> {
        let [main, wal, _] = files_of(db);
        let mut all = std::fs::read(main).unwrap_or_default();
        all.extend(std::fs::read(wal).unwrap_or_default());
        all
    }

    fn holds(bytes: &[u8], needle: &str) -> bool {
        bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
    }

    fn private_params(tag: &str) -> SaveParams {
        let path = format!("/Users/alice/__PRIVATE_{tag}__");
        SaveParams {
            target_key: format!("folder:{path}"),
            target: format!("{{\"folder\":{{\"path\":\"{path}\"}}}}"),
            volume_identity: None,
            status: format!("{{\"name\":\"__PRIVATE_{tag}__\"}}"),
            limits: "{}".into(),
            scanned_at: 1,
            used_bytes_then: None,
        }
    }

    /// ADR 0029 section 6: forgetting leaves no path in freed pages. The chunks are stored raw here so the names are there to
    /// find (compressed, they would not be readable even before); `secure_delete` overwrites freed pages and the write-ahead log
    /// is emptied after a forget, a replacement and an eviction.
    #[test]
    fn a_forgotten_replaced_or_evicted_map_leaves_no_name_in_the_file_or_its_wal() {
        let d = tempfile::tempdir().unwrap();
        let mut db = open_in(d.path());
        let raw = CacheLimits {
            max_bytes: 1 << 30,
            compress: false,
        };

        // Forgotten.
        let a = db
            .save_disk(&private_params("alpha"), &tree("__PRIVATE_alpha__", 30), 1, raw, &Roomy)
            .unwrap();
        assert!(
            holds(&on_disk(db.path()), "__PRIVATE_alpha__"),
            "the names are in the file before (the check is not vacuous)"
        );
        assert!(
            holds(&on_disk(db.path()), "/Users/alice/__PRIVATE_alpha__"),
            "and the target's path in its row"
        );
        db.forget(a.id).unwrap();
        assert!(!holds(&on_disk(db.path()), "__PRIVATE_alpha__"), "forgotten: no name, no path");

        // Replaced by a newer map of the same target.
        db.save_disk(&private_params("beta"), &tree("__PRIVATE_beta_old__", 30), 2, raw, &Roomy)
            .unwrap();
        assert!(holds(&on_disk(db.path()), "__PRIVATE_beta_old__"));
        let replaced = db
            .save_disk(&private_params("beta"), &tree("__PRIVATE_beta_new__", 30), 3, raw, &Roomy)
            .unwrap();
        assert_eq!(replaced.removed.len(), 1);
        let after = on_disk(db.path());
        assert!(!holds(&after, "__PRIVATE_beta_old__"), "replaced: the old names are gone");
        assert!(holds(&after, "__PRIVATE_beta_new__"), "the new map is there");

        // Evicted to make room for another.
        let one = db
            .save_disk(&private_params("gamma"), &tree("__PRIVATE_gamma__", 30), 4, raw, &Roomy)
            .unwrap();
        let cap = CacheLimits {
            max_bytes: one.packed_bytes * 2 + 64,
            compress: false,
        };
        db.save_disk(&private_params("delta"), &tree("__PRIVATE_delta__", 30), 5, cap, &Roomy)
            .unwrap();
        let after = on_disk(db.path());
        assert!(!holds(&after, "__PRIVATE_beta_new__"), "evicted (least recently shown): no name");
        assert!(
            holds(&after, "__PRIVATE_delta__") && holds(&after, "__PRIVATE_gamma__"),
            "the ones kept are there"
        );
    }

    #[test]
    fn the_write_connection_overwrites_freed_pages_and_the_file_gives_the_space_back() {
        let d = tempfile::tempdir().unwrap();
        let mut db = open_in(d.path());
        let secure: i64 = db.conn.query_row("PRAGMA secure_delete", [], |r| r.get(0)).unwrap();
        assert_eq!(secure, 1, "secure_delete is on");
        let vacuum: i64 = db.conn.query_row("PRAGMA auto_vacuum", [], |r| r.get(0)).unwrap();
        assert_eq!(vacuum, 2, "auto_vacuum is incremental, set when the file was created");
        let raw = CacheLimits {
            max_bytes: 1 << 30,
            compress: false,
        };
        let s = db
            .save_disk(&private_params("big"), &tree("__PRIVATE_big__", 3000), 1, raw, &Roomy)
            .unwrap();
        // The pages are in the file, not still in the log, before it is measured.
        db.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
        let grown = std::fs::metadata(db.path()).unwrap().len();
        db.forget(s.id).unwrap();
        let shrunk = std::fs::metadata(db.path()).unwrap().len();
        assert!(shrunk < grown / 2, "{shrunk} bytes after forgetting against {grown} before");
        let wal = std::fs::metadata(&files_of(db.path())[1]).map_or(0, |m| m.len());
        assert_eq!(wal, 0, "the write-ahead log is empty after a forget");
    }

    /// A file that cannot be deleted (Windows, another handle open) is emptied in place, and no name is left in it.
    #[test]
    fn a_database_that_cannot_be_deleted_is_emptied_in_place_without_leaving_a_name() {
        let d = tempfile::tempdir().unwrap();
        let mut db = open_in(d.path());
        let raw = CacheLimits {
            max_bytes: 1 << 30,
            compress: false,
        };
        db.save_disk(&private_params("kept"), &tree("__PRIVATE_kept__", 30), 1, raw, &Roomy)
            .unwrap();
        let path = db.path().to_path_buf();
        drop(db);
        assert!(holds(&on_disk(&path), "__PRIVATE_kept__"));
        wipe_in_place(&path).unwrap();
        assert!(!holds(&on_disk(&path), "__PRIVATE_kept__"), "no name after the wipe");
        let db = open_in(d.path());
        assert!(db.list().unwrap().is_empty(), "and it opens as an empty cache");
        // A file that is not a database cannot be emptied, and says so.
        std::fs::write(d.path().join("junk.db"), vec![7u8; 3000]).unwrap();
        assert!(wipe_in_place(&d.path().join("junk.db")).is_err());
        // Deleting reports a failure instead of hiding it: a folder where a file should be cannot be removed as a file.
        let dir_as_db = d.path().join("x.db");
        std::fs::create_dir(&dir_as_db).unwrap();
        assert!(delete_files(&dir_as_db).is_err());
        assert!(
            delete_files(&d.path().join("never-existed.db")).is_ok(),
            "nothing there is not a failure"
        );
    }
}
