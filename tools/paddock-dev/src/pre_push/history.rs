//! The gate's own history of how long its steps go without progress, per row kind and workspace, cold and warm kept apart
//! (docs/research/2026-11-19-gate-hang-detection.md section 3.7), so the stall limit of a step can be tighter than the fallback where
//! the gate has seen that kind of step finish.
//!
//! One JSON line per finished step in `step-history.jsonl` beside the gate logs: the row kind and workspace, whether its build folder
//! was cold, whether it passed, its run seconds, the longest gap between two progress events and how many there were. A limit is derived
//! only from passed steps of the same kind, workspace and coldness, and only from [`MIN_SAMPLES`] of them; with fewer (or no) records the
//! caller falls back to the fixed gap. The limit can tighten the fallback, never loosen it ([`limit_from`]). Pure: records in, a
//! duration out; the file is read and appended by [`History`].

use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How many passed steps of one class must be on record before a limit is derived from them.
pub const MIN_SAMPLES: usize = 5;

/// The multiple of the longest recorded gap a step may go without progress: 4. Measured basis (WP-0, docs/benchmarks.md "Push gate stall
/// gap (WP-0)"): across 25 cold runs the longest gap was 43.1 s against a median of the runs' longest gaps of 10.5 s, a ratio of 4.1, so
/// a run that has gone 4 times the longest gap seen for its kind is beyond what a healthy run of any kind showed. One cold run per
/// workspace is a small sample: the limit only tightens the fallback (never loosens it) and the watchdog reports before it kills
/// (ER-12, ER-41); revisit K with the week of report-only records in `step-history.jsonl`.
pub const K: f64 = 4.0;

/// The shortest limit a history can give: 120 s, 2.8 times the longest gap measured on any real cold run (43.1 s, WP-0), so a limit
/// derived from quick units still leaves room for one slow unit.
pub const FLOOR: Duration = Duration::from_secs(120);

/// The most records read back (the newest): the file only grows, a limit needs the recent past.
const READ_LINES: usize = 4000;

/// One finished step.
#[derive(Debug, Clone, PartialEq)]
pub struct StepRecord {
    /// The row kind without its workspace (`clippy`, `doctest`).
    pub kind: String,
    /// The workspace folder.
    pub ws: String,
    /// The build folder was cold when the step started.
    pub cold: bool,
    /// The step passed.
    pub ok: bool,
    /// Seconds the step ran.
    pub ran_secs: f64,
    /// The longest time between two progress events (or from the start to the first, or from the last to the end).
    pub max_gap_secs: f64,
    /// How many progress events it showed.
    pub events: u64,
}

impl StepRecord {
    /// The record as its JSON line.
    fn to_json(&self) -> Value {
        json!({"kind": self.kind, "ws": self.ws, "cold": self.cold, "ok": self.ok, "ran_secs": self.ran_secs,
               "max_gap_secs": self.max_gap_secs, "events": self.events})
    }

    /// A record from its line, `None` when it is not one (an older or foreign shape).
    fn from_json(v: &Value) -> Option<StepRecord> {
        Some(StepRecord {
            kind: v["kind"].as_str()?.to_string(),
            ws: v["ws"].as_str()?.to_string(),
            cold: v["cold"].as_bool()?,
            ok: v["ok"].as_bool()?,
            ran_secs: v["ran_secs"].as_f64()?,
            max_gap_secs: v["max_gap_secs"].as_f64()?,
            events: v["events"].as_u64()?,
        })
    }
}

/// The stall limit for a class from `records`, or `None` when there are too few passed steps of it. `fallback` is the most it can be.
pub fn limit_from(records: &[StepRecord], kind: &str, ws: &str, cold: bool, fallback: Duration) -> Option<Duration> {
    let gaps: Vec<f64> = records
        .iter()
        .filter(|r| r.ok && r.kind == kind && r.ws == ws && r.cold == cold && r.events > 0)
        .map(|r| r.max_gap_secs)
        .collect();
    if gaps.len() < MIN_SAMPLES {
        return None;
    }
    let longest = gaps.iter().copied().fold(0.0, f64::max);
    let wanted = Duration::from_secs_f64((longest * K).max(0.0));
    Some(wanted.max(FLOOR).min(fallback))
}

/// The history file.
#[derive(Debug, Clone)]
pub struct History {
    path: PathBuf,
}

impl History {
    /// The history in `path` (created on the first record).
    pub fn new(path: impl Into<PathBuf>) -> History {
        History { path: path.into() }
    }

