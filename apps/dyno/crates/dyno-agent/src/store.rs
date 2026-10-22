//! The history database (SQLite, ADR 0008): points per tier, per-app minutes, disk free space, timeline events,
//! the activity log, alerts, snapshots, boots, app starts and crashes, and health scores.
//!
//! Tables added after v1 (all `IF NOT EXISTS`, so the schema number stays 1): `alarms` and `alarm_events`
//! (`store/alarms.rs`), `recorder_snapshots` (`store/recorder.rs`). Names reserved for later work, not created yet: `baselines`
//! (per-metric time-of-week learned statistics), `ssd_wear` and `battery_trend` (slow health trends).
//!
//! The database is a cache of the past, never the only copy of anything the user made: a damaged file is moved
//! aside and a fresh one created. Values are JSON text (small, debuggable, versioned by serde defaults).

use dyno_core::activity::{ActivityEntry, UndoRecord};
use dyno_core::alerts::Alert;
use dyno_core::forecast::DiskTrendPoint;
use dyno_core::history::{AppMinute, CoreBucket, HistoryPoint, Retention, Tier};
use dyno_core::protocol::{BootRecord, TimelineEvent, TimelineEventKind};
use dyno_core::snapshot::{Snapshot, SnapshotInfo};
use dyno_core::time::{Timestamp, DAY};
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::{Path, PathBuf};

mod alarms;
mod recorder;

/// The schema version.
pub const SCHEMA: i64 = 1;

