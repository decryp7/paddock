//! Running the gates: each one decided by an [`Exec`] (real processes, or a fake in the tests), several at a time, cheapest first,
//! each bounded by its own cap on its own run time and its wait for the heavy slot by the queue's progress (no total shared by the
//! rows, 2026-11-21), and stopped when the person stops the push.

use super::budget::{Slot, Timing, parse_heavy_timing, with_agent_name, with_heavy_flags};
use super::gates::{Gate, Need, Planned};
use super::linux_inputs::{ImageProbe, PROBE_NEXT, image_probe};
use super::progress::{Counter, Progress, with_cargo_json, without_json};
use super::stacks;
use super::watchdog::{Observation, StepInfo, StepWatch, Stuck, WATCH_POLL, Watchdog, phase_line, stuck_how};
use crate::ci_tool::{Output, ToolCall, tail};
use paddock_cli::{CheckId, Finding, Outcome, Reason, Subject, ToolVersion, UnixSeconds, measure};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What one row may run on its own, counted from the moment its command starts (from the heavy slot taken, never from the wait for it;
/// no total is shared by the rows of a push): the backstop, GitLab's default job timeout. Measured cause
/// (docs/research/2026-11-19-gate-hang-detection.md section 0, heavy's activity log 2026-11-14 to 19): the slowest healthy row known is
/// a cold app doctest build, 1122 s (docs/benchmarks.md, "Doctest row of the push gate"); 60 min is over three times it; no healthy
/// row ran longer. Before the stall judgement (WP-2) it is the only thing that ends a hung row. Queue time is not in it (a test reads
/// this constant: with no cap, the over-cap test is red).
pub const ROW_CAP: Duration = Duration::from_secs(60 * 60);

/// The name the summary gives the row cap as the guard that ended a step.
pub const ROW_CAP_GUARD: &str = "row cap";

/// How long after [`ROW_CAP`] the outer limits of a step (heavy's `--run-max`, the process timeout) still wait: the cap is enforced by
/// the executor's monitor, which logs the stacks before it ends the tree, so these are only the backstop if the monitor is gone.
///
/// At least the stack capture's own bound plus 15 s ([`stacks::TOTAL_LIMIT`]): the monitor ends the tree after the capture, and these
/// outer limits must not end it first, mid-capture (review d).
pub const CAP_GRACE: Duration = Duration::from_secs(stacks::TOTAL_LIMIT.as_secs() + 15);

/// How long a step's wait for the machine-wide heavy slot may go without the queue making progress (`PADDOCK_QUEUE_STALL` seconds
/// overrides). The wait itself is not bounded by its length and is never run time: while slots are taken and released or the step's place
/// in the queue moves, it keeps waiting; a queue that shows the same picture for this long ends the wait as "not measured: queue
/// stalled, held by <who>", never a pass (ADR 0102, ER-33). Measured cause (ER-12): the gaps between two queue events (a lock taken or
/// released) while somebody waited, heavy's activity log of 2026-11-14 13:02Z to 2026-11-21 10:12Z on this Mac: 1,386 gaps, p50 81 s,
/// p90 314 s, p95 453 s, p99 813 s, max 1,446 s (docs/benchmarks.md, "Pre-push heavy queue progress gaps"). 30 minutes is over the
/// longest gap seen; a holder that really runs longer is ended by its own row cap, which is the owner's guard, not this wait's.
pub const QUEUE_STALL: Duration = Duration::from_secs(30 * 60);

/// The most steps seen waiting in heavy's queue at once: the longest queue in heavy's activity log, 2026-11-14 to 2026-11-21 ("position 7
/// of 13"; docs/benchmarks.md, "Pre-push heavy queue progress gaps").
const QUEUE_LENGTH_SEEN: u64 = 13;

/// The slowest healthy row measured, in seconds: a cold doctest build, 1,122 s (docs/benchmarks.md, "Doctest row of the push gate").
const SLOWEST_HEALTHY_ROW: u64 = 1122;

/// The outer process limit on one step's whole wait for the heavy slot, so that no call runs unbounded (ER-32). It is a backstop and not
/// the judgement: a queue that keeps moving is waited for and `heavy`'s stall judgement ends one that does not, so this only ends a
/// `heavy` client that is itself hung. Derived from data: the longest queue seen ([`QUEUE_LENGTH_SEEN`], 13) times the slowest healthy
/// row ([`SLOWEST_HEALTHY_ROW`]), 4 h 5 min, doubled for a queue longer than any seen: 8 h 10 min.
pub const WAIT_BACKSTOP: Duration = Duration::from_secs(2 * QUEUE_LENGTH_SEEN * SLOWEST_HEALTHY_ROW);

/// What one step may wait: `stall` is how long the queue may show no progress (heavy's `--queue-stall`). Its run time has no limit
/// here: that is [`ROW_CAP`], from the moment it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most time the queue (heavy's slot, or the build folder) may go without progress.
    pub stall: Duration,
}

/// How a run ended, as a type (review P2-1): every decision about an ending (is it cut short, which guard ended it, must the container be
/// stopped, did it ever get a slot) reads this and never a text, so a new ending cannot be forgotten by one of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// It ended by itself, with any exit code.
    Exited,
    /// The watchdog ended it as stuck.
    Stuck {
        /// The guard's name.
        guard: String,
    },
    /// It reached the row cap (or the backstop beyond it).
    RowCap,
    /// The person stopped the push.
    Cancelled,
    /// The queue it waited in made no progress for the stall limit (or heavy's old `--queue-max`): the command never ran.
    QueueLimit,
    /// It wrote more output than the cap allows.
    OutputCap,
}

impl Ending {
    /// It did not end by itself: a guard, the cap or the person stopped it. Nothing is proven either way, and anything it started
    /// (a container) must be stopped.
    pub fn cut_short(&self) -> bool {
        !matches!(self, Ending::Exited | Ending::QueueLimit)
    }

    /// The guard that ended it, as the summary names it.
    pub fn guard(&self) -> Option<String> {
        match self {
            Ending::Stuck { guard } => Some(guard.clone()),
            Ending::RowCap => Some(ROW_CAP_GUARD.to_string()),
            _ => None,
        }
    }

    /// The ending a process reported in words, for a run that was not made by [`RealExec::run`] (a stand-in executor, `--warm-linux`).
    /// `RealExec::run` itself sets the ending where it knows why.
    pub fn from_how(how: &str) -> Ending {
        if how.starts_with("timed out") {
            Ending::RowCap
        } else if how == "cancelled" {
            Ending::Cancelled
        } else if how.starts_with("queue limit") {
            Ending::QueueLimit
        } else if how.starts_with("stopped at the output cap") {
            Ending::OutputCap
        } else {
            Ending::Exited
        }
    }
}

/// How one run ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    /// Exit status 0.
    pub success: bool,
    /// `exit 101`, `timed out after 2400 s`, `cancelled`.
    pub how: String,
    /// The last lines of the output, for the failure line.
    pub tail: String,
    /// The file that holds the whole output, when there is one.
    pub log: Option<String>,
    /// Queue and run seconds as `heavy` measured them; none for a gate that did not go through it.
    pub timing: Option<Timing>,
    /// How it ended, typed ([`Ending`]); `how` is the same in words for a person.
    pub ending: Ending,
}

impl Done {
    /// It did not end by itself: a guard or the gate's own limit stopped it, or the person did. Nothing is proven either way.
    pub fn cut_short(&self) -> bool {
        self.ending.cut_short()
    }

    /// The guard that ended the run ([`Ending::guard`]): the watchdog's name, or `row cap`.
    pub fn ended_by(&self) -> Option<String> {
        self.ending.guard()
    }

    /// `heavy` gave up waiting for a slot (the queue stalled): the command never ran.
    pub fn no_slot(&self) -> bool {
        self.ending == Ending::QueueLimit
    }
}

/// What the gate needs from the machine. The real one starts processes; the tests' one answers from a table (ER-14).
pub trait Exec: Sync {
    /// Is `program` on PATH?
    fn has_tool(&self, program: &str) -> bool;
    /// Does a Docker daemon answer? Never starts one.
    fn docker_up(&self) -> bool;
    /// Whether the local CI image the Linux Clippy runs in is built. Never builds it.
    fn ci_image(&self) -> super::linux_inputs::ImageProbe {
        super::linux_inputs::ImageProbe::Ready
    }
    /// Whether the container sees `compared`'s files as the host has them ([`super::mount_fresh`]). Never changes either side.
    /// A test's executor answers fresh unless it stands in for a stale mount.
    fn mount_probe(&self, compared: &super::mount_fresh::Compared) -> super::mount_fresh::MountProbe {
        super::mount_fresh::MountProbe::Fresh {
            checked: compared.files.len(),
            of: compared.of,
        }
    }
    /// Runs the gate within `limits` (a heavy step: at most `limits.run` of running and `limits.queue` of waiting for the slot).
    /// `Err`: it could not be started (the program is missing).
    fn run(&self, gate: &Gate, limits: Limits) -> Result<Done, String>;
    /// The 1-minute load average now, for the reason of a row the queue stopped (None: unknown).
    fn load(&self) -> Option<f64> {
        None
    }
    /// Why a heavy gate has no machine-wide slot to go through (no `heavy` on PATH and the build of tools/heavy failed), or
    /// `None`: it would otherwise compete with every other push (ER-24), so it is not measured instead.
    fn heavy_unavailable(&self) -> Option<String> {
        None
    }
    /// This program as the rows `./dev` runs itself start it ([`crate::tool_cache::self_exe`]), or why it cannot be started again.
    /// A test's executor gives a stand-in, so those rows reach it instead of being not measured in a test process.
    fn dev_program(&self) -> Result<String, String> {
        crate::tool_cache::self_exe()
            .map(|p| p.to_string_lossy().to_string())
            .map_err(|e| e.to_string())
    }
}

/// One table row: a shared [`Outcome`] (ADR 0089 amendment, WP-T3) plus what the table prints around it. A pass or a failure is
/// built only through [`measure`] (with the commit and tree as evidence); there is no status a caller can set by hand.
#[derive(Debug, Clone)]
pub struct Row {
    /// How the gate ended: `Clean` (ran and passed), `Findings` (ran and failed), `NotApplicable` (the change does not reach it),
    /// `NotMeasured` (it could not look: nothing is proven, ER-33) or `CheckBroken` (the check itself cannot be trusted).
    pub outcome: Outcome,
    /// Seconds the gate ran (0 when it did not).
    pub secs: f64,
    /// The tail of its output, for a failure.
    pub tail: String,
    /// What to do and how to re-run (failure, not measured, broken).
    pub next: String,
    /// A not-measured row a person named in the override: who. Logged; the push may go, but the row is still not proven.
    pub excused_by: Option<String>,
    /// The guard that ended the step: the watchdog's name, or `row cap` (the summary names it).
    pub ended_by: Option<String>,
    /// A failed step that ran nextest: the tests that ended failed, as nextest names them (binary and test).
    pub failed_tests: Vec<String>,
    /// A failed step that ran nextest: the first panic line of its output, with the panic's message.
    pub first_panic: Option<String>,
    /// The earlier green run this row rests on instead of a run of its own ([`super::reuse`]); `None` for a row that ran.
    pub reused: Option<super::reuse::Reused>,
}

/// The kind of a row, for the table and its JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// It ran and passed.
    Passed,
    /// It ran and failed.
    Failed,
    /// The change does not reach it.
    Skipped,
    /// It could not look.
    NotMeasured,
    /// Not measured, and a person named this gate in the override.
    Accepted,
    /// The check itself cannot be trusted.
    Broken,
}

fn check_id(id: &str) -> CheckId {
    CheckId::new(format!("pre-push:{id}"))
}

impl Row {
    fn plain(outcome: Outcome, secs: f64, tail: String, next: String) -> Row {
        Row {
            outcome,
            secs,
            tail,
            next,
            excused_by: None,
            ended_by: None,
            failed_tests: Vec::new(),
            first_panic: None,
            reused: None,
        }
    }

    /// A pass that rests on an earlier green run with the same key ([`super::reuse`]): a pass like any other for the verdict, made
    /// by [`measure`] for `subject`, and the table says when it was green.
    pub fn reused(id: &str, subject: &Subject, green: super::reuse::Reused) -> Row {
        let o = measure(check_id(id), subject.clone(), tool(), UnixSeconds::now(), || Ok(Vec::new()));
        let mut r = Row::plain(o, 0.0, String::new(), String::new());
        r.reused = Some(green);
        r
    }

    /// A pass this run measured itself: the only kind stored for reuse (never a reused row, a failure, a skipped, not-measured,
    /// broken or excused row).
    pub fn is_fresh_pass(&self) -> bool {
        matches!(self.outcome, Outcome::Clean(_)) && self.reused.is_none() && self.excused_by.is_none()
    }

    /// The row with the guard that ended its step named.
    pub fn ended_by_guard(mut self, guard: Option<String>) -> Row {
        self.ended_by = guard;
        self
    }

    /// The gate `id` ran to its end (not cut short): the measurement reads how it ended. A clean exit finds nothing; any other ends in
    /// one failing finding with `cause` in one line. `tail` and `next` are the caller's to set on a failure.
    pub fn from_run(id: &str, subject: &Subject, done: &Done, cause: impl Into<String>, secs: f64) -> Row {
        let cause = cause.into();
        let o = measure(check_id(id), subject.clone(), tool(), UnixSeconds::now(), || {
            if done.success {
                Ok(Vec::new())
            } else {
                Ok(vec![Finding::failing(cause)])
            }
        });
        Row::plain(o, secs, String::new(), String::new())
    }

    /// The gate `id` ran and failed: `cause` in one line, the `tail` of its output, `next` what to do.
    pub fn failed(id: &str, subject: &Subject, cause: String, tail: String, next: String, secs: f64) -> Row {
        let o = measure(check_id(id), subject.clone(), tool(), UnixSeconds::now(), || {
            Ok(vec![Finding::failing(cause)])
        });
        Row::plain(o, secs, tail, next)
    }

    /// The change does not reach it.
    pub fn skipped(reason: impl Into<String>) -> Row {
        Row::plain(Outcome::NotApplicable(Reason::new(reason)), 0.0, String::new(), String::new())
    }

    /// It could not look: nothing is proven.
    pub fn not_measured(reason: impl Into<String>, next: impl Into<String>) -> Row {
        Row::plain(Outcome::NotMeasured(Reason::new(reason)), 0.0, String::new(), next.into())
    }

    /// The check itself cannot be trusted.
    pub fn broken(cause: impl Into<String>, next: impl Into<String>) -> Row {
        Row::plain(Outcome::CheckBroken(Reason::new(cause)), 0.0, String::new(), next.into())
    }

    /// The row's kind.
    pub fn kind(&self) -> Kind {
        match (&self.outcome, &self.excused_by) {
            (Outcome::NotMeasured(_), Some(_)) => Kind::Accepted,
            (Outcome::Clean(_) | Outcome::KnownCommit(_), _) => Kind::Passed,
            (Outcome::Findings(_), _) => Kind::Failed,
            (Outcome::NotApplicable(_), _) => Kind::Skipped,
            (Outcome::NotMeasured(_), None) => Kind::NotMeasured,
            (Outcome::CheckBroken(_), _) => Kind::Broken,
        }
    }

    /// Why (not measured, skipped, broken), or the failure's cause.
    pub fn reason(&self) -> String {
        self.outcome.detail()
    }
}

/// The tool, with its version.
pub fn tool() -> ToolVersion {
    ToolVersion::new(concat!("paddock-dev pre-push ", env!("CARGO_PKG_VERSION")))
}

pub(crate) fn first_word(id: &str) -> &str {
    id.split(' ').next().unwrap_or(id)
}

/// The line `heavy` writes when it holds the slot for a step (`<agent> holds the lock (pid N) at ...`): before it, a step is queued.
const LOCK_TAKEN: &str = " holds the lock (pid ";
/// What heavy prints when it holds the slot for the step named `name`: before it, that step is queued. The step's own name (not
/// "pre-push") so a line of another step, in this log or a stale one, is not taken for it.
fn lock_line(name: &str) -> String {
    format!("{name}{LOCK_TAKEN}")
}

/// A step's log read from where it stopped: only the new bytes each time (a log can hold megabytes of compiler output), with the
/// last bytes of the previous read carried over so a line cut between two reads is still found.
struct LockSeen {
    needle: String,
    pos: u64,
    carry: String,
}

impl LockSeen {
    fn new(needle: String) -> LockSeen {
        LockSeen {
            needle,
            pos: 0,
            carry: String::new(),
        }
    }

    /// Whether the needle is in what `path` holds now (new bytes since the last call and the carried tail).
    fn seen(&mut self, path: &Path) -> bool {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut f) = std::fs::File::open(path) else { return false };
        if f.metadata().is_ok_and(|m| m.len() < self.pos) {
            // The file was truncated under us: start again.
            self.pos = 0;
            self.carry.clear();
        }
        if f.seek(SeekFrom::Start(self.pos)).is_err() {
            return false;
        }
        let mut buf = Vec::new();
        // Bounded per read: heavy's lines are short; the rest comes at the next poll.
        let Ok(n) = f.take(64 * 1024).read_to_end(&mut buf) else {
            return false;
        };
        self.pos += n as u64;
        self.carry.push_str(&String::from_utf8_lossy(&buf));
        if self.carry.contains(&self.needle) {
            return true;
        }
        let keep = self.needle.len().saturating_sub(1);
        if self.carry.len() > keep {
            let mut cut = self.carry.len() - keep;
            while !self.carry.is_char_boundary(cut) {
                cut += 1;
            }
            self.carry.drain(..cut);
        }
        false
    }
}

