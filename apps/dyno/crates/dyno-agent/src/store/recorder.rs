//! The `recorder_snapshots` table (flight recorder captures).

/// The DDL for the recorder table.
pub(super) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS recorder_snapshots (id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, reason TEXT NOT NULL, bytes INTEGER NOT NULL, data TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS recorder_snapshots_at ON recorder_snapshots (at);
";
