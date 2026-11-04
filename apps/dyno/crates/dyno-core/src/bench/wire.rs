//! The JSON lines a `dyno-bench` child writes on its stdout, and the pure judgements the agent makes from them (ADR 0030
//! section 2). One line is one event: `progress` (which phase, which repetition, a count), `heartbeat` (written by a separate
//! thread once a second, so a long kernel step never looks like a hang) and, last and once, `result`. Everything here is pure:
//! time is passed in as a [`Duration`] on the caller's monotonic clock.

use super::{BenchKind, BenchStatus, GAP_MILLIS, REPETITIONS, SECTION_MILLIS, WARMUP_MILLIS};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The longest line the agent reads from a child, bytes. A longer one ends the kind as failed.
pub const MAX_LINE: usize = 16 * 1024;
/// The most lines the agent reads from one child.
pub const MAX_LINES: usize = 50_000;
/// The longest phase id or label, characters.
pub const MAX_PHASE_TEXT: usize = 120;
/// The longest note, characters.
pub const MAX_NOTE: usize = 200;
/// How often a child writes its heartbeat.
pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(1);
/// A heartbeat this long overdue means the child is not responding.
pub const SILENT_AFTER: Duration = Duration::from_secs(30);

/// Where a child is, as it reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseProgress {
    /// Stable id of the phase, e.g. `cpu-single`.
    pub phase: String,
    /// Plain words for it, e.g. `Single core`.
    pub label: String,
    /// The repetitions finished (0 while warming up and during the first pass); a repetition counts when it ends, so this never
    /// goes backwards.
    pub rep: u32,
    /// How many repetitions there are.
    pub of: u32,
    /// How far, in the phase's own unit (a growing count when the total is unknown, never a static 0).
    pub count: u64,
    /// The total, when known.
    pub total: Option<u64>,
}

/// One line from a child.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "t")]
pub enum Line {
    /// Where the child is.
    Progress(PhaseProgress),
    /// The child is alive.
    Heartbeat,
    /// One second of a sustained run: the work done in that second, in the metric's unit.
    Sample {
        /// Seconds since the run began.
        secs: u32,
        /// The throughput that second.
        throughput: f64,
    },
    /// A fact about how the measurement was made, in words (for example that the cache was bypassed), kept with the result.
    Note {
        /// The words.
        text: String,
    },
    /// How the kind ended; the last line.
    Result {
        /// The outcome.
        status: BenchStatus,
    },
}

impl Line {
    /// The line as JSON, without the newline.
    pub fn to_json(&self) -> String {
        // A line of plain numbers and strings always serialises.
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"t":"heartbeat"}"#.to_string())
    }
}

/// Why a line was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireError {
    /// The line's number, from 1.
    pub line: usize,
    /// What is wrong.
    pub reason: String,
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.reason)
    }
}

/// Parses one line (parse, don't validate: after this the agent works with the type). Never panics; refuses an over-long
/// line, malformed JSON, an unknown event and numbers that cannot be right.
pub fn parse_line(text: &str, line_no: usize) -> Result<Line, WireError> {
    let bad = |reason: String| WireError { line: line_no, reason };
    if text.len() > MAX_LINE {
        return Err(bad(format!("longer than {MAX_LINE} bytes")));
    }
    let line: Line = serde_json::from_str(text.trim()).map_err(|e| bad(format!("not a benchmark event ({e})")))?;
    if let Line::Progress(p) = &line {
        if p.phase.is_empty() || p.phase.chars().count() > MAX_PHASE_TEXT || p.label.chars().count() > MAX_PHASE_TEXT {
            return Err(bad("a phase id or label is empty or too long".into()));
        }
        if p.phase.chars().any(char::is_control) || p.label.chars().any(char::is_control) {
            return Err(bad("a phase id or label has control characters".into()));
        }
        if p.of == 0 || p.of > 1_000 || p.rep > p.of {
            return Err(bad(format!("repetition {} of {} is impossible", p.rep, p.of)));
        }
        if p.total.is_some_and(|t| p.count > t) {
            return Err(bad("a count beyond its total".into()));
        }
    }
    if let Line::Note { text } = &line {
        if text.is_empty() || text.chars().count() > MAX_NOTE || text.chars().any(char::is_control) {
            return Err(bad("a note that is empty, too long or has control characters".into()));
        }
    }
    if let Line::Sample { throughput, .. } = &line {
        if !throughput.is_finite() || *throughput < 0.0 {
            return Err(bad("a sample that is not a finite, non-negative number".into()));
        }
    }
    Ok(line)
}