/// How often the log of a heavy step is read for [`LOCK_TAKEN`]: the queue time is read to this precision, and a second is
/// nothing next to the minutes a queue lasts.
const SLOT_POLL: Duration = Duration::from_millis(250);

/// The outer limit of one step (heavy's `--run-max`, the process timeout): [`ROW_CAP`] and [`CAP_GRACE`]. Nothing earlier rows used
/// of the push's time shortens it (the old shared budget's failure: a step got what earlier rows left of it).
fn own_run_limit(_gate: &Gate) -> Duration {
    ROW_CAP + CAP_GRACE
}

/// For a run `heavy` ended at its run limit (the backstop beyond the row cap), with the run time it had (empty for any other ending).
fn run_limit_note(done: &Done) -> String {
    match (done.ending == Ending::RowCap, done.timing) {
        (true, Some(t)) => format!("; the row cap of {} min spent after {:.0} s of run", ROW_CAP.as_secs() / 60, t.ran),
        _ => String::new(),
    }
}

/// A container row's mount probe ([`super::mount_fresh`]): `None` when the container sees the change's files as the host has them, else
/// the row's "not measured" verdict (a stale mount or a probe that could not look; ER-33). Logs the probe's verdict and duration (ER-27).
fn mount_not_fresh(gate: &Gate, exec: &dyn Exec) -> Option<Row> {
    use super::mount_fresh::{MountProbe, STALE_NEXT};
    let started = Instant::now();
    let compared = gate.mount.clone().unwrap_or_default();
    let probe = exec.mount_probe(&compared);
    let secs = started.elapsed().as_secs_f64();
    match probe {
        MountProbe::Fresh { checked, of } => {
            eprintln!(
                "pre-push: {}: the container's mount matches the host for {checked} of the change's {of} file(s) ({secs:.1} s)",
                gate.id
            );
            None
        }
        MountProbe::Stale { differing } => {
            eprintln!(
                "pre-push: {}: the container's mount is stale for {} file(s) ({secs:.1} s)",
                gate.id,
                differing.len()
            );
            let shown: Vec<&str> = differing.iter().take(5).map(String::as_str).collect();
            let more = differing.len().saturating_sub(shown.len());
            Some(Row::not_measured(
                format!(
                    "the container's bind mount is stale: it sees other content than the host for {}{}, so the row would check an old version",
                    shown.join(", "),
                    if more > 0 { format!(" and {more} more") } else { String::new() }
                ),
                STALE_NEXT,
            ))
        }
        MountProbe::Broken(why) => {
            eprintln!("pre-push: {}: the mount probe could not look ({secs:.1} s): {why}", gate.id);
            Some(Row::not_measured(
                format!("mount probe broken ({why}): it cannot be shown that the container sees the host's files"),
                format!(
                    "run `bash tools/linux-container.sh --id paddock --ci-image -- git status --short` yourself and read what it prints; {STALE_NEXT}"
                ),
            ))
        }
    }
}

/// The verdict of one gate: its needs first, then the run.
pub fn decide(gate: &Gate, exec: &dyn Exec, subject: &Subject) -> Row {
    decide_with(gate, exec, Limits { stall: QUEUE_STALL }, subject, &mut |_| {})
}

/// [`decide`], telling `seen` how the run ended (its queue and run time) before the row is made.
pub fn decide_with(gate: &Gate, exec: &dyn Exec, limits: Limits, subject: &Subject, seen: &mut dyn FnMut(&Done)) -> Row {
    match &gate.need {
        Need::Tool(t) if !exec.has_tool(t) => {
            return Row::not_measured(
                format!("{t} is not installed"),
                format!(
                    "install it (`cargo install {t} --locked`), then re-run `./dev check --pre-push`; an agent reports this instead (ER-33)"
                ),
            );
        }
        Need::Docker | Need::LocalCiImage if !exec.docker_up() => {
            return Row::not_measured(
                "Docker (OrbStack) is not running",
                "start it yourself and re-run; an agent reports this to the coordinator and never starts or restarts it (ER-14)",
            );
        }
        _ => {}
    }
    if gate.need == Need::LocalCiImage {
        match exec.ci_image() {
            ImageProbe::Ready => {}
            ImageProbe::Missing => {
                return Row::not_measured(
                    "local CI image missing",
                    "run `./dev check --warm-linux` (it builds the local CI image first, with its own bound, then warms the workspaces), then push again; the image build is never started under the push budget",
                );
            }
            ImageProbe::Broken(how) => {
                return Row::not_measured(format!("image probe broken ({how})"), PROBE_NEXT);
            }
        }
        if let Some(row) = mount_not_fresh(gate, exec) {
            return row;
        }
    }
    if gate.heavy
        && let Some(why) = exec.heavy_unavailable()
    {
        return Row::not_measured(
            format!("no heavy slot tool, so a compile would run beside every other push's: {why}"),
            "fix the build error above and re-run `./dev check --pre-push`; an agent reports this (ER-24, ER-33)",
        );
    }
    let started = Instant::now();
    let result = exec.run(gate, limits);
    if let Ok(done) = &result {
        seen(done);
    }
    // The run time of a step that went through heavy; the wall time (queue included) only when there is no timing.
    let secs = |d: &Done| d.timing.map_or_else(|| started.elapsed().as_secs_f64(), |t| t.ran);
    match result {
        Err(e) => Row::not_measured(e, "install the program, then re-run `./dev check --pre-push`"),
        // The queue stalled: the command never ran. Not measured, never a pass or a failure (ADR 0102 on ADR 0089's rows); the row
        // names who held the slot and for how long nothing moved.
        Ok(done) if done.no_slot() => Row::not_measured(
            format!(
                "queue stalled, held by {}: {} never got a heavy slot ({}; queued {:.1} s, load {}), so nothing was measured",
                stalled_holder(&done),
                gate.rerun(),
                done.how,
                done.timing.map_or(0.0, |t| t.queued),
                load_text(exec.load())
            ),
            format!(
                "look at `heavy status` for who holds the slot, and re-run `./dev check --pre-push --only {}` when it moves (the wait goes on while the queue moves and ends after {} min without progress: PADDOCK_QUEUE_STALL, in seconds); an agent reports this (ER-24, ER-33)",
                first_word(&gate.id),
                limits.stall.as_secs() / 60
            ),
        ),
        // The watchdog ended it: the step made no progress. Not measured, never a finding or a pass; the row names the step, the last
        // output line and how long nothing moved.
        Ok(done) if matches!(done.ending, Ending::Stuck { .. }) => Row::not_measured(
            format!(
                "stuck: {} was stopped by the watchdog ({}); last output line: {}{}",
                gate.rerun(),
                done.how,
                done.tail.trim_end().lines().last().map_or("(none)", str::trim),
                done.log.as_ref().map_or(String::new(), |l| format!("; output so far in {l}"))
            ),
            format!(
                "read the log, then re-run it alone: {} (a step the watchdog stops is hung or deadlocked, not a finding in your change); an agent reports it (ER-33)",
                gate.rerun()
            ),
        )
        .ended_by_guard(done.ended_by()),
        Ok(done) if done.cut_short() => Row::not_measured(
            format!(
                "{} ({}{}) before it finished{}",
                gate.rerun(),
                done.how,
                run_limit_note(&done),
                done.log.as_ref().map_or(String::new(), |l| format!("; output so far in {l}"))
            ),
            if gate.id.starts_with("clippy-linux") {
                format!(
                    "run `./dev check --warm-linux {}` once (no budget, live output; the first run of a workspace builds all its dependencies in the container's kept volume, minutes per workspace; docs/benchmarks.md), then push again",
                    gate.workspaces.join(" ")
                )
            } else {
                format!(
                    "re-run it alone: {} (a row may run {} min from the moment it starts; waiting for the heavy slot is not run time)",
                    gate.rerun(),
                    ROW_CAP.as_secs() / 60
                )
            },
        )
        .ended_by_guard(done.ended_by()),
        Ok(done) => {
            let hint = match (gate.need == Need::LocalCiImage, done.how.as_str()) {
                (true, "exit 125") => {
                    "docker could not start the container or build the local CI image (read the output above; `docker login dev.decryptology.net` if a base image cannot be pulled)"
                        .to_string()
                }
                (true, "exit 137") => {
                    "the container was killed (out of memory at 12g): raise PADDOCK_LINUX_MEMORY (for example 16g) and re-run".to_string()
                }
                _ => gate.next.clone(),
            };
            let full = done.log.as_ref().map_or(String::new(), |l| format!(" Full output: {l}."));
            // A failed step that ran nextest names its failed tests and first panic, read from its whole output by the one nextest
            // reader (ER-42): the tail alone often ends in the summary, and the exit code says nothing (member gates, 2026-11-24).
            let seen = done
                .log
                .as_ref()
                .filter(|_| !done.success)
                .and_then(|l| std::fs::read(l).ok())
                .and_then(|b| crate::nextest_log::seen(&String::from_utf8_lossy(&b)));
            let mut cause = format!("{} ({})", gate.rerun(), done.how);
            if let Some(s) = &seen {
                if !s.failed.is_empty() {
                    cause.push_str(&format!("; failed tests: {}", s.failed.join(", ")));
                }
                if let Some(p) = &s.first_panic {
                    cause.push_str(&format!("; first panic: {p}"));
                }
            }
            let mut row = Row::from_run(&gate.id, subject, &done, cause, secs(&done));
            if let Some(s) = seen {
                row.failed_tests = s.failed;
                row.first_panic = s.first_panic;
            }
            if !done.success {
                row.tail = done.tail.trim_end().to_string();
                row.next = format!("{hint}. Re-run alone: {}.{full}", gate.rerun());
            }
            row
        }
    }
}

/// How a run is bounded.
pub struct RunOpts<'a> {
    /// Gates at a time.
    pub workers: usize,
    /// How long a step's wait for the heavy slot may go without progress ([`QUEUE_STALL`]). No total is shared by the rows.
    pub queue_stall: Duration,
    /// Set by SIGINT or SIGTERM: nothing more starts.
    pub stop: &'a AtomicBool,
    /// What the run is about (the commit and its tree): the evidence of every pass.
    pub subject: &'a Subject,
    /// The reuse of green results ([`super::reuse`]); `None`: off (`--no-reuse`, `PADDOCK_NO_REUSE`) or a test without it.
    pub reuse: Option<&'a super::reuse::Reuse>,
}

/// Every planned row decided, `workers` at a time, lowest rank first; the result keeps the planned order. The default stall limit,
/// never stopped.
pub fn run_all(planned: &[Planned], exec: &dyn Exec, workers: usize, subject: &Subject) -> Vec<(String, Row)> {
    run_all_with(
        planned,
        exec,
        &RunOpts {
            workers,
            queue_stall: QUEUE_STALL,
            stop: &AtomicBool::new(false),
            subject,
            reuse: None,
        },
    )
}

fn row_id(p: &Planned) -> &str {
    match p {
        Planned::Run(g) => &g.id,
        Planned::Skip { id, .. } | Planned::NotMeasured { id, .. } | Planned::Broken { id, .. } => id,
    }
}

/// [`run_all`] with its bounds. A gate that panics, one that never ran and one a guard or a signal cut off are "not measured",
/// never a pass.
pub fn run_all_with(planned: &[Planned], exec: &dyn Exec, opts: &RunOpts) -> Vec<(String, Row)> {
    run_all_timed(planned, exec, opts).0
}

/// Whether a row may start, from [`Turns::next`].
enum Start {
    Go(Limits),
    Interrupted,
}

/// The rows left and whether a heavy step of the push is in flight. One heavy step at a time: heavy's slots follow the machine's
/// load (several when it is low), so a second heavy step of the same push would queue behind the first at high load, and that
/// wait could not be told from waiting for other agents (ADR 0102). With one in flight, every second a heavy step queues is time
/// spent waiting for others, and the next heavy step is decided only after the one before has ended. While a heavy step is in
/// flight, a free worker takes the next light row instead of waiting.
struct Turns {
    state: Mutex<(VecDeque<usize>, bool)>,
    changed: std::sync::Condvar,
}

impl Turns {
    fn new(order: VecDeque<usize>) -> Turns {
        Turns {
            state: Mutex::new((order, false)),
            changed: std::sync::Condvar::new(),
        }
    }

    /// The next row a free worker takes, and for a gate that runs whether it may start, with the wait it may spend. `None`: nothing
    /// is left.
    fn next(&self, planned: &[Planned], opts: &RunOpts) -> Option<(usize, Option<Start>)> {
        let heavy = |i: usize| matches!(&planned[i], Planned::Run(g) if g.heavy);
        let mut st = self.state.lock().ok()?;
        loop {
            if st.0.is_empty() {
                return None;
            }
            if opts.stop.load(Ordering::SeqCst) {
                let i = st.0.pop_front()?;
                return Some((i, matches!(planned[i], Planned::Run(_)).then_some(Start::Interrupted)));
            }
            let busy = st.1;
            if let Some(pos) = st.0.iter().position(|&i| !(busy && heavy(i))) {
                let i = st.0.remove(pos)?;
                if !matches!(planned[i], Planned::Run(_)) {
                    return Some((i, None));
                }
                if heavy(i) {
                    st.1 = true;
                }
                let start = Start::Go(Limits { stall: opts.queue_stall });
                return Some((i, Some(start)));
            }
            // Only heavy rows are left and one is in flight: woken when it ends; the timeout only re-reads the stop flag.
            st = match self.changed.wait_timeout(st, Duration::from_millis(500)) {
                Ok((g, _)) => g,
                Err(_) => return None,
            };
        }
    }

    /// A step that [`Turns::next`] let start has ended.
    fn end(&self, heavy: bool) {
        if !heavy {
            return;
        }
        if let Ok(mut st) = self.state.lock() {
            st.1 = false;
        }
        self.changed.notify_all();
    }
}

/// `21.4`, or `unknown`.
fn load_text(load: Option<f64>) -> String {
    load.map_or_else(|| "unknown".to_string(), |l| format!("{l:.1}"))
}

/// [`run_all_with`], and the queue and run time of every row that went through `heavy`. A row's run time is its own: the rows share
/// no total, and what a step queued for the slot is reported apart and never counted as run time.
pub fn run_all_timed(planned: &[Planned], exec: &dyn Exec, opts: &RunOpts) -> (Vec<(String, Row)>, BTreeMap<String, Timing>) {
    let mut order: Vec<usize> = (0..planned.len()).collect();
    order.sort_by_key(|&i| match &planned[i] {
        Planned::Run(g) => g.rank,
        _ => 0,
    });
    let turns = Turns::new(order.into_iter().collect::<VecDeque<_>>());
    let timings = Mutex::new(BTreeMap::<String, Timing>::new());
    let results: Vec<Mutex<Option<Row>>> = planned.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..opts.workers.max(1) {
            s.spawn(|| {
                while let Some((i, start)) = turns.next(planned, opts) {
                    let v = match &planned[i] {
                        Planned::Run(g) => match start.unwrap_or(Start::Interrupted) {
                            Start::Interrupted => Row::not_measured(
                                "the push was interrupted before it started",
                                format!("re-run `./dev check --pre-push --only {}`", first_word(&g.id)),
                            ),
                            Start::Go(limits) => {
                                let key = match opts.reuse.map(|r| r.lookup(g)) {
                                    Some(super::reuse::Lookup::Hit(green)) => {
                                        turns.end(g.heavy);
                                        if let Ok(mut slot) = results[i].lock() {
                                            *slot = Some(Row::reused(&g.id, opts.subject, green));
                                        }
                                        continue;
                                    }
                                    Some(super::reuse::Lookup::Miss(k)) => k,
                                    None => None,
                                };
                                let mut seen = |d: &Done| {
                                    if let Some(t) = d.timing
                                        && let Ok(mut m) = timings.lock()
                                    {
                                        m.insert(g.id.clone(), t);
                                    }
                                };
                                let row = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    decide_with(g, exec, limits, opts.subject, &mut seen)
                                }))
                                .unwrap_or_else(|_| {
                                    Row::not_measured(
                                        "the gate's runner crashed",
                                        "report this with the command that ran; re-run it alone",
                                    )
                                });
                                if let (Some(r), Some(k)) = (opts.reuse, &key) {
                                    r.store(g, k, &row);
                                }
                                turns.end(g.heavy);
                                row
                            }
                        },
                        Planned::Skip { reason, .. } => Row::skipped(reason.clone()),
                        Planned::NotMeasured { reason, next, .. } => Row::not_measured(reason.clone(), next.clone()),
                        Planned::Broken { cause, next, .. } => Row::broken(cause.clone(), next.clone()),
                    };
                    if let Ok(mut slot) = results[i].lock() {
                        *slot = Some(v);
                    }
                }
            });
        }
    });
    let rows = planned
        .iter()
        .zip(results)
        .map(|(p, r)| {
            let v = r.into_inner().ok().flatten().unwrap_or_else(|| {
                Row::not_measured(
                    "the gate never ran",
                    "re-run `./dev check --pre-push`; if it happens again, report it",
                )
            });
            (row_id(p).to_string(), v)
        })
        .collect();
    (rows, timings.into_inner().unwrap_or_default())
}

/// How many gates run at once: one while the machine is loaded (ER-24: 20 or more on the 1-minute average), else three; `heavy`
/// still lets one compile at a time through, so the extra workers run the cheap gates meanwhile.
pub fn workers(load: Option<f64>, cores: usize) -> usize {
    match load {
        Some(l) if l >= 20.0 => 1,
        _ => 3.min(cores.max(1)),
    }
}

