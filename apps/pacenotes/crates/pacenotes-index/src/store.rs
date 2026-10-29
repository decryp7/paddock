//! The SQLite store, schema v1 (ADR 0002, ADR 0005): the rebuild rule, the transaction that swaps a generation in, the read side
//! of the core `Store` port, and search.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use pacenotes_core::ports::{Store, StoreError, SymbolPage, SymbolRecord};
use pacenotes_core::{Confidence, FileId, Fingerprint, Generation, Language, Position, RepoPath, Span, SymbolId, SymbolKind};
use paddock_jobs::CancelToken;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::facts::FileFacts;
use crate::indexer::IndexError;
use crate::plan::Known;

/// The schema version this build reads and writes.
pub const SCHEMA_VERSION: i64 = 1;
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

const SCHEMA: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE files (
    file_id BLOB PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    fingerprint TEXT NOT NULL,
    language TEXT,
    size INTEGER NOT NULL,
    generation INTEGER NOT NULL,
    partial INTEGER NOT NULL DEFAULT 0,
    has_errors INTEGER NOT NULL DEFAULT 0,
    module_path TEXT,
    modified_ns INTEGER,
    read_at_ns INTEGER
);
CREATE TABLE quarantine (
    path TEXT PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    reason TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE symbols (
    symbol_id BLOB PRIMARY KEY,
    file_id BLOB NOT NULL REFERENCES files(file_id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    start_line INTEGER NOT NULL, start_col INTEGER NOT NULL, end_line INTEGER NOT NULL, end_col INTEGER NOT NULL,
    depth INTEGER NOT NULL
);
CREATE INDEX symbols_by_file ON symbols(file_id, start_line, start_col);
CREATE INDEX symbols_by_name ON symbols(name);
CREATE TABLE imports (
    file_id BLOB NOT NULL REFERENCES files(file_id) ON DELETE CASCADE,
    target TEXT NOT NULL, start_line INTEGER NOT NULL, start_col INTEGER NOT NULL, end_line INTEGER NOT NULL, end_col INTEGER NOT NULL
);
CREATE INDEX imports_by_file ON imports(file_id);
CREATE TABLE calls (
    file_id BLOB NOT NULL REFERENCES files(file_id) ON DELETE CASCADE,
    caller TEXT, callee TEXT NOT NULL, start_line INTEGER NOT NULL, start_col INTEGER NOT NULL, end_line INTEGER NOT NULL, end_col INTEGER NOT NULL
);
CREATE INDEX calls_by_file ON calls(file_id);
CREATE TABLE chunks (
    chunk_id BLOB PRIMARY KEY,
    file_id BLOB NOT NULL REFERENCES files(file_id) ON DELETE CASCADE,
    symbol TEXT, ordinal INTEGER NOT NULL,
    start_line INTEGER NOT NULL, end_line INTEGER NOT NULL
);
CREATE INDEX chunks_by_file ON chunks(file_id, start_line);
";

/// Why an existing store was not trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebuildReason {
    /// The file is not a SQLite database (its header is wrong).
    NotADatabase,
    /// SQLite says the file is damaged (or could not open it).
    Damaged(String),
    /// The schema version is missing (`None`) or is not this build's (`Some(v)`, older or newer).
    SchemaVersion(Option<i64>),
}

impl std::fmt::Display for RebuildReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RebuildReason::NotADatabase => f.write_str("the index file is not a database"),
            RebuildReason::Damaged(why) => write!(f, "the index file is damaged ({why})"),
            RebuildReason::SchemaVersion(None) => f.write_str("the index file has no schema version"),
            RebuildReason::SchemaVersion(Some(v)) => write!(f, "the index file has schema version {v}, this build uses {SCHEMA_VERSION}"),
        }
    }
}

/// A store, and why it is new when an earlier file was not trusted.
pub struct Opened {
    /// The store.
    pub store: IndexStore,
    /// Set when an existing file was moved aside and replaced; the caller tells the user the index is being rebuilt.
    pub rebuilt: Option<RebuildReason>,
}