/// How alive a run looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Liveness {
    /// Beating, inside the plan's bound.
    Running,
    /// Beating, but past the plan's bound: still working, the person decides.
    StillWorking,
    /// No heartbeat for [`SILENT_AFTER`]: the child may be stuck; the person decides (Cancel). Nothing ends it by itself.
    NotResponding,
}

/// Judges a run. `started` is when the child began, `last_beat` when its last line (of any kind) arrived, `bound` the plan's
/// bound, all on one monotonic clock.
pub fn liveness(now: Duration, started: Duration, last_beat: Duration, bound: Duration) -> Liveness {
    if now.saturating_sub(last_beat) >= SILENT_AFTER {
        Liveness::NotResponding
    } else if now.saturating_sub(started) > bound {
        Liveness::StillWorking
    } else {
        Liveness::Running
    }
}

/// The slowest drive the plan bound allows for, bytes per second (a slow USB stick).
pub const SLOW_DRIVE_BYTES_PER_S: u64 = 10_000_000;
/// What a flush after a write pass may take on that drive, seconds (a pass moves at most what the slow drive moves in its time limit).
pub const FLUSH_ALLOWANCE_SECS: u64 = 6;

/// The time the disk test takes on one drive at the slowest speed the bound allows: the fill at that speed, plus for each
/// repetition and each of the eight measurements its ramp and measured time limits and a flush allowance. Phases end on bytes or
/// time, whichever comes first, so the time limits cap what a slow drive can take.
pub fn disk_duration(file_mib: u32, reps: u32) -> Duration {
    use crate::bench::disk::{MEASURE_MAX_MILLIS, MIB, PATTERNS, RAMP_MAX_MILLIS};
    let fill = Duration::from_secs_f64((u64::from(file_mib) * MIB) as f64 / SLOW_DRIVE_BYTES_PER_S as f64);
    let per_phase = Duration::from_millis(RAMP_MAX_MILLIS + MEASURE_MAX_MILLIS) + Duration::from_secs(FLUSH_ALLOWANCE_SECS);
    fill + per_phase * (u32::try_from(PATTERNS.len() * 2).unwrap_or(8) * reps)
}

/// The time the method itself takes for one kind: warm-up, the timed sections of every repetition and the gaps between them.
pub fn kind_duration(kind: BenchKind) -> Duration {
    kind_duration_with(kind, crate::bench::DISK_DEFAULT_MIB)
}

/// [`kind_duration`] with the disk file's size given.
pub fn kind_duration_with(kind: BenchKind, disk_mib: u32) -> Duration {
    let sections = match kind {
        // Four kernels, each on one core and on all cores.
        BenchKind::Cpu => 8,
        // Four STREAM kernels on one thread and on all, and the latency sweep (about 16 points of 0.2 s, counted as 3 sections).
        BenchKind::Memory => 11,
        BenchKind::Disk => return disk_duration(disk_mib, crate::bench::disk::REPETITIONS),
        BenchKind::Gpu => return Duration::from_secs(crate::bench::estimate::GPU_SECONDS),
    };
    let reps = u64::from(REPETITIONS);
    Duration::from_millis(WARMUP_MILLIS + reps * sections * SECTION_MILLIS + (reps - 1) * GAP_MILLIS)
}

/// The plan's bound: the sum of each kind's time plus 50 %. Past it with a heartbeat the run shows "Still working".
pub fn plan_bound(kinds: &[BenchKind]) -> Duration {
    plan_bound_with(kinds, crate::bench::DISK_DEFAULT_MIB, 1)
}

