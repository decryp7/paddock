//! The gate's own history of how long its steps go without progress, per row kind and workspace, cold and warm kept apart
//! (docs/research/2026-11-19-gate-hang-detection.md section 3.7), so the stall limit of a step can be tighter than the fallback where
//! the gate has seen that kind of step finish.
//!
//! One JSON line per finished step in `step-history.jsonl` beside the gate logs: the row kind and workspace, whether its build folder
//! was cold, whether it passed, its run seconds, the longest gap between two progress events and how many there were. A limit is derived
//! only from passed steps of the same kind, workspace and coldness, and only from [`MIN_SAMPLES`] of them; with fewer (or no) records the
//! caller falls back to the fixed gap. The limit can tighten the fallback, never loosen it ([`limit_from`]). Pure: records in, a
//! duration out; the file is read and appended by [`History`].
//!
//! Hygiene (the watchdog audit of 2026-11-23, cause B: heavy's load-gate wait had been counted as run time, so a record could hold a longest
//! gap of 230 s inside a run that really worked for far less): every record carries the time it was written, the tool version and the
//! revision of the rule that measured it ([`REV`]); a record of an older revision, one that claims a gap longer than its own run, and one older than
//! [`MAX_AGE_SECS`] is never read. The file is owned by the push gate of this checkout (`<git common dir>/paddock-pre-push`), bounded by
//! [`MAX_BYTES`] (two files, so about 2 MiB) and evicted oldest first by the rotation (ER-51).

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

/// The size at which the file is rotated (review P2-4): the current file becomes `step-history.jsonl.1` (replacing an older one) and a new
/// one starts, so the history never grows past about twice this. A record is about 150 bytes, so 1 MiB holds about 7,000 steps, many
/// weeks of pushes; the limit reads at most [`READ_LINES`] of them.
pub const MAX_BYTES: u64 = 1 << 20;

/// The revision of the rule that measures a step's gaps and run time. Bump it when what a record means changes: older records are then
/// ignored (a fresh start), never mixed with the new meaning. 2: the gap counts every progress signal (output events, tree memory
/// change, disk growth) and a step's run time excludes the heavy load-gate wait (03cca969e); revision 1 records had neither.
pub const REV: u32 = 2;

/// The oldest a record may be and still count: 60 days.
pub const MAX_AGE_SECS: u64 = 60 * 24 * 3600;

/// The tool version written into each record.
pub const TOOL: &str = concat!("paddock-dev ", env!("CARGO_PKG_VERSION"));

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
    /// Whether the gap was judged with the tree's memory and disk signals as well as the output ([`super::watchdog::StepInfo::signals`]):
    /// the gaps of the two kinds of step are not mixed in a limit.
    pub signals: bool,
    /// When the record was written: seconds since the Unix epoch.
    pub at: u64,
    /// The tool that wrote it ([`TOOL`]).
    pub tool: String,
    /// The revision of the rule that measured it ([`REV`]).
    pub rev: u32,
    /// The guard that ended the step (`row cap`, a watchdog's name); `None`: it ended by itself.
    pub ended_by: Option<String>,
    /// Seconds into the run at which the watchdog first said "would have stopped" (report-only mode); `None`: it never did.
    pub would_stop_secs: Option<f64>,
}

/// Seconds since the Unix epoch now.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl StepRecord {
    /// The record as its JSON line.
    fn to_json(&self) -> Value {
        json!({"kind": self.kind, "ws": self.ws, "cold": self.cold, "ok": self.ok, "ran_secs": self.ran_secs,
               "max_gap_secs": self.max_gap_secs, "events": self.events, "signals": self.signals, "at": self.at,
               "tool": self.tool, "rev": self.rev, "ended_by": self.ended_by, "would_stop_secs": self.would_stop_secs})
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
            signals: v["signals"].as_bool()?,
            at: v["at"].as_u64()?,
            tool: v["tool"].as_str()?.to_string(),
            rev: u32::try_from(v["rev"].as_u64()?).ok()?,
            ended_by: v["ended_by"].as_str().map(str::to_string),
            would_stop_secs: v["would_stop_secs"].as_f64(),
        })
    }
}

impl StepRecord {
    /// Whether the record may be used at `now` (seconds since the epoch): written by the current rule, not older than [`MAX_AGE_SECS`],
    /// and not claiming a longer silence than its own run.
    pub fn usable(&self, now: u64) -> bool {
        self.rev >= REV && now.saturating_sub(self.at) <= MAX_AGE_SECS && self.max_gap_secs <= self.ran_secs + 1.0
    }
}

/// The stall limit for a class from `records`, or `None` when there are too few passed steps of it. `fallback` is the most it can be.
pub fn limit_from(records: &[StepRecord], kind: &str, ws: &str, cold: bool, signals: bool, fallback: Duration) -> Option<Duration> {
    let gaps: Vec<f64> = records
        .iter()
        .filter(|r| r.ok && r.kind == kind && r.ws == ws && r.cold == cold && r.signals == signals && r.events > 0)
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
    max_bytes: u64,
}