/// A stored chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkRow {
    /// The qualified name of the symbol it holds.
    pub symbol: Option<String>,
    /// Its place among the chunks of the symbol.
    pub ordinal: u32,
    /// First line (1-based).
    pub first_line: u32,
    /// Last line.
    pub last_line: u32,
}

/// A symbol or file that matches a search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// `"file"` or `"symbol"`.
    pub kind: &'static str,
    /// The path, or the qualified name of the symbol.
    pub text: String,
    /// The file the hit is in.
    pub path: RepoPath,
}

/// One file's change in a [`Batch`].
pub struct Upsert {
    /// The file's id (issued once, kept across renames).
    pub file_id: FileId,
    /// Its path now.
    pub path: RepoPath,
    /// Its content fingerprint.
    pub fingerprint: Fingerprint,
    /// Its size in bytes.
    pub size: u64,
    /// New facts, or `None` for a file that is not parsed (tier 1).
    pub facts: Option<FileFacts>,
    /// The file's modification time when it was read (the shortcut of ADR 0006).
    pub modified_ns: Option<i64>,
    /// When it was read, nanoseconds since the Unix epoch.
    pub read_at_ns: i64,
}

/// Everything one run changes; applied in a single transaction.
#[derive(Default)]
pub struct Batch {
    /// Files that are gone.
    pub deleted: Vec<FileId>,
    /// Files that moved: the id stays, the path changes.
    pub renamed: Vec<(FileId, RepoPath)>,
    /// Files added or modified.
    pub upserts: Vec<Upsert>,
    /// The id counter to store (ids are never reused).
    pub next_file_sequence: u64,
    /// Files the worker crashed or hung on: `(path, fingerprint, reason)`. They are not sent to a worker again until the
    /// fingerprint changes.
    pub quarantined: Vec<(RepoPath, Fingerprint, String)>,
}

/// What the store remembers about reading a file, for the shortcut of ADR 0006.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    /// Size in bytes when it was read.
    pub size: u64,
    /// Its modification time then.
    pub modified_ns: Option<i64>,
    /// When it was read.
    pub read_at_ns: Option<i64>,
}

/// The index database.
pub struct IndexStore {
    conn: Mutex<Connection>,
    path: PathBuf,
}

fn db(e: rusqlite::Error) -> IndexError {
    IndexError::Store(e.to_string())
}

fn read_error(e: rusqlite::Error) -> StoreError {
    StoreError::Backend(e.to_string())
}

fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
    Ok(())
}

fn check_existing(path: &Path) -> Result<Connection, RebuildReason> {
    let mut header = [0u8; 16];
    let mut file = std::fs::File::open(path).map_err(|e| RebuildReason::Damaged(e.to_string()))?;
    let got = file.read(&mut header).map_err(|e| RebuildReason::Damaged(e.to_string()))?;
    if got != 16 || &header != SQLITE_HEADER {
        return if got == 0 {
            Err(RebuildReason::SchemaVersion(None))
        } else {
            Err(RebuildReason::NotADatabase)
        };
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(|e| RebuildReason::Damaged(e.to_string()))?;
    let verdict: String = conn
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(|e| RebuildReason::Damaged(e.to_string()))?;
    if verdict != "ok" {
        return Err(RebuildReason::Damaged(verdict));
    }
    let version: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get(0))
        .optional()
        .map_err(|_| RebuildReason::SchemaVersion(None))?;
    match version.and_then(|v| v.parse::<i64>().ok()) {
        Some(SCHEMA_VERSION) => {
            init(&conn).map_err(|e| RebuildReason::Damaged(e.to_string()))?;
            Ok(conn)
        }
        other => Err(RebuildReason::SchemaVersion(other)),
    }
}

