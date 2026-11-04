//! The scheduled wakes (ADR 0032 section 5): one row per schedule, its JSON document in `data` (versioned by serde defaults).

use super::{json, parse, Result, Store};
use dyno_core::power::schedule::WakeSchedule;
use rusqlite::params;

/// The DDL for the scheduled-wake table.
pub(super) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS wake_schedules (id INTEGER PRIMARY KEY, data TEXT NOT NULL);
";

impl Store {
    /// The schedules, by id; a row that no longer parses is skipped.
    pub fn wake_schedules(&self) -> Result<Vec<WakeSchedule>> {
        let mut st = self.conn.prepare("SELECT id, data FROM wake_schedules ORDER BY id")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        Ok(rows
            .filter_map(|r| r.ok())
            .filter_map(|(id, data)| {
                let mut s: WakeSchedule = parse(&data)?;
                s.id = u32::try_from(id).ok()?;
                Some(s)
            })
            .collect())
    }

    /// Stores `schedule`: a new row when its id is 0 (the new id is set and returned), else replaces that row.
    pub fn put_wake_schedule(&self, schedule: &mut WakeSchedule) -> Result<u32> {
        if schedule.id == 0 {
            self.conn.execute("INSERT INTO wake_schedules (data) VALUES ('{}')", [])?;
            schedule.id = u32::try_from(self.conn.last_insert_rowid()).unwrap_or(0);
        }
        self.conn.execute(
            "INSERT OR REPLACE INTO wake_schedules (id, data) VALUES (?1, ?2)",
            params![i64::from(schedule.id), json(schedule)?],
        )?;
        Ok(schedule.id)
    }

    /// Removes a schedule; no error when there is none.
    pub fn delete_wake_schedule(&self, id: u32) -> Result<()> {
        self.conn
            .execute("DELETE FROM wake_schedules WHERE id = ?1", params![i64::from(id)])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules_round_trip_with_new_ids_and_skip_a_damaged_row() {
        let s = Store::memory().unwrap();
        let mut a = WakeSchedule {
            name: "A".into(),
            ..WakeSchedule::default()
        };
        assert_eq!(s.put_wake_schedule(&mut a).unwrap(), 1);
        a.name = "A2".into();
        s.put_wake_schedule(&mut a).unwrap();
        s.conn
            .execute("INSERT INTO wake_schedules (id, data) VALUES (5, 'not json')", [])
            .unwrap();
        let all = s.wake_schedules().unwrap();
        assert_eq!(all.iter().map(|x| (x.id, x.name.as_str())).collect::<Vec<_>>(), [(1, "A2")]);
        s.delete_wake_schedule(1).unwrap();
        s.delete_wake_schedule(9).unwrap();
        assert!(s.wake_schedules().unwrap().is_empty());
    }
}
