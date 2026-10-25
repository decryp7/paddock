//! The alarm tables: `alarms` (one row per alarm, the JSON document plus the columns the agent queries) and
//! `alarm_events` (the change log, bounded by age and count). Added next to the v1 tables with `IF NOT EXISTS`, so
//! older databases gain them on open and nothing existing changes.

use super::{json, parse, Result, Store};
use dyno_core::alarms::{Alarm, AlarmChange};
use dyno_core::time::{Timestamp, DAY};
use rusqlite::params;

/// The DDL for the alarm tables.
pub(super) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS alarms (id INTEGER PRIMARY KEY, rule TEXT NOT NULL, key TEXT NOT NULL, state TEXT NOT NULL, last_seen INTEGER NOT NULL, data TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS alarms_last_seen ON alarms (last_seen);
CREATE TABLE IF NOT EXISTS alarm_events (seq INTEGER PRIMARY KEY AUTOINCREMENT, alarm_id INTEGER NOT NULL, at INTEGER NOT NULL, kind TEXT NOT NULL, data TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS alarm_events_alarm ON alarm_events (alarm_id, seq);
";

/// Most change-log rows kept.
pub const MAX_EVENT_ROWS: i64 = 20_000;
/// Change-log rows older than this are deleted.
pub const EVENT_KEEP_DAYS: i64 = 30;
/// The muted rules live in `meta` under this key (a JSON array).
pub const MUTED_KEY: &str = "alarm.mutedRules";

impl Store {
    /// Inserts or replaces an alarm.
    pub fn put_alarm(&self, a: &Alarm) -> Result<()> {
        let state = serde_json::to_value(a.state)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO alarms (id, rule, key, state, last_seen, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                a.id as i64,
                a.rule,
                a.key,
                state.as_str().unwrap_or(""),
                a.last_seen.millis(),
                json(a)?
            ],
        )?;
        Ok(())
    }

    /// Deletes an alarm.
    pub fn delete_alarm(&self, id: u64) -> Result<()> {
        self.conn.execute("DELETE FROM alarms WHERE id = ?1", [id as i64])?;
        Ok(())
    }

    /// Every stored alarm, newest first, at most `limit`; rows that no longer parse are skipped.
    pub fn alarms(&self, limit: u32) -> Result<Vec<Alarm>> {
        let mut st = self
            .conn
            .prepare("SELECT data FROM alarms ORDER BY last_seen DESC, id DESC LIMIT ?1")?;
        let rows = st.query_map([i64::from(limit)], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).filter_map(|s| parse::<Alarm>(&s)).collect())
    }

    /// Appends a change to the log.
    pub fn put_alarm_event(&self, c: &AlarmChange) -> Result<()> {
        let kind = serde_json::to_value(c.kind)?;
        self.conn.execute(
            "INSERT INTO alarm_events (alarm_id, at, kind, data) VALUES (?1, ?2, ?3, ?4)",
            params![c.alarm.id as i64, c.at.millis(), kind.as_str().unwrap_or(""), json(c)?],
        )?;
        Ok(())
    }

    /// The change log, newest first.
    pub fn alarm_events(&self, alarm_id: Option<u64>, limit: u32) -> Result<Vec<AlarmChange>> {
        let limit = i64::from(limit.clamp(1, 500));
        let mut out = Vec::new();
        match alarm_id {
            Some(id) => {
                let mut st = self
                    .conn
                    .prepare("SELECT data FROM alarm_events WHERE alarm_id = ?1 ORDER BY seq DESC LIMIT ?2")?;
                let rows = st.query_map(params![id as i64, limit], |r| r.get::<_, String>(0))?;
                out.extend(rows.filter_map(|r| r.ok()).filter_map(|s| parse::<AlarmChange>(&s)));
            }
            None => {
                let mut st = self.conn.prepare("SELECT data FROM alarm_events ORDER BY seq DESC LIMIT ?1")?;
                let rows = st.query_map([limit], |r| r.get::<_, String>(0))?;
                out.extend(rows.filter_map(|r| r.ok()).filter_map(|s| parse::<AlarmChange>(&s)));
            }
        }
        Ok(out)
    }

    /// Deletes change-log rows that are too old or beyond the cap.
    pub fn prune_alarm_events(&self, now: Timestamp) -> Result<()> {
        self.conn.execute(
            "DELETE FROM alarm_events WHERE at < ?1",
            [now.minus(EVENT_KEEP_DAYS * DAY).millis()],
        )?;
        self.conn.execute(
            "DELETE FROM alarm_events WHERE seq NOT IN (SELECT seq FROM alarm_events ORDER BY seq DESC LIMIT ?1)",
            [MAX_EVENT_ROWS],
        )?;
        Ok(())
    }

    /// The muted rules.
    pub fn muted_rules(&self) -> Result<Vec<String>> {
        Ok(self.meta(MUTED_KEY)?.and_then(|s| parse::<Vec<String>>(&s)).unwrap_or_default())
    }

    /// Stores the muted rules.
    pub fn set_muted_rules(&self, rules: &[String]) -> Result<()> {
        self.set_meta(MUTED_KEY, &json(&rules)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::alarms::{AlarmEngine, AlarmEvent, ChangeKind};
    use dyno_core::health::Severity;

    fn raise(key: &str, at: i64) -> AlarmEvent {
        AlarmEvent::Raise {
            rule: "r".into(),
            key: key.into(),
            severity: Severity::Warning,
            title: "t".into(),
            detail: String::new(),
            value: Some(1.0),
            metric: None,
            ttl_ms: None,
            at: Timestamp(at),
        }
    }

    #[test]
    fn alarms_events_and_muted_rules_round_trip() {
        let s = Store::memory().unwrap();
        let mut e = AlarmEngine::default();
        for c in e.apply(raise("r:a", 1_000)).into_iter().chain(e.apply(raise("r:b", 2_000))) {
            s.put_alarm(&c.alarm).unwrap();
            s.put_alarm_event(&c).unwrap();
        }
        let stored = s.alarms(10).unwrap();
        assert_eq!(stored.iter().map(|a| a.key.as_str()).collect::<Vec<_>>(), ["r:b", "r:a"]);
        assert_eq!(s.alarms(1).unwrap().len(), 1);
        let ev = s.alarm_events(None, 10).unwrap();
        assert_eq!((ev.len(), ev[0].kind), (2, ChangeKind::Raised));
        assert_eq!(s.alarm_events(Some(1), 10).unwrap().len(), 1);
        s.delete_alarm(1).unwrap();
        assert_eq!(s.alarms(10).unwrap().len(), 1);
        assert!(s.muted_rules().unwrap().is_empty());
        s.set_muted_rules(&["x".into()]).unwrap();
        assert_eq!(s.muted_rules().unwrap(), vec!["x".to_string()]);
        // Junk rows are skipped, not fatal.
        s.conn
            .execute(
                "INSERT INTO alarms (id, rule, key, state, last_seen, data) VALUES (99, 'r', 'k', 'active', 9, 'not json')",
                [],
            )
            .unwrap();
        assert_eq!(s.alarms(10).unwrap().len(), 1);
        s.set_meta(MUTED_KEY, "junk").unwrap();
        assert!(s.muted_rules().unwrap().is_empty());
    }

    #[test]
    fn the_event_log_is_bounded() {
        let s = Store::memory().unwrap();
        let mut e = AlarmEngine::default();
        let c = e.apply(raise("r:a", 1_000)).remove(0);
        s.put_alarm_event(&c).unwrap();
        let mut late = c.clone();
        late.at = Timestamp(100 * DAY);
        s.put_alarm_event(&late).unwrap();
        s.prune_alarm_events(Timestamp(100 * DAY)).unwrap();
        assert_eq!(s.alarm_events(None, 10).unwrap().len(), 1, "the 99-day-old row is gone");
        for _ in 0..5 {
            s.put_alarm_event(&late).unwrap();
        }
        s.conn.execute("DELETE FROM alarm_events WHERE seq < 0", []).unwrap();
        s.prune_alarm_events(Timestamp(100 * DAY)).unwrap();
        assert_eq!(s.alarm_events(None, 500).unwrap().len(), 6);
    }
}