fn move_aside(path: &Path) {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".corrupt");
    let aside = path.with_file_name(name);
    for suffix in ["", "-wal", "-shm"] {
        let mut target = path.as_os_str().to_os_string();
        target.push(suffix);
        let mut dest = aside.as_os_str().to_os_string();
        dest.push(suffix);
        let _ = std::fs::remove_file(&dest);
        if std::fs::rename(&target, &dest).is_err() {
            let _ = std::fs::remove_file(&target);
        }
    }
}

impl IndexStore {
    /// Opens the store at `path`, trusting nothing: a file that is not SQLite, fails SQLite's quick check, has no schema version
    /// or has another version is moved aside to `<name>.corrupt` and a new store is created. The result says whether that
    /// happened and why.
    pub fn open(path: &Path) -> Result<Opened, IndexError> {
        let mut rebuilt = None;
        if path.exists() {
            match check_existing(path) {
                Ok(conn) => {
                    return Ok(Opened {
                        store: IndexStore {
                            conn: Mutex::new(conn),
                            path: path.to_path_buf(),
                        },
                        rebuilt: None,
                    });
                }
                Err(reason) => {
                    move_aside(path);
                    rebuilt = Some(reason);
                }
            }
        }
        let conn = Connection::open(path).map_err(db)?;
        init(&conn).map_err(db)?;
        conn.execute_batch(SCHEMA).map_err(db)?;
        for (key, value) in [
            ("schema_version", SCHEMA_VERSION.to_string()),
            ("generation", "0".to_owned()),
            ("next_file_sequence", "0".to_owned()),
        ] {
            conn.execute("INSERT INTO meta(key, value) VALUES (?1, ?2)", params![key, value])
                .map_err(db)?;
        }
        Ok(Opened {
            store: IndexStore {
                conn: Mutex::new(conn),
                path: path.to_path_buf(),
            },
            rebuilt,
        })
    }