/// The 1-minute load average from `uptime`'s text (`load averages: 21.92 22.50 17.34` or `load average: 1.0, 2.0, 3.0`).
pub fn parse_load(uptime: &str) -> Option<f64> {
    let rest = uptime.split("load average").nth(1)?;
    let rest = rest.trim_start_matches('s').trim_start_matches(':').trim();
    rest.split(|c: char| c.is_whitespace() || c == ',')
        .next()?
        .replace(',', ".")
        .parse()
        .ok()
}

/// The command that puts a heavy step through the machine-wide slot (ER-24): `PADDOCK_HEAVY` as given, else `heavy` on PATH (the
/// shell `heavy.sh` is retired: engineering-playbook's script is a thin wrapper of `heavy`). Empty when there is none (the caller says so).
pub fn heavy_prefix(env: &HashMap<String, String>, has: &dyn Fn(&str) -> bool) -> Vec<String> {
    if let Some(h) = env.get("PADDOCK_HEAVY").filter(|h| !h.trim().is_empty()) {
        if h.trim() == "off" {
            return Vec::new();
        }
        return h.split_whitespace().map(str::to_string).collect();
    }
    if has("heavy") {
        return ["heavy", "run", "pre-push", "--"].map(String::from).to_vec();
    }
    Vec::new()
}

/// The command that puts a heavy step through the machine-wide slot: [`heavy_prefix`], else the `heavy` built from the tree at
/// `root` (once, kept in `built`; `build_limit` bounds that build), held under a name of this run's own for as long as `built` lives
/// ([`crate::tool_cache::Held`]), so another agent pruning or replacing the shared cache cannot fail a later step. The one
/// resolver of the push gate and of `./dev gen check`, so both queue for the same slot. `Err` is the whole explanation; `Ok(empty)` only for the test-only `PADDOCK_HEAVY=off`.
pub fn heavy_command_for(
    root: &Path,
    env: &HashMap<String, String>,
    has_tool: &dyn Fn(&str) -> bool,
    built: &std::sync::OnceLock<Result<crate::tool_cache::Held, String>>,
    build_limit: Duration,
) -> Result<Vec<String>, String> {
    if env.get("PADDOCK_HEAVY").map(|v| v.trim()) == Some("off") {
        // Only a test may run without the slot: anything else is an override that makes the gate not measured.
        return if env.contains_key(TEST_HEAVY_OFF) {
            Ok(Vec::new())
        } else {
            Err("PADDOCK_HEAVY=off is set without the test-only variable PADDOCK_TEST_HEAVY_OFF, so this is an override of the machine-wide slot (OVERRIDE SET); unset it".into())
        };
    }
    let found = heavy_prefix(env, has_tool);
    if !found.is_empty() {
        return Ok(found);
    }
    // A build that failed for a reason that will not pass on its own is remembered for the run; one that timed out (or ran out of budget)
    // is not: the next step builds it again instead of every row of the run reading a stale timeout.
    if built.get().is_none() {
        let result = super::heavy_tool::ensure(root, env, build_limit).and_then(|bin| super::heavy_tool::hold(&bin, env));
        if let Err(e) = &result
            && super::heavy_tool::is_transient(e)
        {
            return Err(e.clone());
        }
        let _ = built.set(result);
    }
    match built.get().ok_or_else(|| "the heavy build was not recorded".to_string())? {
        Ok(held) => Ok(vec![
            held.path().display().to_string(),
            "run".into(),
            "pre-push".into(),
            "--".into(),
        ]),
        Err(e) => Err(e.clone()),
    }
}

/// Whether `program` is a file in a folder of `env`'s `PATH`.
pub fn on_path(env: &HashMap<String, String>, program: &str) -> bool {
    let path = env.get("PATH").cloned().unwrap_or_default();
    std::env::split_paths(&path).any(|d| exists(&d, program))
}

/// Who held the heavy slot while a step queued, from the step's log (`the lock is held by <who> (pid N) for ...`); `None` when the
/// log is unreadable or never named one.
fn queue_holder(log: Option<&str>) -> Option<String> {
    let text = std::fs::read_to_string(log?).ok()?;
    let held = text.lines().rev().find_map(|l| l.split("the lock is held by ").nth(1))?;
    Some(held.split(" for ").next().unwrap_or(held).trim().to_string())
}

/// Who held the slot when a queue stalled, from heavy's line `<agent> queue stalled: ... (queued Q s, held by <who>), so the command
/// never ran` (the failure line after it, which carries the same words without the closing phrase, is skipped).
pub fn stall_line_holder(log: &str) -> Option<String> {
    log.lines().rev().find_map(|l| {
        let rest = l.split(" queue stalled: ").nth(1)?;
        let held = rest.split(", held by ").nth(1)?;
        let (who, _) = held.split_once(", so the command never ran")?;
        Some(who.strip_suffix(')').unwrap_or(who).to_string())
    })
}

/// The holder a stalled queue's row names: heavy's own line, else the last "the lock is held by" it logged, else the build folder's
/// other run (a file lock names no holder), else that nothing was named.
fn stalled_holder(done: &Done) -> String {
    if done.how.contains("another run of this checkout") {
        return "another run of this checkout (the build folder is in use)".to_string();
    }
    let text = done.log.as_deref().and_then(|l| std::fs::read_to_string(l).ok());
    text.as_deref()
        .and_then(stall_line_holder)
        .or_else(|| queue_holder(done.log.as_deref()))
        .or_else(|| text.as_deref().and_then(wait_status))
        .unwrap_or_else(|| "nobody named (heavy logged no holder)".to_string())
}

/// What heavy last said about the wait, from its own `<agent> waiting 126s: <status>` lines in the step's log: `queued, position 7 of 8,
/// behind <agent>` while in the ticket queue, `the lock is held by <agent> (pid N) for 3 min` at the head. `None` before the first line.
pub fn wait_status(log: &str) -> Option<String> {
    log.lines().rev().find_map(|l| {
        let rest = l.split(" waiting ").nth(1)?;
        let (secs, status) = rest.split_once("s: ")?;
        secs.chars().all(|c| c.is_ascii_digit()).then(|| status.trim().to_string())
    })
}

/// The line the push prints while a step waits for the heavy slot: the wait so far and heavy's own status (who holds the slot, or the
/// place in the queue and who is ahead), and that none of it is run time (ER-27: the queue is a state of its own in the log).
pub fn waiting_line(id: &str, waited: Duration, status: Option<&str>, stall: Duration) -> String {
    format!(
        "pre-push: {id}: waiting for a heavy slot, {} s so far; {} (queue time, not run time; the wait ends only after {} min without progress)",
        waited.as_secs(),
        status.unwrap_or("heavy has not said who holds it yet"),
        stall.as_secs() / 60
    )
}

/// How often the push says that a step still waits for the slot.
const WAITING_SAY: Duration = Duration::from_secs(60);

/// Whether the output of a gate run through `heavy` shows that it never got the slot: it printed who holds the lock
/// ("the lock is held by <name> (pid <n>) ...") and never "<name> holds the lock". Returns the phrase for the verdict.
pub fn slot_wait(log: &str) -> Option<String> {
    if log.lines().any(|l| l.contains(LOCK_TAKEN)) {
        return None;
    }
    let held = log.lines().rev().find_map(|l| l.split("the lock is held by ").nth(1))?;
    let who = held.split(" for ").next().unwrap_or(held).trim();
    Some(format!("waiting for the heavy slot, held by {who}"))
}

/// The real machine: processes through [`ToolCall`] (bounded, environment listed, output in a file per gate).
pub struct RealExec {
    /// The repository root.
    pub root: PathBuf,
    /// The caller's environment.
    pub env: HashMap<String, String>,
    /// Where each gate's whole output goes (one `.log` per gate).
    pub logs: PathBuf,
    /// Set by SIGINT or SIGTERM: the running gates' process trees are ended.
    pub stop: Arc<AtomicBool>,
    /// The `heavy` built from tools/heavy when none is on PATH, built once per run (or the reason it could not be).
    pub built_heavy: std::sync::OnceLock<Result<crate::tool_cache::Held, String>>,
    /// How far the heavy step in flight has got ([`Exec::slot`]): written while a step runs through our `heavy`.
    pub live_slot: Mutex<Slot>,
    /// The push is a landing (`train/*`, `main`, a release tag): its heavy steps take the priority lane (`heavy --high`, ADR 0102).
    pub priority: bool,
    /// The local sccache wrapper for the gates that compile (`None`: off, or CI; [`crate::local_sccache`]).
    pub wrapper: crate::local_sccache::Lazy,
    /// What ends a step that has stopped making progress ([`super::watchdog`]); [`super::watchdog::Never`] leaves it to the row cap.
    pub watchdog: Arc<dyn Watchdog>,
    /// What one started row may run ([`ROW_CAP`]); a test sets a short one.
    pub row_cap: Duration,
    /// The state folder of this checkout (`<git common dir>/paddock-pre-push`): the history and the build-folder locks live in it. Given
    /// by the caller, never derived from the logs folder (review P2-2: a test's logs folder gave a lock file in `$TMPDIR`).
    pub state: PathBuf,
}

/// The variable that names heavy's lock folder (tools/heavy `HEAVY_DIR`, which it has always honoured).
pub const HEAVY_DIR_VAR: &str = "HEAVY_DIR";

/// heavy's lock folder for this machine (its queue, history and logs): `<dev-targets>/throttle` (the name after the decided heavy→throttle
/// rename, so the folder moves once; `<dev-targets>/heavy` holds the installed binary) ([`crate::places::dev_targets`], the folder
/// of worktrees and build folders, which a reboot keeps; it was /private/tmp, where every reboot lost the history), read once per
/// process; `None` when it cannot be derived (heavy then uses its own default). The Mac CI runner's configuration sets the same
/// `HEAVY_DIR`, so agents still see CI's `heavy mark` (ADR 0102).
pub fn heavy_dir(root: &Path, env: &HashMap<String, String>) -> Option<PathBuf> {
    static DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        crate::places::dev_targets(root, &|k| env.get(k).cloned())
            .ok()
            .map(|t| t.join("throttle"))
    })
    .clone()
}

/// `HEAVY_DIR` for a heavy child: none when the caller names a lock folder itself (`HEAVY_DIR`, `PLOCK_DIR`, `PLOCK_ROOT`: its value
/// wins, ER-47) or none can be derived, else [`heavy_dir`].
pub fn heavy_dir_env(root: &Path, env: &HashMap<String, String>) -> Vec<(String, String)> {
    if ["HEAVY_DIR", "PLOCK_DIR", "PLOCK_ROOT"]
        .iter()
        .any(|k| env.get(*k).is_some_and(|v| !v.is_empty()))
    {
        return Vec::new();
    }
    match heavy_dir(root, env) {
        // The folder heavy used before (its legacy default): its history is read once, then the new folder's.
        Some(d) => vec![
            (HEAVY_DIR_VAR.to_string(), d.to_string_lossy().into_owned()),
            ("HEAVY_MOVED_FROM".to_string(), "/private/tmp/claude-501".to_string()),
        ],
        None => Vec::new(),
    }
}

/// Variables every gate may see besides the base list: the caller's account (CI's runner has it, and tests read it: paddock-env's
/// macOS ACL test writes `user:$USER`; ER-47), the toolchain, Docker, caches and Paddock's own settings.
pub(crate) const PASS: &[&str] = &[
    "USER",
    "LOGNAME",
    "CARGO_*",
    "RUSTUP_*",
    "RUSTFLAGS",
    "RUSTC_WRAPPER",
    "DOCKER*",
    "PADDOCK_*",
    "HEAVY_*",
    "XWIN_*",
    "SCCACHE_*",
    "NODE_*",
    "CC",
    "CXX",
    "CLANG*",
    "LIBRARY_PATH",
    "SDKROOT",
    "TMPDIR",
    "NPM_CONFIG_*",
    "npm_config_*",
];

/// The variable that, with `PADDOCK_HEAVY=off`, lets a test run a heavy gate without the slot.
pub const TEST_HEAVY_OFF: &str = "PADDOCK_TEST_HEAVY_OFF";

/// The label that marks the container of one run, so only it is ever stopped (ER-14).
const RUN_LABEL: &str = "paddock.run";