/// [`plan_bound`] for a disk file of `disk_mib` MiB on each of `drives` drives (the disk test runs once per drive).
pub fn plan_bound_with(kinds: &[BenchKind], disk_mib: u32, drives: usize) -> Duration {
    plan_bound_full(kinds, disk_mib, drives, None, 0)
}

/// [`plan_bound_with`] where the CPU kind may be a sustained run of `sustained_secs` seconds.
pub fn plan_bound_full(
    kinds: &[BenchKind],
    disk_mib: u32,
    drives: usize,
    sustained_secs: Option<u32>,
    sustained_write_gib: u32,
) -> Duration {
    let sum: Duration = kinds
        .iter()
        .map(|k| {
            let one = match (k, sustained_secs) {
                (BenchKind::Cpu, Some(s)) => Duration::from_secs(u64::from(s)),
                // The sustained write ends by its own time limit at the latest.
                (BenchKind::Disk, _) if sustained_write_gib > 0 => {
                    kind_duration_with(*k, disk_mib) + Duration::from_secs(crate::bench::disk::SUSTAINED_WRITE_MAX_SECS)
                }
                _ => kind_duration_with(*k, disk_mib),
            };
            if *k == BenchKind::Disk {
                one * u32::try_from(drives.max(1)).unwrap_or(1)
            } else {
                one
            }
        })
        .sum();
    // Even a kind with no timed phase (GPU unavailable) gets time to start and answer.
    (sum + Duration::from_secs(10)).mul_f64(1.5)
}

/// What to do with a power change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preflight {
    /// Start.
    Go,
    /// On battery without the person's consent for this run: ask first.
    NeedsConsent,
}

/// Before a run: on battery needs the person's consent, for this run only (D6).
pub fn battery_preflight(on_battery: Option<bool>, consent: bool) -> Preflight {
    if on_battery == Some(true) && !consent {
        Preflight::NeedsConsent
    } else {
        Preflight::Go
    }
}

/// During a run: whether to stop because the machine switched to battery without consent (the safety stop, named as such).
/// An unknown power source never stops a run.
pub fn stop_for_battery(now_on_battery: Option<bool>, consent: bool) -> bool {
    now_on_battery == Some(true) && !consent
}