    /// The file this store lives in.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn meta_number(conn: &Connection, key: &str) -> rusqlite::Result<u64> {
        let text: String = conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))?;
        Ok(text.parse().unwrap_or(0))
    }

    /// The id counter to resume from.
    pub fn next_file_sequence(&self) -> Result<u64, IndexError> {
        Self::meta_number(&self.conn(), "next_file_sequence").map_err(db)
    }

    /// Every file the index knows, sorted by path.
    pub fn known_files(&self) -> Result<Vec<Known>, IndexError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare("SELECT file_id, path, fingerprint FROM files ORDER BY path")
            .map_err(db)?;
        let rows = statement
            .query_map([], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })
            .map_err(db)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, path, fingerprint) = row.map_err(db)?;
            let bytes: [u8; 16] = id
                .try_into()
                .map_err(|_| IndexError::Store("a file id is not 16 bytes".to_owned()))?;
            out.push(Known {
                file_id: FileId::from_bytes(bytes),
                path: RepoPath::parse(&path).map_err(|e| IndexError::Store(e.to_string()))?,
                fingerprint: Fingerprint::from_hex(&fingerprint)
                    .ok_or_else(|| IndexError::Store("a fingerprint is not 64 hex digits".to_owned()))?,
            });
        }
        Ok(out)
    }

    /// The stamp of every file, by path.
    pub fn stamps(&self) -> Result<std::collections::BTreeMap<RepoPath, Stamp>, IndexError> {
        let conn = self.conn();
        let mut statement = conn.prepare("SELECT path, size, modified_ns, read_at_ns FROM files").map_err(db)?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            })
            .map_err(db)?;
        let mut out = std::collections::BTreeMap::new();
        for row in rows {
            let (path, size, modified_ns, read_at_ns) = row.map_err(db)?;
            let path = RepoPath::parse(&path).map_err(|e| IndexError::Store(e.to_string()))?;
            out.insert(
                path,
                Stamp {
                    size: u64::try_from(size).unwrap_or(0),
                    modified_ns,
                    read_at_ns,
                },
            );
        }
        Ok(out)
    }

    /// Files a worker crashed or hung on, with the fingerprint they had then.
    pub fn quarantined(&self) -> Result<std::collections::BTreeMap<RepoPath, (Fingerprint, String)>, IndexError> {
        let conn = self.conn();
        let mut statement = conn.prepare("SELECT path, fingerprint, reason FROM quarantine").map_err(db)?;
        let rows = statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
            .map_err(db)?;
        let mut out = std::collections::BTreeMap::new();
        for row in rows {
            let (path, fingerprint, reason) = row.map_err(db)?;
            let path = RepoPath::parse(&path).map_err(|e| IndexError::Store(e.to_string()))?;
            let fingerprint =
                Fingerprint::from_hex(&fingerprint).ok_or_else(|| IndexError::Store("a fingerprint is not 64 hex digits".to_owned()))?;
            out.insert(path, (fingerprint, reason));
        }
        Ok(out)
    }

    /// Refreshes the stamp of files that were read again and found unchanged, so the next run can skip them. Writes no
    /// generation: nothing the index serves changed.
    pub fn restamp(&mut self, stamps: &[(FileId, Option<i64>, i64)]) -> Result<(), IndexError> {
        let mut guard = self.conn();
        let tx = guard.transaction().map_err(db)?;
        for (id, modified_ns, read_at_ns) in stamps {
            tx.execute(
                "UPDATE files SET modified_ns = ?1, read_at_ns = ?2 WHERE file_id = ?3",
                params![modified_ns, read_at_ns, &id.to_bytes()[..]],
            )
            .map_err(db)?;
        }
        tx.commit().map_err(db)
    }

    /// Applies a run's changes in one transaction and returns the new generation. A failure leaves the previous generation as it
    /// was.
    pub fn commit(&mut self, batch: &Batch) -> Result<Generation, IndexError> {
        let mut guard = self.conn();
        let tx = guard.transaction().map_err(db)?;
        let generation = Self::meta_number(&tx, "generation").map_err(db)? + 1;
        for id in &batch.deleted {
            tx.execute("DELETE FROM files WHERE file_id = ?1", [&id.to_bytes()[..]])
                .map_err(db)?;
        }
        for (id, path) in &batch.renamed {
            tx.execute(
                "UPDATE files SET path = ?1, generation = ?2 WHERE file_id = ?3",
                params![path.as_str(), generation as i64, &id.to_bytes()[..]],
            )
            .map_err(db)?;
        }
        for up in &batch.upserts {
            Self::write_file(&tx, up, generation)?;
        }
        tx.execute("DELETE FROM quarantine WHERE path NOT IN (SELECT path FROM files)", [])
            .map_err(db)?;
        for (path, fingerprint, reason) in &batch.quarantined {
            tx.execute(
                "INSERT OR REPLACE INTO quarantine(path, fingerprint, reason) VALUES (?1, ?2, ?3)",
                params![path.as_str(), fingerprint.to_hex(), reason],
            )
            .map_err(db)?;
        }
        tx.execute("UPDATE meta SET value = ?1 WHERE key = 'generation'", [generation.to_string()])
            .map_err(db)?;
        tx.execute(
            "UPDATE meta SET value = ?1 WHERE key = 'next_file_sequence'",
            [batch.next_file_sequence.to_string()],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(Generation(generation))
    }

    fn write_file(tx: &rusqlite::Transaction<'_>, up: &Upsert, generation: u64) -> Result<(), IndexError> {
        let id = up.file_id.to_bytes();
        let facts = up.facts.as_ref();
        tx.execute("DELETE FROM quarantine WHERE path = ?1", [up.path.as_str()])
            .map_err(db)?;
        tx.execute("DELETE FROM symbols WHERE file_id = ?1", [&id[..]]).map_err(db)?;
        tx.execute("DELETE FROM imports WHERE file_id = ?1", [&id[..]]).map_err(db)?;
        tx.execute("DELETE FROM calls WHERE file_id = ?1", [&id[..]]).map_err(db)?;
        tx.execute("DELETE FROM chunks WHERE file_id = ?1", [&id[..]]).map_err(db)?;
        tx.execute(
            "INSERT INTO files(file_id, path, fingerprint, language, size, generation, partial, has_errors, modified_ns, read_at_ns) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(file_id) DO UPDATE SET path = excluded.path, fingerprint = excluded.fingerprint, language = excluded.language,
                 size = excluded.size, generation = excluded.generation, partial = excluded.partial, has_errors = excluded.has_errors,
                 modified_ns = excluded.modified_ns, read_at_ns = excluded.read_at_ns",
            params![
                &id[..],
                up.path.as_str(),
                up.fingerprint.to_hex(),
                facts.map(|f| f.language.id().to_owned()),
                up.size as i64,
                generation as i64,
                facts.is_some_and(|f| f.partial),
                facts.is_some_and(|f| f.has_errors),
                up.modified_ns,
                up.read_at_ns
            ],
        )
        .map_err(db)?;
        let Some(facts) = facts else { return Ok(()) };
        let span = |s: &Span| (s.start().line(), s.start().column(), s.end().line(), s.end().column());
        let mut seen: HashMap<(SymbolKind, &str), u32> = HashMap::new();
        let mut first_id: HashMap<&str, Vec<SymbolId>> = HashMap::new();
        for s in &facts.symbols {
            let n = seen.entry((s.kind, s.qualified_name.as_str())).or_insert(0);
            let symbol = SymbolId::derive(up.file_id, &facts.language, s.kind, &s.qualified_name, *n);
            *n += 1;
            first_id.entry(s.qualified_name.as_str()).or_default().push(symbol);
            let (a, b, c, d) = span(&s.span);
            tx.execute(
                "INSERT INTO symbols(symbol_id, file_id, kind, name, qualified_name, start_line, start_col, end_line, end_col, depth) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![&symbol.to_bytes()[..], &id[..], s.kind.name(), s.name, s.qualified_name, a, b, c, d, s.depth],
            )
            .map_err(db)?;
        }
        for i in &facts.imports {
            let (a, b, c, d) = span(&i.span);
            tx.execute(
                "INSERT INTO imports(file_id, target, start_line, start_col, end_line, end_col) VALUES (?1,?2,?3,?4,?5,?6)",
                params![&id[..], i.target, a, b, c, d],
            )
            .map_err(db)?;
        }
        for call in &facts.calls {
            let (a, b, c, d) = span(&call.span);
            tx.execute(
                "INSERT INTO calls(file_id, caller, callee, start_line, start_col, end_line, end_col) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![&id[..], call.caller, call.callee, a, b, c, d],
            )
            .map_err(db)?;
        }
        let mut duplicates: HashMap<(Option<&str>, u32), u32> = HashMap::new();
        for chunk in &facts.chunks {
            let key = (chunk.symbol.as_deref(), chunk.ordinal);
            let dup = *duplicates.entry(key).and_modify(|d| *d += 1).or_insert(0);
            let within = chunk
                .symbol
                .as_deref()
                .and_then(|q| first_id.get(q))
                .and_then(|ids| ids.get(dup as usize % ids.len().max(1)).copied());
            let chunk_id = pacenotes_core::ChunkId::derive(up.file_id, within, chunk.ordinal.saturating_add(dup.saturating_mul(1 << 16)));
            let (a, _, c, d) = span(&chunk.span);
            let last = if d == 1 && c > a { c - 1 } else { c };
            tx.execute(
                "INSERT INTO chunks(chunk_id, file_id, symbol, ordinal, start_line, end_line) VALUES (?1,?2,?3,?4,?5,?6)",
                params![&chunk_id.to_bytes()[..], &id[..], chunk.symbol, chunk.ordinal, a, last],
            )
            .map_err(db)?;
        }
        Ok(())
    }

    /// The chunks of a file, in file order.
    pub fn chunks_of(&self, path: &RepoPath) -> Result<Vec<ChunkRow>, IndexError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare("SELECT c.symbol, c.ordinal, c.start_line, c.end_line FROM chunks c JOIN files f ON f.file_id = c.file_id WHERE f.path = ?1 ORDER BY c.start_line, c.ordinal")
            .map_err(db)?;
        let rows = statement
            .query_map([path.as_str()], |r| {
                Ok(ChunkRow {
                    symbol: r.get(0)?,
                    ordinal: r.get(1)?,
                    first_line: r.get(2)?,
                    last_line: r.get(3)?,
                })
            })
            .map_err(db)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db)
    }

    /// Files whose path, and symbols whose name or qualified name, contain `query` (case-insensitive), at most `limit`, files
    /// first. This is the store's own search API (the trap test runs its queries through it).
    pub fn search(&self, query: &str, limit: usize, cancel: &CancelToken) -> Result<Vec<SearchHit>, IndexError> {
        if cancel.is_cancelled() {
            return Err(IndexError::Cancelled);
        }
        let pattern = format!(
            "%{}%",
            query.to_lowercase().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
        );
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT 'file', path, path FROM files WHERE lower(path) LIKE ?1 ESCAPE '\\'
                 UNION ALL
                 SELECT 'symbol', s.qualified_name, f.path FROM symbols s JOIN files f ON f.file_id = s.file_id
                   WHERE lower(s.name) LIKE ?1 ESCAPE '\\' OR lower(s.qualified_name) LIKE ?1 ESCAPE '\\'
                 LIMIT ?2",
            )
            .map_err(db)?;
        let rows = statement
            .query_map(params![pattern, limit as i64], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })
            .map_err(db)?;
        let mut out = Vec::new();
        for row in rows {
            let (kind, text, path) = row.map_err(db)?;
            out.push(SearchHit {
                kind: if kind == "file" { "file" } else { "symbol" },
                text,
                path: RepoPath::parse(&path).map_err(|e| IndexError::Store(e.to_string()))?,
            });
        }
        Ok(out)
    }

    /// A dump of every text column of every table, for diagnostics and for the trap test's leak check ("in no table"). Not bounded:
    /// the store holds names and spans, never file content.
    pub fn dump_text(&self) -> Result<String, IndexError> {
        let conn = self.conn();
        let mut out = String::new();
        for (table, columns) in [
            ("meta", "key || ' ' || value"),
            (
                "files",
                "path || ' ' || fingerprint || ' ' || coalesce(language, '') || ' ' || coalesce(module_path, '')",
            ),
            ("symbols", "kind || ' ' || name || ' ' || qualified_name"),
            ("imports", "target"),
            ("calls", "coalesce(caller, '') || ' ' || callee"),
            ("chunks", "coalesce(symbol, '')"),
            ("quarantine", "path || ' ' || reason"),
        ] {
            let mut statement = conn.prepare(&format!("SELECT {columns} FROM {table}")).map_err(db)?;
            for row in statement.query_map([], |r| r.get::<_, String>(0)).map_err(db)? {
                out.push_str(&row.map_err(db)?);
                out.push('\n');
            }
        }
        Ok(out)
    }

    /// Row counts: `(files, symbols)`.
    pub fn counts(&self) -> Result<(u64, u64), IndexError> {
        let conn = self.conn();
        let files: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).map_err(db)?;
        let symbols: i64 = conn.query_row("SELECT count(*) FROM symbols", [], |r| r.get(0)).map_err(db)?;
        Ok((files as u64, symbols as u64))
    }

    /// The language stored for a path.
    pub fn language_of(&self, path: &RepoPath) -> Result<Option<Language>, IndexError> {
        let conn = self.conn();
        let text: Option<Option<String>> = conn
            .query_row("SELECT language FROM files WHERE path = ?1", [path.as_str()], |r| r.get(0))
            .optional()
            .map_err(db)?;
        Ok(text.flatten().and_then(|t: String| Language::parse(t.as_str()).ok()))
    }

    /// The symbols of a file with their ids, in source order (for tests and diagnostics).
    pub fn symbol_ids(&self, path: &RepoPath) -> Result<Vec<(String, SymbolId)>, IndexError> {
        let conn = self.conn();
        let mut statement = conn.prepare("SELECT s.qualified_name, s.symbol_id FROM symbols s JOIN files f ON f.file_id = s.file_id WHERE f.path = ?1 ORDER BY s.start_line, s.start_col, s.qualified_name").map_err(db)?;
        let rows = statement
            .query_map([path.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)))
            .map_err(db)?;
        let mut out = Vec::new();
        for row in rows {
            let (name, id) = row.map_err(db)?;
            let bytes: [u8; 16] = id
                .try_into()
                .map_err(|_| IndexError::Store("a symbol id is not 16 bytes".to_owned()))?;
            out.push((name, SymbolId::from_bytes(bytes)));
        }
        Ok(out)
    }
}