/// A database error.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// SQLite failed.
    #[error("history database: {0}")]
    Sql(#[from] rusqlite::Error),
    /// A value didn't (de)serialise.
    #[error("history database value: {0}")]
    Json(#[from] serde_json::Error),
}

/// The store's result type.
pub type Result<T> = std::result::Result<T, StoreError>;

/// The open database.
pub struct Store {
    conn: Connection,
}

const SCHEMA_SQL: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS points (tier INTEGER NOT NULL, at INTEGER NOT NULL, data TEXT NOT NULL, PRIMARY KEY (tier, at)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS core_points (tier INTEGER NOT NULL, at INTEGER NOT NULL, data TEXT NOT NULL, PRIMARY KEY (tier, at)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS app_minutes (at INTEGER NOT NULL, app TEXT NOT NULL, cpu REAL NOT NULL, memory INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS app_minutes_at ON app_minutes (at);
CREATE TABLE IF NOT EXISTS disk_free (disk TEXT NOT NULL, at INTEGER NOT NULL, total INTEGER NOT NULL, free INTEGER NOT NULL, PRIMARY KEY (disk, at)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS events (at INTEGER NOT NULL, kind TEXT NOT NULL, label TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS events_at ON events (at);
CREATE TABLE IF NOT EXISTS activity (id TEXT PRIMARY KEY, at INTEGER NOT NULL, entry TEXT NOT NULL, undo TEXT NOT NULL, undone INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS activity_at ON activity (at);
CREATE TABLE IF NOT EXISTS alerts (id TEXT PRIMARY KEY, at INTEGER NOT NULL, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS snapshots (id TEXT PRIMARY KEY, at INTEGER NOT NULL, info TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS boots (at INTEGER PRIMARY KEY, boot_secs REAL, ready_secs REAL);
CREATE TABLE IF NOT EXISTS app_events (at INTEGER NOT NULL, app TEXT NOT NULL, kind TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS app_events_at ON app_events (at);
CREATE TABLE IF NOT EXISTS scores (at INTEGER PRIMARY KEY, score INTEGER NOT NULL);
";

fn tier_id(t: Tier) -> i64 {
    match t {
        Tier::Raw => 0,
        Tier::Minute => 1,
        Tier::QuarterHour => 2,
        Tier::Hour => 3,
    }
}

fn kind_text(k: TimelineEventKind) -> &'static str {
    match k {
        TimelineEventKind::Throttle => "throttle",
        TimelineEventKind::Cap => "cap",
        TimelineEventKind::Release => "release",
        TimelineEventKind::Cleanup => "cleanup",
        TimelineEventKind::Tuning => "tuning",
        TimelineEventKind::Alert => "alert",
        TimelineEventKind::Boot => "boot",
    }
}

fn kind_from(s: &str) -> Option<TimelineEventKind> {
    Some(match s {
        "throttle" => TimelineEventKind::Throttle,
        "cap" => TimelineEventKind::Cap,
        "release" => TimelineEventKind::Release,
        "cleanup" => TimelineEventKind::Cleanup,
        "tuning" => TimelineEventKind::Tuning,
        "alert" => TimelineEventKind::Alert,
        "boot" => TimelineEventKind::Boot,
        _ => return None,
    })
}

fn json<T: Serialize>(v: &T) -> Result<String> {
    Ok(serde_json::to_string(v)?)
}

fn parse<T: DeserializeOwned>(s: &str) -> Option<T> {
    serde_json::from_str(s).ok()
}

/// What an app did: started (after it had no process) or crashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEvent {
    /// Started.
    Start,
    /// Crashed.
    Crash,
}

impl Store {
    /// Opens (or creates) the database at `path`. A file that isn't a usable database is renamed to
    /// `<name>.damaged-<at>` and a fresh one created; the returned note says so.
    pub fn open(path: &Path, now: Timestamp) -> Result<(Store, Option<String>)> {
        match Self::try_open(path) {
            Ok(s) => Ok((s, None)),
            Err(e) => {
                let aside: PathBuf = path.with_extension(format!("damaged-{}", now.millis()));
                let _ = std::fs::rename(path, &aside);
                for ext in ["db-wal", "db-shm"] {
                    let _ = std::fs::remove_file(path.with_extension(ext));
                }
                let s = Self::try_open(path)?;
                Ok((s, Some(format!("The history database was damaged ({e}); Dyno started a new one."))))
            }
        }
    }

    /// An in-memory database (tests, and the fallback when the data folder is not writable).
    pub fn memory() -> Result<Store> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA_SQL)?;
        conn.execute_batch(alarms::SCHEMA_SQL)?;
        conn.execute_batch(recorder::SCHEMA_SQL)?;
        Ok(Store { conn })
    }

    fn try_open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_SQL)?;
        conn.execute_batch(alarms::SCHEMA_SQL)?;
        conn.execute_batch(recorder::SCHEMA_SQL)?;
        let version: Option<String> = conn
            .query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| r.get(0))
            .optional()?;
        match version.and_then(|v| v.parse::<i64>().ok()) {
            None => {
                conn.execute(
                    "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema', ?1)",
                    [SCHEMA.to_string()],
                )?;
            }
            Some(v) if v > SCHEMA => {
                return Err(StoreError::Sql(rusqlite::Error::InvalidParameterName(format!(
                    "written by a newer Dyno (schema {v})"
                ))))
            }
            Some(_) => {}
        }
        Ok(Store { conn })
    }

    /// Stores a point.
    pub fn put_point(&self, tier: Tier, p: &HistoryPoint) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO points (tier, at, data) VALUES (?1, ?2, ?3)",
            params![tier_id(tier), p.at.millis(), json(p)?],
        )?;
        Ok(())
    }

    /// Points of a tier in `[from, to)`, oldest first.
    pub fn points(&self, tier: Tier, from: Timestamp, to: Timestamp) -> Result<Vec<HistoryPoint>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT data FROM points WHERE tier = ?1 AND at >= ?2 AND at < ?3 ORDER BY at")?;
        let rows = st.query_map(params![tier_id(tier), from.millis(), to.millis()], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).filter_map(|s| parse(&s)).collect())
    }

    /// Stores a per-core bucket of a tier.
    pub fn put_core_point(&self, tier: Tier, p: &CoreBucket) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO core_points (tier, at, data) VALUES (?1, ?2, ?3)",
            params![tier_id(tier), p.at.millis(), json(&p.cores)?],
        )?;
        Ok(())
    }

    /// Per-core buckets of a tier in `[from, to)`, oldest first.
    pub fn core_points(&self, tier: Tier, from: Timestamp, to: Timestamp) -> Result<Vec<CoreBucket>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT at, data FROM core_points WHERE tier = ?1 AND at >= ?2 AND at < ?3 ORDER BY at")?;
        let rows = st.query_map(params![tier_id(tier), from.millis(), to.millis()], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        Ok(rows
            .filter_map(|r| r.ok())
            .filter_map(|(at, s)| parse(&s).map(|cores| CoreBucket { at: Timestamp(at), cores }))
            .collect())
    }

    /// Stores one minute's top apps.
    pub fn put_app_minutes(&mut self, rows: &[AppMinute]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut st = tx.prepare_cached("INSERT INTO app_minutes (at, app, cpu, memory) VALUES (?1, ?2, ?3, ?4)")?;
            for r in rows {
                st.execute(params![r.at.millis(), r.app, f64::from(r.cpu), r.memory as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Per-app minutes in `[from, to)`.
    pub fn app_minutes(&self, from: Timestamp, to: Timestamp) -> Result<Vec<AppMinute>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT at, app, cpu, memory FROM app_minutes WHERE at >= ?1 AND at < ?2 ORDER BY at")?;
        let rows = st.query_map(params![from.millis(), to.millis()], |r| {
            Ok(AppMinute {
                at: Timestamp(r.get(0)?),
                app: r.get(1)?,
                cpu: r.get::<_, f64>(2)? as f32,
                memory: r.get::<_, i64>(3)?.max(0) as u64,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Stores a disk's free space.
    pub fn put_disk_free(&self, disk: &str, at: Timestamp, total: u64, free: u64) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO disk_free (disk, at, total, free) VALUES (?1, ?2, ?3, ?4)",
            params![disk, at.millis(), total as i64, free as i64],
        )?;
        Ok(())
    }

    /// Free-space readings per disk in `[from, to)`: (disk, total, points).
    pub fn disk_free(&self, from: Timestamp, to: Timestamp) -> Result<Vec<(String, u64, Vec<DiskTrendPoint>)>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT disk, at, total, free FROM disk_free WHERE at >= ?1 AND at < ?2 ORDER BY disk, at")?;
        let rows = st.query_map(params![from.millis(), to.millis()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        let mut out: Vec<(String, u64, Vec<DiskTrendPoint>)> = Vec::new();
        for (disk, at, total, free) in rows.filter_map(|r| r.ok()) {
            let p = DiskTrendPoint {
                at: Timestamp(at),
                free: free.max(0) as u64,
            };
            match out.last_mut() {
                Some((d, t, pts)) if *d == disk => {
                    *t = total.max(0) as u64;
                    pts.push(p);
                }
                _ => out.push((disk, total.max(0) as u64, vec![p])),
            }
        }
        Ok(out)
    }

    /// Stores a timeline event.
    pub fn put_event(&self, e: &TimelineEvent) -> Result<()> {
        self.conn.execute(
            "INSERT INTO events (at, kind, label) VALUES (?1, ?2, ?3)",
            params![e.at.millis(), kind_text(e.kind), e.label],
        )?;
        Ok(())
    }

    /// Timeline events in `[from, to)` (at most 2 000, the latest).
    pub fn events(&self, from: Timestamp, to: Timestamp) -> Result<Vec<TimelineEvent>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT at, kind, label FROM events WHERE at >= ?1 AND at < ?2 ORDER BY at DESC LIMIT 2000")?;
        let rows = st.query_map(params![from.millis(), to.millis()], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        let mut out: Vec<TimelineEvent> = rows
            .filter_map(|r| r.ok())
            .filter_map(|(at, k, label)| {
                Some(TimelineEvent {
                    at: Timestamp(at),
                    kind: kind_from(&k)?,
                    label,
                })
            })
            .collect();
        out.reverse();
        Ok(out)
    }

    /// Stores an activity entry with its undo record.
    pub fn put_activity(&self, e: &ActivityEntry, undo: &UndoRecord) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO activity (id, at, entry, undo, undone) VALUES (?1, ?2, ?3, ?4, 0)",
            params![e.id, e.at.millis(), json(e)?, json(undo)?],
        )?;
        Ok(())
    }

    /// An entry with its undo record and whether it was undone.
    pub fn activity_entry(&self, id: &str) -> Result<Option<(ActivityEntry, UndoRecord, bool)>> {
        let row: Option<(String, String, i64)> = self
            .conn
            .query_row("SELECT entry, undo, undone FROM activity WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        Ok(row.and_then(|(e, u, d)| Some((parse(&e)?, parse(&u).unwrap_or(UndoRecord::None), d != 0))))
    }

    /// Marks an entry undone.
    pub fn mark_undone(&self, id: &str) -> Result<()> {
        self.conn.execute("UPDATE activity SET undone = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    /// The latest entries, newest first, with their undo records and undone flags.
    pub fn activity(&self, limit: u32) -> Result<Vec<(ActivityEntry, UndoRecord, bool)>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT entry, undo, undone FROM activity ORDER BY at DESC, rowid DESC LIMIT ?1")?;
        let rows = st.query_map([i64::from(limit.min(1_000))], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
        })?;
        Ok(rows
            .filter_map(|r| r.ok())
            .filter_map(|(e, u, d)| Some((parse(&e)?, parse(&u).unwrap_or(UndoRecord::None), d != 0)))
            .collect())
    }

    /// Activity entries of a kind since `from` (for counts in reports).
    pub fn count_activity(&self, kind: &str, from: Timestamp) -> Result<u32> {
        let pattern = format!("%\"kind\":\"{kind}\"%");
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM activity WHERE at >= ?1 AND entry LIKE ?2",
            params![from.millis(), pattern],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as u32)
    }

    /// Bytes freed by cleanups since `from`.
    pub fn cleaned_bytes(&self, from: Timestamp) -> Result<u64> {
        Ok(self
            .activity(1_000)?
            .into_iter()
            .filter(|(e, _, _)| e.at >= from && e.kind == dyno_core::activity::ActivityKind::Cleanup)
            .filter_map(|(e, _, _)| e.bytes)
            .sum())
    }

    /// Stores an alert.
    pub fn put_alert(&self, a: &Alert) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO alerts (id, at, data) VALUES (?1, ?2, ?3)",
            params![a.id, a.at.millis(), json(a)?],
        )?;
        Ok(())
    }

    /// The latest alerts, newest first.
    pub fn alerts(&self, limit: u32) -> Result<Vec<Alert>> {
        let mut st = self.conn.prepare_cached("SELECT data FROM alerts ORDER BY at DESC LIMIT ?1")?;
        let rows = st.query_map([i64::from(limit.min(1_000))], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).filter_map(|s| parse(&s)).collect())
    }

    /// Alerts since `from`.
    pub fn count_alerts(&self, from: Timestamp) -> Result<u32> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM alerts WHERE at >= ?1", [from.millis()], |r| r.get(0))?;
        Ok(n.max(0) as u32)
    }

    /// Stores a snapshot (keeping the 50 latest).
    pub fn put_snapshot(&self, s: &Snapshot) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO snapshots (id, at, info, data) VALUES (?1, ?2, ?3, ?4)",
            params![s.info.id, s.info.at.millis(), json(&s.info)?, json(s)?],
        )?;
        self.conn.execute(
            "DELETE FROM snapshots WHERE id NOT IN (SELECT id FROM snapshots ORDER BY at DESC LIMIT 50)",
            [],
        )?;
        Ok(())
    }

    /// Snapshot headers, newest first.
    pub fn snapshots(&self) -> Result<Vec<SnapshotInfo>> {
        let mut st = self.conn.prepare_cached("SELECT info FROM snapshots ORDER BY at DESC")?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).filter_map(|s| parse(&s)).collect())
    }

    /// One snapshot.
    pub fn snapshot(&self, id: &str) -> Result<Option<Snapshot>> {
        let s: Option<String> = self
            .conn
            .query_row("SELECT data FROM snapshots WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        Ok(s.and_then(|s| parse(&s)))
    }

    /// Records a boot (once per boot time).
    pub fn put_boot(&self, b: &BootRecord) -> Result<bool> {
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO boots (at, boot_secs, ready_secs) VALUES (?1, ?2, ?3)",
            params![b.at.millis(), b.boot_secs.map(f64::from), b.ready_secs.map(f64::from)],
        )?;
        Ok(n > 0)
    }

    /// Updates a boot's ready time.
    pub fn set_boot_ready(&self, at: Timestamp, ready_secs: f32) -> Result<()> {
        self.conn.execute(
            "UPDATE boots SET ready_secs = ?2 WHERE at = ?1",
            params![at.millis(), f64::from(ready_secs)],
        )?;
        Ok(())
    }

    /// Boots in `[from, to)`.
    pub fn boots(&self, from: Timestamp, to: Timestamp) -> Result<Vec<BootRecord>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT at, boot_secs, ready_secs FROM boots WHERE at >= ?1 AND at < ?2 ORDER BY at")?;
        let rows = st.query_map(params![from.millis(), to.millis()], |r| {
            Ok(BootRecord {
                at: Timestamp(r.get(0)?),
                boot_secs: r.get::<_, Option<f64>>(1)?.map(|v| v as f32),
                ready_secs: r.get::<_, Option<f64>>(2)?.map(|v| v as f32),
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Records an app start or crash.
    pub fn put_app_event(&self, at: Timestamp, app: &str, kind: AppEvent) -> Result<()> {
        let k = match kind {
            AppEvent::Start => "start",
            AppEvent::Crash => "crash",
        };
        self.conn.execute(
            "INSERT INTO app_events (at, app, kind) VALUES (?1, ?2, ?3)",
            params![at.millis(), app, k],
        )?;
        Ok(())
    }

    /// App events in `[from, to)`: (app, when, kind).
    pub fn app_events(&self, from: Timestamp, to: Timestamp) -> Result<Vec<(String, Timestamp, AppEvent)>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT app, at, kind FROM app_events WHERE at >= ?1 AND at < ?2 ORDER BY at")?;
        let rows = st.query_map(params![from.millis(), to.millis()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?))
        })?;
        Ok(rows
            .filter_map(|r| r.ok())
            .map(|(a, at, k)| (a, Timestamp(at), if k == "crash" { AppEvent::Crash } else { AppEvent::Start }))
            .collect())
    }

    /// Records the health score (one per hour is plenty).
    pub fn put_score(&self, at: Timestamp, score: u32) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO scores (at, score) VALUES (?1, ?2)",
            params![at.floor_to(3_600_000).millis(), score],
        )?;
        Ok(())
    }

    /// The last score recorded at or before `at`.
    pub fn score_at(&self, at: Timestamp) -> Result<Option<u32>> {
        Ok(self
            .conn
            .query_row(
                "SELECT score FROM scores WHERE at <= ?1 ORDER BY at DESC LIMIT 1",
                [at.millis()],
                |r| r.get::<_, u32>(0),
            )
            .optional()?)
    }

    /// The scores recorded in `[from, to)`, oldest first.
    pub fn scores(&self, from: Timestamp, to: Timestamp) -> Result<Vec<(Timestamp, u32)>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT at, score FROM scores WHERE at >= ?1 AND at < ?2 ORDER BY at")?;
        let rows = st.query_map(params![from.millis(), to.millis()], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, u32>(1)?))
        })?;
        Ok(rows.filter_map(|r| r.ok()).map(|(at, s)| (Timestamp(at), s)).collect())
    }

    /// A small value by key.
    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    /// Stores a small value.
    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute("INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)", params![key, value])?;
        Ok(())
    }

    /// Deletes everything older than the retention allows.
    pub fn prune(&self, r: Retention, now: Timestamp) -> Result<()> {
        for t in Tier::ALL {
            self.conn.execute(
                "DELETE FROM points WHERE tier = ?1 AND at < ?2",
                params![tier_id(t), r.cutoff(t, now).millis()],
            )?;
            self.conn.execute(
                "DELETE FROM core_points WHERE tier = ?1 AND at < ?2",
                params![tier_id(t), r.cutoff(t, now).millis()],
            )?;
        }
        let long = r.cutoff(Tier::Hour, now).millis();
        let minute = r.cutoff(Tier::Minute, now).millis();
        self.conn.execute("DELETE FROM app_minutes WHERE at < ?1", [minute])?;
        for table in ["disk_free", "events", "boots", "app_events", "scores", "alerts"] {
            self.conn.execute(&format!("DELETE FROM {table} WHERE at < ?1"), [long])?;
        }
        self.conn.execute(
            "DELETE FROM activity WHERE id NOT IN (SELECT id FROM activity ORDER BY at DESC LIMIT ?1)",
            [dyno_core::activity::MAX_ENTRIES as i64],
        )?;
        self.conn
            .execute("DELETE FROM activity WHERE at < ?1", [now.minus(400 * DAY).millis()])?;
        self.prune_alarm_events(now)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::activity::{ActivityKind, UndoState};
    use dyno_core::time::{HOUR, MINUTE};

    fn point(at: i64) -> HistoryPoint {
        HistoryPoint {
            at: Timestamp(at),
            cpu: 12.0,
            ..Default::default()
        }
    }

    fn entry(id: &str, at: i64, kind: ActivityKind, bytes: Option<u64>) -> ActivityEntry {
        ActivityEntry {
            id: id.into(),
            at: Timestamp(at),
            kind,
            title: "t".into(),
            detail: "d".into(),
            undo: UndoState::NotUndoable,
            bytes,
        }
    }

    #[test]
    fn points_events_and_pruning() {
        let s = Store::memory().unwrap();
        for i in 0..10 {
            s.put_point(Tier::Raw, &point(i * 5_000)).unwrap();
        }
        s.put_point(Tier::Minute, &point(0)).unwrap();
        assert_eq!(s.points(Tier::Raw, Timestamp(10_000), Timestamp(30_000)).unwrap().len(), 4);
        assert_eq!(s.points(Tier::Minute, Timestamp(0), Timestamp(1)).unwrap().len(), 1);
        let e = TimelineEvent {
            at: Timestamp(5),
            kind: TimelineEventKind::Cap,
            label: "Capped Chrome".into(),
        };
        s.put_event(&e).unwrap();
        assert_eq!(s.events(Timestamp(0), Timestamp(10)).unwrap(), vec![e]);
        for k in ["throttle", "cap", "release", "cleanup", "tuning", "alert", "boot"] {
            assert_eq!(kind_from(k).map(kind_text), Some(k));
        }
        assert!(kind_from("x").is_none());
        let now = Timestamp(30 * DAY);
        s.put_point(Tier::Raw, &point(now.minus(HOUR).millis())).unwrap();
        s.prune(Retention::default(), now).unwrap();
        assert_eq!(s.points(Tier::Raw, Timestamp(0), now).unwrap().len(), 1);
    }

    #[test]
    fn app_minutes_disks_boots_and_starts() {
        let mut s = Store::memory().unwrap();
        s.put_app_minutes(&[AppMinute {
            at: Timestamp(MINUTE),
            app: "Chrome".into(),
            cpu: 3.5,
            memory: 100,
        }])
        .unwrap();
        assert_eq!(s.app_minutes(Timestamp(0), Timestamp(2 * MINUTE)).unwrap()[0].app, "Chrome");
        s.put_disk_free("C:", Timestamp(1), 100, 50).unwrap();
        s.put_disk_free("C:", Timestamp(2), 100, 40).unwrap();
        s.put_disk_free("D:", Timestamp(2), 200, 10).unwrap();
        let d = s.disk_free(Timestamp(0), Timestamp(10)).unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].2.len(), 2);
        assert_eq!(d[1].1, 200);
        let b = BootRecord {
            at: Timestamp(100),
            boot_secs: Some(12.0),
            ready_secs: None,
        };
        assert!(s.put_boot(&b).unwrap());
        assert!(!s.put_boot(&b).unwrap(), "once per boot");
        s.set_boot_ready(Timestamp(100), 30.0).unwrap();
        assert_eq!(s.boots(Timestamp(0), Timestamp(200)).unwrap()[0].ready_secs, Some(30.0));
        s.put_app_event(Timestamp(5), "X", AppEvent::Start).unwrap();
        s.put_app_event(Timestamp(6), "X", AppEvent::Crash).unwrap();
        let ev = s.app_events(Timestamp(0), Timestamp(10)).unwrap();
        assert_eq!(ev[1].2, AppEvent::Crash);
        s.put_score(Timestamp(HOUR + 5), 80).unwrap();
        assert_eq!(s.score_at(Timestamp(2 * HOUR)).unwrap(), Some(80));
        assert_eq!(s.score_at(Timestamp(0)).unwrap(), None);
        s.set_meta("k", "v").unwrap();
        assert_eq!(s.meta("k").unwrap().as_deref(), Some("v"));
    }

    #[test]
    fn activity_alerts_and_snapshots() {
        let s = Store::memory().unwrap();
        let undo = UndoRecord::Tuning { id: "powerPlan".into() };
        s.put_activity(&entry("a", 1, ActivityKind::Tuning, None), &undo).unwrap();
        s.put_activity(&entry("b", 2, ActivityKind::Cleanup, Some(500)), &UndoRecord::None)
            .unwrap();
        let all = s.activity(10).unwrap();
        assert_eq!(all[0].0.id, "b");
        assert_eq!(s.activity_entry("a").unwrap().unwrap().1, undo);
        s.mark_undone("a").unwrap();
        assert!(s.activity_entry("a").unwrap().unwrap().2);
        assert!(s.activity_entry("zzz").unwrap().is_none());
        assert_eq!(s.count_activity("cleanup", Timestamp(0)).unwrap(), 1);
        assert_eq!(s.cleaned_bytes(Timestamp(0)).unwrap(), 500);
        let alert = Alert {
            id: "x".into(),
            at: Timestamp(3),
            kind: dyno_core::alerts::AlertKind::LowDisk,
            severity: dyno_core::health::Severity::Warning,
            title: "t".into(),
            detail: "d".into(),
        };
        s.put_alert(&alert).unwrap();
        assert_eq!(s.alerts(5).unwrap(), vec![alert]);
        assert_eq!(s.count_alerts(Timestamp(0)).unwrap(), 1);
        let snap = dyno_core::snapshot::take("s1".into(), Timestamp(9), "x", 1, 2.0, vec![]);
        s.put_snapshot(&snap).unwrap();
        assert_eq!(s.snapshots().unwrap()[0].id, "s1");
        assert_eq!(s.snapshot("s1").unwrap().unwrap(), snap);
        assert!(s.snapshot("s2").unwrap().is_none());
    }

    #[test]
    fn damaged_files_are_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        std::fs::write(&path, b"this is not a database at all, not even close........................").unwrap();
        let (s, note) = Store::open(&path, Timestamp(7)).unwrap();
        assert!(note.unwrap().contains("damaged"));
        s.put_point(Tier::Raw, &point(0)).unwrap();
        assert!(dir.path().join("history.damaged-7").exists());
        drop(s);
        let (_, note) = Store::open(&path, Timestamp(8)).unwrap();
        assert!(note.is_none(), "a good file opens as is");
        // A database from a newer Dyno is set aside too.
        {
            let c = Connection::open(&path).unwrap();
            c.execute("UPDATE meta SET value = '99' WHERE key = 'schema'", []).unwrap();
        }
        let (_, note) = Store::open(&path, Timestamp(9)).unwrap();
        assert!(note.unwrap().contains("newer"));
    }

    #[test]
    fn scores_are_listed_oldest_first_within_the_range() {
        let s = Store::memory().unwrap();
        for (h, score) in [(1, 60), (2, 70), (5, 90)] {
            s.put_score(Timestamp(h * HOUR), score).unwrap();
        }
        assert_eq!(
            s.scores(Timestamp(HOUR), Timestamp(5 * HOUR)).unwrap(),
            vec![(Timestamp(HOUR), 60), (Timestamp(2 * HOUR), 70)]
        );
        assert!(s.scores(Timestamp(10 * HOUR), Timestamp(11 * HOUR)).unwrap().is_empty());
    }
}
