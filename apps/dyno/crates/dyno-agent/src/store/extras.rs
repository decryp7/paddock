//! The `changes` table (software and startup changes the extras noticed), added next to the v1 tables with
//! `IF NOT EXISTS`, so older databases gain it on open.

use super::{Result, Store};
use dyno_core::extras::changes::{Change, ChangeKind};
use dyno_core::time::Timestamp;
use rusqlite::params;

/// The DDL for the table.
pub(super) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS changes (at INTEGER NOT NULL, kind TEXT NOT NULL, title TEXT NOT NULL, detail TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS changes_at ON changes (at);
";

/// Rows older than this are dropped when new ones are written.
const KEEP_MS: i64 = 180 * dyno_core::time::DAY;

fn kind_text(k: ChangeKind) -> String {
    serde_json::to_value(k)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn kind_from(s: &str) -> Option<ChangeKind> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}

impl Store {
    /// Stores changes and drops those older than half a year.
    pub fn put_changes(&self, changes: &[Change], now: Timestamp) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut st = tx.prepare_cached("INSERT INTO changes (at, kind, title, detail) VALUES (?1, ?2, ?3, ?4)")?;
            for c in changes {
                st.execute(params![c.at.millis(), kind_text(c.kind), c.title, c.detail])?;
            }
        }
        tx.execute("DELETE FROM changes WHERE at < ?1", params![now.millis() - KEEP_MS])?;
        tx.commit()?;
        Ok(())
    }

    /// The stored changes in `[from, to]`, newest first (at most 2 000); rows with an unknown kind are skipped.
    pub fn changes(&self, from: Timestamp, to: Timestamp) -> Result<Vec<Change>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT at, kind, title, detail FROM changes WHERE at >= ?1 AND at <= ?2 ORDER BY at DESC LIMIT 2000")?;
        let rows = st.query_map(params![from.millis(), to.millis()], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        Ok(rows
            .filter_map(|r| r.ok())
            .filter_map(|(at, kind, title, detail)| {
                Some(Change {
                    at: Timestamp(at),
                    kind: kind_from(&kind)?,
                    title,
                    detail,
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_round_trip_and_old_ones_are_dropped() {
        let s = Store::memory().unwrap();
        let now = Timestamp(400 * dyno_core::time::DAY);
        let mk = |at: i64, kind| Change {
            at: Timestamp(at),
            kind,
            title: format!("t{at}"),
            detail: "d".into(),
        };
        s.put_changes(&[mk(1, ChangeKind::Installed), mk(now.millis() - 1000, ChangeKind::Updated)], now)
            .unwrap();
        let all = s.changes(Timestamp(0), now).unwrap();
        assert_eq!(all.len(), 1, "the year-old row is gone");
        assert_eq!(all[0].kind, ChangeKind::Updated);
        s.conn
            .execute("INSERT INTO changes VALUES (?1, 'unheardOf', 'x', 'y')", params![now.millis() - 5])
            .unwrap();
        assert_eq!(s.changes(Timestamp(0), now).unwrap().len(), 1, "an unknown kind is skipped");
        assert!(s.changes(now, Timestamp(0)).unwrap().is_empty());
    }
}
