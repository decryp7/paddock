//! The usage index: a SQLite file (WAL, busy timeout) with the tracked folders and the history of their sizes, as cargo keeps its
//! global cache tracker (`cargo clean gc`). It is a cache: the owner records are the source of truth, so a missing or corrupt
//! index is set aside and rebuilt by the next `scan`, and nothing fails because of it.

use rusqlite::{Connection, params};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::Duration;

/// How long a writer waits for another one.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(15);
/// Size history older than this is dropped.
pub const HISTORY_KEEP_SECS: u64 = 90 * 86_400;
const SCHEMA_VERSION: i64 = 1;

/// One folder as the index stores it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FolderRow {
    /// The folder.
    pub path: String,
    /// `worktree`, `build`, `slot`, `scratch`, or empty for a folder without a record.
    pub kind: String,
    /// The record's owner (empty for an orphan).
    pub owner: String,
    /// The record's branch.
    pub branch: String,
    /// The record's head.
    pub head: String,
    /// Seconds since the epoch.
    pub created: u64,
    /// Seconds since the epoch (the later of the record's and the folder's own time).
    pub last_used: u64,
    /// `live`, `merged`, `stale-pushed`, `unsaved` or `orphan`.
    pub state: String,
    /// The record's repository.
    pub repo: String,
    /// The record's linked worktree.
    pub link: String,
}

/// One measurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeRow {
    /// The folder.
    pub path: String,
    /// Bytes on disk.
    pub bytes: u64,
    /// The measurement stopped at its time limit: the real size is larger.
    pub partial: bool,
    /// Seconds since the epoch.
    pub measured_at: u64,
}

/// An open index.
pub struct Index {
    conn: Connection,
}

fn e(what: &str) -> impl Fn(rusqlite::Error) -> String + '_ {
    move |err| format!("{what}: {err}")
}

/// Why an index could not be opened: its file is damaged (set aside and rebuilt), or something else is wrong now (another
/// writer holds it, no permission, a full disk: the file may be fine, so it is never moved).
#[derive(Debug)]
pub enum OpenError {
    /// Not a database, corrupt, or a schema this tool does not read.
    Damaged(String),
    /// Anything else.
    Other(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Damaged(s) | OpenError::Other(s) => f.write_str(s),
        }
    }
}

fn damaged(err: &rusqlite::Error) -> bool {
    matches!(
        err.sqlite_error_code(),
        Some(rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt)
    )
}

fn classify(what: &str) -> impl Fn(rusqlite::Error) -> OpenError + '_ {
    move |err| {
        let text = format!("{what}: {err}");
        if damaged(&err) {
            OpenError::Damaged(text)
        } else {
            OpenError::Other(text)
        }
    }
}

impl Index {
    /// Opens (creating it) the index at `path`.
    pub fn open(path: &Path) -> Result<Index, String> {
        Index::open_timeout(path, BUSY_TIMEOUT).map_err(|e| e.to_string())
    }

    /// Opens the index read-only: nothing is created, no pragma that writes is run, no schema is made. For readers that must
    /// not change the file (`disk check` at a job's start, `clean`'s size lookup).
    pub fn open_read_only(path: &Path) -> Result<Index, String> {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(e("cannot open the index read-only"))?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(e("busy timeout"))?;
        Ok(Index { conn })
    }

