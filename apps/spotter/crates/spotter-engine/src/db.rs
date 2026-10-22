//! The SQLite adapter for Spotter's state and history: per-watch state, the event feed, daily check counts,
//! downloads, the HTTP validator cache and the digest queue. Versioned (`PRAGMA user_version` + migrations), in
//! WAL mode, every change in a transaction, and tolerant of damage: an unreadable database is moved aside as
//! `.corrupt` and a fresh one is started with a warning; if even that is impossible the engine runs in memory.
//!
//! Core types ([`WatchState`], [`Event`]) are stored as JSON columns, so the core owns their shape and tolerance.
use crate::error::{EngineError, Result};
use crate::fetch::Validators;
use rusqlite::{params, Connection, OptionalExtension};
use spotter_core::journal::{DayCounts, DownloadRecord, Event, Journal, DAY_MS, MAX_DOWNLOADS, MAX_EVENTS, RETENTION_DAYS};
use spotter_core::state::WatchState;
use spotter_core::time::Timestamp;
use spotter_core::watch::WatchId;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// The schema this build writes.
pub const SCHEMA: u32 = 1;

/// Migrations: entry `n` upgrades the schema from `n` to `n + 1`.
const MIGRATIONS: [&str; 1] = ["
CREATE TABLE watch_state (id TEXT PRIMARY KEY, json TEXT NOT NULL);
CREATE TABLE events (id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, watch TEXT, json TEXT NOT NULL);
CREATE INDEX events_at ON events(at);
CREATE TABLE check_days (watch TEXT NOT NULL, day INTEGER NOT NULL, ok INTEGER NOT NULL, failed INTEGER NOT NULL, PRIMARY KEY (watch, day));
CREATE TABLE downloads (id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, watch TEXT NOT NULL, json TEXT NOT NULL);
CREATE TABLE http_cache (watch TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, etag TEXT, last_modified TEXT);
CREATE TABLE digest_queue (id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, watch TEXT NOT NULL, json TEXT NOT NULL);
CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"];

/// The history database.
pub struct Db {
    conn: Mutex<Connection>,
    path: Option<PathBuf>,
}

fn internal(e: rusqlite::Error) -> EngineError {
    EngineError::Internal(format!(
        "Spotter's history database failed ({e}). Restart Spotter; if it keeps happening, delete spotter.db."
    ))
}

fn configure(conn: &Connection) -> rusqlite::Result<()> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

enum OpenFailure {
    Damaged(String),
    Newer(u32),
}

fn migrate(conn: &Connection) -> std::result::Result<(), OpenFailure> {
    let damaged = |e: rusqlite::Error| OpenFailure::Damaged(e.to_string());
    configure(conn).map_err(damaged)?;
    let version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(damaged)?;
    if version > SCHEMA {
        return Err(OpenFailure::Newer(version));
    }
    for (n, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = || -> rusqlite::Result<()> { conn.execute_batch(&format!("BEGIN;{sql}PRAGMA user_version = {};COMMIT;", n + 1)) };
        if let Err(e) = tx() {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(damaged(e));
        }
    }
    let check: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(damaged)?;
    if check != "ok" {
        return Err(OpenFailure::Damaged(check));
    }
    Ok(())
}

fn open_file(path: &Path) -> std::result::Result<Connection, OpenFailure> {
    let conn = Connection::open(path).map_err(|e| OpenFailure::Damaged(e.to_string()))?;
    migrate(&conn)?;
    Ok(conn)
}

/// Moves a damaged database (and its WAL files) aside; the name is `<file>.corrupt`, then `.corrupt-2`, ...
fn move_aside(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let dir = path.parent()?;
    let target = (1..1000)
        .map(|n| {
            dir.join(if n == 1 {
                format!("{name}.corrupt")
            } else {
                format!("{name}.corrupt-{n}")
            })
        })
        .find(|p| !p.exists())?;
    std::fs::rename(path, &target).ok()?;
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(dir.join(format!("{name}{suffix}")));
    }
    Some(target)
}