fn slug(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// The log's file name for a gate id: a short readable prefix plus a hash of the whole id, so it stays well under the 255 bytes
/// of a file name however many workspaces the id names, and two ids never share a log. The full id goes to the `.id` file beside it.
fn log_name(id: &str) -> String {
    use sha2::{Digest, Sha256};
    let prefix: String = slug(id).chars().take(48).collect();
    let hash: String = Sha256::digest(id.as_bytes()).iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("{prefix}-{hash}")
}

impl RealExec {
    /// The command that puts a heavy gate through the slot: `PADDOCK_HEAVY` or `heavy` on PATH, else the `heavy`
    /// built from this tree ([`super::heavy_tool`]). Empty for `PADDOCK_HEAVY=off` (hermetic tests only); `Err` when the build failed.
    fn heavy_command(&self) -> Result<Vec<String>, String> {
        heavy_command_for(
            &self.root,
            &self.env,
            &|p| self.has_tool(p),
            &self.built_heavy,
            super::heavy_tool::BUILD_LIMIT,
        )
    }

    /// How far the heavy step in flight has got, read live (the push has one in flight at most): [`Slot::Queued`] until `heavy`
    /// holds the slot for it.
    fn slot(&self) -> Slot {
        self.live_slot.lock().map_or(Slot::Unknown, |g| *g)
    }

    fn set_slot(&self, to: Slot) {
        if let Ok(mut g) = self.live_slot.lock() {
            *g = to;
        }
    }

    /// Whether this heavy gate goes through the `heavy` of tools/heavy (the one with `--queue-stall`, `--run-max` and its log lines).
    fn runs_through_our_heavy(&self, limits: Limits) -> bool {
        let prefix = self.heavy_command().unwrap_or_default();
        with_heavy_flags(&prefix, false, limits.stall, ROW_CAP)
            .iter()
            .any(|a| a == "--run-max")
    }

    fn call(&self, gate: &Gate, limits: Limits, log: &Path, label: Option<&str>, name: &str) -> ToolCall {
        let own = own_run_limit(gate);
        // A heavy step: its run time is limited by heavy itself (`--run-max`), its wait for a slot separately (`--queue-stall`: no
        // progress of the queue); the outer timeout is only a backstop for the run.
        let run_max = own;
        let heavy = if gate.heavy {
            with_agent_name(
                &with_heavy_flags(&self.heavy_command().unwrap_or_default(), self.priority, limits.stall, run_max),
                name,
            )
        } else {
            Vec::new()
        };
        let limited = heavy.iter().any(|a| a == "--run-max");
        // Cargo's JSON messages are the progress of a build row (the diagnostics stay rendered on stderr); the command a person re-runs
        // (`Gate::rerun`) has no such flag.
        let gate_args = if gate.progress == Progress::CargoJson && crate::cargo_target::is_cargo(&gate.program) {
            with_cargo_json(&gate.args)
        } else {
            gate.args.clone()
        };
        let (program, args): (String, Vec<String>) = match heavy.split_first() {
            Some((first, rest)) => (
                first.clone(),
                rest.iter()
                    .cloned()
                    .chain(std::iter::once(gate.program.clone()))
                    .chain(gate_args.iter().cloned())
                    .collect(),
            ),
            None => (gate.program.clone(), gate_args),
        };
        // Through our `heavy` the queue and the run are bounded apart (`--queue-stall`, `--run-max`). The wait for a slot has no total
        // bound here: the queue ending it is heavy's stall judgement, so the outer timeout covers the run only; for a step through
        // another `heavy` (whose queue cannot be told apart) the row cap from its start is the bound, and the monitor enforces it.
        let timeout = if limited {
            run_max + WAIT_BACKSTOP + Duration::from_secs(60)
        } else {
            own
        };
        let mut call = ToolCall::new(program)
            .args(args)
            .pass(PASS.iter().copied())
            .cwd(self.root.join(&gate.cwd))
            .timeout(timeout)
            .cancel_on(Arc::clone(&self.stop));
        if gate.heavy && !self.env.contains_key("CARGO_BUILD_JOBS") {
            call = call.env("CARGO_BUILD_JOBS", "2");
        }
        // The row's own name (its id: the step and its workspace), so heavy keeps how long it takes and lets a row that takes seconds
        // go first (the run's agent name differs every run, so it cannot be the key). An older heavy ignores the variable.
        if gate.heavy {
            call = call.env("HEAVY_ROW", &gate.id);
            // heavy's lock folder, where a reboot keeps its history (heavy_dir_env).
            for (k, v) in heavy_dir_env(&self.root, &self.env) {
                call = call.env(k, v);
            }
        }
        for (k, v) in &gate.env {
            call = call.env(k, v);
        }
        // A cargo gate that names its build folder as configuration never also gets it as a variable (the variable would win over
        // the configuration and sccache hashes it), and compiles through the local sccache when there is one.
        let hands_folder = gate.env.iter().any(|(k, _)| k == crate::ci_run::GATE_TARGET_DIR);
        if hands_folder || (crate::cargo_target::is_cargo(&gate.program) && crate::cargo_target::names_target_dir(&gate.args)) {
            call = call.without("CARGO_TARGET_DIR").without("CARGO_BUILD_TARGET_DIR");
            if gate.heavy
                && let Some(w) = self.wrapper.get()
            {
                call = crate::local_sccache::apply(call, w);
            }
        }
        if let Some(l) = label {
            call = call.env("PADDOCK_LINUX_RUN_LABEL", format!("{RUN_LABEL}={l}"));
        }
        call.output(Output::File(log.to_path_buf()))
    }

    fn docker(&self) -> String {
        self.env.get("DOCKER").cloned().unwrap_or_else(|| "docker".into())
    }

    /// Stops the container this run started (by its own label) and nothing else.
    fn stop_container(&self, label: &str) {
        let list = ToolCall::new(self.docker())
            .args(["ps", "-q", "--filter", &format!("label={RUN_LABEL}={label}")])
            .pass(PASS.iter().copied())
            .timeout(Duration::from_secs(30))
            .run(&self.env, &mut std::io::sink());
        let Ok(list) = list else { return };
        let ids: Vec<String> = list.stdout.split_whitespace().map(str::to_string).collect();
        if ids.is_empty() {
            return;
        }
        let _ = ToolCall::new(self.docker())
            .args(["stop", "--time", "10"])
            .args(ids)
            .pass(PASS.iter().copied())
            .timeout(Duration::from_secs(60))
            .run(&self.env, &mut std::io::sink());
    }
}

/// In a unit test the dev binary ([`crate::tool_cache::self_exe`]) is the test harness itself: started as a gate, `<harness> gates
/// check` is a nested run of every unit test whose name holds `gates` or `check` (about 335), 10 s alone and over 30 s under load,
/// with the fixture repository as its folder (2026-11-18, `a_delete_only_push_and_an_explicit_base_never_use_evidence` stopped at
/// nextest's 60 s twice in a row). Refused with its cause, so a test that reaches the real executor fails fast and named.
#[cfg(test)]
fn harness_as_gate(program: &str) -> Option<String> {
    let me = std::env::current_exe().ok()?;
    (Path::new(program) == me)
        .then(|| format!("a unit test would start its own test binary ({program}) as a gate: give command_in a fake executor (exec::Exec)"))
}

/// The agent name of one heavy step of this process: `pre-push-<pid>-<n>`.
fn step_name() -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!("pre-push-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst))
}

/// Deletes all but the newest `keep` run folders under `root` (each push run has its own log folder): a folder at a time, by exact
/// path, through the one guarded remover. Best effort: a folder that cannot be removed stays.
pub fn prune_run_logs(root: &Path, keep: usize) {
    // A folder whose owner (the pid that made it) still runs is a push in progress: never pruned, whatever its age.
    let owner_runs = |dir: &Path| -> bool {
        let file = dir.join("owner");
        let Ok(text) = std::fs::read_to_string(&file) else { return false };
        let Some(pid) = text.lines().next().and_then(|l| l.trim().parse::<u32>().ok()) else {
            return false;
        };
        let written_ms = std::fs::metadata(&file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        // Gone only when the process table says so; unreadable counts as running (never pruned on a failed read).
        paddock_proc::ProcessId::recorded_presence(pid, written_ms) != paddock_proc::Presence::Gone
    };
    let Ok(read) = std::fs::read_dir(root) else { return };
    let mut dirs: Vec<(std::time::SystemTime, PathBuf)> = read
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    dirs.sort();
    let drop = dirs.len().saturating_sub(keep);
    for (_, d) in dirs.into_iter().take(drop) {
        if owner_runs(&d) {
            continue;
        }
        let _ = crate::safe_remove::remove_under(root, &d, None);
    }
}

/// Marks `dir` (this run's log folder) as a push in progress: its owner file holds this process's pid. Best effort.
pub fn claim_run_logs(dir: &Path) {
    if std::fs::create_dir_all(dir).is_ok() {
        let _ = std::fs::write(dir.join("owner"), format!("{}\n", std::process::id()));
    }
}

/// A token no other run has: process id and clock.
pub fn run_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{:x}-{nanos:x}", std::process::id())
}

impl Exec for RealExec {
    fn has_tool(&self, program: &str) -> bool {
        on_path(&self.env, program)
    }

    fn docker_up(&self) -> bool {
        docker_answers(&self.env)
    }

    fn heavy_unavailable(&self) -> Option<String> {
        self.heavy_command().err()
    }

    fn ci_image(&self) -> ImageProbe {
        let ran = ToolCall::new("bash")
            .args([
                "tools/linux-container.sh",
                "--id",
                "paddock",
                "--ci-image",
                "--image-ready",
                "--root",
            ])
            .args([self.root.to_string_lossy().to_string()])
            .cwd(&self.root)
            .pass(PASS.iter().copied())
            .timeout(Duration::from_secs(60))
            .run(&self.env, &mut std::io::sink());
        match ran {
            Ok(r) => image_probe(r.how.strip_prefix("exit ").and_then(|n| n.trim().parse().ok())),
            Err(e) => ImageProbe::Broken(format!("could not start: {e}")),
        }
    }

    fn mount_probe(&self, compared: &super::mount_fresh::Compared) -> super::mount_fresh::MountProbe {
        use super::mount_fresh::{SCRIPT, check};
        // The same script on both sides: plain `sh` in the checkout, and through the container helper with the row's own mount.
        let run = |call: ToolCall| -> Result<String, String> {
            let r = call
                .cwd(&self.root)
                .pass(PASS.iter().copied())
                .timeout(Duration::from_secs(120))
                .run(&self.env, &mut std::io::sink())?;
            if r.success {
                Ok(r.stdout)
            } else {
                Err(format!("{}: {}", r.how, tail(&r.stderr, 3).trim()))
            }
        };
        let host = |files: &[String]| run(ToolCall::new("sh").args(["-c", SCRIPT, "sh"]).args(files.iter().cloned()));
        let container = |files: &[String]| {
            run(ToolCall::new("bash")
                .args(["tools/linux-container.sh", "--id", "paddock", "--ci-image", "--root"])
                .args([self.root.to_string_lossy().to_string()])
                .args(["--", "sh", "-c", SCRIPT, "sh"])
                .args(files.iter().cloned()))
        };
        check(&compared.files, compared.of, host, container)
    }

    fn load(&self) -> Option<f64> {
        let ran = ToolCall::new("uptime")
            .timeout(Duration::from_secs(10))
            .run(&self.env, &mut std::io::sink())
            .ok()?;
        parse_load(&ran.stdout)
    }

    fn run(&self, gate: &Gate, limits: Limits) -> Result<Done, String> {
        #[cfg(test)]
        if let Some(why) = harness_as_gate(&gate.program) {
            return Err(why);
        }
        crate::test_cost::charge_argv("pre_push::exec (gate row)", &gate.program, &gate.args)?;
        let _ = std::fs::create_dir_all(&self.logs);
        let log = self.logs.join(format!("{}.log", log_name(&gate.id)));
        // Best-effort: the .id file only helps a reader; a failed write never stops the gate.
        let _ = std::fs::write(log.with_extension("id"), format!("{}\n", gate.id));
        let label = matches!(gate.need, Need::Docker | Need::LocalCiImage).then(run_token);
        // Each step has a name of its own for heavy (`pre-push-<pid>-<n>`), and its log is new: nothing an earlier or another push
        // wrote can be read for this step's lines.
        let name = step_name();
        // The cargo build folder is shared by the clippy and doctest rows of a workspace and by other pushes of this checkout: cargo
        // would make the second wait for the first's lock while it already held a heavy slot (ER-43). The wait is here, before any slot
        // is taken, counted as queue time and bounded by the same stall limit (a file lock names no holder, so the row says "another
        // run of this checkout"); the lock is held until the step ends.
        let (build_lock, lock_wait) = match gate.build_dir.as_deref().filter(|_| gate.heavy) {
            Some(dir) => match self.lock_build_folder(dir, &gate.id, limits.stall) {
                Ok(held) => held,
                Err(waited) => {
                    return Ok(Done {
                        success: false,
                        how: format!(
                            "queue stalled: no progress for {} s; held by another run of this checkout (the build folder is in use, its holder is past its row cap or unknown)",
                            waited.as_secs()
                        ),
                        tail: String::new(),
                        log: None,
                        timing: Some(Timing {
                            queued: waited.as_secs_f64(),
                            ran: 0.0,
                        }),
                        ending: Ending::QueueLimit,
                    });
                }
            },
            None => (BuildLock { held: None }, Duration::ZERO),
        };
        let call = self.call(gate, limits, &log, label.as_deref(), &name);
        let _ = std::fs::File::create(&log);
        // Our `heavy` writes "holds the lock" to the log when the slot is taken: until then every second is waiting for others,
        // and the row cap counts only from then (`RealExec::slot`). Another heavy prints no such line: nothing is known then.
        let ours = gate.heavy && self.runs_through_our_heavy(limits);
        let info = StepInfo {
            id: &gate.id,
            heavy: gate.heavy,
            progress: gate.progress,
            kind: gate.id.split(' ').next().unwrap_or(&gate.id),
            ws: &gate.cwd,
        };
        let watch = self.watchdog.watch(&info);
        let root: Arc<Mutex<Option<paddock_proc::ProcessId>>> = Arc::default();
        let call = {
            let root = Arc::clone(&root);
            call.on_start(Arc::new(move |id| {
                if let Ok(mut r) = root.lock() {
                    *r = id;
                }
            }))
        };
        let stuck: Mutex<Option<Stuck>> = Mutex::new(None);
        let began = Instant::now();
        if ours {
            self.set_slot(Slot::Queued { since: began });
        }
        let done = AtomicBool::new(false);
        let ran = std::thread::scope(|s| {
            if ours {
                s.spawn(|| {
                    let mut seen = LockSeen::new(lock_line(&name));
                    let mut said = began + WAITING_SAY;
                    while !done.load(Ordering::SeqCst) {
                        if seen.seen(&log) {
                            self.set_slot(Slot::Taken {
                                since: began,
                                at: Instant::now(),
                            });
                            build_lock.restamp(&gate.id, unix_now());
                            return;
                        }
                        if Instant::now() >= said {
                            said += WAITING_SAY;
                            let status = std::fs::read_to_string(&log).ok().and_then(|t| wait_status(&t));
                            eprintln!("{}", waiting_line(&gate.id, began.elapsed(), status.as_deref(), limits.stall));
                        }
                        std::thread::sleep(SLOT_POLL);
                    }
                });
            }
            // Always watched: the row cap is enforced here (so the stacks are written before the tree is ended), and the
            // watchdog, when there is one, judges the step.
            let (log, root, done, stuck, heavy, progress) = (&log, &root, &done, &stuck, gate.heavy, gate.progress);
            let watcher = s.spawn(move || self.watch_step(watch, log, root, done, stuck, began, ours, heavy, progress));
            let ran = call.run(&self.env, &mut std::io::sink());
            done.store(true, Ordering::SeqCst);
            (ran, watcher.join().ok().flatten())
        });
        let (ran, watch) = ran;
        if ours {
            self.set_slot(Slot::Unknown);
        }
        let mut ran = ran?;
        // The one place the tool's words become a typed ending; every later decision reads `ending`.
        let mut ending = Ending::from_how(&ran.how);
        // The watchdog or the row cap ended the tree: say so, whatever the OS reported (a signal), so the row is "not measured".
        if let Some(s) = stuck.lock().ok().and_then(|mut g| g.take()) {
            ran.success = false;
            if s.guard == ROW_CAP_GUARD {
                // The row cap reads as a timeout (it is one), not as a stall.
                ran.how = format!("timed out after {} s of running ({ROW_CAP_GUARD})", s.quiet.as_secs());
                ending = Ending::RowCap;
            } else {
                ran.how = stuck_how(&s);
                ending = Ending::Stuck { guard: s.guard };
            }
        }
        let mut timing = None;
        if gate.heavy {
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            timing = parse_heavy_timing(&text);
            // heavy's own limits: 79 no slot in time (the command never ran), 80 the command was ended at its run limit.
            let how = ran.how.clone();
            match how.as_str() {
                "exit 79" if timing.is_some() => {
                    ran.how = format!("no progress of the queue for {} s", limits.stall.as_secs());
                    ending = Ending::QueueLimit;
                }
                "exit 80" if timing.is_some() => {
                    ran.how = format!("timed out after {} s of running", timing.map_or(0.0, |t| t.ran).round());
                    ending = Ending::RowCap;
                }
                // Cut short while queued for the heavy slot: say so, the gate itself never ran (ER-33).
                _ if ending == Ending::RowCap && !ran.success => {
                    if let Some(w) = slot_wait(&text) {
                        ran.how = format!("{}: {w}", ran.how);
                    }
                }
                _ => {}
            }
        }
        // Any ending that is not the container run's own (the row cap, the watchdog, the person) stops the container too: the heavy
        // slot is free when the client ends, and a container left compiling would run beside the next step (review P1-2).
        if let Some(l) = &label
            && !ran.success
            && ending.cut_short()
        {
            self.stop_container(l);
        }
        // The step's phases with their durations (ER-27): on the console and beside the gate logs.
        let wall = began.elapsed().as_secs_f64();
        drop(build_lock);
        if !lock_wait.is_zero() {
            // The wait for the folder was queue time (waiting for others), not run time.
            let t = timing.get_or_insert(Timing {
                queued: 0.0,
                ran: (began.elapsed().saturating_sub(lock_wait)).as_secs_f64(),
            });
            t.queued += lock_wait.as_secs_f64();
        }
        let note = watch.and_then(|mut w| w.finish(timing.map_or(wall, |t| t.ran), ran.success));
        let line = phase_line(
            &gate.id,
            timing.map(|t| t.queued),
            timing.map_or(wall, |t| t.ran),
            &ran.how,
            ending.guard().as_deref(),
            note.as_deref(),
        );
        eprintln!("{line}");
        {
            use std::io::Write;
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.logs.join("phases.log"))
                .and_then(|mut f| writeln!(f, "{line}"));
        }
        Ok(Done {
            success: ran.success,
            how: ran.how,
            tail: tail(
                &if gate.progress == Progress::CargoJson {
                    without_json(&ran.stderr)
                } else {
                    ran.stderr.clone()
                },
                12,
            ),
            log: Some(log.display().to_string()),
            timing,
            ending,
        })
    }
}

/// Since when a step counts as running, and whether the watchdog may judge it: `None` while it must not be watched at all, our `heavy`
/// queueing (the wait for the slot is waiting for others, ER-43). A heavy step through another `heavy` has no lock line, so a queue cannot
/// be told from a run: the row cap counts from its start (its bound is honest, a step that is only queued for an hour is cut) and the
/// watchdog does not judge it. Every other step runs, and is judged, from its start.
fn active_since(ours: bool, heavy: bool, slot: Slot, began: Instant) -> Option<(Instant, bool)> {
    match (ours, heavy) {
        (true, _) => match slot {
            Slot::Taken { at, .. } => Some((at, true)),
            _ => None,
        },
        (false, true) => Some((began, false)),
        (false, false) => Some((began, true)),
    }
}

/// The row cap's verdict: a row that has run for its whole cap, counted from the moment its command started (never from the start of
/// its wait for the heavy slot: [`active_since`]), is ended. The one place the cap is decided, so a test reads it with plain durations.
fn row_cap_verdict(active_for: Duration, cap: Duration) -> Option<Stuck> {
    (active_for >= cap).then(|| Stuck {
        guard: ROW_CAP_GUARD.into(),
        reason: format!("the row ran its whole cap of {} s", cap.as_secs()),
        quiet: active_for,
    })
}

/// Seconds since the Unix epoch.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// How the lock file of a build folder is opened: for writing, not for appending. A Windows append-only handle has no right to change
/// the file's length, so `set_len(0)` failed there and the second record was written after the first: waiters read the old holder's
/// pid and time (train 19 trial, run 8560: the restamp test failed on both tries).
fn lock_file_options() -> std::fs::OpenOptions {
    let mut o = std::fs::OpenOptions::new();
    o.create(true).write(true);
    o
}

/// Writes `pid`, the time and the step into the lock file just taken, for the waiters to read, as the whole content of the file. A
/// failure is returned for the caller to say: a waiter that finds no record sees a holder it knows nothing about, which is not live.
fn write_holder_record(file: &std::fs::File, id: &str, now: u64) -> std::io::Result<()> {
    use std::io::{Seek, SeekFrom, Write};
    file.set_len(0)?;
    let mut f = file;
    f.seek(SeekFrom::Start(0))?;
    f.write_all(format!("{}\n{}\n{}\n", std::process::id(), now, id).as_bytes())
}

/// [`write_holder_record`], and a line in the log when it failed (ER-27): the step still runs, the waiters just cannot judge it.
fn write_holder_record_or_say(file: &std::fs::File, id: &str, now: u64) {
    if let Err(e) = write_holder_record(file, id, now) {
        eprintln!(
            "pre-push: {id}: the holder record of the build folder lock could not be written ({e}); a waiter will see a holder it knows nothing about. Next step: none needed, the step runs."
        );
    }
}

/// The picture a waiter has of a build folder's holder from its record (`pid`, start time, step id): the holder is live while it is
/// inside `cap`, the bound its own row lives under. An empty or unreadable record is a holder nobody knows (not live, no change).
fn build_holder_view(record: &str, now: u64, cap: Duration) -> paddock_proc::queue_stall::QueueView {
    use paddock_proc::queue_stall::{Holder, QueueView, within_cap};
    let mut lines = record.lines();
    let pid = lines.next().and_then(|l| l.trim().parse::<u32>().ok());
    let epoch = lines.next().and_then(|l| l.trim().parse::<u64>().ok());
    let name = lines.next().unwrap_or("another run of this checkout").to_string();
    let Some(pid) = pid else { return QueueView::default() };
    QueueView {
        position: 0,
        holders: vec![Holder { name, pid, epoch }],
        live: within_cap(epoch, now, cap),
    }
}

