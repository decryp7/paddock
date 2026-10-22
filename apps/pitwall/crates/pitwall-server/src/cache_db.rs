//! The disk cache (ADR 0019): one SQLite file, `cache.sqlite`, behind the core's [`CacheStore`] port.
//!
//! - WAL mode; readers never wait for the writer. Every write (stores, hit counts, evictions, sweeps, clearing) runs on
//!   a writer thread, so the request path only reads. A store that is not on disk yet is found in `pending`.
//! - Owner-only (0600 on Unix; Windows keeps the per-user data folder's rights).
//! - Versioned schema (`user_version`). A file that is not a database, is damaged or has another version is renamed
//!   aside (`cache.sqlite.bad-<time>`) and a fresh one is created; if even that fails the caller falls back to memory.
//! - Rows hold the key hash, the value, created/last-used times, size, hits, model id and app id; never an API key.
//! - Per kind: a size cap with least-recently-used eviction, and the expiry rule of the core's `Expiry`.

use pitwall_core::cache::{CacheStore, Expiry, Key, Kind, KindStats, Limits, Meta, Scope};
use pitwall_core::time::Millis;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard};

/// The schema version (`PRAGMA user_version`).
pub const SCHEMA_VERSION: i64 = 1;
/// Writes queued before further ones are dropped (a cache may lose a store; it must not hold up a request).
const QUEUE: usize = 4096;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

type Limiter = Arc<Mutex<HashMap<Kind, Limits>>>;
type Pending = Arc<Mutex<HashMap<(Kind, Key), (Vec<u8>, Meta, Millis)>>>;

enum Job {
    Put(Kind, Key, Vec<u8>, Meta, Millis),
    Touch(Kind, Key, Millis),
    Delete(Kind, Key),
    Clear(Scope, SyncSender<u64>),
    Sweep(Kind, Millis, u64, SyncSender<u64>),
    Trim(Kind),
    Flush(SyncSender<()>),
}

/// The SQLite-backed store.
pub struct SqliteStore {
    reader: Mutex<Connection>,
    tx: SyncSender<Job>,
    limits: Limiter,
    pending: Pending,
    last_error: Arc<Mutex<Option<String>>>,
}

fn kind_of(s: &str) -> Option<Kind> {
    Kind::ALL.into_iter().find(|k| k.name() == s)
}

fn create_private(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let _ = std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(path);
    }
    #[cfg(not(unix))]
    let _ = path;
    crate::paths::restrict(path);
}

fn side_files(path: &Path) -> [PathBuf; 2] {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    [
        path.with_file_name(format!("{name}-wal")),
        path.with_file_name(format!("{name}-shm")),
    ]
}

fn set_up(path: &Path, flags: OpenFlags) -> rusqlite::Result<Connection> {
    let c = Connection::open_with_flags(path, flags)?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(c)
}

fn init(path: &Path) -> rusqlite::Result<Connection> {
    create_private(path);
    let c = set_up(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE)?;
    let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let tables: i64 = c.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
    if tables > 0 && version != SCHEMA_VERSION {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let ok: String = c.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
    if ok != "ok" {
        return Err(rusqlite::Error::InvalidQuery);
    }
    c.pragma_update(None, "journal_mode", "WAL")?;
    c.pragma_update(None, "synchronous", "NORMAL")?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS entries (
            kind TEXT NOT NULL, key BLOB NOT NULL, value BLOB NOT NULL, size INTEGER NOT NULL,
            created INTEGER NOT NULL, used INTEGER NOT NULL, hits INTEGER NOT NULL DEFAULT 0,
            model TEXT NOT NULL, app TEXT NOT NULL, preset TEXT NOT NULL, PRIMARY KEY (kind, key)) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS entries_used ON entries (kind, used);",
    )?;
    c.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(c)
}