impl Db {
    /// Opens (creating or migrating) the database at `path`. Never fails: damage is worked around and reported in
    /// the warnings.
    pub fn open(path: &Path) -> (Db, Vec<String>) {
        let mut warnings = Vec::new();
        let conn = match open_file(path) {
            Ok(c) => Some(c),
            Err(failure) => {
                let why = match failure {
                    OpenFailure::Newer(v) => format!("was written by a newer Spotter (schema {v})"),
                    OpenFailure::Damaged(e) => format!("could not be read ({e})"),
                };
                match move_aside(path) {
                    Some(moved) => {
                        warnings.push(format!(
                            "The history database {why}. It was kept as {} and Spotter started with a fresh one: the watches re-check and re-baseline.",
                            moved.display()
                        ));
                        open_file(path).ok()
                    }
                    None => {
                        warnings.push(format!("The history database {why} and could not be moved aside."));
                        None
                    }
                }
            }
        };
        match conn {
            Some(conn) => (
                Db {
                    conn: Mutex::new(conn),
                    path: Some(path.to_path_buf()),
                },
                warnings,
            ),
            None => {
                warnings.push("History is kept in memory only for this run (the database folder is not writable).".into());
                let (db, mut more) = (Db::in_memory(), Vec::new());
                warnings.append(&mut more);
                (db, warnings)
            }
        }
    }

    /// A database in memory (tests, and the fallback).
    pub fn in_memory() -> Db {
        let conn = Connection::open_in_memory().unwrap_or_else(|_| {
            // Opening an in-memory database only fails when SQLite itself is unusable.
            Connection::open_in_memory().unwrap_or_else(|e| panic_free_exit(&e))
        });
        let _ = migrate(&conn);
        Db {
            conn: Mutex::new(conn),
            path: None,
        }
    }

    /// Where the file is (`None` for a database in memory).
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The schema version of the open database.
    pub fn schema_version(&self) -> Result<u32> {
        self.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(internal)
    }

    // ---- watch state ----