/// A held advisory lock on a build folder; dropping it (or the process ending) releases it.
pub struct BuildLock {
    /// The locked file: held for its lock, and where the holder's record is kept.
    held: Option<std::fs::File>,
}

impl BuildLock {
    /// Writes the holder's record again with `now` as its start: called when the step's heavy slot is taken, so the cap a waiter
    /// counts is the one the step runs under, from when it runs and not from when it took the folder (it may have queued for the
    /// slot for longer than the cap).
    fn restamp(&self, id: &str, now: u64) {
        if let Some(f) = &self.held {
            write_holder_record_or_say(f, id, now);
        }
    }
}

impl RealExec {
    /// The lock file of a build folder: in the state folder beside the gate logs, so every push of this checkout names the same file
    /// for the same folder, and nothing is written into the repository or the build folder.
    pub(crate) fn build_lock_path(&self, dir: &str) -> PathBuf {
        self.state
            .join("build-locks")
            .join(format!("{}.lock", log_name(&self.root.join(dir).to_string_lossy())))
    }

    /// Takes the lock of build folder `dir` for step `id`, waiting while another run holds it (the wait is queue time, never run
    /// time: [`Slot::Queued`]). The holder writes who it is and when it took the lock into the file, and the waiter judges the wait
    /// like every other wait of the gate ([`paddock_proc::queue_stall`]): a holder inside its own row cap is progress, a new holder is
    /// progress, and the same holder past its cap for `stall` ends the wait. `Ok` carries the lock and how long it waited; `Err` how
    /// long it waited before giving up (a stall, or the person stopped the push).
    fn lock_build_folder(&self, dir: &str, id: &str, stall: Duration) -> Result<(BuildLock, Duration), Duration> {
        let path = self.build_lock_path(dir);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let Ok(file) = lock_file_options().open(&path) else {
            eprintln!(
                "pre-push: no lock file for the build folder {dir} ({}): the step runs without it",
                path.display()
            );
            return Ok((BuildLock { held: None }, Duration::ZERO));
        };
        let began = Instant::now();
        let mut clock = paddock_proc::queue_stall::StallClock::new(stall, 0);
        let mut said = false;
        loop {
            match file.try_lock() {
                Ok(()) => {
                    let waited = began.elapsed();
                    if said {
                        eprintln!("pre-push: the build folder {dir} is free after {:.1} s", waited.as_secs_f64());
                        self.set_slot(Slot::Unknown);
                    }
                    write_holder_record_or_say(&file, id, unix_now());
                    return Ok((BuildLock { held: Some(file) }, if said { waited } else { Duration::ZERO }));
                }
                Err(std::fs::TryLockError::WouldBlock) => {}
                // The OS does not do advisory locks here: cargo's own lock serialises, as before.
                Err(std::fs::TryLockError::Error(_)) => return Ok((BuildLock { held: None }, Duration::ZERO)),
            }
            if !said {
                said = true;
                eprintln!("pre-push: the build folder {dir} is in use by another run; waiting for it before taking a heavy slot (ER-43)");
                self.set_slot(Slot::Queued { since: began });
            }
            let view = build_holder_view(&std::fs::read_to_string(&path).unwrap_or_default(), unix_now(), ROW_CAP + CAP_GRACE);
            let quiet = u64::try_from(began.elapsed().as_micros()).unwrap_or(u64::MAX);
            if clock.observe(&view, quiet).is_some() || self.stop.load(Ordering::SeqCst) {
                self.set_slot(Slot::Unknown);
                return Err(began.elapsed());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Writes down where the step's tree is (process list, stacks) and only then ends it, by identity through `paddock_proc`
    /// (rule 53); the capture is bounded ([`stacks::TOTAL_LIMIT`]) and its file named in the log (ER-27).
    fn capture_then_kill(&self, root: paddock_proc::ProcessId, why: &Stuck, log: &Path) {
        let name = log.file_stem().map_or("step".to_string(), |n| n.to_string_lossy().to_string());
        let pids: Vec<u32> = stacks::tree(root).iter().map(|p| p.pid()).collect();
        let c = stacks::capture(
            stacks::this_os(),
            &pids,
            &format!("{}: {} (no progress for {} s)", why.guard, why.reason, why.quiet.as_secs()),
            &self.logs,
            &name,
            &self.env,
            &|p| self.has_tool(p),
        );
        eprintln!("pre-push: {name}: {}", c.note);
        paddock_proc::kill_tree(root);
    }

    /// Shows a running step to its watch once per [`WATCH_POLL`] until it ends; a `Some` verdict ends the step's whole process tree
    /// through `paddock_proc` (by identity, rule 53) and is left in `stuck`. A step is shown only while it runs: not while it queues
    /// for the slot (ER-43; our `heavy` is known to hold the slot from [`Slot::Taken`]), and a heavy step through another `heavy`
    /// (no lock line, so no way to tell a queue from a run) is not watched, the row cap ends it.
    #[allow(clippy::too_many_arguments)]
    fn watch_step(
        &self,
        mut w: Option<Box<dyn StepWatch>>,
        log: &Path,
        root: &Mutex<Option<paddock_proc::ProcessId>>,
        done: &AtomicBool,
        stuck: &Mutex<Option<Stuck>>,
        began: Instant,
        ours: bool,
        heavy: bool,
        progress: Progress,
    ) -> Option<Box<dyn StepWatch>> {
        let mut counter = Counter::new(progress);
        let mut last_check = Instant::now();
        while !done.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(100));
            if last_check.elapsed() < WATCH_POLL {
                continue;
            }
            last_check = Instant::now();
            let active_since = active_since(ours, heavy, self.slot(), began);
            let (Some((since, judged)), Some(id)) = (active_since, root.lock().ok().and_then(|r| *r)) else {
                continue;
            };
            if judged {
                counter.read_new(log);
            }
            let tail = crate::ci_tool::read_tail(log, 4096);
            let last_line = tail.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").to_string();
            let now = Instant::now();
            let active_for = now.saturating_duration_since(since);
            let verdict = if let Some(over) = row_cap_verdict(active_for, self.row_cap) {
                Some(over)
            } else if !judged {
                None
            } else {
                w.as_mut().and_then(|w| {
                    w.check(&Observation {
                        now,
                        active_for,
                        log_len: std::fs::metadata(log).map_or(0, |m| m.len()),
                        last_line: &last_line,
                        progress: counter.events(),
                        units: counter.units().0,
                        compiled: counter.units().1,
                        last_event: counter.last_event(),
                        waiting_on: counter.waiting_on(),
                        cpu: paddock_proc::tree_cpu_time(id),
                        root: id,
                    })
                })
            };
            if let Some(s) = verdict {
                eprintln!(
                    "pre-push: {} ends the step: {} (after {} s; last output line: {})",
                    s.guard,
                    s.reason,
                    s.quiet.as_secs(),
                    last_line
                );
                self.capture_then_kill(id, &s, log);
                if let Ok(mut g) = stuck.lock() {
                    *g = Some(s);
                }
                return w;
            }
        }
        w
    }
}

fn exists(dir: &Path, program: &str) -> bool {
    dir.join(program).is_file() || (cfg!(windows) && dir.join(format!("{program}.exe")).is_file())
}

/// Whether a Docker daemon is listening, asked without waking one: OrbStack's `docker` starts its VM on any command (ER-14: a
/// start of shared infrastructure by an agent), so on Unix the socket is connected to instead. Windows has no such launcher.
#[cfg(unix)]
pub(crate) fn docker_answers(env: &HashMap<String, String>) -> bool {
    docker_sockets(env)
        .iter()
        .any(|p| std::os::unix::net::UnixStream::connect(p).is_ok())
}

/// The sockets a Docker client would use: `DOCKER_HOST`, then OrbStack's, Docker Desktop's and the system's.
#[cfg(unix)]
pub fn docker_sockets(env: &HashMap<String, String>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(host) = env.get("DOCKER_HOST") {
        if let Some(p) = host.strip_prefix("unix://") {
            out.push(PathBuf::from(p));
        }
        return out;
    }
    if let Some(home) = env.get("HOME") {
        out.push(Path::new(home).join(".orbstack/run/docker.sock"));
        out.push(Path::new(home).join(".docker/run/docker.sock"));
    }
    out.push(PathBuf::from("/var/run/docker.sock"));
    out
}

#[cfg(not(unix))]
pub(crate) fn docker_answers(env: &HashMap<String, String>) -> bool {
    let docker = env.get("DOCKER").cloned().unwrap_or_else(|| "docker".into());
    ToolCall::new(docker)
        .args(["info"])
        .pass(PASS.iter().copied())
        .timeout(Duration::from_secs(30))
        .run(env, &mut std::io::sink())
        .is_ok_and(|r| r.success)
}

/// A commit and tree for tests that need a [`Subject`].
#[cfg(test)]
pub(crate) fn test_subject() -> Subject {
    use paddock_cli::outcome::{Sha, TreeId};
    Subject::Commit {
        sha: Sha::new("a1"),
        tree: TreeId::new("t1"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An executor whose heavy step got no slot: `heavy` exited 79 and its log says who held the slot.
    struct NoSlot {
        log: PathBuf,
    }

    impl Exec for NoSlot {
        fn has_tool(&self, _: &str) -> bool {
            true
        }
        fn docker_up(&self) -> bool {
            true
        }
        fn run(&self, _: &Gate, _: Limits) -> Result<Done, String> {
            Ok(Done {
                success: false,
                how: "queue limit: no slot taken in time".into(),
                tail: String::new(),
                log: Some(self.log.display().to_string()),
                timing: Some(Timing { queued: 1800.0, ran: 0.0 }),
                ending: Ending::QueueLimit,
            })
        }
    }

    #[test]
    fn a_step_that_got_no_slot_says_who_it_queued_behind() {
        let d = tempfile::tempdir().unwrap();
        let log = d.path().join("g.log");
        std::fs::write(&log, "pre-push waiting: the lock is held by agent-7 (pid 41) for 31 min\n").unwrap();
        let Planned::Run(g) = gate_row("clippy a", true) else {
            unreachable!()
        };
        let limits = Limits {
            stall: Duration::from_secs(1800),
        };
        let row = decide_with(&g, &NoSlot { log }, limits, &test_subject(), &mut |_| {});
        assert!(
            row.reason().starts_with("queue stalled, held by agent-7 (pid 41)") && row.reason().contains("queued 1800.0 s"),
            "{}",
            row.reason()
        );
        // Without a holder in the log the reason still says the queue time, and does not invent one.
        let row = decide_with(
            &g,
            &NoSlot {
                log: d.path().join("none.log"),
            },
            limits,
            &test_subject(),
            &mut |_| {},
        );
        assert!(
            row.reason().contains("queued 1800.0 s") && row.reason().starts_with("queue stalled, held by nobody named"),
            "{}",
            row.reason()
        );
    }

    /// The build folder's holder is read from its record: live while inside the row cap, unknown (not live, no holder) for an empty or
    /// unreadable record, and not live past the cap.
    #[test]
    fn the_build_folder_holder_is_read_from_its_record() {
        let cap = ROW_CAP + CAP_GRACE;
        let fresh = build_holder_view("4242\n1000\nclippy apps/dyno\n", 1000 + 3000, cap);
        assert!(
            fresh.live && fresh.holders[0].pid == 4242 && fresh.holders[0].name == "clippy apps/dyno",
            "{fresh:?}"
        );
        let old = build_holder_view("4242\n1000\nclippy apps/dyno\n", 1000 + cap.as_secs(), cap);
        assert!(
            !old.live && old.holders.len() == 1,
            "past its cap the holder is not trusted: {old:?}"
        );
        for none in ["", "not a pid\n", "\n\n"] {
            let v = build_holder_view(none, 5000, cap);
            assert!(!v.live && v.holders.is_empty(), "{none:?}: {v:?}");
        }
    }

    /// P2 of the re-review: the holder's stamp is when it started to RUN, not when it took the folder. A holder that waited 70 minutes in
    /// heavy's queue after taking the build folder is not past its cap when its heavy slot is taken: the record is written again then.
    #[test]
    fn a_holder_that_waited_in_heavys_queue_is_stamped_again_when_its_slot_is_taken() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("f.lock");
        let file = lock_file_options().open(&path).unwrap();
        let t0 = 1_000_000u64;
        write_holder_record(&file, "clippy a", t0).unwrap();
        let held = BuildLock { held: Some(file) };
        let cap = ROW_CAP + CAP_GRACE;
        let later = t0 + 70 * 60;
        let before = build_holder_view(&std::fs::read_to_string(&path).unwrap(), later, cap);
        assert!(
            !before.live,
            "stamped at the folder lock, 70 minutes old: past the cap, the bug: {before:?}"
        );
        held.restamp("clippy a", later);
        let after = build_holder_view(&std::fs::read_to_string(&path).unwrap(), later + 60, cap);
        assert!(after.live && after.holders.len() == 1, "stamped when its slot was taken: {after:?}");
    }

    /// The Windows cause of the restamp test's failure: an append-only handle may not shorten the file there, so the second record
    /// went after the first. The lock file is opened for writing, never for appending (the options carry no `append`); this runs on
    /// every OS, because the Debug text of the options is the one place the flag can be read back.
    #[test]
    fn the_lock_file_is_opened_for_writing_not_for_appending() {
        let shown = format!("{:?}", lock_file_options());
        assert!(shown.contains("append: false") && shown.contains("write: true"), "{shown}");
    }

    /// A record left by an earlier holder is replaced whole, however long it was, and a failed write is returned, not swallowed.
    #[test]
    fn a_holder_record_replaces_an_older_longer_one_and_a_failed_write_is_reported() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("f.lock");
        std::fs::write(&path, "4242\n999\na much longer step name than the new one\n").unwrap();
        let file = lock_file_options().open(&path).unwrap();
        write_holder_record(&file, "x", 5_000).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("{}\n5000\nx\n", std::process::id())
        );
        let read_only = std::fs::File::open(&path).unwrap();
        assert!(
            write_holder_record(&read_only, "x", 6_000).is_err(),
            "a handle that cannot write says so"
        );
    }

    /// Heavy's own line names the holder of a stalled queue; the failure line after it (the same words without the closing phrase) is
    /// not taken for it, and a log without either names nobody.
    #[test]
    fn the_holder_of_a_stalled_queue_is_read_from_heavys_line() {
        let log = "pre-push-7-1 queue stalled: the queue made no progress for 1800 s (queued 1801.2 s, held by agent-7 (pid 41), other (pid 9)), so the command never ran\n\
                   heavy 0.1 (macos) exit 0x0000004F: queue stalled: no progress for 1800 s, held by agent-7 (pid 41), other (pid 9) (queued 1801.2 s); the command never ran. Next: x\n";
        assert_eq!(stall_line_holder(log).as_deref(), Some("agent-7 (pid 41), other (pid 9)"));
        assert_eq!(stall_line_holder("the lock is held by agent-7 (pid 41) for 3 min\n"), None);
        assert_eq!(stall_line_holder(""), None);
    }

    /// The wait for the slot is a state of its own in the log: the time so far, the holder, and that it is not run time.
    #[test]
    fn the_waiting_line_names_the_time_waited_and_the_holder() {
        let log = "heavy: pre-push-1-2 waiting 60s: queued, position 3 of 4, behind agent-7\nheavy: pre-push-1-2 waiting 125s: queued, position 2 of 4, behind agent-7\n";
        assert_eq!(wait_status(log).as_deref(), Some("queued, position 2 of 4, behind agent-7"));
        assert_eq!(wait_status("compiling x\n"), None);
        let line = waiting_line(
            "clippy a",
            Duration::from_secs(125),
            wait_status(log).as_deref(),
            Duration::from_secs(1800),
        );
        assert!(
            line.contains("clippy a")
                && line.contains("125 s so far")
                && line.contains("position 2 of 4, behind agent-7")
                && line.contains("not run time"),
            "{line}"
        );
        assert!(waiting_line("x", Duration::ZERO, None, QUEUE_STALL).contains("has not said who holds it yet"));
    }

    /// The row cap counts only run time: a step that queued for hours and then ran 10 s is nowhere near it, a step still waiting is
    /// not judged at all, and one that has run its whole cap is over it. No sleeps: the instants are made up.
    #[test]
    fn the_row_cap_counts_from_the_slot_taken_never_from_the_wait() {
        let t0 = Instant::now();
        let taken = t0 + Duration::from_secs(7200);
        // Two hours queued, taken, 10 s later: far under the cap.
        let (since, _) = active_since(true, true, Slot::Taken { since: t0, at: taken }, t0).unwrap();
        assert_eq!(taken.saturating_duration_since(since), Duration::ZERO);
        assert!(row_cap_verdict((taken + Duration::from_secs(10)).saturating_duration_since(since), ROW_CAP).is_none());
        // Still queued: not judged by the cap at all, however long.
        assert_eq!(active_since(true, true, Slot::Queued { since: t0 }, t0), None);
        // A row that has run its whole cap is over it, and says so.
        let over = row_cap_verdict(ROW_CAP, ROW_CAP).expect("a row at its cap is over it");
        assert_eq!(over.guard, ROW_CAP_GUARD);
        assert!(row_cap_verdict(ROW_CAP - Duration::from_secs(1), ROW_CAP).is_none());
    }

