//! The `recorder_snapshots` table (flight recorder captures).

/// The DDL for the recorder table.
pub(super) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS recorder_snapshots (id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, reason TEXT NOT NULL, bytes INTEGER NOT NULL, data TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS recorder_snapshots_at ON recorder_snapshots (at);
";

use super::{Result, Store};
use dyno_core::recorder::{CaptureReason, RecorderSnapshot, RecorderSnapshotInfo, MAX_SNAPSHOTS, MAX_SNAPSHOT_BYTES};
use dyno_core::time::Timestamp;
use rusqlite::{params, OptionalExtension};

impl Store {
    /// Saves a snapshot (thinned to fit the size bound) and drops the oldest beyond [`MAX_SNAPSHOTS`].
    pub fn put_recorder_snapshot(&self, snapshot: RecorderSnapshot) -> Result<RecorderSnapshotInfo> {
        let (text, kept) = snapshot.to_bounded_json()?;
        let reason = serde_json::to_string(&kept.reason)?;
        self.conn.execute(
            "INSERT INTO recorder_snapshots (at, reason, bytes, data) VALUES (?1, ?2, ?3, ?4)",
            params![kept.at.millis(), reason, text.len() as i64, text],
        )?;
        let id = self.conn.last_insert_rowid();
        self.conn.execute(
            "DELETE FROM recorder_snapshots WHERE id NOT IN (SELECT id FROM recorder_snapshots ORDER BY id DESC LIMIT ?1)",
            [MAX_SNAPSHOTS as i64],
        )?;
        Ok(RecorderSnapshotInfo {
            id: id as u64,
            at: kept.at,
            reason: kept.reason,
            bytes: text.len().min(MAX_SNAPSHOT_BYTES) as u32,
            samples: kept.samples.len() as u32,
        })
    }

    /// The saved snapshots, newest first (without their samples).
    pub fn recorder_snapshots(&self) -> Result<Vec<RecorderSnapshotInfo>> {
        let mut st = self
            .conn
            .prepare("SELECT id, at, reason, bytes, data FROM recorder_snapshots ORDER BY id DESC LIMIT ?1")?;
        let rows = st.query_map([MAX_SNAPSHOTS as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        Ok(rows
            .filter_map(|r| r.ok())
            .filter_map(|(id, at, reason, bytes, data)| {
                let reason: CaptureReason = serde_json::from_str(&reason).ok()?;
                // Counting samples needs the document; thinned ones are cheap enough at 20 rows.
                let samples = RecorderSnapshot::parse(&data).map_or(0, |s| s.samples.len() as u32);
                Some(RecorderSnapshotInfo {
                    id: id as u64,
                    at: Timestamp(at),
                    reason,
                    bytes: bytes.clamp(0, i64::from(u32::MAX)) as u32,
                    samples,
                })
            })
            .collect())
    }

    /// One saved snapshot; `None` when it is gone or damaged.
    pub fn recorder_snapshot(&self, id: u64) -> Result<Option<RecorderSnapshot>> {
        let text: Option<String> = self
            .conn
            .query_row("SELECT data FROM recorder_snapshots WHERE id = ?1", [id as i64], |r| r.get(0))
            .optional()?;
        Ok(text.and_then(|t| RecorderSnapshot::parse(&t)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::recorder::RecorderSample;

    fn snap(at: i64, n: usize) -> RecorderSnapshot {
        let samples = (0..n)
            .map(|i| RecorderSample {
                at: Timestamp(at + i as i64 * 1_000),
                cpu_total: 1.0,
                cpu_cores: vec![1.0; 4],
                memory_used: 1,
                memory_total: 2,
                disk_read_bps: 0.0,
                disk_write_bps: 0.0,
                net_rx_bps: 0.0,
                net_tx_bps: 0.0,
                top: vec![],
            })
            .collect();
        RecorderSnapshot::new(Timestamp(at), CaptureReason::Manual, samples)
    }

    #[test]
    fn snapshots_round_trip_and_the_count_is_bounded() {
        let s = Store::memory().unwrap();
        let first = s.put_recorder_snapshot(snap(1_000, 3)).unwrap();
        assert_eq!((first.samples, first.reason.clone()), (3, CaptureReason::Manual));
        assert!(first.bytes > 0);
        assert_eq!(s.recorder_snapshot(first.id).unwrap().unwrap().samples.len(), 3);
        assert!(s.recorder_snapshot(999).unwrap().is_none());
        for i in 0..MAX_SNAPSHOTS + 3 {
            s.put_recorder_snapshot(snap(2_000 + i as i64, 1)).unwrap();
        }
        let list = s.recorder_snapshots().unwrap();
        assert_eq!(list.len(), MAX_SNAPSHOTS);
        assert!(list[0].id > list[1].id, "newest first");
        assert!(s.recorder_snapshot(first.id).unwrap().is_none(), "the oldest were dropped");
    }

    #[test]
    fn damaged_rows_are_skipped() {
        let s = Store::memory().unwrap();
        s.conn
            .execute(
                "INSERT INTO recorder_snapshots (at, reason, bytes, data) VALUES (1, 'junk', 1, 'junk')",
                [],
            )
            .unwrap();
        s.conn
            .execute(
                "INSERT INTO recorder_snapshots (at, reason, bytes, data) VALUES (2, '\"manual\"', 1, 'junk')",
                [],
            )
            .unwrap();
        let list = s.recorder_snapshots().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].samples, 0);
        assert!(s.recorder_snapshot(2).unwrap().is_none());
    }
}