    /// Loads every watch's state; unreadable rows are skipped with a warning.
    pub fn load_states(&self) -> Result<(BTreeMap<WatchId, WatchState>, Vec<String>)> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT id, json FROM watch_state").map_err(internal)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(internal)?;
        let mut out = BTreeMap::new();
        let mut warnings = Vec::new();
        for row in rows {
            let (id, json) = row.map_err(internal)?;
            match (WatchId::parse(&id), serde_json::from_str::<WatchState>(&json)) {
                (Ok(id), Ok(s)) => {
                    out.insert(id, s);
                }
                _ => warnings.push(format!("The state of watch \"{id}\" could not be read; it starts fresh.")),
            }
        }
        Ok((out, warnings))
    }

    /// Saves one watch's state.
    pub fn save_state(&self, id: &WatchId, state: &WatchState) -> Result<()> {
        let json = serde_json::to_string(state).map_err(|e| EngineError::Internal(format!("State could not be encoded: {e}")))?;
        self.conn()
            .execute(
                "INSERT INTO watch_state (id, json) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET json = excluded.json",
                params![id.as_str(), json],
            )
            .map_err(internal)?;
        Ok(())
    }

    /// Forgets everything about a watch (state, history, cache, queued digest items).
    pub fn delete_watch(&self, id: &WatchId) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(internal)?;
        for table in [
            "watch_state WHERE id",
            "events WHERE watch",
            "check_days WHERE watch",
            "downloads WHERE watch",
            "http_cache WHERE watch",
            "digest_queue WHERE watch",
        ] {
            tx.execute(&format!("DELETE FROM {table} = ?1"), params![id.as_str()])
                .map_err(internal)?;
        }
        tx.execute("DELETE FROM kv WHERE key = ?1", params![format!("page:{}", id.as_str())])
            .map_err(internal)?;
        tx.commit().map_err(internal)
    }

    // ---- journal ----

    /// Appends an event (the feed keeps the newest [`MAX_EVENTS`]).
    pub fn record_event(&self, event: &Event) -> Result<()> {
        let json = serde_json::to_string(event).map_err(|e| EngineError::Internal(e.to_string()))?;
        let conn = self.conn();
        conn.execute(
            "INSERT INTO events (at, watch, json) VALUES (?1, ?2, ?3)",
            params![event.at.0, event.watch.as_ref().map(WatchId::as_str), json],
        )
        .map_err(internal)?;
        conn.execute(
            "DELETE FROM events WHERE id <= (SELECT id FROM events ORDER BY id DESC LIMIT 1 OFFSET ?1)",
            params![MAX_EVENTS as i64],
        )
        .map_err(internal)?;
        Ok(())
    }

    /// Events, newest first, at most `limit`, older than `before` when given.
    pub fn events(&self, limit: usize, before: Option<Timestamp>) -> Result<Vec<Event>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT json FROM events WHERE at < ?1 ORDER BY at DESC, id DESC LIMIT ?2")
            .map_err(internal)?;
        let rows = stmt
            .query_map(params![before.map_or(i64::MAX, |t| t.0), limit as i64], |r| r.get::<_, String>(0))
            .map_err(internal)?;
        let mut out = Vec::new();
        for row in rows {
            if let Ok(e) = serde_json::from_str::<Event>(&row.map_err(internal)?) {
                out.push(e);
            }
        }
        Ok(out)
    }

    /// Counts one check for the watch's reliability numbers.
    pub fn count_check(&self, watch: &WatchId, at: Timestamp, ok: bool) -> Result<()> {
        let day = at.0.div_euclid(DAY_MS);
        let (o, f) = if ok { (1, 0) } else { (0, 1) };
        self.conn()
            .execute(
                "INSERT INTO check_days (watch, day, ok, failed) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(watch, day) DO UPDATE SET ok = ok + ?3, failed = failed + ?4",
                params![watch.as_str(), day, o, f],
            )
            .map_err(internal)?;
        Ok(())
    }

    /// Records a finished download.
    pub fn record_download(&self, d: &DownloadRecord) -> Result<()> {
        let json = serde_json::to_string(d).map_err(|e| EngineError::Internal(e.to_string()))?;
        let conn = self.conn();
        conn.execute(
            "INSERT INTO downloads (at, watch, json) VALUES (?1, ?2, ?3)",
            params![d.at.0, d.watch.as_str(), json],
        )
        .map_err(internal)?;
        conn.execute(
            "DELETE FROM downloads WHERE id <= (SELECT id FROM downloads ORDER BY id DESC LIMIT 1 OFFSET ?1)",
            params![MAX_DOWNLOADS as i64],
        )
        .map_err(internal)?;
        Ok(())
    }

    /// The whole journal, for the dashboard.
    pub fn load_journal(&self) -> Result<Journal> {
        let conn = self.conn();
        let mut journal = Journal::default();
        let mut stmt = conn.prepare("SELECT json FROM events ORDER BY at ASC, id ASC").map_err(internal)?;
        for row in stmt.query_map([], |r| r.get::<_, String>(0)).map_err(internal)? {
            if let Ok(e) = serde_json::from_str::<Event>(&row.map_err(internal)?) {
                journal.events.push_back(e);
            }
        }
        let mut stmt = conn
            .prepare("SELECT json FROM downloads ORDER BY at ASC, id ASC")
            .map_err(internal)?;
        for row in stmt.query_map([], |r| r.get::<_, String>(0)).map_err(internal)? {
            if let Ok(d) = serde_json::from_str::<DownloadRecord>(&row.map_err(internal)?) {
                journal.downloads.push_back(d);
            }
        }
        let mut stmt = conn.prepare("SELECT watch, day, ok, failed FROM check_days").map_err(internal)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, u32>(2)?,
                    r.get::<_, u32>(3)?,
                ))
            })
            .map_err(internal)?;
        for row in rows {
            let (watch, day, ok, failed) = row.map_err(internal)?;
            if let Ok(id) = WatchId::parse(&watch) {
                journal.checks.entry(id).or_default().insert(day, DayCounts { ok, failed });
            }
        }
        Ok(journal)
    }

    /// Drops history older than [`RETENTION_DAYS`] and that of watches that no longer exist.
    pub fn prune(&self, now: Timestamp, watches: &[WatchId]) -> Result<()> {
        let cutoff = now.0 - RETENTION_DAYS * DAY_MS;
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(internal)?;
        for table in ["events", "downloads"] {
            tx.execute(&format!("DELETE FROM {table} WHERE at < ?1"), params![cutoff])
                .map_err(internal)?;
        }
        tx.execute("DELETE FROM check_days WHERE day < ?1", params![cutoff.div_euclid(DAY_MS)])
            .map_err(internal)?;
        let keep: Vec<&str> = watches.iter().map(WatchId::as_str).collect();
        for (table, col) in [
            ("events", "watch"),
            ("downloads", "watch"),
            ("check_days", "watch"),
            ("watch_state", "id"),
            ("http_cache", "watch"),
            ("digest_queue", "watch"),
        ] {
            let ids: Vec<String> = {
                let mut stmt = tx
                    .prepare(&format!("SELECT DISTINCT {col} FROM {table} WHERE {col} IS NOT NULL"))
                    .map_err(internal)?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(internal)?;
                rows.filter_map(std::result::Result::ok).collect()
            };
            for id in ids.iter().filter(|i| !keep.contains(&i.as_str())) {
                tx.execute(&format!("DELETE FROM {table} WHERE {col} = ?1"), params![id])
                    .map_err(internal)?;
            }
        }
        tx.commit().map_err(internal)
    }

    // ---- HTTP validator cache ----

    /// The validators remembered for a watch, when they were taken for the same `fingerprint` (source settings).
    pub fn cached_validators(&self, watch: &WatchId, fingerprint: &str) -> Result<Option<Validators>> {
        self.conn()
            .query_row(
                "SELECT etag, last_modified FROM http_cache WHERE watch = ?1 AND fingerprint = ?2",
                params![watch.as_str(), fingerprint],
                |r| {
                    Ok(Validators {
                        etag: r.get(0)?,
                        last_modified: r.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(internal)
    }

    /// Remembers (or, when empty, forgets) the validators of a watch's last fetch.
    pub fn store_validators(&self, watch: &WatchId, fingerprint: &str, v: &Validators) -> Result<()> {
        let conn = self.conn();
        if v.is_empty() {
            conn.execute("DELETE FROM http_cache WHERE watch = ?1", params![watch.as_str()])
                .map_err(internal)?;
        } else {
            conn.execute(
                "INSERT INTO http_cache (watch, fingerprint, etag, last_modified) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(watch) DO UPDATE SET fingerprint = ?2, etag = ?3, last_modified = ?4",
                params![watch.as_str(), fingerprint, v.etag, v.last_modified],
            )
            .map_err(internal)?;
        }
        Ok(())
    }

    // ---- digest queue ----

    /// Queues a notification for the next digest.
    pub fn queue_digest(&self, watch: &WatchId, at: Timestamp, json: &str) -> Result<()> {
        self.conn()
            .execute(
                "INSERT INTO digest_queue (at, watch, json) VALUES (?1, ?2, ?3)",
                params![at.0, watch.as_str(), json],
            )
            .map_err(internal)?;
        Ok(())
    }

    /// Takes (and removes) everything queued, oldest first.
    pub fn take_digest(&self) -> Result<Vec<String>> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(internal)?;
        let items: Vec<String> = {
            let mut stmt = tx.prepare("SELECT json FROM digest_queue ORDER BY id").map_err(internal)?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(internal)?;
            rows.filter_map(std::result::Result::ok).collect()
        };
        tx.execute("DELETE FROM digest_queue", []).map_err(internal)?;
        tx.commit().map_err(internal)?;
        Ok(items)
    }

    /// How many items wait for the digest.
    pub fn digest_len(&self) -> Result<usize> {
        self.conn()
            .query_row("SELECT COUNT(*) FROM digest_queue", [], |r| r.get::<_, i64>(0))
            .map(|n| n as usize)
            .map_err(internal)
    }

    // ---- key/value ----

    /// A small value (page baselines, the last digest day).
    pub fn kv_get(&self, key: &str) -> Result<Option<String>> {
        self.conn()
            .query_row("SELECT value FROM kv WHERE key = ?1", params![key], |r| r.get(0))
            .optional()
            .map_err(internal)
    }

    /// Stores a small value.
    pub fn kv_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn()
            .execute(
                "INSERT INTO kv (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(internal)?;
        Ok(())
    }
}

/// SQLite is unusable (not just a bad file): there is nothing sensible left to do but stop this operation.
fn panic_free_exit(e: &rusqlite::Error) -> Connection {
    eprintln!("Spotter cannot use SQLite: {e}");
    std::process::exit(70)
}

#[cfg(test)]
mod tests {
    use super::*;
    use spotter_core::journal::EventKind;

    fn id(s: &str) -> WatchId {
        WatchId::parse(s).unwrap()
    }

    fn event(at: i64, w: &str) -> Event {
        Event {
            at: Timestamp(at),
            watch: Some(id(w)),
            kind: EventKind::Recovered,
        }
    }

    #[test]
    fn states_round_trip_and_unreadable_rows_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let (db, w) = Db::open(&dir.path().join("s.db"));
        assert!(w.is_empty());
        assert_eq!(db.schema_version().unwrap(), SCHEMA);
        let mut s = WatchState {
            failures: 3,
            ..WatchState::default()
        };
        db.save_state(&id("a"), &s).unwrap();
        s.failures = 4;
        db.save_state(&id("a"), &s).unwrap();
        db.conn()
            .execute("INSERT INTO watch_state VALUES ('b', 'garbage'), ('BAD ID', '{}')", [])
            .unwrap();
        let (states, warnings) = db.load_states().unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[&id("a")].failures, 4);
        assert_eq!(warnings.len(), 2);
        drop(db);
        let (again, _) = Db::open(&dir.path().join("s.db"));
        assert_eq!(again.load_states().unwrap().0.len(), 1, "persisted on disk");
        assert!(again.path().is_some());
    }

    #[test]
    fn a_corrupt_database_is_moved_aside_with_a_warning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        std::fs::write(
            &path,
            b"this is definitely not a sqlite database, just text that is long enough to be a header",
        )
        .unwrap();
        let (db, warnings) = Db::open(&path);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains(".corrupt"));
        assert!(dir.path().join("s.db.corrupt").exists());
        db.save_state(&id("a"), &WatchState::default()).unwrap();
        drop(db);
        std::fs::write(
            &path,
            b"another broken file that is not a database at all, again long enough to read",
        )
        .unwrap();
        let (_, w2) = Db::open(&path);
        assert!(w2[0].contains("corrupt-2"), "a second damaged file gets its own name: {w2:?}");
    }

    #[test]
    fn a_newer_schema_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        {
            let c = Connection::open(&path).unwrap();
            c.pragma_update(None, "user_version", SCHEMA + 5).unwrap();
        }
        let (db, w) = Db::open(&path);
        assert!(w[0].contains("newer Spotter"), "{w:?}");
        assert_eq!(db.schema_version().unwrap(), SCHEMA);
    }

    #[test]
    fn an_old_schema_is_migrated_and_a_bad_migration_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch("CREATE TABLE unrelated (x INTEGER);").unwrap();
        }
        let (db, w) = Db::open(&path);
        assert!(w.is_empty());
        assert_eq!(db.schema_version().unwrap(), 1);
        db.kv_set("k", "v").unwrap();
        // A database that already has one of the tables fails its migration: it is treated as damaged.
        let path2 = dir.path().join("t.db");
        {
            let c = Connection::open(&path2).unwrap();
            c.execute_batch("CREATE TABLE kv (a INTEGER);").unwrap();
        }
        let (_, w2) = Db::open(&path2);
        assert_eq!(w2.len(), 1);
        assert!(dir.path().join("t.db.corrupt").exists());
    }

    #[test]
    fn an_unwritable_place_falls_back_to_memory() {
        let dir = tempfile::tempdir().unwrap();
        let (db, w) = Db::open(&dir.path().join("missing-folder").join("s.db"));
        assert!(w.iter().any(|m| m.contains("memory")), "{w:?}");
        db.kv_set("a", "b").unwrap();
        assert_eq!(db.kv_get("a").unwrap().as_deref(), Some("b"));
        assert!(db.path().is_none());
    }

    #[test]
    fn events_are_bounded_paged_and_newest_first() {
        let db = Db::in_memory();
        for i in 0..(MAX_EVENTS as i64 + 20) {
            db.record_event(&event(i, "a")).unwrap();
        }
        let all = db.load_journal().unwrap();
        assert_eq!(all.events.len(), MAX_EVENTS);
        let page = db.events(5, None).unwrap();
        assert_eq!(
            page.iter().map(|e| e.at.0).collect::<Vec<_>>(),
            (0..5).map(|i| MAX_EVENTS as i64 + 19 - i).collect::<Vec<_>>()
        );
        let older = db.events(3, Some(Timestamp(100))).unwrap();
        assert_eq!(older[0].at.0, 99);
        db.conn()
            .execute("INSERT INTO events (at, watch, json) VALUES (1, NULL, 'junk')", [])
            .unwrap();
        assert!(
            db.events(2000, None).unwrap().iter().all(|e| e.at.0 != 1 || e.watch.is_some()),
            "junk rows are skipped"
        );
        assert_eq!(db.load_journal().unwrap().events.len(), MAX_EVENTS);
    }

    #[test]
    fn counts_downloads_and_the_journal() {
        let db = Db::in_memory();
        let t = Timestamp(5 * DAY_MS + 7);
        db.count_check(&id("a"), t, true).unwrap();
        db.count_check(&id("a"), t, true).unwrap();
        db.count_check(&id("a"), t, false).unwrap();
        for i in 0..(MAX_DOWNLOADS as i64 + 3) {
            db.record_download(&DownloadRecord {
                at: Timestamp(i),
                watch: id("a"),
                version: "1".into(),
                size: 1,
                ok: true,
            })
            .unwrap();
        }
        let j = db.load_journal().unwrap();
        assert_eq!(j.checks[&id("a")][&5], DayCounts { ok: 2, failed: 1 });
        assert_eq!(j.downloads.len(), MAX_DOWNLOADS);
        db.conn().execute("INSERT INTO check_days VALUES ('BAD ID', 1, 1, 1)", []).unwrap();
        assert_eq!(db.load_journal().unwrap().checks.len(), 1);
    }

    #[test]
    fn pruning_drops_old_history_and_removed_watches() {
        let db = Db::in_memory();
        let now = Timestamp(1000 * DAY_MS);
        db.record_event(&event(now.0 - 500 * DAY_MS, "a")).unwrap();
        db.record_event(&event(now.0, "a")).unwrap();
        db.record_event(&event(now.0, "gone")).unwrap();
        db.save_state(&id("gone"), &WatchState::default()).unwrap();
        db.count_check(&id("gone"), now, true).unwrap();
        db.count_check(&id("a"), Timestamp(now.0 - 500 * DAY_MS), true).unwrap();
        db.store_validators(
            &id("gone"),
            "f",
            &Validators {
                etag: Some("e".into()),
                last_modified: None,
            },
        )
        .unwrap();
        db.queue_digest(&id("gone"), now, "{}").unwrap();
        db.prune(now, &[id("a")]).unwrap();
        let j = db.load_journal().unwrap();
        assert_eq!(j.events.len(), 1);
        assert!(j.checks.is_empty());
        assert!(db.load_states().unwrap().0.is_empty());
        assert_eq!(
            (db.cached_validators(&id("gone"), "f").unwrap(), db.digest_len().unwrap()),
            (None, 0)
        );
    }

    #[test]
    fn deleting_a_watch_forgets_everything() {
        let db = Db::in_memory();
        let a = id("a");
        db.save_state(&a, &WatchState::default()).unwrap();
        db.record_event(&event(1, "a")).unwrap();
        db.record_event(&event(2, "b")).unwrap();
        db.kv_set("page:a", "x").unwrap();
        db.kv_set("page:b", "y").unwrap();
        db.delete_watch(&a).unwrap();
        assert!(db.load_states().unwrap().0.is_empty());
        assert_eq!(db.load_journal().unwrap().events.len(), 1);
        assert_eq!(
            (db.kv_get("page:a").unwrap(), db.kv_get("page:b").unwrap().as_deref()),
            (None, Some("y"))
        );
    }

    #[test]
    fn validators_need_the_same_fingerprint() {
        let db = Db::in_memory();
        let v = Validators {
            etag: Some("\"e1\"".into()),
            last_modified: Some("Wed, 21 Oct 2026 07:28:00 GMT".into()),
        };
        db.store_validators(&id("a"), "fp1", &v).unwrap();
        assert_eq!(db.cached_validators(&id("a"), "fp1").unwrap(), Some(v.clone()));
        assert_eq!(db.cached_validators(&id("a"), "fp2").unwrap(), None, "the source changed");
        db.store_validators(&id("a"), "fp1", &Validators::default()).unwrap();
        assert_eq!(
            db.cached_validators(&id("a"), "fp1").unwrap(),
            None,
            "a server without validators clears the cache"
        );
    }

    #[test]
    fn the_digest_queue_is_taken_once_in_order() {
        let db = Db::in_memory();
        db.queue_digest(&id("a"), Timestamp(1), "one").unwrap();
        db.queue_digest(&id("b"), Timestamp(2), "two").unwrap();
        assert_eq!(db.digest_len().unwrap(), 2);
        assert_eq!(db.take_digest().unwrap(), ["one", "two"]);
        assert!(db.take_digest().unwrap().is_empty());
    }

    #[test]
    fn a_poisoned_lock_recovers() {
        let db = std::sync::Arc::new(Db::in_memory());
        let d2 = db.clone();
        let _ = std::thread::spawn(move || {
            let _g = d2.conn();
            panic!("poison");
        })
        .join();
        db.kv_set("a", "b").unwrap();
    }
}