    fn gate_row(id: &str, heavy: bool) -> Planned {
        Planned::Run(Gate {
            id: id.into(),
            cwd: String::new(),
            program: "true".into(),
            args: vec![],
            env: Vec::new(),
            heavy,
            need: Need::Nothing,
            rank: 1,
            next: String::new(),
            workspaces: Vec::new(),
            notice: None,
            progress: crate::pre_push::progress::Progress::Lines,
            build_dir: None,
            mount: None,
        })
    }

    #[test]
    fn load_comes_out_of_both_uptime_formats() {
        assert_eq!(parse_load("16:09  up 17:24, 1 user, load averages: 21.92 22.50 17.34"), Some(21.92));
        assert_eq!(parse_load("up 3 days, load average: 0.52, 0.58, 0.59"), Some(0.52));
        assert_eq!(parse_load("no numbers"), None);
    }

    #[test]
    fn a_loaded_machine_runs_one_gate_at_a_time() {
        assert_eq!(workers(Some(35.0), 14), 1);
        assert_eq!(workers(Some(5.0), 14), 3);
        assert_eq!(workers(None, 2), 2);
        assert_eq!(workers(Some(1.0), 0), 1);
    }

    /// heavy's lock folder goes to `<dev-targets>/throttle`, a folder a reboot keeps (it was in /private/tmp and its history was lost at
    /// every reboot, so no row was ever quick; 2026-11-25), unless the caller names one. One process reads it once (nextest runs each
    /// test in a process of its own).
    #[test]
    fn heavys_lock_folder_goes_to_the_dev_targets_folder_unless_the_caller_names_one() {
        let dir = tempfile::tempdir().unwrap();
        // An absolute folder on every OS (`/dev-targets-x` is not absolute on Windows, so no folder was derived there).
        let targets = dir.path().join("dev-targets-x");
        let mut env: HashMap<String, String> = [(crate::places::DEV_TARGETS_ENV.to_string(), targets.display().to_string())].into();
        let got = heavy_dir_env(dir.path(), &env);
        assert_eq!(got[0], (HEAVY_DIR_VAR.to_string(), targets.join("throttle").display().to_string()));
        assert_eq!(got[1].0, "HEAVY_MOVED_FROM");
        env.insert("HEAVY_DIR".into(), "/mine".into());
        assert!(heavy_dir_env(dir.path(), &env).is_empty(), "the caller's own folder wins");
    }

    #[test]
    fn heavy_is_found_when_the_variable_is_not_set() {
        let env = HashMap::new();
        assert!(heavy_prefix(&env, &|_| false).is_empty());
        assert!(
            heavy_prefix(&env, &|p| p == "heavy.sh").is_empty(),
            "the shell heavy.sh is retired: it is no slot tool of ours"
        );
        assert_eq!(heavy_prefix(&env, &|p| p == "heavy"), ["heavy", "run", "pre-push", "--"]);
        let set = HashMap::from([("PADDOCK_HEAVY".to_string(), "my-heavy --x".to_string())]);
        assert_eq!(heavy_prefix(&set, &|_| true), ["my-heavy", "--x"]);
    }

    #[test]
    fn a_log_that_only_waited_names_the_holder() {
        let waited =
            "heavy: the lock is held by Bob (pid 12) for 3m, running: cargo clippy\nheavy: the lock is held by Bob (pid 12) for 4m\n";
        assert_eq!(
            slot_wait(waited).as_deref(),
            Some("waiting for the heavy slot, held by Bob (pid 12)")
        );
        let ran = format!("{waited}heavy: pre-push holds the lock (pid 9) at 10:00:00Z, waited 5s\n");
        assert_eq!(slot_wait(&ran), None);
        assert_eq!(slot_wait("error: boom\n"), None);
    }

    #[test]
    fn heavy_off_runs_without_the_slot_and_is_the_only_way_to() {
        let off = HashMap::from([("PADDOCK_HEAVY".to_string(), "off".to_string())]);
        assert!(heavy_prefix(&off, &|_| true).is_empty());
    }

    /// A stand-in for `heavy`: a directory is the lock (`mkdir`), a waiter prints who holds it as heavy does, the holder runs
    /// its command with the lock held. Fake commands only (ER-14): no real build runs.
    #[cfg(unix)]
    mod slot {
        use super::*;
        use crate::pre_push::gates::Gate;

        const FAKE_HEAVY: &str = r#"
lock="$1"; shift
while ! mkdir "$lock" 2>/dev/null; do
  echo "heavy: the lock is held by $(cat "$lock/owner" 2>/dev/null || echo other) (pid 1) for 0m" >&2
  sleep 0.1
done
echo "pre-push holds the lock (pid $$) at now, waited 0s" >&2
echo "$$" > "$lock/owner"
"$@"; rc=$?
rm -rf "$lock"
exit $rc
"#;

        fn real(dir: &Path, lock: &Path) -> RealExec {
            let script = dir.join("fake-heavy.sh");
            std::fs::write(&script, FAKE_HEAVY).unwrap();
            RealExec {
                root: dir.to_path_buf(),
                env: HashMap::from([
                    ("PADDOCK_HEAVY".to_string(), format!("sh {} {}", script.display(), lock.display())),
                    ("PATH".to_string(), std::env::var("PATH").unwrap_or_default()),
                ]),
                logs: dir.join("logs"),
                stop: Arc::new(AtomicBool::new(false)),
                built_heavy: Default::default(),
                live_slot: Default::default(),
                priority: false,
                wrapper: crate::local_sccache::Lazy::off(),
                watchdog: Arc::new(crate::pre_push::watchdog::Never),
                row_cap: ROW_CAP,
                state: dir.join("state"),
            }
        }

        fn gate(id: &str, command: &str) -> Gate {
            Gate {
                id: id.into(),
                cwd: String::new(),
                program: "sh".into(),
                args: vec!["-c".into(), command.into()],
                env: Vec::new(),
                heavy: true,
                need: Need::Nothing,
                rank: 2,
                next: "fix it".into(),
                workspaces: Vec::new(),
                notice: None,
                progress: crate::pre_push::progress::Progress::Lines,
                build_dir: None,
                mount: None,
            }
        }

        /// The positive control of `harness_as_gate` (ER-33): the test binary itself is refused as a gate, by the guard and by the
        /// real executor, with the cause named; any other program is not.
        #[test]
        fn the_real_executor_refuses_to_start_the_test_binary_as_a_gate() {
            let me = std::env::current_exe().unwrap().to_string_lossy().to_string();
            let why = harness_as_gate(&me).expect("the test binary is refused");
            assert!(why.contains("its own test binary") && why.contains(&me), "{why}");
            assert_eq!(harness_as_gate("/bin/true"), None);
            let d = tempfile::tempdir().unwrap();
            let ex = real(d.path(), &d.path().join("lock"));
            let g = Gate {
                program: me,
                args: vec!["gates".into(), "check".into()],
                heavy: false,
                ..gate("gates-register", "")
            };
            let limits = Limits {
                stall: Duration::from_secs(5),
            };
            let err = ex.run(&g, limits).expect_err("the test binary never starts as a gate");
            assert!(err.contains("its own test binary"), "{err}");
        }

        /// The gates-register row's finding (C100's `::error` line, as `./dev gates check` prints it for a check no CI line runs) comes
        /// before a 14-line table; it must be in the failed row's output, not cut off by the table (review of feat/pre-push-unwired-check).
        #[test]
        fn a_finding_printed_before_a_long_table_is_in_the_failed_rows_output() {
            let d = tempfile::tempdir().unwrap();
            let ex = real(d.path(), &d.path().join("lock"));
            let cmd = "echo '::error file=conformance/src/checks/C999.mjs,line=1::C100 conformance C999 is run by no reachable, resolved CI line'; \
                       i=0; while [ $i -lt 14 ]; do echo \"| table row $i |\"; i=$((i+1)); done; exit 5";
            let g = Gate {
                heavy: false,
                ..gate("gates-register", cmd)
            };
            let row = decide(&g, &ex, &test_subject());
            assert_eq!(row.kind(), Kind::Failed);
            assert!(row.tail.contains("C100 conformance C999 is run by no reachable"), "{}", row.tail);
        }

        /// A row's child keeps the caller's account (`USER`, `LOGNAME`) as CI's runner gives it (ER-47): the member gate's `tests crates`
        /// row ran paddock-env's macOS ACL test without `USER`, so it wrote `user: allow ...` and failed (train 2026-11-26-g). A token
        /// stays out (the control).
        #[test]
        fn a_rows_child_keeps_the_callers_account_and_never_a_token() {
            let d = tempfile::tempdir().unwrap();
            let mut ex = real(d.path(), &d.path().join("lock"));
            ex.env.insert("USER".into(), "someone".into());
            ex.env.insert("LOGNAME".into(), "someone".into());
            ex.env.insert("GITEA_TOKEN".into(), "secret".into());
            let g = Gate {
                heavy: false,
                ..gate(
                    "tests crates",
                    r#"test "$USER" = someone && test "$LOGNAME" = someone && test -z "${GITEA_TOKEN:-}""#,
                )
            };
            let row = decide(&g, &ex, &test_subject());
            assert_eq!(row.kind(), Kind::Passed, "{}", row.reason());
        }

        /// A failed nextest row said only "exit 100": its failed tests and first panic line are in the row's cause and its JSON, read
        /// from the step's whole output by the one nextest reader (ER-42), not only from the tail.
        #[test]
        fn a_failed_nextest_row_names_its_failed_tests_and_first_panic() {
            let d = tempfile::tempdir().unwrap();
            let ex = real(d.path(), &d.path().join("lock"));
            let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ci-runs/nextest-r0-fail.log");
            // Forty lines after the run push the panic out of the 12-line tail.
            let cmd = format!(
                "cat '{}' >&2; i=0; while [ $i -lt 40 ]; do echo \"after $i\" >&2; i=$((i+1)); done; exit 100",
                fixture.display()
            );
            let g = Gate {
                heavy: false,
                ..gate("member-audits", &cmd)
            };
            let row = decide(&g, &ex, &test_subject());
            assert_eq!(row.kind(), Kind::Failed);
            assert!(
                !row.tail.contains("panicked"),
                "the control: the tail alone has no panic: {}",
                row.tail
            );
            assert_eq!(row.failed_tests, ["nxflaky t::always_fails", "nxflaky t::flaky_once"]);
            assert_eq!(
                row.first_panic.as_deref(),
                Some("thread 't::always_fails' (163) panicked at src/lib.rs:7:9: no")
            );
            let reason = row.reason();
            assert!(
                reason.contains("failed tests: nxflaky t::always_fails, nxflaky t::flaky_once")
                    && reason.contains("first panic: thread 't::always_fails' (163) panicked at src/lib.rs:7:9: no"),
                "{reason}"
            );
            let r = crate::pre_push::render("h", &[("member-audits".to_string(), row)]);
            let j = &r.data["gates"][0];
            assert_eq!(
                j["failed_tests"],
                serde_json::json!(["nxflaky t::always_fails", "nxflaky t::flaky_once"]),
                "{}",
                r.data
            );
            assert_eq!(j["first_panic"], "thread 't::always_fails' (163) panicked at src/lib.rs:7:9: no");
            // Control: a failed row that ran no nextest names no tests.
            let plain = decide(
                &Gate {
                    heavy: false,
                    ..gate("lint", "echo bad >&2; exit 1")
                },
                &ex,
                &test_subject(),
            );
            assert!(plain.failed_tests.is_empty() && plain.first_panic.is_none());
        }

        /// A row naming many workspaces has an id longer than a file name may be (255 bytes): the log still gets written under a short
        /// name, the full id is kept beside it, and two long ids that share a prefix do not share a log.
        #[test]
        fn a_gate_with_a_very_long_id_still_gets_a_log() {
            let d = tempfile::tempdir().unwrap();
            let ex = real(d.path(), &d.path().join("slot.lock"));
            let ws = |tail: &str| {
                (0..40)
                    .map(|i| format!("apps/some-long-workspace-{i}"))
                    .chain([tail.to_string()])
                    .collect::<Vec<_>>()
                    .join(" + ")
            };
            let (a, b) = (format!("clippy-linux {}", ws("x")), format!("clippy-linux {}", ws("y")));
            assert!(a.len() > 255);
            let mut g = gate(&a, "echo from-a");
            g.heavy = false;
            let done = ex
                .run(
                    &g,
                    Limits {
                        stall: Duration::from_secs(30),
                    },
                )
                .expect("runs");
            let log = PathBuf::from(done.log.expect("a log path"));
            assert!(log.file_name().unwrap().len() <= 100, "{log:?}");
            assert!(std::fs::read_to_string(&log).unwrap().contains("from-a"));
            assert_eq!(std::fs::read_to_string(log.with_extension("id")).unwrap().trim(), a);
            assert_ne!(log_name(&a), log_name(&b));
            assert_eq!(
                log_name("clippy a"),
                "clippy-a-8985c681c020",
                "a pinned name: the hash is part of the contract"
            );
        }

        #[test]
        fn two_pushes_at_once_take_the_slot_in_turn() {
            let d = tempfile::tempdir().unwrap();
            let lock = d.path().join("slot.lock");
            let ex = real(d.path(), &lock);
            let trace = d.path().join("trace");
            let cmd = |n: &str| format!("echo start-{n} >> {t}; sleep 1; echo end-{n} >> {t}", t = trace.display());
            std::thread::scope(|s| {
                for n in ["a", "b"] {
                    let (ex, g) = (&ex, gate(&format!("clippy {n}"), &cmd(n)));
                    s.spawn(move || assert!(decide(&g, ex, &test_subject()).kind() == Kind::Passed));
                }
            });
            let lines: Vec<String> = std::fs::read_to_string(&trace).unwrap().lines().map(String::from).collect();
            assert_eq!(lines.len(), 4, "{lines:?}");
            // Never two starts in a row: the second waited for the first to end.
            assert!(lines[0].starts_with("start-") && lines[1].starts_with("end-"), "{lines:?}");
            assert_eq!(
                lines[0].trim_start_matches("start-"),
                lines[1].trim_start_matches("end-"),
                "{lines:?}"
            );
        }

        #[test]
        fn a_slot_held_past_the_budget_is_not_measured_and_says_who_holds_it() {
            let d = tempfile::tempdir().unwrap();
            let lock = d.path().join("slot.lock");
            std::fs::create_dir(&lock).unwrap();
            std::fs::write(lock.join("owner"), "Bob").unwrap();
            // The fake heavy is not ours: its queue cannot be told from its run, so the row cap counts from the start (2 s here).
            let mut ex = real(d.path(), &lock);
            ex.row_cap = Duration::from_secs(2);
            let g = gate("clippy a", "echo never runs");
            let r = decide(&g, &ex, &test_subject());
            assert_eq!(
                r.kind(),
                Kind::NotMeasured,
                "a slot that was never taken is not a pass or a failure: {r:?}"
            );
            assert!(r.reason().contains("waiting for the heavy slot, held by Bob"), "{}", r.reason());
        }

