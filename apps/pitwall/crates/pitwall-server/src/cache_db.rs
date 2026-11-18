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
use rusqlite::{params, Connection, OpenFlags};
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

/// One value to store: its key, bytes and who it belongs to.
type Item = (Key, Vec<u8>, Meta);

enum Job {
    /// A batch of stores of one kind: one transaction, one trim.
    PutMany(Kind, Vec<Item>, Millis),
    /// Hit counts of a batch of reads: one transaction.
    TouchMany(Kind, Vec<Key>, Millis),
    /// Expired entries found by a read: one transaction.
    DeleteMany(Kind, Vec<Key>),
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
        Self::open_with(path, response, embedding, |_| {})
    }

    /// [`SqliteStore::open`], with `tune` run on each connection (writer, then reader) before it is used: the tests count
    /// commits and statements there.
    fn open_with(path: &Path, response: Limits, embedding: Limits, tune: impl Fn(&mut Connection)) -> Result<SqliteStore, String> {
        let mut writer = match init(path) {
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
        tune(&mut writer);
        let mut reader = set_up(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| format!("the cache database cannot be read: {e}"))?;
        tune(&mut reader);
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

/// The bytes each kind holds, kept by the writer thread so a store never sums the whole table: read once on the first need
/// and updated by every store and eviction; any other change to the rows (clear, sweep, delete, a failed write) forgets it.
type Sizes = HashMap<Kind, i64>;

/// Keys per `IN (...)` list (SQLite's variable limit is far higher; this keeps statements small).
const IN_CHUNK: usize = 500;

/// `?{from}, ?{from+1}, ...` for `n` variables.
fn placeholders(n: usize, from: usize) -> String {
    (from..from + n).map(|i| format!("?{i}")).collect::<Vec<_>>().join(",")
}

/// Runs `f` in one transaction: one commit for everything it did.
fn in_tx(c: &Connection, f: impl FnOnce(&Connection) -> rusqlite::Result<()>) -> rusqlite::Result<()> {
    let tx = c.unchecked_transaction()?;
    f(&tx)?;
    tx.commit()
}

fn writer_loop(c: &Connection, rx: &Receiver<Job>, limits: &Limiter, pending: &Pending, err: &Mutex<Option<String>>) {
    let mut sizes = Sizes::new();
    while let Ok(job) = rx.recv() {
        match job {
            Job::PutMany(kind, items, now) => {
                let r = in_tx(c, |c| put_many(c, kind, &items, now, limits, &mut sizes));
                if r.is_err() {
                    sizes.remove(&kind);
                }
                record(err, r);
                let mut p = lock(pending);
                for (key, value, _) in &items {
                    if p.get(&(kind, *key)).is_some_and(|(v, _, t)| *t == now && v == value) {
                        p.remove(&(kind, *key));
                    }
                }
            }
            Job::TouchMany(kind, keys, now) => record(
                err,
                in_tx(c, |c| {
                    let mut q = c.prepare_cached("UPDATE entries SET used=?3, hits=hits+1 WHERE kind=?1 AND key=?2")?;
                    for key in &keys {
                        q.execute(params![kind.name(), key.as_slice(), now as i64])?;
                    }
                    Ok(())
                }),
            ),
            Job::DeleteMany(kind, keys) => {
                sizes.remove(&kind);
                record(
                    err,
                    in_tx(c, |c| {
                        let mut q = c.prepare_cached("DELETE FROM entries WHERE kind=?1 AND key=?2")?;
                        for key in &keys {
                            q.execute(params![kind.name(), key.as_slice()])?;
                        }
                        Ok(())
                    }),
                );
            }
            Job::Clear(scope, reply) => {
                sizes.clear();
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
                sizes.remove(&kind);
                let n = limits_for(limits, kind).map_or(0, |l| sweep(c, kind, l.expiry, now, max));
                let _ = reply.send(n);
            }
            Job::Trim(kind) => {
                let r = in_tx(c, |c| trim(c, kind, limits, &mut sizes));
                if r.is_err() {
                    sizes.remove(&kind);
                }
                record(err, r);
            }
            Job::Flush(reply) => {
                let _ = reply.send(());
            }
        }
    }
}

/// What `kind` holds, in bytes: the running figure, read from the table once.
fn total_of(c: &Connection, kind: Kind, sizes: &mut Sizes) -> rusqlite::Result<i64> {
    if let Some(t) = sizes.get(&kind) {
        return Ok(*t);
    }
    let t: i64 = c.query_row(
        "SELECT coalesce(sum(size),0) FROM entries WHERE kind=?1",
        params![kind.name()],
        |r| r.get(0),
    )?;
    sizes.insert(kind, t);
    Ok(t)
}

/// Stores a batch inside the caller's transaction: the sizes of the rows it replaces come in one `IN (...)` read per
/// chunk, the rows go in through one prepared statement, and the kind is trimmed once at the end (ADR 0108, PW-2).
fn put_many(c: &Connection, kind: Kind, items: &[Item], now: Millis, limits: &Limiter, sizes: &mut Sizes) -> rusqlite::Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let mut total = total_of(c, kind, sizes)?;
    let mut held: HashMap<Key, i64> = HashMap::new();
    for chunk in items.chunks(IN_CHUNK) {
        let sql = format!(
            "SELECT key, size FROM entries WHERE kind=?1 AND key IN ({})",
            placeholders(chunk.len(), 2)
        );
        let mut q = c.prepare(&sql)?;
        let kn = kind.name();
        let args = std::iter::once(&kn as &dyn rusqlite::ToSql).chain(chunk.iter().map(|i| &i.0 as &dyn rusqlite::ToSql));
        let mut rows = q.query(rusqlite::params_from_iter(args))?;
        while let Some(r) = rows.next()? {
            if let Ok(k) = Key::try_from(r.get::<_, Vec<u8>>(0)?) {
                held.insert(k, r.get(1)?);
            }
        }
    }
    {
        let mut ins = c.prepare_cached(
            "INSERT OR REPLACE INTO entries (kind,key,value,size,created,used,hits,model,app,preset) VALUES (?1,?2,?3,?4,?5,?5,0,?6,?7,?8)",
        )?;
        for (key, value, meta) in items {
            let size = value.len() as i64;
            ins.execute(params![
                kind.name(),
                key.as_slice(),
                value.as_slice(),
                size,
                now as i64,
                meta.model,
                meta.app,
                meta.preset
            ])?;
            total += size - held.insert(*key, size).unwrap_or(0);
        }
    }
    sizes.insert(kind, total);
    trim(c, kind, limits, sizes)
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

/// Evicts least recently used entries until the kind is within its size cap, in the caller's transaction. The kind's size
/// is the running figure; only an eviction reads rows.
fn trim(c: &Connection, kind: Kind, limits: &Limiter, sizes: &mut Sizes) -> rusqlite::Result<()> {
    let Some(l) = limits_for(limits, kind) else { return Ok(()) };
    let total = total_of(c, kind, sizes)?;
    let mut over = total - i64::try_from(l.max_bytes).unwrap_or(i64::MAX);
    if over <= 0 {
        return Ok(());
    }
    let mut doomed: Vec<Vec<u8>> = vec![];
    let mut freed = 0;
    {
        let mut q = c.prepare("SELECT key, size FROM entries WHERE kind=?1 ORDER BY used ASC")?;
        let mut rows = q.query(params![kind.name()])?;
        while over > 0 {
            let Some(r) = rows.next()? else { break };
            doomed.push(r.get(0)?);
            let size = r.get::<_, i64>(1)?;
            over -= size;
            freed += size;
        }
    }
    for chunk in doomed.chunks(IN_CHUNK) {
        let sql = format!("DELETE FROM entries WHERE kind=?1 AND key IN ({})", placeholders(chunk.len(), 2));
        let kn = kind.name();
        let args = std::iter::once(&kn as &dyn rusqlite::ToSql).chain(chunk.iter().map(|k| k as &dyn rusqlite::ToSql));
        c.execute(&sql, rusqlite::params_from_iter(args))?;
    }
    sizes.insert(kind, total - freed);
    Ok(())
}

impl CacheStore for SqliteStore {
    fn get(&self, kind: Kind, key: &Key, now: Millis) -> Option<Vec<u8>> {
        self.get_many(kind, std::slice::from_ref(key), now).pop().flatten()
    }

    /// One `IN (...)` read per chunk of keys; the hit counts and the expired entries it found go to the writer as one job each.
    fn get_many(&self, kind: Kind, keys: &[Key], now: Millis) -> Vec<Option<Vec<u8>>> {
        let mut out: Vec<Option<Vec<u8>>> = vec![None; keys.len()];
        let Some(exp) = self.limits_of(kind).map(|l| l.expiry) else {
            return out;
        };
        let mut want: Vec<usize> = vec![];
        {
            let pending = lock(&self.pending);
            for (i, k) in keys.iter().enumerate() {
                match pending.get(&(kind, *k)) {
                    Some((v, _, stored)) => out[i] = (!exp.expired(*stored, *stored, now)).then(|| v.clone()),
                    None => want.push(i),
                }
            }
        }
        if want.is_empty() {
            return out;
        }
        let mut rows: HashMap<Key, (Vec<u8>, i64, i64)> = HashMap::new();
        {
            let reader = lock(&self.reader);
            let distinct: Vec<Key> = want
                .iter()
                .map(|i| keys[*i])
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            for chunk in distinct.chunks(IN_CHUNK) {
                let sql = format!(
                    "SELECT key, value, created, used FROM entries WHERE kind=?1 AND key IN ({})",
                    placeholders(chunk.len(), 2)
                );
                let kn = kind.name();
                let args = std::iter::once(&kn as &dyn rusqlite::ToSql).chain(chunk.iter().map(|k| k as &dyn rusqlite::ToSql));
                let read = (|| -> rusqlite::Result<()> {
                    let mut q = reader.prepare(&sql)?;
                    let mut found = q.query(rusqlite::params_from_iter(args))?;
                    while let Some(r) = found.next()? {
                        if let Ok(k) = Key::try_from(r.get::<_, Vec<u8>>(0)?) {
                            rows.insert(k, (r.get(1)?, r.get(2)?, r.get(3)?));
                        }
                    }
                    Ok(())
                })();
                // A read that fails is a miss, as for a single key.
                let _ = read;
            }
        }
        let (mut touched, mut expired) = (vec![], vec![]);
        for i in want {
            let Some((value, created, used)) = rows.get(&keys[i]) else {
                continue;
            };
            if exp.expired(*created as Millis, *used as Millis, now) {
                if !expired.contains(&keys[i]) {
                    expired.push(keys[i]);
                }
            } else {
                out[i] = Some(value.clone());
                if !touched.contains(&keys[i]) {
                    touched.push(keys[i]);
                }
            }
        }
        if !expired.is_empty() {
            let _ = self.tx.try_send(Job::DeleteMany(kind, expired));
        }
        if !touched.is_empty() {
            let _ = self.tx.try_send(Job::TouchMany(kind, touched, now));
        }
        out
    }

    fn put(&self, kind: Kind, key: Key, value: Vec<u8>, meta: Meta, now: Millis) {
        self.put_many(kind, vec![(key, value, meta)], now);
    }

    /// One job for the whole batch: one transaction, one trim.
    fn put_many(&self, kind: Kind, items: Vec<(Key, Vec<u8>, Meta)>, now: Millis) {
        let Some(l) = self.limits_of(kind) else { return };
        let items: Vec<Item> = items.into_iter().filter(|(_, v, _)| v.len() as u64 <= l.max_bytes).collect();
        if items.is_empty() {
            return;
        }
        {
            let mut pending = lock(&self.pending);
            for (k, v, m) in &items {
                pending.insert((kind, *k), (v.clone(), m.clone(), now));
            }
        }
        let keys: Vec<Key> = items.iter().map(|i| i.0).collect();
        if self.tx.try_send(Job::PutMany(kind, items, now)).is_err() {
            let mut pending = lock(&self.pending);
            for k in keys {
                pending.remove(&(kind, k));
            }
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

    /// What the database was asked to do (ADR 0108): every statement's text and every commit, seen by SQLite itself.
    #[derive(Clone, Default)]
    struct Spy {
        sql: Arc<Mutex<Vec<String>>>,
        commits: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Spy {
        fn attach(&self, c: &mut Connection) {
            use rusqlite::trace::{TraceEvent, TraceEventCodes};
            let sql = self.sql.clone();
            c.trace_v2(
                TraceEventCodes::SQLITE_TRACE_STMT,
                Some(move |ev: TraceEvent<'_>| {
                    if let TraceEvent::Stmt(_, text) = ev {
                        lock(&sql).push(text.to_string());
                    }
                }),
            )
            .unwrap();
            let n = self.commits.clone();
            c.commit_hook(Some(move || {
                n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                false
            }))
            .unwrap();
        }
        fn commits(&self) -> usize {
            self.commits.load(std::sync::atomic::Ordering::SeqCst)
        }
        /// Statements so far whose text contains `part`.
        fn statements(&self, part: &str) -> usize {
            lock(&self.sql).iter().filter(|s| s.contains(part)).count()
        }
        fn all_statements(&self) -> usize {
            lock(&self.sql).len()
        }
    }

    fn open_spied(dir: &Path, max: u64) -> (SqliteStore, Spy) {
        let spy = Spy::default();
        let s = {
            let spy = spy.clone();
            SqliteStore::open_with(
                &dir.join("cache.sqlite"),
                lim(max, Some(1000), false),
                lim(max, Some(1000), true),
                move |c| spy.attach(c),
            )
            .unwrap()
        };
        s.flush();
        (s, spy)
    }

    fn batch(from: u8, n: u8, size: usize) -> Vec<(Key, Vec<u8>, Meta)> {
        (from..from + n).map(|i| ([i; 32], vec![i; size], meta("app", "model"))).collect()
    }

    /// PW-2 (ADR 0108): a batch of N stores is one commit and about one statement per row, however large N is.
    #[test]
    fn a_batch_of_stores_is_one_commit() {
        let d = tempfile::tempdir().unwrap();
        let (s, spy) = open_spied(d.path(), 1_000_000);
        let (commits, statements) = (spy.commits(), spy.all_statements());
        s.put_many(Kind::Embedding, batch(1, 60, 16), 5);
        s.flush();
        assert_eq!(spy.commits() - commits, 1, "one commit for 60 vectors");
        let used = spy.all_statements() - statements;
        assert!(used <= 60 + 6, "one insert per row and a handful around them, got {used}");
        assert_eq!(s.stats(Kind::Embedding).entries, 60);
        assert!(s.last_error().is_none());
    }

    /// The positive control (ER-33): the same 60 vectors stored one by one are 60 commits, so the counter above can see a loop.
    #[test]
    fn the_spy_sees_one_commit_per_single_store() {
        let d = tempfile::tempdir().unwrap();
        let (s, spy) = open_spied(d.path(), 1_000_000);
        let commits = spy.commits();
        for (k, v, m) in batch(1, 60, 16) {
            s.put(Kind::Embedding, k, v, m, 5);
            s.flush();
        }
        assert_eq!(spy.commits() - commits, 60);
    }

    /// PW-2: the cache's size is a running figure: stores never sum the table again after the first.
    #[test]
    fn stores_never_sum_the_table_again() {
        let d = tempfile::tempdir().unwrap();
        let (s, spy) = open_spied(d.path(), 1_000_000);
        let before = spy.statements("sum(size)");
        for round in 0..5u8 {
            s.put_many(Kind::Embedding, batch(round * 10, 10, 16), 5);
            s.flush();
        }
        assert_eq!(
            spy.statements("sum(size)") - before,
            0,
            "the first sum was read at open, the figure is kept since"
        );
        assert_eq!(s.stats(Kind::Embedding).entries, 50);
    }

    /// PW-2: reading N keys is one `IN (...)` statement, and the hit counts are one commit.
    #[test]
    fn a_batch_of_reads_is_one_statement_and_one_commit() {
        let d = tempfile::tempdir().unwrap();
        let (s, spy) = open_spied(d.path(), 1_000_000);
        s.put_many(Kind::Embedding, batch(1, 40, 8), 5);
        s.flush();
        let (reads, commits) = (spy.statements("FROM entries WHERE kind=?1 AND key IN"), spy.commits());
        let mut keys: Vec<Key> = (1..=40u8).map(|i| [i; 32]).collect();
        keys.push([200; 32]);
        let got = s.get_many(Kind::Embedding, &keys, 6);
        s.flush();
        assert_eq!(got.iter().filter(|v| v.is_some()).count(), 40);
        assert!(got[40].is_none(), "an unknown key is a miss, in its place");
        assert_eq!(got[3].as_deref(), Some(&[4u8; 8][..]));
        assert_eq!(
            spy.statements("FROM entries WHERE kind=?1 AND key IN") - reads,
            1,
            "one read for 41 keys"
        );
        assert_eq!(spy.commits() - commits, 1, "one commit for 40 hit counts");
        assert_eq!(s.stats(Kind::Embedding).hits, 40);
    }

    #[test]
    fn a_batch_over_the_chunk_size_reads_in_chunks_and_keeps_order() {
        let d = tempfile::tempdir().unwrap();
        let s = open(d.path(), 10_000_000);
        let items: Vec<_> = (0..1200u32)
            .map(|i| {
                let mut k = [0u8; 32];
                k[..4].copy_from_slice(&i.to_be_bytes());
                (k, i.to_be_bytes().to_vec(), meta("a", "m"))
            })
            .collect();
        let keys: Vec<Key> = items.iter().map(|i| i.0).collect();
        s.put_many(Kind::Embedding, items, 1);
        s.flush();
        let got = s.get_many(Kind::Embedding, &keys, 2);
        for (i, v) in got.iter().enumerate() {
            assert_eq!(v.as_deref(), Some(&(i as u32).to_be_bytes()[..]), "key {i}");
        }
        assert_eq!(s.stats(Kind::Embedding).entries, 1200);
    }

    /// The running size agrees with the table after replacing, evicting, deleting and clearing.
    #[test]
    fn the_running_size_stays_right_and_the_cap_holds() {
        let d = tempfile::tempdir().unwrap();
        let s = open(d.path(), 100);
        s.put_many(Kind::Embedding, batch(1, 3, 20), 1);
        s.put_many(Kind::Embedding, batch(4, 3, 20), 2);
        s.flush();
        let st = s.stats(Kind::Embedding);
        assert_eq!((st.entries, st.bytes), (5, 100), "one oldest entry went to fit 120 bytes into 100");
        assert!(
            (4..=6u8).all(|i| s.get(Kind::Embedding, &[i; 32], 3).is_some()),
            "the newer batch stays"
        );
        // A batch that stores the same key twice and replaces a held one: the last value counts once.
        s.put_many(
            Kind::Embedding,
            vec![
                ([4; 32], vec![0; 10], meta("a", "m")),
                ([4; 32], vec![0; 30], meta("a", "m")),
                ([9; 32], vec![0; 10], meta("a", "m")),
            ],
            4,
        );
        s.flush();
        let st = s.stats(Kind::Embedding);
        assert_eq!(st.bytes, 100, "stats are the table's own sum: the cap held");
        assert_eq!(s.get(Kind::Embedding, &[4; 32], 5).unwrap().len(), 30);
        s.clear(&Scope::All);
        s.put_many(Kind::Embedding, batch(1, 5, 20), 6);
        s.put_many(Kind::Embedding, batch(6, 1, 20), 7);
        s.flush();
        let st = s.stats(Kind::Embedding);
        assert_eq!(
            (st.entries, st.bytes),
            (5, 100),
            "clearing forgot the old figure; the cap holds again"
        );
        assert!(s.last_error().is_none());
    }

    /// Expired entries found by a batch read go in one delete job, and the batch answers them as misses.
    #[test]
    fn a_batch_read_drops_what_expired() {
        let d = tempfile::tempdir().unwrap();
        let s = open(d.path(), 10_000);
        s.put_many(Kind::Response, batch(1, 3, 4), 10);
        s.flush();
        let got = s.get_many(Kind::Response, &[[1; 32], [2; 32], [3; 32]], 5000);
        assert!(got.iter().all(Option::is_none), "created at 10, ttl 1000");
        s.flush();
        assert_eq!(s.stats(Kind::Response).entries, 0);
    }

    #[test]
    fn a_batch_sees_what_is_still_pending() {
        let d = tempfile::tempdir().unwrap();
        let s = open(d.path(), 10_000);
        s.put_many(Kind::Embedding, batch(1, 3, 4), 10);
        let got = s.get_many(Kind::Embedding, &[[1; 32], [3; 32], [7; 32]], 11);
        assert_eq!(got.iter().map(Option::is_some).collect::<Vec<_>>(), [true, true, false]);
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
        // No expiry at all. A fresh entry: the late read above may already have removed [2; 32], depending on
        // whether its store had been written when the read ran.
        s.configure(Kind::Response, lim(1000, None, false));
        s.put(Kind::Response, [3; 32], b"c".to_vec(), meta("a", "m"), 0);
        s.flush();
        assert!(s.get(Kind::Response, &[3; 32], u64::MAX / 4).is_some());
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