/// The words for the safety stop on battery.
pub const BATTERY_STOP_REASON: &str = "Stopped for safety: the computer switched to battery and you had not agreed to run on battery.";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::BenchMetric;

    fn p(rep: u32, of: u32) -> Line {
        Line::Progress(PhaseProgress {
            phase: "cpu-single".into(),
            label: "Single core".into(),
            rep,
            of,
            count: 3,
            total: None,
        })
    }

    #[test]
    fn every_line_round_trips() {
        let lines = [
            p(2, 5),
            Line::Heartbeat,
            Line::Result {
                status: BenchStatus::Done {
                    metrics: vec![BenchMetric::higher("a", "A", 1.0, "u")],
                },
            },
            Line::Result {
                status: BenchStatus::Unavailable { reason: "none".into() },
            },
        ];
        for l in lines {
            assert_eq!(parse_line(&l.to_json(), 1).unwrap(), l);
        }
    }

    #[test]
    fn bad_lines_are_refused_with_their_number_and_never_panic() {
        let long = format!(r#"{{"t":"heartbeat","pad":"{}"}}"#, "x".repeat(MAX_LINE));
        for (text, needle) in [
            ("", "not a benchmark event"),
            ("{not json", "not a benchmark event"),
            (r#"{"t":"explode"}"#, "not a benchmark event"),
            (long.as_str(), "longer than"),
            (
                r#"{"t":"progress","phase":"","label":"x","rep":1,"of":5,"count":1,"total":null}"#,
                "empty or too long",
            ),
            (
                r#"{"t":"progress","phase":"a\u0007","label":"x","rep":1,"of":5,"count":1,"total":null}"#,
                "control",
            ),
            (
                r#"{"t":"progress","phase":"a","label":"x","rep":6,"of":5,"count":1,"total":null}"#,
                "impossible",
            ),
            (
                r#"{"t":"progress","phase":"a","label":"x","rep":1,"of":0,"count":1,"total":null}"#,
                "impossible",
            ),
            (
                r#"{"t":"progress","phase":"a","label":"x","rep":1,"of":5,"count":9,"total":3}"#,
                "beyond its total",
            ),
        ] {
            let e = parse_line(text, 7).unwrap_err();
            assert_eq!(e.line, 7);
            assert!(e.reason.contains(needle), "{text:.40}: {e}");
            assert!(e.to_string().starts_with("line 7: "));
        }
        let end = r#"{"t":"progress","phase":"a","label":"x","rep":0,"of":5,"count":0,"total":null}"#;
        assert!(parse_line(end, 1).is_ok(), "repetition 0 is the warm-up");
    }

    #[test]
    fn liveness_follows_the_heartbeat_and_the_bound_and_nothing_ends_a_run() {
        let s = Duration::from_secs;
        let bound = s(60);
        assert_eq!(liveness(s(10), s(0), s(9), bound), Liveness::Running);
        // A beating child past the bound is still working.
        assert_eq!(liveness(s(100), s(0), s(99), bound), Liveness::StillWorking);
        // A child that stopped beating is not responding, inside or past the bound.
        assert_eq!(liveness(s(40), s(0), s(9), bound), Liveness::NotResponding);
        assert_eq!(liveness(s(200), s(0), s(100), bound), Liveness::NotResponding);
        // Exactly SILENT_AFTER is silent; one second less is not.
        assert_eq!(liveness(s(39), s(0), s(10), bound), Liveness::Running);
        assert_eq!(liveness(s(40), s(0), s(10), bound), Liveness::NotResponding);
        // A clock that reads earlier than the last beat never underflows.
        assert_eq!(liveness(s(1), s(5), s(9), bound), Liveness::Running);
    }

    #[test]
    fn the_disk_bound_is_the_fill_at_ten_mb_per_s_plus_every_phase_at_its_time_limits() {
        // 1 GiB fill at 10 MB/s = 107.4 s; 3 repetitions x 8 measurements x (1 s ramp + 5 s measure + 6 s flush) = 288 s.
        let d = disk_duration(1024, 3);
        assert!((d.as_secs_f64() - (107.374_182_4 + 288.0)).abs() < 1e-6, "{d:?}");
        assert!(disk_duration(4096, 3) > d && disk_duration(256, 3) < d);
        // Two drives take twice as long; no drive chosen still bounds one.
        let one = plan_bound_with(&[BenchKind::Disk], 1024, 1);
        assert!(plan_bound_with(&[BenchKind::Disk], 1024, 2) > one);
        assert_eq!(plan_bound_with(&[BenchKind::Disk], 1024, 0), one);
    }

    #[test]
    fn the_plan_bound_grows_with_the_kinds() {
        let cpu = plan_bound(&[BenchKind::Cpu]);
        assert!(cpu > kind_duration(BenchKind::Cpu));
        assert!(plan_bound(&[BenchKind::Cpu, BenchKind::Memory]) > cpu);
        assert!(plan_bound(&[BenchKind::Gpu]) > Duration::ZERO);
        // 1 s warm-up, 5 repetitions of eight 1 s sections (four kernels, one core and all) and four 2 s gaps: 49 s.
        assert_eq!(kind_duration(BenchKind::Cpu), Duration::from_secs(49));
        // The same for memory with eleven sections: 64 s.
        assert_eq!(kind_duration(BenchKind::Memory), Duration::from_secs(64));
    }

    #[test]
    fn battery_consent_is_per_run_and_an_unknown_source_never_blocks() {
        assert_eq!(battery_preflight(Some(true), false), Preflight::NeedsConsent);
        assert_eq!(battery_preflight(Some(true), true), Preflight::Go);
        assert_eq!(battery_preflight(Some(false), false), Preflight::Go);
        assert_eq!(battery_preflight(None, false), Preflight::Go);
        assert!(stop_for_battery(Some(true), false));
        assert!(!stop_for_battery(Some(true), true));
        assert!(!stop_for_battery(Some(false), false));
        assert!(!stop_for_battery(None, false));
    }
}