impl SqliteStore {
    /// Opens (or creates) the database. A file that cannot be used is moved aside and replaced; the error is only
    /// returned when even a fresh file cannot be created.
    pub fn open(path: &Path, response: Limits, embedding: Limits) -> Result<SqliteStore, String> {
        let writer = match init(path) {
            Ok(c) => c,
            Err(_) => {
                let stamp = crate::paths::now_ms();
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let _ = std::fs::rename(path, path.with_file_name(format!("{name}.bad-{stamp}")));
                for s in side_files(path) {
                    let _ = std::fs::remove_file(s);
                }
                init(path).map_err(|e| format!("the cache database cannot be created: {e}"))?
            }
        };
        let reader = set_up(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| format!("the cache database cannot be read: {e}"))?;
        let limits: Limiter = Arc::new(Mutex::new(HashMap::from([
            (Kind::Response, response),
            (Kind::Embedding, embedding),
        ])));
        let pending: Pending = Arc::default();
        let last_error: Arc<Mutex<Option<String>>> = Arc::default();
        let (tx, rx) = sync_channel(QUEUE);
        {
            let (limits, pending, err) = (limits.clone(), pending.clone(), last_error.clone());
            std::thread::Builder::new()
                .name("pitwall-cache-db".into())
                .spawn(move || writer_loop(&writer, &rx, &limits, &pending, &err))
                .map_err(|e| format!("the cache writer cannot start: {e}"))?;
        }
        let s = SqliteStore {
            reader: Mutex::new(reader),
            tx,
            limits,
            pending,
            last_error,
        };
        for k in Kind::ALL {
            let _ = s.tx.try_send(Job::Trim(k));
        }
        Ok(s)
    }

    /// The last write error, if any (cleared by the next successful write).
    pub fn last_error(&self) -> Option<String> {
        lock(&self.last_error).clone()
    }

    fn limits_of(&self, kind: Kind) -> Option<Limits> {
        lock(&self.limits).get(&kind).copied()
    }

    fn ask(&self, make: impl FnOnce(SyncSender<u64>) -> Job) -> u64 {
        let (r, rx) = sync_channel(1);
        if self.tx.send(make(r)).is_err() {
            return 0;
        }
        rx.recv().unwrap_or(0)
    }
}

fn record(err: &Mutex<Option<String>>, r: rusqlite::Result<()>) {
    *lock(err) = r.err().map(|e| e.to_string());
}

fn writer_loop(c: &Connection, rx: &Receiver<Job>, limits: &Limiter, pending: &Pending, err: &Mutex<Option<String>>) {
    while let Ok(job) = rx.recv() {
        match job {
            Job::Put(kind, key, value, meta, now) => {
                let r = c
                    .execute(
                        "INSERT OR REPLACE INTO entries (kind,key,value,size,created,used,hits,model,app,preset) VALUES (?1,?2,?3,?4,?5,?5,0,?6,?7,?8)",
                        params![kind.name(), key.as_slice(), value.as_slice(), value.len() as i64, now as i64, meta.model, meta.app, meta.preset],
                    )
                    .map(|_| ());
                let ok = r.is_ok();
                record(err, r);
                let mut p = lock(pending);
                if p.get(&(kind, key)).is_some_and(|(v, _, t)| *t == now && *v == value) {
                    p.remove(&(kind, key));
                }
                drop(p);
                if ok {
                    record(err, trim(c, kind, limits));
                }
            }
            Job::Touch(kind, key, now) => record(
                err,
                c.execute(
                    "UPDATE entries SET used=?3, hits=hits+1 WHERE kind=?1 AND key=?2",
                    params![kind.name(), key.as_slice(), now as i64],
                )
                .map(|_| ()),
            ),
            Job::Delete(kind, key) => record(
                err,
                c.execute("DELETE FROM entries WHERE kind=?1 AND key=?2", params![kind.name(), key.as_slice()])
                    .map(|_| ()),
            ),
            Job::Clear(scope, reply) => {
                let n = match &scope {
                    Scope::All => c.execute("DELETE FROM entries", []),
                    Scope::Kind(k) => c.execute("DELETE FROM entries WHERE kind=?1", params![k.name()]),
                    Scope::App(a) => c.execute("DELETE FROM entries WHERE app=?1", params![a]),
                    Scope::Model(m) => c.execute("DELETE FROM entries WHERE model=?1", params![m]),
                    Scope::Preset(p) => c.execute("DELETE FROM entries WHERE preset=?1", params![p]),
                };
                lock(pending).retain(|(k, _), (_, meta, _)| !scope.covers(*k, meta));
                let _ = reply.send(n.unwrap_or(0) as u64);
            }
            Job::Sweep(kind, now, max, reply) => {
                let n = limits_for(limits, kind).map_or(0, |l| sweep(c, kind, l.expiry, now, max));
                let _ = reply.send(n);
            }
            Job::Trim(kind) => record(err, trim(c, kind, limits)),
            Job::Flush(reply) => {
                let _ = reply.send(());
            }
        }
    }
}

