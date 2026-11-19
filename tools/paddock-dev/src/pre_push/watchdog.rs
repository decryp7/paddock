//! The per-step watchdog of the push gate: what ends a step that has stopped making progress (the hang detector), apart from the
//! gate's one row cap ([`super::exec::ROW_CAP`]), which is only the last resort.
//!
//! This file is the plumbing; the detection rule is a separate design and plugs in here. A [`Watchdog`] is asked once per step
//! whether it wants to watch it ([`Watchdog::watch`]); the [`StepWatch`] it returns is then shown an [`Observation`] of the running
//! step about once a second ([`WATCH_POLL`]) and answers with a [`Stuck`] when the step should end. The executor ends the step's
//! process tree through `paddock_proc` (identity: pid and start time, rule 53), reports the row as `not measured: stuck` and names the
//! guard in the summary. Pure: no process and no clock is read here, so every rule is tested with an observation in hand.
//!
//! Time queued for the machine-wide heavy slot is never watched (ER-43: the waiter takes no slot and a queue is not a hang): the
//! executor shows a step to its watch only from the moment the slot is taken (or from the start, for a step that needs none).

use super::progress::Progress;
use paddock_proc::ProcessId;
use std::time::{Duration, Instant};

/// How often a running step is shown to its watch. A second is nothing next to the minutes a hang is judged over.
pub const WATCH_POLL: Duration = Duration::from_secs(1);

/// Which step is about to run.
#[derive(Debug, Clone, Copy)]
pub struct StepInfo<'a> {
    /// The row id (`doctest crates`, `clippy-windows apps/dyno`).
    pub id: &'a str,
    /// Whether it goes through the heavy slot.
    pub heavy: bool,
    /// How its output is read for progress ([`super::progress`]).
    pub progress: Progress,
    /// The row kind without its workspace (`clippy`, `clippy-windows`, `doctest`, ...): the history's key with [`StepInfo::ws`].
    pub kind: &'a str,
    /// The workspace folder the step runs in (empty: the repository root).
    pub ws: &'a str,
    /// The step's build folder was cold when it started (nothing built in it yet): cold and warm runs differ by about 85 times and
    /// are never mixed (the research note, section 2).
    pub cold: bool,
}

/// What the executor can see of a running step.
#[derive(Debug, Clone, Copy)]
pub struct Observation<'a> {
    /// When this reading was taken, on the executor's clock.
    pub now: Instant,
    /// How long the step has been running: since it took the slot (a step that needs none: since its start). Queue time is not in it.
    pub active_for: Duration,
    /// The size of the step's output file now (it only grows).
    pub log_len: u64,
    /// The last non-empty line of the output so far (empty when there is none).
    pub last_line: &'a str,
    /// How many progress events the step's output has shown ([`super::progress::Counter::events`]): a unit finished, a test line.
    pub progress: u64,
    /// The line of the last progress event.
    pub last_event: &'a str,
    /// Cargo's wait for another cargo's lock, while the step is in it: a wait state, never a stall.
    pub waiting_on: Option<&'a str>,
    /// The CPU time of the step's whole process tree, when the OS reports it.
    pub cpu: Option<Duration>,
    /// The step's process (pid and start time).
    pub root: ProcessId,
}

/// A watch's verdict: the step is stuck and should end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stuck {
    /// The name of the guard that decided, as the summary prints it.
    pub guard: String,
    /// Why, in one clause (the signal that stopped).
    pub reason: String,
    /// How long it has shown no progress.
    pub quiet: Duration,
}

/// The watch of one step.
pub trait StepWatch: Send {
    /// Looks at the step; `Some` ends it. Called about once per [`WATCH_POLL`] while the step runs, never while it queues.
    fn check(&mut self, seen: &Observation<'_>) -> Option<Stuck>;

    /// The step has ended (`ran` seconds, `ok` when it passed): the watch writes what it learned (history) and returns a note for the
    /// step's phase line (for example "would have stopped at 912 s"), or `None`.
    fn finish(&mut self, _ran: f64, _ok: bool) -> Option<String> {
        None
    }
}

/// The detector: chooses a watch for each step.
pub trait Watchdog: Send + Sync {
    /// The detector's name, printed in the run's header.
    fn name(&self) -> &str;
    /// The watch of this step, or `None` to leave it to the row cap.
    fn watch(&self, step: &StepInfo<'_>) -> Option<Box<dyn StepWatch>>;
}

/// No detector: no step is ended as stuck, only the row cap ends one.
#[derive(Debug, Clone, Copy, Default)]
pub struct Never;

impl Watchdog for Never {
    fn name(&self) -> &str {
        "none (the row cap only)"
    }

    fn watch(&self, _step: &StepInfo<'_>) -> Option<Box<dyn StepWatch>> {
        None
    }
}

/// How a step ended in the words of the log and the summary: `exit 0`, `stuck (guard): ...`, `timed out ...`.
/// The text after `stuck (` up to `)` is the guard's name.
pub fn stuck_how(s: &Stuck) -> String {
    format!("stuck ({}): {}; no progress for {} s", s.guard, s.reason, s.quiet.as_secs())
}

/// The guard that ended a run, from how it ended: the watchdog's own name for `stuck (...)`, `row cap` for a run that reached the
/// cap, `None` for a run that ended by itself or that a person interrupted.
pub fn ended_by(how: &str) -> Option<String> {
    if let Some(rest) = how.strip_prefix("stuck (") {
        return rest.split_once(')').map(|(g, _)| g.to_string());
    }
    how.starts_with("timed out").then(|| "row cap".to_string())
}

/// One step's phases with their durations, one line (ER-27): queued for the slot, running, and how it ended; the guard that ended it
/// is named when one did.
pub fn phase_line(id: &str, queued: Option<f64>, ran: f64, how: &str, note: Option<&str>) -> String {
    let queued = queued.map_or(String::new(), |q| format!("queued {q:.1} s, "));
    let guard = ended_by(how).map_or(String::new(), |g| format!("; ended by guard: {g}"));
    let note = note.map_or(String::new(), |n| format!("; watchdog: {n}"));
    format!("pre-push: {id}: {queued}ran {ran:.1} s; ended: {how}{guard}{note}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guard_that_ended_a_step_is_read_back_from_how_it_ended() {
        let s = Stuck {
            guard: "fake".into(),
            reason: "nothing moved".into(),
            quiet: Duration::from_secs(300),
        };
        let how = stuck_how(&s);
        assert_eq!(how, "stuck (fake): nothing moved; no progress for 300 s");
        assert_eq!(ended_by(&how).as_deref(), Some("fake"));
        assert_eq!(ended_by("timed out after 10800 s of running").as_deref(), Some("row cap"));
        for by_itself in ["exit 0", "exit 101", "cancelled", "queue limit: no slot taken in time"] {
            assert_eq!(ended_by(by_itself), None, "{by_itself}");
        }
    }

    #[test]
    fn a_phase_line_has_the_durations_and_names_the_guard() {
        assert_eq!(
            phase_line("doctest crates", Some(12.34), 45.6, "exit 0", None),
            "pre-push: doctest crates: queued 12.3 s, ran 45.6 s; ended: exit 0"
        );
        let line = phase_line("clippy crates", None, 3.0, "timed out after 5 s of running", None);
        assert!(line.contains("ran 3.0 s") && line.ends_with("ended by guard: row cap"), "{line}");
    }

    #[test]
    fn no_detector_watches_nothing() {
        let step = StepInfo {
            id: "x",
            heavy: true,
            progress: Progress::Lines,
            kind: "x",
            ws: "",
            cold: false,
        };
        assert!(Never.watch(&step).is_none());
    }
}