    /// [`Index::open`] with the busy timeout given.
    pub fn open_timeout(path: &Path, timeout: Duration) -> Result<Index, OpenError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|err| OpenError::Other(format!("cannot create {}: {err}", parent.display())))?;
        }
        if path.is_dir() {
            return Err(OpenError::Damaged(format!("{} is a folder, not a database file", path.display())));
        }
        let conn = Connection::open(path).map_err(classify("cannot open the index"))?;
        conn.busy_timeout(timeout).map_err(classify("busy timeout"))?;
        // The first real read: a file that is not a database fails here.
        let mode: String = conn
            .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
            .map_err(classify("not a usable index"))?;
        if !mode.eq_ignore_ascii_case("wal") && !mode.eq_ignore_ascii_case("memory") {
            return Err(OpenError::Other(format!("the index could not use WAL (journal mode {mode})")));
        }
        conn.pragma_update(None, "synchronous", "NORMAL").map_err(classify("pragma"))?;
        let check: String = conn
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(classify("integrity check"))?;
        if check != "ok" {
            return Err(OpenError::Damaged(format!("integrity check says {check}")));
        }
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(classify("version"))?;
        if version != 0 && version != SCHEMA_VERSION {
            return Err(OpenError::Damaged(format!(
                "schema version {version}, this tool reads {SCHEMA_VERSION}"
            )));
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS folders (
                path TEXT PRIMARY KEY, kind TEXT NOT NULL, owner TEXT NOT NULL, branch TEXT NOT NULL, head TEXT NOT NULL,
                created INTEGER NOT NULL, last_used INTEGER NOT NULL, state TEXT NOT NULL, repo TEXT NOT NULL, link TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS sizes (
                path TEXT NOT NULL, bytes INTEGER NOT NULL, partial INTEGER NOT NULL, measured_at INTEGER NOT NULL,
                PRIMARY KEY (path, measured_at));",
        )
        .map_err(classify("schema"))?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(classify("version"))?;
        Ok(Index { conn })
    }

    /// Opens the index; a file that is damaged (not a database, corrupt, another schema) is renamed to `<name>.corrupt-<time>` and
    /// a new one is made. Any other failure (busy, no permission) leaves the file where it is and gives `None`: the caller works
    /// without an index. Both come with the reasons to print.
    pub fn open_or_recover(path: &Path, now: u64) -> (Option<Index>, Vec<String>) {
        Index::open_or_recover_timeout(path, now, BUSY_TIMEOUT)
    }

    /// [`Index::open_or_recover`] with the busy timeout given.
    pub fn open_or_recover_timeout(path: &Path, now: u64, timeout: Duration) -> (Option<Index>, Vec<String>) {
        let mut notes = Vec::new();
        match Index::open_timeout(path, timeout) {
            Ok(i) => return (Some(i), notes),
            Err(OpenError::Other(why)) => {
                notes.push(format!(
                    "index {} cannot be used right now ({why}); working without it, the file is left as it is",
                    path.display()
                ));
                return (None, notes);
            }
            Err(OpenError::Damaged(first)) => {
                notes.push(format!("index {} is unusable ({first})", path.display()));
                if path.exists() {
                    let mut aside = path.as_os_str().to_owned();
                    aside.push(format!(".corrupt-{now}"));
                    match std::fs::rename(path, &aside) {
                        Ok(()) => {
                            // Its WAL files belong to the file just moved.
                            for ext in ["-wal", "-shm"] {
                                let mut side = path.as_os_str().to_owned();
                                side.push(ext);
                                let _ = std::fs::remove_file(side);
                            }
                            notes.push(format!("set it aside as {}", std::path::Path::new(&aside).display()));
                        }
                        Err(err) => {
                            notes.push(format!("could not move it aside ({err}); working without the index"));
                            return (None, notes);
                        }
                    }
                }
            }
        }
        match Index::open_timeout(path, timeout) {
            Ok(i) => (Some(i), notes),
            Err(second) => {
                notes.push(format!("a new index could not be made either ({second}); working without it"));
                (None, notes)
            }
        }
    }

    /// Makes the stored folders exactly `rows`: updates, inserts and removes the ones that are gone (their size history stays,
    /// until it ages out).
    pub fn sync_folders(&mut self, rows: &[FolderRow]) -> Result<(), String> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(e("begin"))?;
        tx.execute("CREATE TEMP TABLE IF NOT EXISTS seen (path TEXT PRIMARY KEY)", [])
            .map_err(e("temp table"))?;
        tx.execute("DELETE FROM seen", []).map_err(e("temp table"))?;
        {
            let mut up = tx
                .prepare(
                    "INSERT INTO folders (path, kind, owner, branch, head, created, last_used, state, repo, link)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                     ON CONFLICT(path) DO UPDATE SET kind = ?2, owner = ?3, branch = ?4, head = ?5, created = ?6,
                       last_used = ?7, state = ?8, repo = ?9, link = ?10",
                )
                .map_err(e("prepare"))?;
            let mut seen = tx.prepare("INSERT OR IGNORE INTO seen (path) VALUES (?1)").map_err(e("prepare"))?;
            for r in rows {
                up.execute(params![
                    r.path,
                    r.kind,
                    r.owner,
                    r.branch,
                    r.head,
                    r.created as i64,
                    r.last_used as i64,
                    r.state,
                    r.repo,
                    r.link
                ])
                .map_err(e("write folder"))?;
                seen.execute(params![r.path]).map_err(e("write folder"))?;
            }
        }
        tx.execute("DELETE FROM folders WHERE path NOT IN (SELECT path FROM seen)", [])
            .map_err(e("remove gone folders"))?;
        tx.commit().map_err(e("commit"))
    }

    /// Appends measurements and drops history older than [`HISTORY_KEEP_SECS`].
    pub fn record_sizes(&mut self, rows: &[SizeRow], now: u64) -> Result<(), String> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(e("begin"))?;
        {
            let mut ins = tx
                .prepare("INSERT OR REPLACE INTO sizes (path, bytes, partial, measured_at) VALUES (?1, ?2, ?3, ?4)")
                .map_err(e("prepare"))?;
            for r in rows {
                ins.execute(params![r.path, r.bytes as i64, i64::from(r.partial), r.measured_at as i64])
                    .map_err(e("write size"))?;
            }
        }
        tx.execute(
            "DELETE FROM sizes WHERE measured_at < ?1",
            params![now.saturating_sub(HISTORY_KEEP_SECS) as i64],
        )
        .map_err(e("prune"))?;
        tx.commit().map_err(e("commit"))
    }

    /// Every stored folder.
    pub fn folders(&self) -> Result<Vec<FolderRow>, String> {
        let mut st = self
            .conn
            .prepare("SELECT path, kind, owner, branch, head, created, last_used, state, repo, link FROM folders ORDER BY path")
            .map_err(e("read folders"))?;
        let rows = st
            .query_map([], |r| {
                Ok(FolderRow {
                    path: r.get(0)?,
                    kind: r.get(1)?,
                    owner: r.get(2)?,
                    branch: r.get(3)?,
                    head: r.get(4)?,
                    created: r.get::<_, i64>(5)?.max(0) as u64,
                    last_used: r.get::<_, i64>(6)?.max(0) as u64,
                    state: r.get(7)?,
                    repo: r.get(8)?,
                    link: r.get(9)?,
                })
            })
            .map_err(e("read folders"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e("read folders"))?;
        Ok(rows)
    }

    /// The newest measurement of every folder.
    pub fn latest_sizes(&self) -> Result<HashMap<String, SizeRow>, String> {
        let mut st = self
            .conn
            .prepare(
                "SELECT s.path, s.bytes, s.partial, s.measured_at FROM sizes s
                 JOIN (SELECT path, MAX(measured_at) AS m FROM sizes GROUP BY path) l ON l.path = s.path AND l.m = s.measured_at",
            )
            .map_err(e("read sizes"))?;
        let rows = st
            .query_map([], |r| {
                Ok(SizeRow {
                    path: r.get(0)?,
                    bytes: r.get::<_, i64>(1)?.max(0) as u64,
                    partial: r.get::<_, i64>(2)? != 0,
                    measured_at: r.get::<_, i64>(3)?.max(0) as u64,
                })
            })
            .map_err(e("read sizes"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e("read sizes"))?;
        Ok(rows.into_iter().map(|r| (r.path.clone(), r)).collect())
    }

    /// Every measurement, oldest first per folder.
    pub fn history(&self) -> Result<BTreeMap<String, Vec<SizeRow>>, String> {
        let mut st = self
            .conn
            .prepare("SELECT path, bytes, partial, measured_at FROM sizes ORDER BY path, measured_at")
            .map_err(e("read history"))?;
        let rows = st
            .query_map([], |r| {
                Ok(SizeRow {
                    path: r.get(0)?,
                    bytes: r.get::<_, i64>(1)?.max(0) as u64,
                    partial: r.get::<_, i64>(2)? != 0,
                    measured_at: r.get::<_, i64>(3)?.max(0) as u64,
                })
            })
            .map_err(e("read history"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e("read history"))?;
        let mut out: BTreeMap<String, Vec<SizeRow>> = BTreeMap::new();
        for r in rows {
            out.entry(r.path.clone()).or_default().push(r);
        }
        Ok(out)
    }
}

/// Growth of a folder over a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Growth {
    /// The folder.
    pub path: String,
    /// Its newest size.
    pub now_bytes: u64,
    /// Newest minus the size at the start of the window (positive only).
    pub grew_bytes: u64,
}

/// The folders that grew most over the last `window_secs`, biggest growth first. The baseline is the newest measurement at or
/// before the window's start, else the oldest one inside it; a folder measured once has no growth to tell.
pub fn growth(history: &BTreeMap<String, Vec<SizeRow>>, now: u64, window_secs: u64, top: usize) -> Vec<Growth> {
    let start = now.saturating_sub(window_secs);
    let mut out: Vec<Growth> = history
        .iter()
        .filter_map(|(path, rows)| {
            let latest = rows.last()?;
            let base = rows.iter().rev().find(|r| r.measured_at <= start).or_else(|| rows.first())?;
            if base.measured_at == latest.measured_at {
                return None;
            }
            (latest.bytes > base.bytes).then(|| Growth {
                path: path.clone(),
                now_bytes: latest.bytes,
                grew_bytes: latest.bytes - base.bytes,
            })
        })
        .collect();
    out.sort_by(|a, b| b.grew_bytes.cmp(&a.grew_bytes).then_with(|| a.path.cmp(&b.path)));
    out.truncate(top);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(path: &str, state: &str) -> FolderRow {
        FolderRow {
            path: path.into(),
            kind: "worktree".into(),
            owner: "implementer:x".into(),
            state: state.into(),
            created: 10,
            last_used: 20,
            ..FolderRow::default()
        }
    }

    #[test]
    fn folders_and_sizes_round_trip_and_gone_folders_leave() {
        let dir = tempfile::tempdir().unwrap();
        let mut i = Index::open(&dir.path().join("i.sqlite")).unwrap();
        i.sync_folders(&[folder("/a", "live"), folder("/b", "merged")]).unwrap();
        i.sync_folders(&[folder("/b", "stale-pushed")]).unwrap();
        let rows = i.folders().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, "stale-pushed");
        i.record_sizes(
            &[
                SizeRow {
                    path: "/b".into(),
                    bytes: 5,
                    partial: false,
                    measured_at: 100,
                },
                SizeRow {
                    path: "/b".into(),
                    bytes: 9,
                    partial: true,
                    measured_at: 200,
                },
            ],
            200,
        )
        .unwrap();
        let latest = i.latest_sizes().unwrap();
        assert_eq!((latest["/b"].bytes, latest["/b"].partial), (9, true));
        assert_eq!(i.history().unwrap()["/b"].len(), 2, "history is kept");
    }

    #[test]
    fn history_older_than_ninety_days_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let mut i = Index::open(&dir.path().join("i.sqlite")).unwrap();
        let now = 200 * 86_400;
        i.record_sizes(
            &[SizeRow {
                path: "/a".into(),
                bytes: 1,
                partial: false,
                measured_at: 1,
            }],
            now,
        )
        .unwrap();
        assert!(i.history().unwrap().is_empty());
    }

    #[test]
    fn a_file_that_is_not_a_database_is_set_aside_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("i.sqlite");
        std::fs::write(
            &path,
            b"this is not sqlite, it is a long enough line of text to look like a damaged header",
        )
        .unwrap();
        let (index, notes) = Index::open_or_recover(&path, 42);
        let mut index = index.expect("a new index is made");
        assert!(notes.iter().any(|n| n.contains("unusable")), "{notes:?}");
        assert!(
            dir.path().join("i.sqlite.corrupt-42").exists(),
            "the damaged file is kept for a look"
        );
        index.sync_folders(&[folder("/a", "live")]).unwrap();
        assert_eq!(index.folders().unwrap().len(), 1);
    }

    #[test]
    fn a_folder_where_the_file_should_be_is_set_aside_too() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("i.sqlite");
        std::fs::create_dir(&path).unwrap();
        let (index, notes) = Index::open_or_recover(&path, 1);
        assert!(index.is_some(), "{notes:?}");
        assert!(dir.path().join("i.sqlite.corrupt-1").is_dir());
    }

    #[test]
    fn two_writers_at_once_both_get_their_rows_in() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("i.sqlite");
        Index::open(&path).unwrap();
        let handles: Vec<_> = (0..2)
            .map(|w| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let mut i = Index::open(&path).unwrap();
                    for n in 0..40 {
                        i.record_sizes(
                            &[SizeRow {
                                path: format!("/w{w}/{n}"),
                                bytes: n,
                                partial: false,
                                measured_at: 1_000_000_000 + n,
                            }],
                            1_000_000_000 + n,
                        )
                        .unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(Index::open(&path).unwrap().latest_sizes().unwrap().len(), 80);
    }

    #[test]
    fn growth_compares_with_the_start_of_the_window_and_skips_single_measurements() {
        let day = 86_400;
        let now = 40 * day;
        let m = |path: &str, bytes, days_ago: u64| SizeRow {
            path: path.into(),
            bytes,
            partial: false,
            measured_at: now - days_ago * day,
        };
        let mut h = BTreeMap::new();
        h.insert(
            "/grew".to_string(),
            vec![m("/grew", 100, 30), m("/grew", 150, 8), m("/grew", 400, 0)],
        );
        h.insert("/small".to_string(), vec![m("/small", 10, 3), m("/small", 15, 0)]);
        h.insert("/once".to_string(), vec![m("/once", 10, 0)]);
        h.insert("/shrank".to_string(), vec![m("/shrank", 100, 9), m("/shrank", 50, 0)]);
        let g = growth(&h, now, 7 * day, 10);
        assert_eq!(g.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(), ["/grew", "/small"]);
        assert_eq!(g[0].grew_bytes, 250, "against the measurement at the window's start (8 days ago)");
        assert_eq!(g[1].grew_bytes, 5, "no older one: against the oldest inside the window");
    }

    #[test]
    fn a_busy_index_is_left_where_it_is_and_the_caller_works_without_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("i.sqlite");
        let mut good = Index::open(&path).unwrap();
        good.sync_folders(&[folder("/a", "live")]).unwrap();
        // Another process holds the file exclusively.
        let holder = Connection::open(&path).unwrap();
        holder.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let (index, notes) = Index::open_or_recover_timeout(&path, 9, Duration::from_millis(50));
        assert!(index.is_none(), "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("left as it is")), "{notes:?}");
        assert!(path.exists(), "a busy index is not a damaged one");
        assert!(!dir.path().join("i.sqlite.corrupt-9").exists());
        holder.execute_batch("ROLLBACK").unwrap();
        assert_eq!(Index::open(&path).unwrap().folders().unwrap().len(), 1, "its rows are still there");
    }

    #[test]
    fn a_read_only_open_changes_nothing_and_makes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("i.sqlite");
        assert!(Index::open_read_only(&path).is_err());
        assert!(!path.exists(), "a read-only open never creates the file");
        let mut i = Index::open(&path).unwrap();
        i.record_sizes(
            &[SizeRow {
                path: "/a".into(),
                bytes: 3,
                partial: false,
                measured_at: 1_000_000_000,
            }],
            1_000_000_000,
        )
        .unwrap();
        drop(i);
        let before = std::fs::read(&path).unwrap();
        let ro = Index::open_read_only(&path).unwrap();
        assert_eq!(ro.latest_sizes().unwrap()["/a"].bytes, 3);
        drop(ro);
        assert_eq!(std::fs::read(&path).unwrap(), before, "the database file is untouched");
        // A write attempt through the read-only handle fails (SQLite may still make its -shm/-wal side files to read a WAL file).
        let ro = Index::open_read_only(&path).unwrap();
        assert!(ro.conn.execute("INSERT INTO folders (path, kind, owner, branch, head, created, last_used, state, repo, link) VALUES ('x','','','','',0,0,'','','')", []).is_err());
    }
}
