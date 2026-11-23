//! `./dev gate-watchdog report`: what the progress watchdog did in its report-only window, from the files the push gate leaves
//! (docs/research/2026-11-19-gate-hang-detection.md; the audit of 2026-11-23 found 11 false alarms in 13 calls, so enforcing needs a
//! clean window on record first, ER-41).
//!
//! Two sources, both written by the gate (ER-27):
//! - `step-history.jsonl` in the gate's state folder ([`super::history`]): one record per watched step with whether it passed, how it
//!   ended (`ended_by`: the guard that ended it, none when it ended by itself) and when the watchdog first said "would have stopped".
//!   It is the durable one (the run folders below are pruned to the newest 20).
//! - `logs/<run>/phases.log`: the phase line of each step with the watchdog's note and its signal readings; the report shows these as the
//!   evidence of the calls.
//!
//! A step the watchdog called stuck is, by how it ended:
//! - **false alarm**: it passed (it was healthy). The number that must be 0 before enforcing.
//! - **catch**: the row cap (or, enforcing, the watchdog) ended it: the call was right, and the report says how many seconds earlier.
//! - **ended on its own, failed**: it failed by itself after the call; neither right nor wrong.
//!
//! A **miss** is a step the row cap ended that the watchdog never called. Pure: records and text in, a summary and its words out.

use super::history::{History, StepRecord, now_secs};
use clap::{Args, Subcommand};
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::path::{Path, PathBuf};

/// The window that counts as clean: at least this many days of records …
pub const CLEAN_DAYS: u64 = 7;
/// … holding at least this many watched steps (the audit of 2026-11-23 saw 13 calls in the 4 days since 11-19: a week of pushes holds a
/// few hundred steps). First values: the first window's own numbers revise them (ER-12; docs/benchmarks.md, "Push gate watchdog window").
pub const CLEAN_STEPS: usize = 100;

/// `gate-watchdog ...`
#[derive(Subcommand)]
pub enum WatchdogCommand {
    /// Summarise the watchdog's calls in the report-only window: false alarms, catches and misses, and whether the window is clean.
    Report(ReportArgs),
}

/// `gate-watchdog report`'s arguments.
#[derive(Args)]
pub struct ReportArgs {
    /// The repository (default: the current checkout); its gate state folder is `<git common dir>/paddock-pre-push`.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// The gate state folder itself (a test, or a copy of another machine's).
    #[arg(long)]
    pub state: Option<PathBuf>,
    /// The window: records of the last this many days (default [`CLEAN_DAYS`]).
    #[arg(long, default_value_t = CLEAN_DAYS)]
    pub days: u64,
    /// Watched steps the window must hold to count as clean (default [`CLEAN_STEPS`]).
    #[arg(long, default_value_t = CLEAN_STEPS)]
    pub min_steps: usize,
}

/// One call of the watchdog, as the history has it.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    /// The row kind and workspace.
    pub step: String,
    /// When the record was written (seconds since the epoch).
    pub at: u64,
    /// Seconds into the run at which it said "would have stopped".
    pub said_at: f64,
    /// The run's seconds.
    pub ran: f64,
    /// The verdict on the call.
    pub verdict: Verdict,
}

/// How a call turned out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The step passed: it was healthy.
    FalseAlarm,
    /// A guard ended the step: the call was right.
    Catch,
    /// The step failed by itself.
    EndedOnItsOwn,
}

/// What the window holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Usable records in the window (watched steps).
    pub watched: usize,
    /// Of those, judged with the memory and disk signals.
    pub with_signals: usize,
    /// Records of the window the history refuses (an old rule, an impossible gap): not counted.
    pub unusable: usize,
    /// The calls, oldest first.
    pub calls: Vec<Call>,
    /// Steps the row cap ended that the watchdog never called.
    pub misses: usize,
    /// The first and last record's time, when there are any.
    pub span: Option<(u64, u64)>,
}

impl Summary {
    /// Calls on healthy steps.
    pub fn false_alarms(&self) -> usize {
        self.calls.iter().filter(|c| c.verdict == Verdict::FalseAlarm).count()
    }

    /// Calls a guard proved right.
    pub fn catches(&self) -> usize {
        self.calls.iter().filter(|c| c.verdict == Verdict::Catch).count()
    }

    /// Calls on steps that failed by themselves.
    pub fn ended_on_their_own(&self) -> usize {
        self.calls.iter().filter(|c| c.verdict == Verdict::EndedOnItsOwn).count()
    }

    /// Whole days the records span.
    pub fn days(&self) -> u64 {
        self.span.map_or(0, |(a, b)| b.saturating_sub(a) / 86_400)
    }