impl Store for IndexStore {
    fn generation(&self, cancel: &CancelToken) -> Result<Generation, StoreError> {
        if cancel.is_cancelled() {
            return Err(StoreError::Cancelled);
        }
        Self::meta_number(&self.conn(), "generation").map(Generation).map_err(read_error)
    }

    fn file_id(&self, path: &RepoPath, cancel: &CancelToken) -> Result<Option<FileId>, StoreError> {
        if cancel.is_cancelled() {
            return Err(StoreError::Cancelled);
        }
        let conn = self.conn();
        let id: Option<Vec<u8>> = conn
            .query_row("SELECT file_id FROM files WHERE path = ?1", [path.as_str()], |r| r.get(0))
            .optional()
            .map_err(read_error)?;
        id.map(|bytes| {
            <[u8; 16]>::try_from(bytes)
                .map(FileId::from_bytes)
                .map_err(|_| StoreError::Backend("a file id is not 16 bytes".to_owned()))
        })
        .transpose()
    }

    fn symbols_in_file(&self, file: FileId, limit: usize, cancel: &CancelToken) -> Result<SymbolPage, StoreError> {
        if cancel.is_cancelled() {
            return Err(StoreError::Cancelled);
        }
        let conn = self.conn();
        let mut statement = conn
            .prepare("SELECT s.symbol_id, s.kind, s.qualified_name, s.start_line, s.start_col, s.end_line, s.end_col, f.has_errors FROM symbols s JOIN files f ON f.file_id = s.file_id WHERE s.file_id = ?1 ORDER BY s.start_line, s.start_col, s.qualified_name LIMIT ?2")
            .map_err(read_error)?;
        let rows = statement
            .query_map(params![&file.to_bytes()[..], limit as i64 + 1], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, u32>(3)?,
                    r.get::<_, u32>(4)?,
                    r.get::<_, u32>(5)?,
                    r.get::<_, u32>(6)?,
                    r.get::<_, bool>(7)?,
                ))
            })
            .map_err(read_error)?;
        let mut symbols = Vec::new();
        for row in rows {
            let (id, kind, qualified_name, a, b, c, d, has_errors) = row.map_err(read_error)?;
            let corrupt = |what: &str| StoreError::Backend(format!("a symbol row has a bad {what}"));
            let span = Position::new(a, b)
                .zip(Position::new(c, d))
                .and_then(|(s, e)| Span::new(s, e).ok())
                .ok_or_else(|| corrupt("span"))?;
            symbols.push(SymbolRecord {
                id: SymbolId::from_bytes(<[u8; 16]>::try_from(id).map_err(|_| corrupt("id"))?),
                file,
                kind: SymbolKind::from_name(&kind).ok_or_else(|| corrupt("kind"))?,
                qualified_name,
                span,
                confidence: if has_errors { Confidence::Heuristic } else { Confidence::Resolved },
            });
        }
        let truncated = symbols.len() > limit;
        symbols.truncate(limit);
        SymbolPage::new(symbols, truncated).map_err(|e| StoreError::Backend(e.to_string()))
    }
}