fn limits_for(limits: &Limiter, kind: Kind) -> Option<Limits> {
    lock(limits).get(&kind).copied()
}

fn sweep(c: &Connection, kind: Kind, exp: Expiry, now: Millis, max: u64) -> u64 {
    let Some(ttl) = exp.ttl else { return 0 };
    let col = if exp.sliding { "used" } else { "created" };
    let Some(cutoff) = now.checked_sub(ttl) else { return 0 };
    let cutoff = cutoff as i64;
    let sql = format!("DELETE FROM entries WHERE kind=?1 AND key IN (SELECT key FROM entries WHERE kind=?1 AND {col} <= ?2 LIMIT ?3)");
    c.execute(&sql, params![kind.name(), cutoff, i64::try_from(max).unwrap_or(i64::MAX)])
        .map_or(0, |n| n as u64)
}

/// Evicts least recently used entries until the kind is within its size cap.
fn trim(c: &Connection, kind: Kind, limits: &Limiter) -> rusqlite::Result<()> {
    let Some(l) = limits_for(limits, kind) else { return Ok(()) };
    let total: i64 = c.query_row(
        "SELECT coalesce(sum(size),0) FROM entries WHERE kind=?1",
        params![kind.name()],
        |r| r.get(0),
    )?;
    let mut over = total - i64::try_from(l.max_bytes).unwrap_or(i64::MAX);
    if over <= 0 {
        return Ok(());
    }
    let mut doomed: Vec<Vec<u8>> = vec![];
    {
        let mut q = c.prepare("SELECT key, size FROM entries WHERE kind=?1 ORDER BY used ASC")?;
        let mut rows = q.query(params![kind.name()])?;
        while over > 0 {
            let Some(r) = rows.next()? else { break };
            doomed.push(r.get(0)?);
            over -= r.get::<_, i64>(1)?;
        }
    }
    for k in doomed {
        c.execute("DELETE FROM entries WHERE kind=?1 AND key=?2", params![kind.name(), k])?;
    }
    Ok(())
}

impl CacheStore for SqliteStore {
    fn get(&self, kind: Kind, key: &Key, now: Millis) -> Option<Vec<u8>> {
        let exp = self.limits_of(kind)?.expiry;
        if let Some((v, _, stored)) = lock(&self.pending).get(&(kind, *key)) {
            return (!exp.expired(*stored, *stored, now)).then(|| v.clone());
        }
        let row = lock(&self.reader)
            .query_row(
                "SELECT value, created, used FROM entries WHERE kind=?1 AND key=?2",
                params![kind.name(), key.as_slice()],
                |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
            )
            .optional()
            .ok()
            .flatten()?;
        if exp.expired(row.1 as Millis, row.2 as Millis, now) {
            let _ = self.tx.try_send(Job::Delete(kind, *key));
            return None;
        }
        let _ = self.tx.try_send(Job::Touch(kind, *key, now));
        Some(row.0)
    }

    fn put(&self, kind: Kind, key: Key, value: Vec<u8>, meta: Meta, now: Millis) {
        let Some(l) = self.limits_of(kind) else { return };
        if value.len() as u64 > l.max_bytes {
            return;
        }
        lock(&self.pending).insert((kind, key), (value.clone(), meta.clone(), now));
        if self.tx.try_send(Job::Put(kind, key, value, meta, now)).is_err() {
            lock(&self.pending).remove(&(kind, key));
        }
    }

    fn clear(&self, scope: &Scope) -> u64 {
        self.ask(|r| Job::Clear(scope.clone(), r))
    }

    fn stats(&self, kind: Kind) -> KindStats {
        self.flush();
        lock(&self.reader)
            .query_row(
                "SELECT count(*), coalesce(sum(size),0), coalesce(sum(hits),0), min(created) FROM entries WHERE kind=?1",
                params![kind.name()],
                |r| {
                    Ok(KindStats {
                        entries: r.get::<_, i64>(0)? as u64,
                        bytes: r.get::<_, i64>(1)? as u64,
                        hits: r.get::<_, i64>(2)? as u64,
                        oldest: r.get::<_, Option<i64>>(3)?.map(|t| t as Millis),
                    })
                },
            )
            .unwrap_or_default()
    }

    fn configure(&self, kind: Kind, limits: Limits) {
        lock(&self.limits).insert(kind, limits);
        let _ = self.tx.try_send(Job::Trim(kind));
    }

    fn sweep(&self, kind: Kind, now: Millis, max: u64) -> u64 {
        self.ask(|r| Job::Sweep(kind, now, max, r))
    }