    /// Why the window is not clean (empty: it is): a false alarm, too few days or too few steps.
    pub fn not_clean_because(&self, days: u64, min_steps: usize) -> Vec<String> {
        let mut why = Vec::new();
        if self.false_alarms() > 0 {
            why.push(format!(
                "{} false alarm(s): healthy steps the watchdog called stuck",
                self.false_alarms()
            ));
        }
        if self.days() < days {
            why.push(format!("the records span {} day(s), a clean window needs {days}", self.days()));
        }
        if self.watched < min_steps {
            why.push(format!("{} watched step(s), a clean window needs {min_steps}", self.watched));
        }
        why
    }
}

/// Whether a guard name is a watchdog's (it ended the step in enforce mode) and not the row cap.
fn is_watchdog(guard: &str) -> bool {
    guard.starts_with("progress watchdog")
}

/// The summary of `records` (as [`History::all_records`] gives them) for the window of `days` days before `now`.
pub fn summarise(records: &[StepRecord], now: u64, days: u64) -> Summary {
    let from = now.saturating_sub(days.saturating_mul(86_400));
    let in_window: Vec<&StepRecord> = records.iter().filter(|r| r.at >= from).collect();
    let (usable, unusable): (Vec<&StepRecord>, Vec<&StepRecord>) = in_window.into_iter().partition(|r| r.usable(now));
    let mut calls = Vec::new();
    let mut misses = 0;
    for r in &usable {
        let step = if r.ws.is_empty() {
            r.kind.clone()
        } else {
            format!("{} {}", r.kind, r.ws)
        };
        match (&r.would_stop_secs, &r.ended_by) {
            (Some(said), by) => calls.push(Call {
                step,
                at: r.at,
                said_at: *said,
                ran: r.ran_secs,
                verdict: match (by, r.ok) {
                    (Some(_), _) => Verdict::Catch,
                    (None, true) => Verdict::FalseAlarm,
                    (None, false) => Verdict::EndedOnItsOwn,
                },
            }),
            // Enforcing: the watchdog ended it, so it never said "would have". That is a catch by the watchdog itself.
            (None, Some(by)) if is_watchdog(by) => calls.push(Call {
                step,
                at: r.at,
                said_at: r.ran_secs,
                ran: r.ran_secs,
                verdict: Verdict::Catch,
            }),
            (None, Some(_)) => misses += 1,
            (None, None) => {}
        }
    }
    Summary {
        watched: usable.len(),
        with_signals: usable.iter().filter(|r| r.signals).count(),
        unusable: unusable.len(),
        calls,
        misses,
        span: usable.iter().map(|r| r.at).min().zip(usable.iter().map(|r| r.at).max()),
    }
}

/// A `watchdog: would have stopped ...` note of a phase line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseCall {
    /// The row id.
    pub id: String,
    /// How the step ended, in the line's words.
    pub ended: String,
    /// The watchdog's note: when it would have stopped, why, and the signal readings.
    pub note: String,
}

/// The calls in the text of one `phases.log` ([`super::watchdog::phase_line`] writes each line).
pub fn phase_calls(text: &str) -> Vec<PhaseCall> {
    text.lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("pre-push: ")?;
            let (head, note) = rest.split_once("; watchdog: ")?;
            if !note.starts_with("would have stopped") {
                return None;
            }
            let (id, tail) = head.split_once(": ")?;
            let ended = tail.split_once("; ended: ").map_or("", |(_, e)| e);
            Some(PhaseCall {
                id: id.to_string(),
                ended: ended.to_string(),
                note: note.to_string(),
            })
        })
        .collect()
}

/// The words of the report. `phases` are the calls found in the retained run logs (the evidence), `days` and `min_steps` the clean
/// window asked for.
pub fn words(s: &Summary, phases: &[PhaseCall], days: u64, min_steps: usize) -> String {
    let mut out = format!(
        "gate watchdog, last {days} day(s): {} watched step(s) ({} judged with memory and disk signals), {} record(s) refused (an old rule or an impossible gap)\n",
        s.watched, s.with_signals, s.unusable
    );
    out.push_str(&format!(
        "  calls: {}: false alarms {} (healthy steps called stuck), catches {} (a guard ended the step), ended on their own and failed {}\n",
        s.calls.len(),
        s.false_alarms(),
        s.catches(),
        s.ended_on_their_own()
    ));
    out.push_str(&format!(
        "  misses: {} (the row cap ended a step the watchdog never called)\n",
        s.misses
    ));
    for c in &s.calls {
        out.push_str(&format!(
            "  {:?}: {} said at {:.0} s of a {:.0} s run (record time {})\n",
            c.verdict, c.step, c.said_at, c.ran, c.at
        ));
    }
    for p in phases.iter().rev().take(10) {
        out.push_str(&format!("  evidence: {}: ended: {}; {}\n", p.id, p.ended, p.note));
    }
    let why = s.not_clean_because(days, min_steps);
    if why.is_empty() {
        out.push_str(
            "CLEAN: record this window in docs/benchmarks.md (\"Push gate watchdog window\") and flip the one line `DEFAULT_MODE` in tools/paddock-dev/src/pre_push/stall.rs from `Mode::ReportOnly` to `Mode::Enforce`.\n",
        );
    } else {
        out.push_str(&format!(
            "NOT CLEAN: {}. Keep report-only; fix the cause of each call first (ER-12, ER-41).\n",
            why.join("; ")
        ));
    }
    out
}