        /// A repository with the heavy tool's inputs committed, and a `cargo` that "builds" a fake heavy (a stand-in that records
        /// its arguments and runs the gate's command), found first on the PATH of the run (ER-14: no real build).
        fn repo_with_fake_cargo(d: &Path, cargo_ok: bool) -> RealExec {
            let git = |args: &[&str]| {
                let ok = crate::git_env::git()
                    .args(args)
                    .current_dir(d)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .output()
                    .unwrap();
                assert!(ok.status.success(), "{args:?}: {}", String::from_utf8_lossy(&ok.stderr));
            };
            std::fs::create_dir_all(d.join("tools/heavy")).unwrap();
            std::fs::write(d.join("tools/heavy/inputs.txt"), "tools/heavy\n").unwrap();
            std::fs::write(d.join("tools/heavy/Cargo.toml"), "# fake\n").unwrap();
            git(&["init", "-q"]);
            git(&["add", "."]);
            git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "x"]);
            let bin = d.join("fakebin");
            std::fs::create_dir_all(&bin).unwrap();
            let cargo = paddock_testkit::StandIn::new(&bin, "cargo");
            let cargo = if cargo_ok {
                use paddock_testkit::{Actions, Case, Exec, StandIn};
                // What the build "makes": a heavy that answers `--version`, records its arguments and runs the gate's command.
                let heavy = StandIn::new(&bin, "heavy")
                    .answers(
                        Actions::new()
                            .appends_with(&d.join("heavy-calls").display().to_string(), "{{args}}\n")
                            .execs(&Exec::call_arguments().after("--")),
                    )
                    .when(&Case::new().args_start_with(&["--version"]), Actions::new().stdout("heavy-fake"));
                let target = "{flag_unquoted:build.target-dir=}";
                cargo.answers(
                    Actions::new()
                        .makes_dir(&format!("{target}/release"))
                        .installs(&format!("{target}/release/heavy"), &heavy),
                )
            } else {
                cargo.stderr("error[E0432]: unresolved import").exit(101)
            };
            cargo.install().unwrap();
            let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
            RealExec {
                root: d.to_path_buf(),
                env: HashMap::from([
                    ("PATH".to_string(), path),
                    ("PADDOCK_DEV_HOME".to_string(), d.join("home").display().to_string()),
                    ("HOME".to_string(), d.display().to_string()),
                    ("PADDOCK_DEV_TARGET_DIR".to_string(), d.join("tgt").display().to_string()),
                ]),
                logs: d.join("logs"),
                stop: Arc::new(AtomicBool::new(false)),
                built_heavy: Default::default(),
                live_slot: Default::default(),
                priority: false,
                wrapper: crate::local_sccache::Lazy::off(),
                watchdog: Arc::new(crate::pre_push::watchdog::Never),
                row_cap: ROW_CAP,
                state: d.join("state"),
            }
        }

        /// The build folder travels as cargo configuration: a `CARGO_TARGET_DIR` in the caller's environment never reaches the
        /// gate (sccache would hash it), and the wrapper's variables reach a compiling gate only when the wrapper is on.
        #[test]
        fn a_cargo_gate_never_gets_the_build_folder_as_a_variable_and_compiles_through_the_wrapper_when_there_is_one() {
            let d = tempfile::tempdir().unwrap();
            let mut ex = repo_with_fake_cargo(d.path(), true);
            let var = "CARGO_TARGET_DIR";
            ex.env.insert(var.into(), "/mine".into());
            let mut g = gate("clippy a", "");
            g.program = "cargo".into();
            g.args = crate::cargo_target::with_target_dir(&["clippy".to_string()], "/t");
            let limits = Limits {
                stall: Duration::from_secs(60),
            };
            let log = d.path().join("g.log");
            let env_of = |ex: &RealExec| ex.call(&g, limits, &log, None, "pre-push-1-1").child_env(&ex.env);
            let plain = env_of(&ex);
            assert!(
                plain.iter().all(|(k, _)| k != "CARGO_TARGET_DIR" && k != "RUSTC_WRAPPER"),
                "{plain:?}"
            );
            ex.wrapper = crate::local_sccache::Lazy::ready(crate::local_sccache::Ready {
                bin: "/x/sccache".into(),
                vars: vec![("RUSTC_WRAPPER".into(), "/x/sccache".into())],
            });
            let wrapped = env_of(&ex);
            assert!(wrapped.contains(&("RUSTC_WRAPPER".into(), "/x/sccache".into())), "{wrapped:?}");
            assert!(wrapped.iter().all(|(k, _)| k != "CARGO_TARGET_DIR"), "{wrapped:?}");
            // A gate that is not cargo (conformance, a script) is never wrapped.
            let mut s = gate("conformance", "true");
            s.heavy = true;
            let sh = ex.call(&s, limits, &log, None, "pre-push-1-1").child_env(&ex.env);
            assert!(sh.iter().all(|(k, _)| k != "RUSTC_WRAPPER"), "{sh:?}");
            // A heavy gate names its row for heavy's history; a gate that does not go through heavy has no such variable.
            assert!(sh.contains(&("HEAVY_ROW".into(), "conformance".into())), "{sh:?}");
            let mut light = gate("fmt crates", "true");
            light.heavy = false;
            let l = ex.call(&light, limits, &log, None, "pre-push-1-1").child_env(&ex.env);
            assert!(l.iter().all(|(k, _)| k != "HEAVY_ROW"), "{l:?}");
        }

        /// The live queue state ([`Exec::slot`], ADR 0102 amendment 2026-11-18): queued from the start of a step through our
        /// heavy, taken when the step's log says heavy holds the slot FOR THIS STEP (its own agent name), unknown again when the
        /// step is over. The built heavy is the fake of `repo_with_fake_cargo`; the gate's own command writes the line heavy writes
        /// (with the name it reads from the fake's record of its arguments), between two waits for the test (files, bounded).
        /// The log held a line of an earlier run before the step began, and another push's line arrives in it meanwhile: neither
        /// is this step's.
        #[test]
        fn the_slot_is_queued_until_the_log_says_heavy_holds_the_lock_for_this_step() {
            let d = tempfile::tempdir().unwrap();
            let dir = d.path().display().to_string();
            let wait = |f: &str| format!("n=0; while [ ! -f {dir}/{f} ] && [ $n -lt 400 ]; do n=$((n+1)); sleep 0.05; done");
            let mine = format!("sed -n 's/.* \\(pre-push-[0-9]*-[0-9]*\\) -- .*/\\1/p' {dir}/heavy-calls | tail -1");
            let command = format!(
                ": > {dir}/queued; echo 'pre-push-999999-1 holds the lock (pid 5) at 00:00:00, waited 1s, load 1'; : > {dir}/foreign; {}; \
                 echo \"$({mine}) holds the lock (pid 2) at 00:00:00, waited 5s, load 1\"; : > {dir}/taken; {}",
                wait("go"),
                wait("finish"),
            );
            let ex = repo_with_fake_cargo(d.path(), true);
            // An earlier run's log of this very gate, with heavy's line in it.
            std::fs::create_dir_all(&ex.logs).unwrap();
            std::fs::write(
                ex.logs.join(format!("{}.log", log_name("clippy a"))),
                "pre-push holds the lock (pid 1) at 00:00:00, waited 0s, load 1\n",
            )
            .unwrap();
            let seen = |f: &str| d.path().join(f).is_file();
            let until = |what: &dyn Fn() -> bool| {
                let t = Instant::now();
                while !what() && t.elapsed() < Duration::from_secs(20) {
                    std::thread::yield_now();
                }
                assert!(what(), "the step never got there");
            };
            assert_eq!(ex.slot(), Slot::Unknown, "no step in flight");
            // heavy's build (and the cleanup's process scan, empty in a test: worktree::procs) happens once, here, before anything is observed: the state below is driven
            // by the step's own log lines, not by how long a build or a scan took on a loaded machine (ER-12, ER-28).
            ex.heavy_command().expect("the fake heavy is built and held");
            std::thread::scope(|sc| {
                let step = sc.spawn(|| decide(&gate("clippy a", &command), &ex, &test_subject()).kind());
                until(&|| seen("foreign"));
                assert!(
                    matches!(ex.slot(), Slot::Queued { .. }),
                    "neither the earlier run's line nor another push's is this step's: {:?}",
                    ex.slot()
                );
                std::fs::write(d.path().join("go"), "").unwrap();
                until(&|| matches!(ex.slot(), Slot::Taken { .. }));
                std::fs::write(d.path().join("finish"), "").unwrap();
                assert_eq!(step.join().unwrap(), Kind::Passed);
            });
            assert_eq!(ex.slot(), Slot::Unknown, "the step is over");
        }

        /// A long push's log folder is never pruned under it: a folder whose owner (the pid that made it, with the time of the record)
        /// still runs stays, whatever its age; one whose owner is gone, or that has no owner file, is pruned beyond the newest `keep`.
        #[test]
        fn a_run_folder_of_a_running_push_is_not_pruned_and_one_of_a_gone_push_is() {
            let d = tempfile::tempdir().unwrap();
            let root = d.path();
            let make = |name: &str, age: u64, owner: Option<u32>| {
                let dir = root.join(name);
                std::fs::create_dir(&dir).unwrap();
                if let Some(pid) = owner {
                    std::fs::write(dir.join("owner"), format!("{pid}\n")).unwrap();
                }
                let at = std::time::SystemTime::now() - Duration::from_secs(age);
                std::fs::File::open(&dir).unwrap().set_modified(at).unwrap();
            };
            make("running-old", 5000, Some(std::process::id()));
            make("gone-old", 4000, Some(4_000_000_000));
            make("unowned-old", 3000, None);
            make("new-1", 20, None);
            make("new-2", 10, None);
            prune_run_logs(root, 2);
            let mut left: Vec<String> = std::fs::read_dir(root)
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            left.sort();
            assert_eq!(
                left,
                ["new-1", "new-2", "running-old"],
                "the running push's folder stays, the others beyond the newest two go"
            );
        }

        #[test]
        fn a_log_is_read_from_where_it_stopped_and_only_the_steps_own_line_counts() {
            let d = tempfile::tempdir().unwrap();
            let f = d.path().join("g.log");
            let append = |t: &str| {
                use std::io::Write;
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&f)
                    .unwrap()
                    .write_all(t.as_bytes())
                    .unwrap();
            };
            let mut seen = LockSeen::new(lock_line("pre-push-7-3"));
            assert!(!seen.seen(&f), "no file yet");
            append("pre-push holds the lock (pid 1)\npre-push-7-30 holds the lock (pid 2)\n");
            assert!(
                !seen.seen(&f),
                "another step's line, and a name that only starts like ours, are not ours"
            );
            // A line cut between two reads is found.
            append("pre-push-7-3 hol");
            assert!(!seen.seen(&f));
            append("ds the lock (pid 4) at x\n");
            assert!(seen.seen(&f));
            // Only new bytes are read: a megabyte of output is read once, not at every poll.
            let mut seen = LockSeen::new(lock_line("pre-push-7-9"));
            append(&"x".repeat(200 * 1024));
            while seen.pos < std::fs::metadata(&f).unwrap().len() {
                let before = seen.pos;
                assert!(!seen.seen(&f));
                assert!(seen.pos > before && seen.pos - before <= 64 * 1024, "{} -> {}", before, seen.pos);
            }
            let at = seen.pos;
            assert!(!seen.seen(&f));
            assert_eq!(seen.pos, at, "nothing new: nothing read");
        }

        /// The executor (`ci run`) starts cargo for a gate that is not cargo itself: when the gate hands it a build folder it compiles
        /// through the local sccache like a cargo gate does, and never gets the folder as a variable.
        #[test]
        fn a_gate_that_hands_the_executor_a_build_folder_compiles_through_the_wrapper() {
            let d = tempfile::tempdir().unwrap();
            let mut ex = repo_with_fake_cargo(d.path(), true);
            ex.wrapper = crate::local_sccache::Lazy::ready(crate::local_sccache::Ready {
                bin: "/x/sccache".into(),
                vars: vec![("RUSTC_WRAPPER".into(), "/x/sccache".into())],
            });
            let limits = Limits {
                stall: Duration::from_secs(60),
            };
            let log = d.path().join("g.log");
            let mut g = gate("conformance-tests", "true");
            g.heavy = true;
            g.env.push(("PADDOCK_GATE_TARGET_DIR".into(), "/t/x".into()));
            let with = ex.call(&g, limits, &log, None, "pre-push-1-1").child_env(&ex.env);
            assert!(with.contains(&("RUSTC_WRAPPER".into(), "/x/sccache".into())), "{with:?}");
            g.env.clear();
            let without = ex.call(&g, limits, &log, None, "pre-push-1-1").child_env(&ex.env);
            assert!(
                without.iter().all(|(k, _)| k != "RUSTC_WRAPPER"),
                "a gate with no folder is never wrapped: {without:?}"
            );
        }

        #[test]
        fn a_landing_pushs_heavy_step_takes_the_priority_lane() {
            let d = tempfile::tempdir().unwrap();
            let mut ex = repo_with_fake_cargo(d.path(), true);
            ex.priority = true;
            let g = gate("clippy a", "echo ran >> out.txt");
            assert!(decide(&g, &ex, &test_subject()).kind() == Kind::Passed);
            let calls = std::fs::read_to_string(d.path().join("heavy-calls")).unwrap();
            assert!(calls.contains("run --high --queue-stall 1800 --run-max "), "{calls}");
        }

        #[test]
        fn a_heavy_built_from_the_tree_carries_the_gate_and_is_cached_under_its_hash() {
            let d = tempfile::tempdir().unwrap();
            let ex = repo_with_fake_cargo(d.path(), true);
            let g = gate("clippy a", "echo ran >> out.txt");
            assert!(decide(&g, &ex, &test_subject()).kind() == Kind::Passed);
            let calls = std::fs::read_to_string(d.path().join("heavy-calls")).unwrap();
            assert!(
                calls.contains("run --queue-stall 1800 --run-max ") && calls.contains(" pre-push-") && calls.contains(" -- "),
                "{calls}"
            );
            assert!(!calls.contains("--high"), "a normal push does not take the priority lane: {calls}");
            assert!(d.path().join("out.txt").is_file(), "the gate's command ran under the built heavy");
            let cached: Vec<_> = std::fs::read_dir(d.path().join("home/bin")).unwrap().flatten().collect();
            assert!(
                cached.len() == 1 && cached[0].file_name().to_string_lossy().starts_with("heavy-"),
                "{cached:?}"
            );
            // The per-build folder is gone (not under the cache folder: the build folder is `PADDOCK_DEV_TARGET_DIR`).
            let left: Vec<_> = std::fs::read_dir(d.path().join("tgt"))
                .map(|r| r.flatten().collect())
                .unwrap_or_default();
            assert!(left.is_empty(), "the build folder was not removed: {left:?}");
            assert!(!d.path().join("home").join("target-heavy-x").exists());
            // A second run finds it in the cache: cargo is not needed.
            std::fs::remove_file(d.path().join("fakebin/cargo")).unwrap();
            let ex2 = RealExec {
                built_heavy: Default::default(),
                live_slot: Default::default(),
                priority: false,
                wrapper: crate::local_sccache::Lazy::off(),
                env: ex.env.clone(),
                root: ex.root.clone(),
                logs: ex.logs.clone(),
                stop: ex.stop.clone(),
                watchdog: Arc::new(crate::pre_push::watchdog::Never),
                row_cap: ROW_CAP,
                state: ex.state.clone(),
            };
            assert!(decide(&g, &ex2, &test_subject()).kind() == Kind::Passed);
        }

        #[test]
        fn a_heavy_that_will_not_build_is_not_measured_and_names_the_error() {
            let d = tempfile::tempdir().unwrap();
            let ex = repo_with_fake_cargo(d.path(), false);
            let r = decide(&gate("clippy a", "true"), &ex, &test_subject());
            assert_eq!(r.kind(), Kind::NotMeasured, "{r:?}");
            let reason = r.reason();
            assert!(
                reason.contains("unresolved import") && reason.contains("no heavy slot tool"),
                "{reason}"
            );
            assert!(r.next.contains("re-run"), "{}", r.next);
        }

        #[test]
        fn heavy_off_without_the_test_variable_is_an_override_and_not_measured() {
            let d = tempfile::tempdir().unwrap();
            let mut ex = real(d.path(), &d.path().join("l"));
            ex.env.insert("PADDOCK_HEAVY".into(), "off".into());
            let r = decide(&gate("clippy a", "true"), &ex, &test_subject());
            assert_eq!(r.kind(), Kind::NotMeasured, "{r:?}");
            assert!(r.reason().contains("OVERRIDE SET"), "{}", r.reason());
        }

        /// A stand-in step (paddock-testkit's `StandIn`: no shell fixture) that prints one line and then sleeps `sleep` seconds.
        fn quiet_step(dir: &Path, sleep: u64) -> Gate {
            let step = paddock_testkit::StandIn::new(dir, "quiet-step")
                .stdout("compiling x")
                .sleeps(Duration::from_secs(sleep))
                .install()
                .unwrap();
            Gate {
                program: step.program().to_string_lossy().to_string(),
                args: Vec::new(),
                heavy: false,
                ..gate("tests quiet", "")
            }
        }

        /// A watchdog for the tests: watches every step and ends it at its `after`-th look; records what it was shown.
        struct Fires {
            after: u32,
            seen: Arc<Mutex<Vec<(u64, String, bool)>>>,
        }

        struct FiresWatch {
            n: u32,
            after: u32,
            seen: Arc<Mutex<Vec<(u64, String, bool)>>>,
        }

        impl Watchdog for Fires {
            fn name(&self) -> &str {
                "fake"
            }
            fn watch(&self, _step: &StepInfo<'_>) -> Option<Box<dyn StepWatch>> {
                Some(Box::new(FiresWatch {
                    n: 0,
                    after: self.after,
                    seen: Arc::clone(&self.seen),
                }))
            }
        }

        impl StepWatch for FiresWatch {
            fn check(&mut self, o: &Observation<'_>) -> Option<Stuck> {
                self.seen
                    .lock()
                    .unwrap()
                    .push((o.log_len, o.last_line.to_string(), o.cpu.is_some()));
                self.n += 1;
                (self.n >= self.after).then(|| Stuck {
                    guard: "fake".into(),
                    reason: "the stand-in went quiet".into(),
                    quiet: Duration::from_secs(300),
                })
            }
        }

        /// The positive control of the watchdog plumbing (ER-33): a step its watch ends is stopped long before it would have ended,
        /// and its row is "not measured: stuck", never a pass, with the step, the last output line and the guard named.
        #[test]
        fn a_step_the_watchdog_ends_is_not_measured_stuck_and_the_guard_is_named() {
            let d = tempfile::tempdir().unwrap();
            let mut ex = real(d.path(), &d.path().join("l"));
            let seen = Arc::new(Mutex::new(Vec::new()));
            ex.watchdog = Arc::new(Fires {
                after: 2,
                seen: Arc::clone(&seen),
            });
            let g = quiet_step(d.path(), 60);
            let t = Instant::now();
            let r = decide(&g, &ex, &test_subject());
            assert!(
                t.elapsed() < Duration::from_secs(30),
                "the 60 s step was stopped, not waited for: {:?}",
                t.elapsed()
            );
            assert_eq!(r.kind(), Kind::NotMeasured, "{r:?}");
            let why = r.reason();
            assert!(why.starts_with("stuck:") && why.contains("last output line: compiling x"), "{why}");
            assert!(why.contains("stuck (fake)") && why.contains("no progress for 300 s"), "{why}");
            assert_eq!(r.ended_by.as_deref(), Some("fake"));
            let seen = seen.lock().unwrap();
            assert!(
                seen.len() >= 2 && seen.iter().all(|(len, line, cpu)| *len > 0 && line == "compiling x" && *cpu),
                "{seen:?}"
            );
            let phases = std::fs::read_to_string(d.path().join("logs/phases.log")).unwrap();
            assert!(phases.contains("ended by guard: fake"), "{phases}");
        }

        /// The same step with a watch that never fires, and with no watchdog, runs to its end and passes: the guard ends nothing by
        /// itself, only what its watch says.
        #[test]
        fn a_step_whose_watch_stays_silent_runs_to_its_end_and_passes() {
            let d = tempfile::tempdir().unwrap();
            let mut ex = real(d.path(), &d.path().join("l"));
            let g = quiet_step(d.path(), 2);
            let r = decide(&g, &ex, &test_subject());
            assert_eq!((r.kind(), r.ended_by.clone()), (Kind::Passed, None), "{r:?}");
            ex.watchdog = Arc::new(Fires {
                after: u32::MAX,
                seen: Arc::default(),
            });
            let r = decide(&g, &ex, &test_subject());
            assert_eq!((r.kind(), r.ended_by.clone()), (Kind::Passed, None), "{r:?}");
        }

        /// A stand-in step printing `lines` (each one output line), then sleeping `sleep` seconds, read as cargo's JSON progress.
        fn json_step(dir: &Path, lines: &[&str], sleep: u64) -> Gate {
            let mut step = paddock_testkit::StandIn::new(dir, "json-step");
            for l in lines {
                step = step.stdout(l);
            }
            let step = step.sleeps(Duration::from_secs(sleep)).install().unwrap();
            Gate {
                program: step.program().to_string_lossy().to_string(),
                args: Vec::new(),
                heavy: false,
                progress: crate::pre_push::progress::Progress::CargoJson,
                ..gate("tests json", "")
            }
        }

        const UNIT_LINE: &str = r#"{"reason":"compiler-artifact","package_id":"x 0.1.0","fresh":false}"#;

        fn stall_dog(mode: crate::pre_push::stall::Mode, gap: u64, history: Option<&Path>) -> Arc<dyn Watchdog> {
            Arc::new(crate::pre_push::stall::StallWatchdog::new(
                mode,
                Duration::from_secs(gap),
                history.map(|p| Arc::new(crate::pre_push::history::History::new(p))),
            ))
        }

        /// The positive control of the progress watchdog (ER-33, ER-41): a step that finished one unit and then shows nothing is
        /// stopped as stuck (enforcing), its stacks written, long before the step would have ended; the same step in report-only
        /// mode is left running and passes, with "would have stopped" in its phase line.
        #[test]
        fn a_step_that_shows_no_progress_for_the_gap_is_stuck_when_enforced_and_only_reported_otherwise() {
            use crate::pre_push::stall::Mode;
            let d = tempfile::tempdir().unwrap();
            let mut ex = real(d.path(), &d.path().join("l"));
            ex.watchdog = stall_dog(Mode::Enforce, 3, None);
            let g = json_step(d.path(), &[UNIT_LINE], 60);
            let t = Instant::now();
            let r = decide(&g, &ex, &test_subject());
            assert!(t.elapsed() < Duration::from_secs(40), "stopped, not waited for: {:?}", t.elapsed());
            assert_eq!(r.kind(), Kind::NotMeasured, "{r:?}");
            assert!(
                r.reason().starts_with("stuck:") && r.reason().contains("progress watchdog"),
                "{}",
                r.reason()
            );
            assert!(r.ended_by.as_deref().is_some_and(|g| g.starts_with("progress watchdog")));
            // the report-only control: the same silence, a step that ends by itself after 8 s
            let d = tempfile::tempdir().unwrap();
            let mut ex = real(d.path(), &d.path().join("l"));
            ex.watchdog = stall_dog(Mode::ReportOnly, 3, None);
            let g = json_step(d.path(), &[UNIT_LINE], 8);
            let r = decide(&g, &ex, &test_subject());
            assert_eq!(r.kind(), Kind::Passed, "report-only never ends a step: {r:?}");
            let phases = std::fs::read_to_string(d.path().join("logs/phases.log")).unwrap();
            assert!(phases.contains("watchdog: would have stopped"), "{phases}");
        }

        /// Cargo waiting for another cargo's file lock is a wait state, not a stall: the same silence that is stuck above is not
        /// stuck when the step's last line says it waits for a lock.
        #[test]
        fn a_step_waiting_for_a_file_lock_is_not_stuck() {
            use crate::pre_push::stall::Mode;
            let d = tempfile::tempdir().unwrap();
            let mut ex = real(d.path(), &d.path().join("l"));
            ex.watchdog = stall_dog(Mode::Enforce, 3, None);
            let g = json_step(d.path(), &[UNIT_LINE, "    Blocking waiting for file lock on build directory"], 8);
            let r = decide(&g, &ex, &test_subject());
            assert_eq!((r.kind(), r.ended_by.clone()), (Kind::Passed, None), "{r:?}");
        }

        /// A finished step is written to the history, cold or warm by its build folder.
        #[test]
        fn a_finished_step_is_recorded_in_the_history_cold_or_warm() {
            use crate::pre_push::stall::Mode;
            let d = tempfile::tempdir().unwrap();
            let mut ex = real(d.path(), &d.path().join("l"));
            let h = d.path().join("step-history.jsonl");
            ex.watchdog = stall_dog(Mode::ReportOnly, 900, Some(&h));
            // a step shorter than one look (a second) is never watched and so leaves no record: this one lasts two
            let mut g = json_step(d.path(), &[UNIT_LINE, "error: the lint"], 2);
            g.id = "clippy crates".into();
            g.cwd = String::new();
            g.build_dir = Some("nothing-built-here".into());
            let r = decide(&g, &ex, &test_subject());
            assert_eq!(r.kind(), Kind::Passed, "the stand-in exits 0: {r:?}");
            let recs = crate::pre_push::history::History::new(&h).records();
            assert_eq!(recs.len(), 1, "{recs:?}");
            assert_eq!(
                (recs[0].kind.as_str(), recs[0].cold, recs[0].ok, recs[0].events),
                ("clippy", true, true, 1)
            );
        }

        /// The side finding of the research note (ER-43): a row waits for its build folder BEFORE it takes a heavy slot. With the folder
        /// held by another run, the step is not measured at the queue allowance (it never started), the heavy slot was never taken
        /// meanwhile, and once the folder is free the same step passes.
        #[test]
        fn a_row_waits_for_its_build_folder_outside_the_heavy_slot() {
            let d = tempfile::tempdir().unwrap();
            let slot = d.path().join("l");
            let ex = real(d.path(), &slot);
            let mut g = gate("clippy a", "true");
            g.build_dir = Some("tgt/a".into());
            let lock_path = ex.build_lock_path("tgt/a");
            std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
            let other = std::fs::OpenOptions::new().create(true).append(true).open(&lock_path).unwrap();
            other.lock().unwrap();
            let limits = Limits {
                stall: Duration::from_secs(2),
            };
            let r = decide_with(&g, &ex, limits, &test_subject(), &mut |_| {});
            assert_eq!(r.kind(), Kind::NotMeasured, "{r:?}");
            assert!(r.reason().contains("build folder is in use"), "{}", r.reason());
            assert!(!slot.exists(), "no heavy slot was taken while the folder was busy");
            drop(other);
            let r = decide_with(&g, &ex, limits, &test_subject(), &mut |_| {});
            assert_eq!(r.kind(), Kind::Passed, "the control: free folder, same step: {r:?}");
        }

        /// There is no total shared by the rows (2026-11-21): a 6 s step runs to its end and passes, heavy or not, and what ends a step is
        /// its own row cap.
        #[test]
        fn a_row_is_bounded_only_by_its_own_cap() {
            let d = tempfile::tempdir().unwrap();
            let ex = real(d.path(), &d.path().join("l"));
            let g = quiet_step(d.path(), 6);
            let r = decide(&g, &ex, &test_subject());
            assert_eq!(r.kind(), Kind::Passed, "{r:?}");
            // a heavy row through another heavy (no way to split its queue from its run) is bounded by the row cap from its start
            let mut g = quiet_step(d.path(), 6);
            g.heavy = true;
            let r = decide(&g, &ex, &test_subject());
            assert_eq!(r.kind(), Kind::Passed, "{r:?}");
        }

        /// Review P1-2: a step the watchdog ends as stuck stops its Linux container as well (the heavy slot is freed when the client
        /// ends, and a container left compiling would run beside the next step). Any cut-short ending stops it.
        #[test]
        fn a_container_run_the_watchdog_ends_is_stopped_with_its_label() {
            let d = tempfile::tempdir().unwrap();
            let mut ex = real(d.path(), &d.path().join("l"));
            let docker = paddock_testkit::StandIn::new(d.path(), "docker")
                .records_args()
                .when_args(&["ps"], paddock_testkit::Actions::new().stdout("abc123").exit(0))
                .install()
                .unwrap();
            ex.env.insert("DOCKER".into(), docker.program().to_string_lossy().to_string());
            ex.watchdog = Arc::new(Fires {
                after: 2,
                seen: Arc::default(),
            });
            let mut g = quiet_step(d.path(), 60);
            g.need = Need::Docker;
            let limits = Limits {
                stall: Duration::from_secs(300),
            };
            let done = ex.run(&g, limits).unwrap();
            assert!(done.cut_short() && !done.success, "{done:?}");
            let calls = docker.calls().unwrap();
            assert!(
                calls.iter().any(|c| c.first().is_some_and(|a| a == "stop")),
                "the container was stopped: {calls:?}"
            );
        }

        /// Review P2-1: every decision about an ending reads the type. A cut-short ending is exactly one that is not the run's own (the
        /// guards, the cap, the person, the output cap); the guard named is the watchdog's or the row cap.
        #[test]
        fn the_ending_type_answers_cut_short_and_guard_for_every_ending() {
            let stuck = Ending::Stuck { guard: "fake".into() };
            let all = [
                (Ending::Exited, false, None),
                (stuck, true, Some("fake")),
                (Ending::RowCap, true, Some("row cap")),
                (Ending::Cancelled, true, None),
                (Ending::QueueLimit, false, None),
                (Ending::OutputCap, true, None),
            ];
            for (e, cut, guard) in all {
                assert_eq!(e.cut_short(), cut, "{e:?}");
                assert_eq!(e.guard().as_deref(), guard, "{e:?}");
            }
            assert_eq!(Ending::from_how("timed out after 5 s"), Ending::RowCap);
            assert_eq!(Ending::from_how("cancelled"), Ending::Cancelled);
            assert_eq!(Ending::from_how("queue limit: no slot"), Ending::QueueLimit);
            assert_eq!(Ending::from_how("exit 101"), Ending::Exited);
        }

        /// Review P2-2: the lock files live under the state folder the caller gave, never beside the logs or in the temp folder.
        #[test]
        fn the_build_folder_locks_live_under_the_given_state_folder() {
            let d = tempfile::tempdir().unwrap();
            let ex = real(d.path(), &d.path().join("l"));
            let p = ex.build_lock_path("tgt/a");
            assert!(p.starts_with(d.path().join("state").join("build-locks")), "{}", p.display());
            assert_ne!(p, ex.build_lock_path("tgt/b"), "one file per folder");
            assert_eq!(p, ex.build_lock_path("tgt/a"), "the same folder, the same file");
        }

        /// The row cap is the backstop: a step still running when it is spent is stopped (its stacks written first) and "not measured",
        /// never a pass, and the summary names the cap.
        #[test]
        fn a_step_still_running_at_the_row_cap_is_not_measured_and_names_it() {
            let d = tempfile::tempdir().unwrap();
            let mut ex = real(d.path(), &d.path().join("l"));
            ex.row_cap = Duration::from_secs(2);
            let g = quiet_step(d.path(), 60);
            let t = Instant::now();
            let r = decide(&g, &ex, &test_subject());
            assert!(t.elapsed() < Duration::from_secs(30), "{:?}", t.elapsed());
            assert_eq!(r.kind(), Kind::NotMeasured, "{r:?}");
            assert_eq!(r.ended_by.as_deref(), Some("row cap"));
            assert!(r.reason().contains("timed out"), "{}", r.reason());
            let stacks = std::fs::read_dir(d.path().join("logs"))
                .unwrap()
                .flatten()
                .any(|f| f.file_name().to_string_lossy().ends_with(".stacks.txt"));
            assert!(stacks, "the stacks were written before the tree was ended");
        }

        /// Queue time is never judged (ER-43): a step queued for our slot is not shown to its watch; a running one is, from the moment it
        /// took the slot. A step through another `heavy` (no lock line to tell a queue from a run) has its row cap counted from its start
        /// and is not judged.
        #[test]
        fn only_a_running_step_is_shown_to_its_watch() {
            let t0 = Instant::now();
            let at = t0 + Duration::from_secs(40);
            assert_eq!(
                active_since(true, true, Slot::Queued { since: t0 }, t0),
                None,
                "queued behind others"
            );
            assert_eq!(active_since(true, true, Slot::Unknown, t0), None);
            assert_eq!(
                active_since(true, true, Slot::Taken { since: t0, at }, t0),
                Some((at, true)),
                "the queue is not run time"
            );
            assert_eq!(
                active_since(false, true, Slot::Unknown, t0),
                Some((t0, false)),
                "another heavy: capped from the start, never judged"
            );
            assert_eq!(
                active_since(false, false, Slot::Unknown, t0),
                Some((t0, true)),
                "a step with no slot runs from its start"
            );
        }

        #[test]
        fn a_run_ended_at_its_limit_says_which_limit_ran_out() {
            let done = Done {
                success: false,
                how: "timed out after 700 s of running".into(),
                tail: String::new(),
                log: None,
                timing: Some(Timing { queued: 5.0, ran: 700.0 }),
                ending: Ending::RowCap,
            };
            assert_eq!(run_limit_note(&done), "; the row cap of 60 min spent after 700 s of run");
            let other = Done {
                how: "exit 1".into(),
                ending: Ending::Exited,
                ..done
            };
            assert_eq!(run_limit_note(&other), "", "any other ending says nothing");
        }

        /// Review (a): a build that timed out is not remembered as the answer for the whole run. The first call (a cargo that outlasts a
        /// 1 s limit) is an error that names the timeout; the next call, with the cargo healthy again, builds and answers. A build that
        /// failed for good (a compile error) IS remembered: the second call does not build again.
        #[test]
        fn a_heavy_build_that_timed_out_is_tried_again_and_one_that_failed_for_good_is_not() {
            let d = tempfile::tempdir().unwrap();
            let ex = repo_with_fake_cargo(d.path(), true);
            // The fake cargo is a StandIn: its behaviour is the spec beside it. Swap in one that outlasts the limit, then put the
            // healthy spec back.
            let spec = d.path().join("fakebin/cargo.stand-in.json");
            let good = std::fs::read_to_string(&spec).unwrap();
            paddock_testkit::StandIn::new(&d.path().join("fakebin"), "cargo")
                .sleeps(Duration::from_secs(20))
                .install()
                .unwrap();
            let has = |p: &str| ex.has_tool(p);
            let first = heavy_command_for(&ex.root, &ex.env, &has, &ex.built_heavy, Duration::from_secs(1));
            assert!(first.as_ref().is_err_and(|e| e.contains("timed out")), "{first:?}");
            assert!(ex.built_heavy.get().is_none(), "a timeout is not remembered");
            std::fs::write(&spec, good).unwrap();
            let second = heavy_command_for(&ex.root, &ex.env, &has, &ex.built_heavy, Duration::from_secs(600));
            assert!(second.is_ok(), "the second call builds it: {second:?}");
            // a deterministic failure is remembered for the run
            let d2 = tempfile::tempdir().unwrap();
            let bad = repo_with_fake_cargo(d2.path(), false);
            let has2 = |p: &str| bad.has_tool(p);
            let one = heavy_command_for(&bad.root, &bad.env, &has2, &bad.built_heavy, Duration::from_secs(600));
            assert!(one.is_err() && bad.built_heavy.get().is_some(), "{one:?}");
        }

        /// Review (d): the outer limits of a step leave the monitor room to write the stacks (their own bound plus 15 s) before they
        /// would end the tree themselves.
        #[test]
        fn the_grace_beyond_the_row_cap_outlasts_the_stack_capture() {
            assert!(CAP_GRACE >= stacks::TOTAL_LIMIT + Duration::from_secs(15), "{CAP_GRACE:?}");
            assert_eq!(own_run_limit(&gate("x", "true")), ROW_CAP + CAP_GRACE);
        }

        #[test]
        fn heavy_off_still_runs_without_the_slot_for_hermetic_tests() {
            let d = tempfile::tempdir().unwrap();
            let mut ex = real(d.path(), &d.path().join("l"));
            ex.env.insert("PADDOCK_HEAVY".into(), "off".into());
            ex.env.insert(TEST_HEAVY_OFF.into(), "1".into());
            ex.env.insert("PATH".into(), "/usr/bin:/bin".into());
            assert_eq!(decide(&gate("clippy a", "true"), &ex, &test_subject()).kind(), Kind::Passed);
        }
    }

    #[cfg(unix)]
    #[test]
    fn docker_is_looked_for_at_its_sockets_and_never_started() {
        let env = HashMap::from([("HOME".to_string(), "/h".to_string())]);
        let s = docker_sockets(&env);
        assert!(s.contains(&PathBuf::from("/h/.orbstack/run/docker.sock")) && s.contains(&PathBuf::from("/var/run/docker.sock")));
        let host = HashMap::from([("DOCKER_HOST".to_string(), "unix:///x/d.sock".to_string())]);
        assert_eq!(docker_sockets(&host), [PathBuf::from("/x/d.sock")]);
        // No daemon behind a socket path: not up, and nothing was run to find out.
        let none = HashMap::from([("DOCKER_HOST".to_string(), "unix:///nonexistent/paddock-docker.sock".to_string())]);
        assert!(!docker_answers(&none));
    }
}