impl History {
    /// The history in `path` (created on the first record).
    pub fn new(path: impl Into<PathBuf>) -> History {
        History::with_max(path, MAX_BYTES)
    }

    /// A history rotated at `max_bytes` (a test makes it small).
    pub fn with_max(path: impl Into<PathBuf>, max_bytes: u64) -> History {
        History {
            path: path.into(),
            max_bytes,
        }
    }

    /// The previous file, `<name>.1`.
    fn older(&self) -> PathBuf {
        let mut name = self.path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        name.push(".1");
        self.path.with_file_name(name)
    }

    /// Adds one record. Best effort: a history that cannot be written only means the next step has less to go on.
    pub fn append(&self, r: &StepRecord) {
        let Ok(line) = serde_json::to_string(&r.to_json()) else { return };
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // A file past its size becomes the older one (best effort: two pushes rotating at once lose at most one old file).
        if std::fs::metadata(&self.path).is_ok_and(|m| m.len() >= self.max_bytes) {
            let _ = std::fs::rename(&self.path, self.older());
        }
        // One write of a short line in append mode: concurrent pushes do not interleave inside a line.
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| f.write_all(format!("{line}\n").as_bytes()));
    }

    /// The newest usable records ([`StepRecord::usable`]; a line that does not parse is skipped: the file may hold an older shape).
    pub fn records(&self) -> Vec<StepRecord> {
        self.records_at(now_secs())
    }

    /// [`History::records`] as of `now` (seconds since the epoch).
    pub fn records_at(&self, now: u64) -> Vec<StepRecord> {
        self.all_records().into_iter().filter(|r| r.usable(now)).collect()
    }

    /// Every record that parses, usable or not (the report counts what it dropped).
    pub fn all_records(&self) -> Vec<StepRecord> {
        // The older file first, then the current one: the newest records are the last.
        let text = format!(
            "{}{}",
            std::fs::read_to_string(self.older()).unwrap_or_default(),
            std::fs::read_to_string(&self.path).unwrap_or_default()
        );
        let lines: Vec<&str> = text.lines().collect();
        let from = lines.len().saturating_sub(READ_LINES);
        lines[from..]
            .iter()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|v| StepRecord::from_json(&v))
            .collect()
    }

    /// The limit for a class ([`limit_from`]) from the file.
    pub fn limit(&self, kind: &str, ws: &str, cold: bool, signals: bool, fallback: Duration) -> Option<Duration> {
        limit_from(&self.records(), kind, ws, cold, signals, fallback)
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
            signals: false,
            at: now_secs(),
            tool: TOOL.into(),
            rev: REV,
            ended_by: None,
            would_stop_secs: None,
        }
    }

    const FALLBACK: Duration = Duration::from_secs(15 * 60);

    #[test]
    fn with_too_few_passed_steps_there_is_no_limit_and_the_caller_falls_back() {
        let few: Vec<_> = (0..MIN_SAMPLES - 1).map(|_| rec("clippy", "crates", false, true, 3.0)).collect();
        assert_eq!(limit_from(&few, "clippy", "crates", false, false, FALLBACK), None);
        assert_eq!(limit_from(&[], "clippy", "crates", false, false, FALLBACK), None);
    }

    #[test]
    fn the_limit_is_a_multiple_of_the_longest_gap_between_the_floor_and_the_fallback() {
        let mk = |gap: f64| -> Vec<StepRecord> { (0..MIN_SAMPLES).map(|_| rec("clippy", "crates", false, true, gap)).collect() };
        // quick units: the floor
        assert_eq!(limit_from(&mk(3.0), "clippy", "crates", false, false, FALLBACK), Some(FLOOR));
        // a slow unit: K times it
        assert_eq!(
            limit_from(&mk(60.0), "clippy", "crates", false, false, FALLBACK),
            Some(Duration::from_secs(240))
        );
        // never above the fallback: the history only tightens
        assert_eq!(limit_from(&mk(900.0), "clippy", "crates", false, false, FALLBACK), Some(FALLBACK));
    }

    #[test]
    fn cold_and_warm_workspaces_and_kinds_are_never_mixed_and_failures_do_not_count() {
        let mut v: Vec<StepRecord> = (0..MIN_SAMPLES).map(|_| rec("doctest", "apps/dyno", true, true, 200.0)).collect();
        v.extend((0..MIN_SAMPLES).map(|_| rec("doctest", "apps/dyno", false, true, 2.0)));
        v.extend((0..MIN_SAMPLES).map(|_| rec("doctest", "crates", false, false, 500.0)));
        assert_eq!(
            limit_from(&v, "doctest", "apps/dyno", true, false, FALLBACK),
            Some(Duration::from_secs(800))
        );
        assert_eq!(limit_from(&v, "doctest", "apps/dyno", false, false, FALLBACK), Some(FLOOR));
        assert_eq!(
            limit_from(&v, "doctest", "crates", false, false, FALLBACK),
            None,
            "failed steps are no evidence"
        );
        assert_eq!(limit_from(&v, "clippy", "apps/dyno", true, false, FALLBACK), None);
    }

    /// Review P2-4: the file never grows without bound: past the size it becomes the older file (replacing the one before), the records
    /// of both are read, and the oldest are gone.
    #[test]
    fn the_file_is_rotated_by_size_and_both_parts_are_read() {
        let d = tempfile::tempdir().unwrap();
        let h = History::with_max(d.path().join("step-history.jsonl"), 2000);
        for i in 0..40 {
            h.append(&rec("clippy", "crates", false, true, i as f64));
        }
        let size = |p: &Path| std::fs::metadata(p).map_or(0, |m| m.len());
        assert!(
            size(h.path()) < 2000 + 450,
            "the current file stays near the limit: {}",
            size(h.path())
        );
        assert!(size(&h.older()) > 0, "the previous part was kept");
        let recs = h.records();
        assert!(recs.len() > 5 && recs.len() < 40, "the oldest records are gone: {}", recs.len());
        assert_eq!(recs.last().unwrap().max_gap_secs, 39.0, "the newest is last");
        let gaps: Vec<f64> = recs.iter().map(|r| r.max_gap_secs).collect();
        assert!(gaps.windows(2).all(|w| w[0] < w[1]), "in order across the two files: {gaps:?}");
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
        assert_eq!(h.limit("clippy", "crates", false, false, FALLBACK), Some(Duration::from_secs(120)));
    }

    /// The record carries the time it was written and the tool that wrote it, and they survive the file.
    #[test]
    fn a_record_carries_a_time_and_the_tool_version() {
        let d = tempfile::tempdir().unwrap();
        let h = History::new(d.path().join("step-history.jsonl"));
        let before = now_secs();
        h.append(&rec("clippy", "crates", false, true, 3.0));
        let text = std::fs::read_to_string(h.path()).unwrap();
        let v: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert!(v["at"].as_u64().unwrap() >= before, "{text}");
        assert!(v["tool"].as_str().unwrap().starts_with("paddock-dev "), "{text}");
        assert_eq!(v["rev"].as_u64(), Some(u64::from(REV)));
        let back = &h.records()[0];
        assert_eq!((back.tool.as_str(), back.rev), (TOOL, REV));
    }

    /// Hygiene (cause B of the 2026-11-23 audit): a record of the old shape or revision, one that claims a gap longer than its own run
    /// (heavy's wait counted as run time) and one past the age limit are never read; the same set with them kept would give a limit,
    /// which is the positive control that they would have changed the answer.
    #[test]
    fn records_of_an_old_rule_an_impossible_gap_or_a_great_age_are_not_used() {
        let d = tempfile::tempdir().unwrap();
        let h = History::new(d.path().join("step-history.jsonl"));
        // revision 1 had none of the new fields: it does not even parse
        let old = r#"{"kind":"clippy","ws":"crates","cold":false,"ok":true,"ran_secs":100.0,"max_gap_secs":230.0,"events":10}"#;
        std::fs::write(h.path(), format!("{old}\n")).unwrap();
        for _ in 0..MIN_SAMPLES {
            h.append(&StepRecord {
                rev: REV - 1,
                ..rec("clippy", "crates", false, true, 30.0)
            });
            h.append(&StepRecord {
                ran_secs: 100.0,
                ..rec("clippy", "crates", false, true, 230.0)
            });
            h.append(&StepRecord {
                at: now_secs() - MAX_AGE_SECS - 10,
                ..rec("clippy", "crates", false, true, 30.0)
            });
        }
        assert!(h.records().is_empty(), "{:?}", h.records());
        assert_eq!(h.all_records().len(), 3 * MIN_SAMPLES, "all of them parse, none is usable");
        assert_eq!(h.limit("clippy", "crates", false, false, FALLBACK), None);
        // control: the same records made good are used
        for _ in 0..MIN_SAMPLES {
            h.append(&rec("clippy", "crates", false, true, 30.0));
        }
        assert_eq!(h.limit("clippy", "crates", false, false, FALLBACK), Some(Duration::from_secs(120)));
    }

    /// Steps judged with the memory and disk signals and steps judged by their output alone have different gaps: never one limit for both.
    #[test]
    fn signal_and_output_only_steps_are_not_mixed() {
        let mut v: Vec<StepRecord> = (0..MIN_SAMPLES).map(|_| rec("clippy", "crates", false, true, 200.0)).collect();
        v.extend((0..MIN_SAMPLES).map(|_| StepRecord {
            signals: true,
            ..rec("clippy", "crates", false, true, 2.0)
        }));
        assert_eq!(
            limit_from(&v, "clippy", "crates", false, false, FALLBACK),
            Some(Duration::from_secs(800))
        );
        assert_eq!(limit_from(&v, "clippy", "crates", false, true, FALLBACK), Some(FLOOR));
    }
}
