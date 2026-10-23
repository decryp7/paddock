//! The hardware tables (WP-23, WP-24): `hardware_readings` (slow readings of SSD wear and battery capacity, one row per
//! kind, device and time, the JSON document in `data`) and `throttle_episodes` (one row per finished throttling
//! episode). Added with `IF NOT EXISTS` like the other late tables.

use super::{json, parse, Result, Store};
use dyno_core::throttle::ThrottleEpisode;
use dyno_core::time::{Timestamp, DAY};
use rusqlite::{params, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// The DDL for the hardware tables.
pub(super) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS hardware_readings (kind TEXT NOT NULL, device TEXT NOT NULL, at INTEGER NOT NULL, data TEXT NOT NULL, PRIMARY KEY (kind, device, at)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS throttle_episodes (start INTEGER PRIMARY KEY, data TEXT NOT NULL);
";

/// Reading kinds.
pub mod kind {
    /// SSD wear counters (a `DiskWear` document).
    pub const SSD: &str = "ssd";
    /// Battery capacity (a `BatteryCapacity` document).
    pub const BATTERY: &str = "battery";
}

/// Readings are kept this long (a battery's trend is the point of keeping them).
pub const READINGS_KEEP_DAYS: i64 = 1_100;
/// Throttling episodes are kept this long.
pub const EPISODES_KEEP_DAYS: i64 = 400;
/// Readings per device returned at most (twice a day for three years is under this).
pub const MAX_READINGS: i64 = 3_000;
/// The meta key holding when Dyno first saw this machine.
pub const FIRST_SEEN_KEY: &str = "firstSeen";

impl Store {
    /// Stores a reading document for a device.
    pub fn put_hardware_reading<T: Serialize>(&self, kind: &str, device: &str, at: Timestamp, doc: &T) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO hardware_readings (kind, device, at, data) VALUES (?1, ?2, ?3, ?4)",
            params![kind, device, at.millis(), json(doc)?],
        )?;
        Ok(())
    }

    /// The devices that have readings of `kind`.
    pub fn hardware_devices(&self, kind: &str) -> Result<Vec<String>> {
        let mut st = self
            .conn
            .prepare("SELECT DISTINCT device FROM hardware_readings WHERE kind = ?1 ORDER BY device")?;
        let rows = st.query_map([kind], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// A device's readings, oldest first, at most [`MAX_READINGS`] (the newest); rows that no longer parse are skipped.
    pub fn hardware_readings<T: DeserializeOwned>(&self, kind: &str, device: &str) -> Result<Vec<(Timestamp, T)>> {
        let mut st = self.conn.prepare(
            "SELECT at, data FROM (SELECT at, data FROM hardware_readings WHERE kind = ?1 AND device = ?2 ORDER BY at DESC LIMIT ?3) ORDER BY at",
        )?;
        let rows = st.query_map(params![kind, device, MAX_READINGS], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        Ok(rows
            .filter_map(|r| r.ok())
            .filter_map(|(at, s)| parse::<T>(&s).map(|d| (Timestamp(at), d)))
            .collect())
    }

    /// When the newest reading of a device was stored.
    pub fn last_hardware_reading(&self, kind: &str, device: &str) -> Result<Option<Timestamp>> {
        Ok(self
            .conn
            .query_row(
                "SELECT MAX(at) FROM hardware_readings WHERE kind = ?1 AND device = ?2",
                params![kind, device],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
            .map(Timestamp))
    }

    /// Stores a finished throttling episode (replacing one with the same start).
    pub fn put_throttle_episode(&self, e: &ThrottleEpisode) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO throttle_episodes (start, data) VALUES (?1, ?2)",
            params![e.start.millis(), json(e)?],
        )?;
        Ok(())
    }

    /// Episodes that began at or after `from`, newest first, at most `limit`.
    pub fn throttle_episodes(&self, from: Timestamp, limit: u32) -> Result<Vec<ThrottleEpisode>> {
        let mut st = self
            .conn
            .prepare("SELECT data FROM throttle_episodes WHERE start >= ?1 ORDER BY start DESC LIMIT ?2")?;
        let rows = st.query_map(params![from.millis(), i64::from(limit)], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).filter_map(|s| parse::<ThrottleEpisode>(&s)).collect())
    }

    /// When Dyno first saw this machine: stored on the first call, the same afterwards.
    pub fn first_seen(&self, now: Timestamp) -> Result<Timestamp> {
        if let Some(t) = self.meta(FIRST_SEEN_KEY)?.and_then(|s| s.parse::<i64>().ok()) {
            return Ok(Timestamp(t));
        }
        self.set_meta(FIRST_SEEN_KEY, &now.millis().to_string())?;
        Ok(now)
    }

    /// Deletes hardware rows older than their retention.
    pub(super) fn prune_hardware(&self, now: Timestamp) -> Result<()> {
        self.conn.execute(
            "DELETE FROM hardware_readings WHERE at < ?1",
            [now.minus(READINGS_KEEP_DAYS * DAY).millis()],
        )?;
        self.conn.execute(
            "DELETE FROM throttle_episodes WHERE start < ?1",
            [now.minus(EPISODES_KEEP_DAYS * DAY).millis()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::hardware::BatteryCapacity;
    use dyno_core::throttle::ThrottleCause;

    fn cap(full: u32) -> BatteryCapacity {
        BatteryCapacity {
            id: "b".into(),
            design: 5000,
            full,
            cycles: Some(10),
            rated_cycles: None,
        }
    }

    #[test]
    fn readings_are_kept_per_device_oldest_first() {
        let s = Store::memory().unwrap();
        for (i, full) in [4900u32, 4800, 4700].iter().enumerate() {
            s.put_hardware_reading(kind::BATTERY, "b", Timestamp(i as i64 * 10), &cap(*full))
                .unwrap();
        }
        s.put_hardware_reading(kind::BATTERY, "other", Timestamp(5), &cap(1)).unwrap();
        s.put_hardware_reading(kind::SSD, "d", Timestamp(5), &cap(2)).unwrap();
        let rows: Vec<(Timestamp, BatteryCapacity)> = s.hardware_readings(kind::BATTERY, "b").unwrap();
        assert_eq!(rows.iter().map(|r| r.1.full).collect::<Vec<_>>(), vec![4900, 4800, 4700]);
        assert_eq!(s.hardware_devices(kind::BATTERY).unwrap(), vec!["b", "other"]);
        assert_eq!(s.last_hardware_reading(kind::BATTERY, "b").unwrap(), Some(Timestamp(20)));
        assert_eq!(s.last_hardware_reading(kind::BATTERY, "none").unwrap(), None);
        // A document that no longer parses is skipped, not fatal.
        s.put_hardware_reading(kind::BATTERY, "b", Timestamp(30), &"garbage").unwrap();
        let rows: Vec<(Timestamp, BatteryCapacity)> = s.hardware_readings(kind::BATTERY, "b").unwrap();
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn episodes_round_trip_and_prune() {
        let s = Store::memory().unwrap();
        let ep = |start: i64| ThrottleEpisode {
            start: Timestamp(start),
            end: Some(Timestamp(start + 1)),
            cause: ThrottleCause::Thermal,
            avg_lost_percent: 20.0,
            peak_lost_percent: 30.0,
            peak_temperature_c: Some(95.0),
        };
        let now = Timestamp(1_000 * DAY);
        let old = ep(now.minus(500 * DAY).millis());
        let recent = ep(now.minus(2 * DAY).millis());
        s.put_throttle_episode(&old).unwrap();
        s.put_throttle_episode(&recent).unwrap();
        s.put_throttle_episode(&recent).unwrap();
        assert_eq!(s.throttle_episodes(Timestamp(0), 10).unwrap(), vec![recent.clone(), old]);
        assert_eq!(s.throttle_episodes(now.minus(30 * DAY), 10).unwrap(), vec![recent.clone()]);
        assert_eq!(s.throttle_episodes(Timestamp(0), 1).unwrap().len(), 1);
        s.put_hardware_reading(kind::SSD, "d", now.minus(1_200 * DAY), &cap(1)).unwrap();
        s.put_hardware_reading(kind::SSD, "d", now.minus(DAY), &cap(1)).unwrap();
        s.prune_hardware(now).unwrap();
        assert_eq!(s.throttle_episodes(Timestamp(0), 10).unwrap(), vec![recent]);
        assert_eq!(s.hardware_readings::<BatteryCapacity>(kind::SSD, "d").unwrap().len(), 1);
    }

    #[test]
    fn first_seen_is_remembered() {
        let s = Store::memory().unwrap();
        assert_eq!(s.first_seen(Timestamp(5)).unwrap(), Timestamp(5));
        assert_eq!(s.first_seen(Timestamp(99)).unwrap(), Timestamp(5));
    }
}