    /// Adds one record. Best effort: a history that cannot be written only means the next step has less to go on.
    pub fn append(&self, r: &StepRecord) {
        let Ok(line) = serde_json::to_string(&r.to_json()) else { return };
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // One write of a short line in append mode: concurrent pushes do not interleave inside a line.
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| f.write_all(format!("{line}\n").as_bytes()));
    }

    /// The newest records (a line that does not parse is skipped: the file may hold an older shape).
    pub fn records(&self) -> Vec<StepRecord> {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        let lines: Vec<&str> = text.lines().collect();
        let from = lines.len().saturating_sub(READ_LINES);
        lines[from..]
            .iter()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|v| StepRecord::from_json(&v))
            .collect()
    }

    /// The limit for a class ([`limit_from`]) from the file.
    pub fn limit(&self, kind: &str, ws: &str, cold: bool, fallback: Duration) -> Option<Duration> {
        limit_from(&self.records(), kind, ws, cold, fallback)
    }

    /// Where the file is.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(kind: &str, ws: &str, cold: bool, ok: bool, gap: f64) -> StepRecord {
        StepRecord {
            kind: kind.into(),
            ws: ws.into(),
            cold,
            ok,
            ran_secs: 100.0,
            max_gap_secs: gap,
            events: 10,
        }
    }

    const FALLBACK: Duration = Duration::from_secs(15 * 60);

    #[test]
    fn with_too_few_passed_steps_there_is_no_limit_and_the_caller_falls_back() {
        let few: Vec<_> = (0..MIN_SAMPLES - 1).map(|_| rec("clippy", "crates", false, true, 3.0)).collect();
        assert_eq!(limit_from(&few, "clippy", "crates", false, FALLBACK), None);
        assert_eq!(limit_from(&[], "clippy", "crates", false, FALLBACK), None);
    }

    #[test]
    fn the_limit_is_a_multiple_of_the_longest_gap_between_the_floor_and_the_fallback() {
        let mk = |gap: f64| -> Vec<StepRecord> { (0..MIN_SAMPLES).map(|_| rec("clippy", "crates", false, true, gap)).collect() };
        // quick units: the floor
        assert_eq!(limit_from(&mk(3.0), "clippy", "crates", false, FALLBACK), Some(FLOOR));
        // a slow unit: K times it
        assert_eq!(
            limit_from(&mk(60.0), "clippy", "crates", false, FALLBACK),
            Some(Duration::from_secs(240))
        );
        // never above the fallback: the history only tightens
        assert_eq!(limit_from(&mk(900.0), "clippy", "crates", false, FALLBACK), Some(FALLBACK));
    }

    #[test]
    fn cold_and_warm_workspaces_and_kinds_are_never_mixed_and_failures_do_not_count() {
        let mut v: Vec<StepRecord> = (0..MIN_SAMPLES).map(|_| rec("doctest", "apps/dyno", true, true, 200.0)).collect();
        v.extend((0..MIN_SAMPLES).map(|_| rec("doctest", "apps/dyno", false, true, 2.0)));
        v.extend((0..MIN_SAMPLES).map(|_| rec("doctest", "crates", false, false, 500.0)));
        assert_eq!(
            limit_from(&v, "doctest", "apps/dyno", true, FALLBACK),
            Some(Duration::from_secs(800))
        );
        assert_eq!(limit_from(&v, "doctest", "apps/dyno", false, FALLBACK), Some(FLOOR));
        assert_eq!(
            limit_from(&v, "doctest", "crates", false, FALLBACK),
            None,
            "failed steps are no evidence"
        );
        assert_eq!(limit_from(&v, "clippy", "apps/dyno", true, FALLBACK), None);
    }

    #[test]
    fn the_file_round_trips_and_skips_a_line_it_cannot_read() {
        let d = tempfile::tempdir().unwrap();
        let h = History::new(d.path().join("logs/step-history.jsonl"));
        for _ in 0..MIN_SAMPLES {
            h.append(&rec("clippy", "crates", false, true, 30.0));
        }
        let mut f = std::fs::OpenOptions::new().append(true).open(h.path()).unwrap();
        std::io::Write::write_all(&mut f, b"not json\n").unwrap();
        assert_eq!(h.records().len(), MIN_SAMPLES);
        assert_eq!(h.limit("clippy", "crates", false, FALLBACK), Some(Duration::from_secs(120)));
    }
}