/// `./dev gate-watchdog report`.
pub fn command(c: WatchdogCommand) -> Result<Report, CliError> {
    let WatchdogCommand::Report(a) = c;
    let state = match (&a.state, &a.root) {
        (Some(s), _) => s.clone(),
        (None, root) => {
            let root = match root {
                Some(r) => r.clone(),
                None => std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?,
            };
            super::state_dir(&root)
        }
    };
    let history = History::new(state.join("step-history.jsonl"));
    let now = now_secs();
    let summary = summarise(&history.all_records(), now, a.days);
    let phases = read_phase_calls(&state.join("logs"));
    let text = words(&summary, &phases, a.days, a.min_steps);
    let why = summary.not_clean_because(a.days, a.min_steps);
    Ok(Report::new(
        json!({
            "watched": summary.watched, "withSignals": summary.with_signals, "unusable": summary.unusable,
            "falseAlarms": summary.false_alarms(), "catches": summary.catches(), "endedOnTheirOwn": summary.ended_on_their_own(),
            "misses": summary.misses, "days": summary.days(), "clean": why.is_empty(), "notCleanBecause": why,
        }),
        text,
    ))
}

/// The calls of every retained run folder under `logs`, oldest folder first.
fn read_phase_calls(logs: &Path) -> Vec<PhaseCall> {
    let Ok(read) = std::fs::read_dir(logs) else { return Vec::new() };
    let mut dirs: Vec<(std::time::SystemTime, PathBuf)> = read
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    dirs.sort();
    dirs.into_iter()
        .filter_map(|(_, d)| std::fs::read_to_string(d.join("phases.log")).ok())
        .flat_map(|t| phase_calls(&t))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pre_push::history::{REV, TOOL};

    const DAY: u64 = 86_400;
    const NOW: u64 = 1_800_000_000;

    fn rec(at_days_ago: u64, ok: bool, would: Option<f64>, ended_by: Option<&str>) -> StepRecord {
        StepRecord {
            kind: "conformance".into(),
            ws: String::new(),
            cold: false,
            ok,
            ran_secs: 100.0,
            max_gap_secs: 20.0,
            events: 3,
            signals: true,
            at: NOW - at_days_ago * DAY,
            tool: TOOL.into(),
            rev: REV,
            ended_by: ended_by.map(str::to_string),
            would_stop_secs: would,
        }
    }

    /// The fixture of the audit's shapes: healthy steps with and without a call, a failure after a call, a catch and a miss.
    fn fixture() -> Vec<StepRecord> {
        let mut v: Vec<StepRecord> = (0..10).map(|i| rec(i % 7, true, None, None)).collect();
        v.push(rec(1, true, Some(80.0), None)); // false alarm
        v.push(rec(2, true, Some(70.0), None)); // false alarm
        v.push(rec(3, false, Some(90.0), None)); // failed on its own
        v.push(StepRecord {
            ran_secs: 3600.0,
            max_gap_secs: 3000.0,
            ..rec(4, false, Some(300.0), Some("row cap"))
        }); // catch
        v.push(StepRecord {
            ran_secs: 3600.0,
            max_gap_secs: 3000.0,
            ..rec(5, false, None, Some("row cap"))
        }); // miss
        v.push(StepRecord {
            rev: 1,
            ..rec(2, true, Some(50.0), None)
        }); // old rule: refused
        v.push(rec(30, true, Some(50.0), None)); // outside the window
        v
    }

    #[test]
    fn the_report_counts_false_alarms_catches_and_misses_from_a_fixture() {
        let s = summarise(&fixture(), NOW, 7);
        assert_eq!(s.watched, 15, "10 healthy + 2 false alarms + 1 failed + 1 catch + 1 miss");
        assert_eq!((s.false_alarms(), s.catches(), s.ended_on_their_own(), s.misses), (2, 1, 1, 1));
        assert_eq!(s.unusable, 1, "the old-rule record is counted as refused, not as a call");
        assert_eq!(s.with_signals, 15);
        // positive control: without the window the old record outside it would be a third false alarm
        assert_eq!(summarise(&fixture(), NOW, 60).false_alarms(), 3);
        // a window with a false alarm is not clean, whatever else it holds
        let why = s.not_clean_because(5, 10);
        assert_eq!(why.len(), 1, "{why:?}");
        assert!(why[0].contains("2 false alarm"), "{why:?}");
    }

    #[test]
    fn a_window_is_clean_only_with_no_false_alarm_enough_days_and_enough_steps() {
        let clean: Vec<StepRecord> = (0..120).map(|i| rec(i % 8, true, None, None)).collect();
        let s = summarise(&clean, NOW, 8);
        assert!(s.not_clean_because(7, 100).is_empty(), "{:?}", s.not_clean_because(7, 100));
        // controls: too few steps, too few days, one false alarm each break it
        assert!(!summarise(&clean[..20], NOW, 8).not_clean_because(7, 100).is_empty());
        let short: Vec<StepRecord> = (0..120).map(|i| rec(i % 3, true, None, None)).collect();
        assert!(!summarise(&short, NOW, 8).not_clean_because(7, 100).is_empty());
        let mut one = clean.clone();
        one.push(rec(1, true, Some(60.0), None));
        assert!(!summarise(&one, NOW, 8).not_clean_because(7, 100).is_empty());
        // no records at all is never clean
        assert!(!summarise(&[], NOW, 7).not_clean_because(7, 100).is_empty());
    }

    #[test]
    fn an_enforced_stop_is_a_catch_by_the_watchdog() {
        let r = rec(1, false, None, Some("progress watchdog, limit 300 s"));
        let s = summarise(&[r], NOW, 7);
        assert_eq!((s.catches(), s.misses), (1, 0));
    }

    #[test]
    fn the_phase_lines_of_the_calls_are_read_with_their_readings() {
        let text = "\
pre-push: fmt crates: ran 3.1 s; ended: exit 0
pre-push: conformance: ran 91.0 s; ended: exit 0; watchdog: would have stopped at 70 s of run: no unit or line finished for 70 s; at the end signals: tree memory 80 MiB (0 moves over 1 MiB), disk 1 MiB (0 growths), 2 output events
pre-push: doctest crates: queued 4.0 s, ran 456.0 s; ended: timed out after 3600 s of running; ended by guard: row cap; watchdog: would have stopped at 300 s of run: x
";
        let calls = phase_calls(text);
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert_eq!(calls[0].id, "conformance");
        assert_eq!(calls[0].ended, "exit 0");
        assert!(calls[0].note.contains("tree memory 80 MiB"), "{:?}", calls[0]);
        assert!(calls[1].ended.starts_with("timed out"), "{:?}", calls[1]);
    }

    #[test]
    fn the_words_say_the_flip_only_for_a_clean_window() {
        let clean: Vec<StepRecord> = (0..120).map(|i| rec(i % 8, true, None, None)).collect();
        let ok = words(&summarise(&clean, NOW, 8), &[], 7, 100);
        assert!(ok.contains("CLEAN: record this window") && ok.contains("false alarms 0"), "{ok}");
        let bad = words(&summarise(&fixture(), NOW, 7), &[], 7, 100);
        assert!(
            bad.contains("NOT CLEAN") && bad.contains("false alarms 2") && bad.contains("misses: 1"),
            "{bad}"
        );
        assert!(!bad.contains("CLEAN: record"), "{bad}");
    }

    /// The command reads the files the gate writes: a history file and a retained run's phases.log.
    #[test]
    fn the_command_reads_the_state_folder() {
        let d = tempfile::tempdir().unwrap();
        let h = History::new(d.path().join("step-history.jsonl"));
        for r in [
            StepRecord {
                at: now_secs(),
                ..rec(0, true, Some(80.0), None)
            },
            StepRecord {
                at: now_secs(),
                ..rec(0, true, None, None)
            },
        ] {
            h.append(&r);
        }
        let run = d.path().join("logs/run1");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(
            run.join("phases.log"),
            "pre-push: conformance: ran 9.0 s; ended: exit 0; watchdog: would have stopped at 8 s of run: r\n",
        )
        .unwrap();
        let rep = command(WatchdogCommand::Report(ReportArgs {
            root: None,
            state: Some(d.path().to_path_buf()),
            days: 7,
            min_steps: 1,
        }))
        .unwrap();
        let v = rep.json();
        assert_eq!(
            (v["watched"].as_u64(), v["falseAlarms"].as_u64(), v["clean"].as_bool()),
            (Some(2), Some(1), Some(false)),
            "{v}"
        );
        assert!(rep.text.contains("evidence: conformance"), "{}", rep.text);
    }
}
