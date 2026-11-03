//! The saved Wake-on-LAN computers (ADR 0032, D6): one row per device, its JSON document in `data` (versioned by serde
//! defaults). The SecureOn password is never here: it lives in the credential store.

use super::{json, parse, Result, Store};
use dyno_core::wol::WolDevice;
use rusqlite::params;

/// The DDL for the Wake-on-LAN table.
pub(super) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS wol_devices (id INTEGER PRIMARY KEY, data TEXT NOT NULL);
";

impl Store {
    /// The saved computers, by id; a row that no longer parses is skipped.
    pub fn wol_devices(&self) -> Result<Vec<WolDevice>> {
        let mut st = self.conn.prepare("SELECT id, data FROM wol_devices ORDER BY id")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        Ok(rows
            .filter_map(|r| r.ok())
            .filter_map(|(id, data)| {
                let mut d: WolDevice = parse(&data)?;
                d.id = u64::try_from(id).ok()?;
                Some(d)
            })
            .collect())
    }

    /// Stores `device`: a new row when its id is 0 (the new id is returned and set), else replaces that row.
    pub fn put_wol_device(&self, device: &mut WolDevice) -> Result<u64> {
        if device.id == 0 {
            self.conn.execute("INSERT INTO wol_devices (data) VALUES ('{}')", [])?;
            device.id = u64::try_from(self.conn.last_insert_rowid()).unwrap_or(0);
        }
        self.conn.execute(
            "INSERT OR REPLACE INTO wol_devices (id, data) VALUES (?1, ?2)",
            params![i64::try_from(device.id).unwrap_or(0), json(device)?],
        )?;
        Ok(device.id)
    }

    /// Removes a device; no error when there is none.
    pub fn delete_wol_device(&self, id: u64) -> Result<()> {
        self.conn
            .execute("DELETE FROM wol_devices WHERE id = ?1", params![i64::try_from(id).unwrap_or(0)])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::wol::Mac;

    fn dev(name: &str) -> WolDevice {
        WolDevice {
            name: name.into(),
            mac: Mac([2, 0, 0x5e, 0x10, 0, 1]),
            ..WolDevice::default()
        }
    }

    #[test]
    fn devices_round_trip_with_new_ids_and_replace_in_place() {
        let s = Store::memory().unwrap();
        let (mut a, mut b) = (dev("A"), dev("B"));
        assert_eq!(s.put_wol_device(&mut a).unwrap(), 1);
        assert_eq!(s.put_wol_device(&mut b).unwrap(), 2);
        a.name = "A2".into();
        s.put_wol_device(&mut a).unwrap();
        let all = s.wol_devices().unwrap();
        assert_eq!(
            all.iter().map(|d| (d.id, d.name.as_str())).collect::<Vec<_>>(),
            [(1, "A2"), (2, "B")]
        );
        s.delete_wol_device(1).unwrap();
        s.delete_wol_device(99).unwrap();
        assert_eq!(s.wol_devices().unwrap().len(), 1);
    }

    #[test]
    fn a_damaged_row_is_skipped_not_fatal() {
        let s = Store::memory().unwrap();
        s.conn
            .execute("INSERT INTO wol_devices (id, data) VALUES (5, 'not json')", [])
            .unwrap();
        let mut d = dev("ok");
        s.put_wol_device(&mut d).unwrap();
        assert_eq!(s.wol_devices().unwrap().len(), 1);
    }
}
