//! The issue and baseline tables: `issues` (what the OS logs said, one row per event, deduplicated by id), `baseline`
//! (the programs seen on this machine) and `baseline_startup` (startup entries seen). Added next to the v1 tables with
//! `IF NOT EXISTS`, so older databases gain them on open and nothing existing changes.

use super::{json, parse, Result, Store};
use dyno_core::issues::{Issue, IssueKind};
use dyno_core::time::Timestamp;
use dyno_core::unusual::baseline::{Change, Entry};
use rusqlite::params;

/// The DDL for the tables.
pub(super) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS issues (id TEXT PRIMARY KEY, at INTEGER NOT NULL, kind TEXT NOT NULL, data TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS issues_at ON issues (at);
CREATE TABLE IF NOT EXISTS baseline (key TEXT PRIMARY KEY, last_seen INTEGER NOT NULL, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS baseline_startup (id TEXT PRIMARY KEY, at INTEGER NOT NULL);
";

/// The `meta` key holding when the baseline started.
const SINCE_KEY: &str = "unusual.since";

fn kind_text(k: IssueKind) -> String {
    serde_json::to_value(k)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

impl Store {
    /// Stores issues; those already stored (same id) are ignored. Returns the ones that were new.
    pub fn put_issues(&self, issues: &[Issue]) -> Result<Vec<Issue>> {
        let tx = self.conn.unchecked_transaction()?;
        let mut added = Vec::new();
        {
            let mut st = tx.prepare_cached("INSERT OR IGNORE INTO issues (id, at, kind, data) VALUES (?1, ?2, ?3, ?4)")?;
            for i in issues {
                if st.execute(params![i.id, i.at.millis(), kind_text(i.kind), json(i)?])? > 0 {
                    added.push(i.clone());
                }
            }
        }
        tx.commit()?;
        Ok(added)
    }

    /// The issues in `[from, to]`, newest first, at most `limit`; rows that no longer parse are skipped.
    pub fn issues(&self, from: Timestamp, to: Timestamp, limit: u32) -> Result<Vec<Issue>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT data FROM issues WHERE at >= ?1 AND at <= ?2 ORDER BY at DESC, id LIMIT ?3")?;
        let rows = st.query_map(params![from.millis(), to.millis(), i64::from(limit)], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).filter_map(|s| parse::<Issue>(&s)).collect())
    }

    /// How many issues of `kind` happened in `[from, to)`.
    pub fn count_issues(&self, kind: IssueKind, from: Timestamp, to: Timestamp) -> Result<u32> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM issues WHERE kind = ?1 AND at >= ?2 AND at < ?3",
            params![kind_text(kind), from.millis(), to.millis()],
            |r| r.get(0),
        )?)
    }

    /// The baseline as stored: when it started, the programs and the startup ids.
    pub fn load_baseline(&self) -> Result<(Option<Timestamp>, Vec<Entry>, Vec<String>)> {
        let since = self.meta(SINCE_KEY)?.and_then(|v| v.parse::<i64>().ok()).map(Timestamp);
        let mut st = self.conn.prepare("SELECT data FROM baseline")?;
        let entries = st
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .filter_map(|s| parse::<Entry>(&s))
            .collect();
        let mut st = self.conn.prepare("SELECT id FROM baseline_startup")?;
        let startup = st.query_map([], |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).collect();
        Ok((since, entries, startup))
    }

    /// Persists what a look at the process table changed.
    pub fn apply_baseline(&self, changes: &[Change]) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        for c in changes {
            match c {
                Change::Entry(e) => {
                    tx.execute(
                        "INSERT OR REPLACE INTO baseline (key, last_seen, data) VALUES (?1, ?2, ?3)",
                        params![e.key, e.last_seen.millis(), json(e)?],
                    )?;
                }
                Change::Remove(keys) => {
                    for k in keys {
                        tx.execute("DELETE FROM baseline WHERE key = ?1", [k])?;
                    }
                }
                Change::Startup(id, at) => {
                    tx.execute(
                        "INSERT OR IGNORE INTO baseline_startup (id, at) VALUES (?1, ?2)",
                        params![id, at.millis()],
                    )?;
                }
                Change::Since(at) => {
                    tx.execute(
                        "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
                        params![SINCE_KEY, at.millis().to_string()],
                    )?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Forgets the baseline.
    pub fn clear_baseline(&self) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM baseline", [])?;
        tx.execute("DELETE FROM baseline_startup", [])?;
        tx.execute("DELETE FROM meta WHERE key = ?1", [SINCE_KEY])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::issues::IssueSourceId;
    use dyno_core::unusual::baseline::Status;
    use dyno_core::unusual::Signing;

    fn issue(at: i64, kind: IssueKind, key: &str) -> Issue {
        Issue::new(IssueSourceId::MacReports, Timestamp(at), kind, "App", "t", "e", key)
    }

    #[test]
    fn issues_are_stored_once_and_queried_by_window() {
        let s = Store::memory().unwrap();
        let a = issue(1_000, IssueKind::AppCrash, "a");
        let b = issue(2_000, IssueKind::AppHang, "b");
        assert_eq!(s.put_issues(&[a.clone(), b.clone()]).unwrap().len(), 2);
        // Reading the same log again adds nothing.
        assert!(s.put_issues(&[a.clone(), b.clone()]).unwrap().is_empty());
        let got = s.issues(Timestamp(0), Timestamp(5_000), 10).unwrap();
        assert_eq!(got, vec![b.clone(), a.clone()]);
        assert_eq!(s.issues(Timestamp(1_500), Timestamp(5_000), 10).unwrap(), vec![b]);
        assert_eq!(s.issues(Timestamp(0), Timestamp(5_000), 1).unwrap().len(), 1);
        assert_eq!(s.count_issues(IssueKind::AppCrash, Timestamp(0), Timestamp(5_000)).unwrap(), 1);
        assert_eq!(s.count_issues(IssueKind::AppCrash, Timestamp(0), Timestamp(1_000)).unwrap(), 0);
        assert_eq!(s.count_issues(IssueKind::BadShutdown, Timestamp(0), Timestamp(5_000)).unwrap(), 0);
    }

    #[test]
    fn a_damaged_row_is_skipped_not_fatal() {
        let s = Store::memory().unwrap();
        s.conn
            .execute(
                "INSERT INTO issues (id, at, kind, data) VALUES ('x', 5, 'appCrash', 'not json')",
                [],
            )
            .unwrap();
        s.put_issues(&[issue(6, IssueKind::AppCrash, "ok")]).unwrap();
        assert_eq!(s.issues(Timestamp(0), Timestamp(10), 10).unwrap().len(), 1);
    }

    #[test]
    fn old_issues_are_pruned_with_the_rest() {
        let s = Store::memory().unwrap();
        s.put_issues(&[issue(1, IssueKind::AppCrash, "old")]).unwrap();
        let now = Timestamp(10 * 365 * 86_400_000);
        s.prune(dyno_core::history::Retention::default(), now).unwrap();
        assert!(s.issues(Timestamp(0), now, 10).unwrap().is_empty());
    }

    #[test]
    fn the_baseline_round_trips_through_changes() {
        let s = Store::memory().unwrap();
        let e = Entry {
            key: "/a".into(),
            name: "a".into(),
            exe: Some("/a".into()),
            first_seen: Timestamp(1),
            last_seen: Timestamp(2),
            learned: true,
            status: Status::Known,
            signing: Signing::Signed("Acme".into()),
            flag: None,
        };
        s.apply_baseline(&[
            Change::Since(Timestamp(7)),
            Change::Entry(e.clone()),
            Change::Startup("login:a".into(), Timestamp(3)),
        ])
        .unwrap();
        let (since, entries, startup) = s.load_baseline().unwrap();
        assert_eq!(since, Some(Timestamp(7)));
        assert_eq!(entries, vec![e.clone()]);
        assert_eq!(startup, vec!["login:a".to_string()]);
        let mut changed = e.clone();
        changed.status = Status::Known;
        s.apply_baseline(&[Change::Entry(changed.clone())]).unwrap();
        assert_eq!(s.load_baseline().unwrap().1, vec![changed]);
        s.apply_baseline(&[Change::Remove(vec!["/a".into()])]).unwrap();
        assert!(s.load_baseline().unwrap().1.is_empty());
        s.apply_baseline(&[]).unwrap();
        s.apply_baseline(&[Change::Entry(e)]).unwrap();
        s.clear_baseline().unwrap();
        assert_eq!(s.load_baseline().unwrap(), (None, vec![], vec![]));
    }
}