    fn flush(&self) {
        let (r, rx) = sync_channel(1);
        if self.tx.send(Job::Flush(r)).is_ok() {
            let _ = rx.recv();
        }
    }
}

/// Which `kind` strings a database holds (used by tests and the integrity check).
#[allow(dead_code)]
pub fn kinds_in(path: &Path) -> Vec<Kind> {
    let Ok(c) = set_up(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return vec![];
    };
    let Ok(mut q) = c.prepare("SELECT DISTINCT kind FROM entries") else {
        return vec![];
    };
    q.query_map([], |r| r.get::<_, String>(0))
        .map(|rows| rows.filter_map(Result::ok).filter_map(|k| kind_of(&k)).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lim(max: u64, ttl: Option<Millis>, sliding: bool) -> Limits {
        Limits {
            max_bytes: max,
            expiry: Expiry { ttl, sliding },
        }
    }

    fn open(dir: &Path, max: u64) -> SqliteStore {
        SqliteStore::open(&dir.join("cache.sqlite"), lim(max, Some(1000), false), lim(max, Some(1000), true)).unwrap()
    }

    fn meta(app: &str, model: &str) -> Meta {
        Meta {
            app: app.into(),
            model: model.into(),
            preset: String::new(),
        }
    }

    #[test]
    fn entries_survive_a_restart_with_their_counts() {
        let d = tempfile::tempdir().unwrap();
        {
            let s = open(d.path(), 10_000);
            s.put(Kind::Response, [1; 32], b"answer".to_vec(), meta("debrief", "m"), 10);
            assert_eq!(s.get(Kind::Response, &[1; 32], 11).unwrap(), b"answer", "found while still pending");
            s.flush();
            assert_eq!(s.get(Kind::Response, &[1; 32], 12).unwrap(), b"answer");
            s.flush();
        }
        let s = open(d.path(), 10_000);
        assert_eq!(s.get(Kind::Response, &[1; 32], 20).unwrap(), b"answer");
        let st = s.stats(Kind::Response);
        assert_eq!((st.entries, st.bytes, st.oldest), (1, 6, Some(10)));
        assert_eq!(st.hits, 2, "hits are counted on disk");
        assert_eq!(s.stats(Kind::Embedding), KindStats::default());
        assert!(s.last_error().is_none());
        assert_eq!(kinds_in(&d.path().join("cache.sqlite")), vec![Kind::Response]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(d.path().join("cache.sqlite")).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "owner only");
        }
    }

    #[test]
    fn the_size_cap_evicts_least_recently_used_first() {
        let d = tempfile::tempdir().unwrap();
        let s = open(d.path(), 10);
        s.put(Kind::Response, [1; 32], vec![0; 4], meta("a", "m"), 1);
        s.put(Kind::Response, [2; 32], vec![0; 4], meta("a", "m"), 2);
        s.flush();
        assert!(s.get(Kind::Response, &[1; 32], 3).is_some());
        s.flush();
        s.put(Kind::Response, [3; 32], vec![0; 4], meta("a", "m"), 4);
        s.flush();
        assert!(s.get(Kind::Response, &[2; 32], 5).is_none(), "2 was least recently used");
        assert!(s.get(Kind::Response, &[1; 32], 5).is_some() && s.get(Kind::Response, &[3; 32], 5).is_some());
        s.put(Kind::Response, [4; 32], vec![0; 11], meta("a", "m"), 6);
        s.flush();
        assert!(s.get(Kind::Response, &[4; 32], 7).is_none(), "larger than the cache");
        // Lowering the cap trims at once.
        s.configure(Kind::Response, lim(4, Some(1000), false));
        s.flush();
        assert_eq!(s.stats(Kind::Response).entries, 1);
    }

    #[test]
    fn expiry_is_absolute_for_answers_and_sliding_for_embeddings_and_never_served_late() {
        let d = tempfile::tempdir().unwrap();
        let s = open(d.path(), 1000);
        s.put(Kind::Response, [1; 32], b"a".to_vec(), meta("a", "m"), 0);
        s.put(Kind::Embedding, [1; 32], b"e".to_vec(), meta("a", "m"), 0);
        s.flush();
        assert!(s.get(Kind::Embedding, &[1; 32], 900).is_some(), "used at 900");
        s.flush();
        assert!(s.get(Kind::Embedding, &[1; 32], 1800).is_some(), "sliding: kept to 1900");
        s.flush();
        assert!(
            s.get(Kind::Response, &[1; 32], 1000).is_none(),
            "answers expire 1000 after they were stored"
        );
        s.flush();
        assert_eq!(s.stats(Kind::Response).entries, 0, "the late read removed it");
        // A pending store is expired by the same rule.
        s.put(Kind::Response, [2; 32], b"b".to_vec(), meta("a", "m"), 5000);
        assert!(s.get(Kind::Response, &[2; 32], 9000).is_none());
        s.flush();
        // No expiry at all.
        s.configure(Kind::Response, lim(1000, None, false));
        assert!(s.get(Kind::Response, &[2; 32], u64::MAX / 4).is_some());
    }

    #[test]
    fn a_sweep_is_bounded_and_removes_only_the_expired() {
        let d = tempfile::tempdir().unwrap();
        let s = open(d.path(), 1000);
        for i in 0..5u8 {
            s.put(Kind::Response, [i; 32], vec![1], meta("a", "m"), 0);
        }
        s.put(Kind::Response, [9; 32], vec![1], meta("a", "m"), 5500);
        s.flush();
        assert_eq!(s.sweep(Kind::Response, 6000, 2), 2);
        assert_eq!(s.sweep(Kind::Response, 6000, 100), 3);
        assert_eq!(s.sweep(Kind::Response, 6000, 100), 0);
        assert_eq!(s.stats(Kind::Response).entries, 1);
        s.configure(Kind::Response, lim(1000, None, false));
        assert_eq!(s.sweep(Kind::Response, u64::MAX / 4, 100), 0, "never expires");
    }

    #[test]
    fn clearing_by_scope() {
        let d = tempfile::tempdir().unwrap();
        let s = open(d.path(), 1000);
        s.put(Kind::Response, [1; 32], vec![1], meta("debrief", "m1"), 0);
        s.put(Kind::Response, [2; 32], vec![1], meta("pitcrew", "m1"), 0);
        s.put(Kind::Embedding, [3; 32], vec![1], meta("debrief", "m2"), 0);
        s.put(Kind::Embedding, [4; 32], vec![1], meta("pitcrew", "m2"), 0);
        assert_eq!(s.clear(&Scope::App("debrief".into())), 2);
        assert_eq!(s.clear(&Scope::Model("m2".into())), 1);
        assert_eq!(s.clear(&Scope::Kind(Kind::Embedding)), 0);
        s.put(
            Kind::Response,
            [6; 32],
            vec![1],
            Meta {
                preset: "fast".into(),
                ..meta("x", "m9")
            },
            0,
        );
        assert_eq!(s.clear(&Scope::Preset("fast".into())), 1);
        assert_eq!(s.clear(&Scope::All), 1);
        assert_eq!(s.stats(Kind::Response).entries, 0);
        // Pending (not yet written) stores are cleared too.
        s.put(Kind::Response, [5; 32], vec![1], meta("x", "m"), 0);
        s.clear(&Scope::All);
        assert!(s.get(Kind::Response, &[5; 32], 1).is_none());
    }

    #[test]
    fn a_corrupt_file_is_renamed_aside_and_recreated() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("cache.sqlite");
        std::fs::write(
            &path,
            b"this is not a database at all, just text that is long enough to look like one",
        )
        .unwrap();
        let s = open(d.path(), 1000);
        s.put(Kind::Response, [1; 32], vec![1], meta("a", "m"), 0);
        s.flush();
        assert_eq!(s.stats(Kind::Response).entries, 1);
        let aside: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(".bad-"))
            .collect();
        assert_eq!(aside.len(), 1, "the damaged file is kept for inspection");
    }

    #[test]
    fn another_schema_version_is_replaced() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("cache.sqlite");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch("CREATE TABLE entries (x); PRAGMA user_version = 99;").unwrap();
        }
        let s = open(d.path(), 1000);
        s.put(Kind::Embedding, [1; 32], vec![1], meta("a", "m"), 0);
        s.flush();
        assert_eq!(s.stats(Kind::Embedding).entries, 1);
        let v: i64 = Connection::open(&path)
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn an_unusable_location_is_an_error_the_caller_can_show() {
        let d = tempfile::tempdir().unwrap();
        let e = SqliteStore::open(
            &d.path().join("missing").join("cache.sqlite"),
            lim(1, None, false),
            lim(1, None, false),
        );
        assert!(e.is_err());
    }
}
