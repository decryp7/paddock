//! Running the gates: each one decided by an [`Exec`] (real processes, or a fake in the tests), several at a time, cheapest first,
//! inside a total time budget, and stopped when the person stops the push.

use super::budget::{BudgetClock, Timing, parse_heavy_timing, with_heavy_flags};
use super::gates::{Gate, Need, Planned};
use super::linux_inputs::{ImageProbe, PROBE_NEXT, image_probe};
use crate::ci_tool::{Output, ToolCall, tail};
use paddock_cli::{CheckId, Finding, Outcome, Reason, Subject, ToolVersion, UnixSeconds, measure};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The push gate's total budget (ADR 0038 amendment A2.5): what has not run by then is "not measured". It counts RUN time: time a
/// step queued for the heavy slot behind other agents' steps is credited back ([`BudgetClock`], ADR 0102) and bounded on its own
/// by [`QUEUE_MAX`].
pub const BUDGET: Duration = Duration::from_secs(15 * 60);

/// The push's queue allowance: the most time all its heavy steps together may wait for the machine-wide slot behind other agents'
/// steps (`PADDOCK_QUEUE_MAX` seconds overrides). A step that would start after the allowance is spent, or whose wait reaches what
/// is left of it, is "not measured" with its queue time and the load, never a pass (ADR 0102). Kept at 30 min, now for the whole
/// push instead of per step (so a push waits at most 15 min of run plus 30 min of queue; it fails closed). The measurement is per
/// step so far: heavy's activity log of 2026-11-14 13:02-19:09Z, 145 pre-push steps, queue time (lock wait plus load gate) p50
/// 21 s, p90 386 s, p95 703 s, p99 993 s, max 1,187 s, so one step's worst wait fits (docs/benchmarks.md, "Pre-push heavy queue
/// times"). Per-push queue time is logged from now on (`queue-times.log` beside the gate logs); revisit the value with it.
pub const QUEUE_MAX: Duration = Duration::from_secs(2 * 15 * 60);

/// What one step may spend: `run` of running (heavy's `--run-max`) and `queue` of waiting for the slot (heavy's `--queue-max`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The run time left of the push's budget.
    pub run: Duration,
    /// The queue time left of the push's allowance.
    pub queue: Duration,
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
}

impl Done {
    /// It did not end by itself: the budget or the gate's own limit stopped it, or the person did. Nothing is proven either way.
    pub fn cut_short(&self) -> bool {
        self.how.starts_with("timed out") || self.how == "cancelled" || self.how.starts_with("stopped at the output cap")
    }

    /// `heavy` gave up waiting for a slot: the command never ran.
    pub fn no_slot(&self) -> bool {
        self.how.starts_with("queue limit")
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
    /// Runs the gate within `limits` (a heavy step: at most `limits.run` of running and `limits.queue` of waiting for the slot).
    /// `Err`: it could not be started (the program is missing).
    fn run(&self, gate: &Gate, limits: Limits) -> Result<Done, String>;
    /// The 1-minute load average now, for the reason of a row the queue stopped (None: unknown).
    fn load(&self) -> Option<f64> {
        None
    }
    /// The clock the push's budget is read on: the monotonic clock; a test's fake moves its own.
    fn now(&self) -> Instant {
        Instant::now()
    }
    /// Why a heavy gate has no machine-wide slot to go through (no `heavy` on PATH and the build of tools/heavy failed), or
    /// `None`: it would otherwise compete with every other push (ER-24), so it is not measured instead.
    fn heavy_unavailable(&self, _left: Duration) -> Option<String> {
        None
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
        }
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

fn first_word(id: &str) -> &str {
    id.split(' ').next().unwrap_or(id)
}

/// The most one step may run on its own (heavy's `--run-max` is this or what the push's budget has left, whichever is less).
fn own_run_limit(gate: &Gate) -> Duration {
    Duration::from_secs(if gate.heavy { 3000 } else { 900 })
}

/// For a run `heavy` ended at its run limit: which limit it was, with the run time it had, so a row says whether the push's run
/// budget or the gate's own limit ran out (empty for any other ending).
fn run_limit_note(gate: &Gate, limits: Limits, done: &Done) -> String {
    let (true, Some(t)) = (done.how.starts_with("timed out after"), done.timing) else {
        return String::new();
    };
    if limits.run < own_run_limit(gate) {
        format!("; run budget spent after {:.0} s of run", t.ran)
    } else {
        format!(
            "; the gate's own run limit of {} s spent after {:.0} s of run",
            own_run_limit(gate).as_secs(),
            t.ran
        )
    }
}

/// The verdict of one gate: its needs first, then the run.
pub fn decide(gate: &Gate, exec: &dyn Exec, left: Duration, subject: &Subject) -> Row {
    decide_with(
        gate,
        exec,
        Limits {
            run: left,
            queue: QUEUE_MAX,
        },
        subject,
        &mut |_| {},
    )
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
    }
    if gate.heavy
        && let Some(why) = exec.heavy_unavailable(limits.run)
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
        // No slot means the command never ran: not measured, never a pass or a failure (ADR 0102 on ADR 0089's rows).
        Ok(done) if done.no_slot() => Row::not_measured(
            format!(
                "{} never got a heavy slot (queue allowance spent: queued {:.1} s of the {} min allowed, {}; load {}): the machine was busy with other steps, so nothing was measured",
                gate.rerun(),
                done.timing.map_or(0.0, |t| t.queued),
                limits.queue.as_secs() / 60,
                done.how,
                load_text(exec.load())
            ),
            format!(
                "look at `heavy status` and re-run `./dev check --pre-push --only {}` when it is free; the push's queue allowance is PADDOCK_QUEUE_MAX (seconds), and an agent reports this (ER-24, ER-33)",
                first_word(&gate.id)
            ),
        ),
        Ok(done) if done.cut_short() => Row::not_measured(
            format!(
                "{} ({}{}) before it finished{}",
                gate.rerun(),
                done.how,
                run_limit_note(gate, limits, &done),
                done.log.as_ref().map_or(String::new(), |l| format!("; output so far in {l}"))
            ),
            if gate.id.starts_with("clippy-linux") {
                format!(
                    "run `./dev check --warm-linux {}` once (no budget, live output; the first run of a workspace builds all its dependencies in the container's kept volume, minutes per workspace; docs/benchmarks.md), then push again",
                    gate.workspaces.join(" ")
                )
            } else {
                format!(
                    "re-run it alone: {} (the whole gate has a budget of {} min)",
                    gate.rerun(),
                    BUDGET.as_secs() / 60
                )
            },
        ),
        Ok(done) => {
            let hint = match (gate.id.starts_with("clippy-linux"), done.how.as_str()) {
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
            let mut row = Row::from_run(&gate.id, subject, &done, format!("{} ({})", gate.rerun(), done.how), secs(&done));
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
    /// The total budget of run time: a gate that would start later is not measured, one that runs over is stopped.
    pub budget: Duration,
    /// The push's queue allowance ([`QUEUE_MAX`]): the time its heavy steps may wait for the slot in all.
    pub queue: Duration,
    /// Set by SIGINT or SIGTERM: nothing more starts.
    pub stop: &'a AtomicBool,
    /// What the run is about (the commit and its tree): the evidence of every pass.
    pub subject: &'a Subject,
}

/// Every planned row decided, `workers` at a time, lowest rank first; the result keeps the planned order. The default budget, never
/// stopped.
pub fn run_all(planned: &[Planned], exec: &dyn Exec, workers: usize, subject: &Subject) -> Vec<(String, Row)> {
    run_all_with(
        planned,
        exec,
        &RunOpts {
            workers,
            budget: BUDGET,
            queue: QUEUE_MAX,
            stop: &AtomicBool::new(false),
            subject,
        },
    )
}

fn row_id(p: &Planned) -> &str {
    match p {
        Planned::Run(g) => &g.id,
        Planned::Skip { id, .. } | Planned::NotMeasured { id, .. } | Planned::Broken { id, .. } => id,
    }
}

/// [`run_all`] with its bounds. A gate that panics, one that never ran and one the budget or a signal cut off are "not measured",
/// never a pass.
pub fn run_all_with(planned: &[Planned], exec: &dyn Exec, opts: &RunOpts) -> Vec<(String, Row)> {
    run_all_timed(planned, exec, opts).0
}

/// Whether a row may start, from [`Turns::next`].
enum Start {
    Go(Limits),
    Interrupted,
    BudgetSpent,
    QueueSpent,
}

/// The rows left and whether a heavy step of the push is in flight. One heavy step at a time: heavy has one machine-wide slot (more
/// only with `HEAVY_DIR` and `HEAVY_SLOTS`), so a second heavy step of the same push would only queue behind the first, and that
/// wait could not be told from waiting for other agents (ADR 0102). With one in flight, every second a heavy step queues is time
/// spent waiting for others, and the next heavy step is decided only after the one before has ended and its queue time is on the
/// clock. While a heavy step is in flight, a free worker takes the next light row instead of waiting.
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

    /// The next row a free worker takes, and for a gate that runs whether it may start, with the run and queue time it may spend.
    /// `None`: nothing is left.
    fn next(&self, planned: &[Planned], clock: &BudgetClock, opts: &RunOpts, now: &dyn Fn() -> Instant) -> Option<(usize, Option<Start>)> {
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
                let run = clock.left(now());
                let queue = opts.queue.saturating_sub(clock.credit());
                let start = if run.is_zero() {
                    Start::BudgetSpent
                } else if heavy(i) && queue.is_zero() {
                    Start::QueueSpent
                } else {
                    st.1 |= heavy(i);
                    Start::Go(Limits { run, queue })
                };
                return Some((i, Some(start)));
            }
            // Only heavy rows are left and one is in flight: woken when it ends; the timeout only re-reads the stop flag.
            st = match self.changed.wait_timeout(st, Duration::from_millis(500)) {
                Ok((g, _)) => g,
                Err(_) => return None,
            };
        }
    }

    /// A step that [`Turns::next`] let start has ended (its queue time is already on the clock).
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

/// [`run_all_with`], and the queue and run time of every row that went through `heavy`. The budget is run time: what a step spent
/// queued behind other agents' steps is credited back ([`BudgetClock`]).
pub fn run_all_timed(planned: &[Planned], exec: &dyn Exec, opts: &RunOpts) -> (Vec<(String, Row)>, BTreeMap<String, Timing>) {
    let mut order: Vec<usize> = (0..planned.len()).collect();
    order.sort_by_key(|&i| match &planned[i] {
        Planned::Run(g) => g.rank,
        _ => 0,
    });
    let now = || exec.now();
    let clock = BudgetClock::new(now(), opts.budget);
    let turns = Turns::new(order.into_iter().collect::<VecDeque<_>>());
    let timings = Mutex::new(BTreeMap::<String, Timing>::new());
    let results: Vec<Mutex<Option<Row>>> = planned.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..opts.workers.max(1) {
            s.spawn(|| {
                while let Some((i, start)) = turns.next(planned, &clock, opts, &now) {
                    let v = match &planned[i] {
                        Planned::Run(g) => match start.unwrap_or(Start::Interrupted) {
                            Start::Interrupted => Row::not_measured(
                                "the push was interrupted before it started",
                                format!("re-run `./dev check --pre-push --only {}`", first_word(&g.id)),
                            ),
                            Start::BudgetSpent => Row::not_measured(
                                format!(
                                    "run budget spent: the {} min of run time of the push gate was used by earlier steps before this one started ({:.1} s queued for other agents' heavy steps did not count against it)",
                                    opts.budget.as_secs() / 60,
                                    clock.credit().as_secs_f64()
                                ),
                                format!("re-run `./dev check --pre-push --only {}`", first_word(&g.id)),
                            ),
                            Start::QueueSpent => Row::not_measured(
                                format!(
                                    "never started: the push's queue allowance of {} min was spent waiting for other agents' heavy steps (queued {:.1} s in all, load {})",
                                    opts.queue.as_secs() / 60,
                                    clock.credit().as_secs_f64(),
                                    load_text(exec.load())
                                ),
                                format!(
                                    "`heavy status` shows who holds the slot; re-run `./dev check --pre-push --only {}` when the load is lower; an agent reports this (ER-24, ER-33)",
                                    first_word(&g.id)
                                ),
                            ),
                            Start::Go(limits) => {
                                let started = now();
                                let mut seen = |d: &Done| {
                                    if let Some(t) = d.timing {
                                        clock.record(started, now(), Duration::from_secs_f64(t.queued.max(0.0)));
                                        if let Ok(mut m) = timings.lock() {
                                            m.insert(g.id.clone(), t);
                                        }
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

/// The command that puts a heavy step through the machine-wide slot (ER-24): `PADDOCK_HEAVY` as given, else `heavy.sh` or `heavy`
/// on PATH. Empty when there is none (the caller says so).
pub fn heavy_prefix(env: &HashMap<String, String>, has: &dyn Fn(&str) -> bool) -> Vec<String> {
    if let Some(h) = env.get("PADDOCK_HEAVY").filter(|h| !h.trim().is_empty()) {
        if h.trim() == "off" {
            return Vec::new();
        }
        return h.split_whitespace().map(str::to_string).collect();
    }
    if has("heavy.sh") {
        return vec!["heavy.sh".into()];
    }
    if has("heavy") {
        return ["heavy", "run", "pre-push", "--"].map(String::from).to_vec();
    }
    Vec::new()
}

/// The command that puts a heavy step through the machine-wide slot: [`heavy_prefix`], else the `heavy` built from the tree at
/// `root` (once, kept in `built`; `left` bounds that build), held under a name of this run's own for as long as `built` lives
/// ([`crate::tool_cache::Held`]), so another agent pruning or replacing the shared cache cannot fail a later step. The one
/// resolver of the push gate and of `./dev gen check`, so both queue for the same slot. `Err` is the whole explanation; `Ok(empty)` only for the test-only `PADDOCK_HEAVY=off`.
pub fn heavy_command_for(
    root: &Path,
    env: &HashMap<String, String>,
    has_tool: &dyn Fn(&str) -> bool,
    built: &std::sync::OnceLock<Result<crate::tool_cache::Held, String>>,
    left: Duration,
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
    match built.get_or_init(|| super::heavy_tool::ensure(root, env, left).and_then(|bin| super::heavy_tool::hold(&bin, env))) {
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

/// Whether the output of a gate run through `heavy` shows that it never got the slot: it printed who holds the lock
/// ("the lock is held by <name> (pid <n>) ...") and never "<name> holds the lock". Returns the phrase for the verdict.
pub fn slot_wait(log: &str) -> Option<String> {
    if log.lines().any(|l| l.contains(" holds the lock (pid ")) {
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
    /// The push is a landing (`train/*`, `main`, a release tag): its heavy steps take the priority lane (`heavy --high`, ADR 0102).
    pub priority: bool,
    /// The local sccache wrapper for the gates that compile (`None`: off, or CI; [`crate::local_sccache`]).
    pub wrapper: crate::local_sccache::Lazy,
}

/// Variables every gate may see besides the base list: the toolchain, Docker, caches and Paddock's own settings.
pub(crate) const PASS: &[&str] = &[
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
    /// The command that puts a heavy gate through the slot: `PADDOCK_HEAVY`, `heavy.sh` or `heavy` on PATH, else the `heavy`
    /// built from this tree ([`super::heavy_tool`]). Empty for `PADDOCK_HEAVY=off` (hermetic tests only); `Err` when the build failed.
    ///
    /// `left` is the run time the push has left on its credited clock ([`BudgetClock`]): the build of heavy is cut off there, so
    /// time queued for the slot never counts against it.
    fn heavy_command(&self, left: Duration) -> Result<Vec<String>, String> {
        heavy_command_for(&self.root, &self.env, &|p| self.has_tool(p), &self.built_heavy, left)
    }

    fn call(&self, gate: &Gate, limits: Limits, log: &Path, label: Option<&str>) -> ToolCall {
        let left = limits.run;
        let own = own_run_limit(gate);
        // A heavy step: its run time is limited by heavy itself (`--run-max`), its wait for a slot separately (`--queue-max`);
        // the outer timeout is only a backstop for both plus a minute.
        let run_max = own.min(left);
        let heavy = if gate.heavy {
            with_heavy_flags(
                &self.heavy_command(limits.run).unwrap_or_default(),
                self.priority,
                limits.queue,
                run_max,
            )
        } else {
            Vec::new()
        };
        let limited = heavy.iter().any(|a| a == "--run-max");
        let (program, args): (String, Vec<String>) = match heavy.split_first() {
            Some((first, rest)) => (
                first.clone(),
                rest.iter()
                    .cloned()
                    .chain(std::iter::once(gate.program.clone()))
                    .chain(gate.args.iter().cloned())
                    .collect(),
            ),
            None => (gate.program.clone(), gate.args.clone()),
        };
        let timeout = if limited {
            run_max + limits.queue + Duration::from_secs(60)
        } else {
            own.min(left)
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
        for (k, v) in &gate.env {
            call = call.env(k, v);
        }
        // A cargo gate that names its build folder as configuration never also gets it as a variable (the variable would win over
        // the configuration and sccache hashes it), and compiles through the local sccache when there is one.
        if crate::cargo_target::is_cargo(&gate.program) && crate::cargo_target::names_target_dir(&gate.args) {
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

/// A token no other run has: process id and clock.
fn run_token() -> String {
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

    fn heavy_unavailable(&self, left: Duration) -> Option<String> {
        self.heavy_command(left).err()
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

    fn load(&self) -> Option<f64> {
        let ran = ToolCall::new("uptime")
            .timeout(Duration::from_secs(10))
            .run(&self.env, &mut std::io::sink())
            .ok()?;
        parse_load(&ran.stdout)
    }

    fn run(&self, gate: &Gate, limits: Limits) -> Result<Done, String> {
        let _ = std::fs::create_dir_all(&self.logs);
        let log = self.logs.join(format!("{}.log", log_name(&gate.id)));
        // Best-effort: the .id file only helps a reader; a failed write never stops the gate.
        let _ = std::fs::write(log.with_extension("id"), format!("{}\n", gate.id));
        let label = matches!(gate.need, Need::Docker | Need::LocalCiImage).then(run_token);
        let mut ran = self
            .call(gate, limits, &log, label.as_deref())
            .run(&self.env, &mut std::io::sink())?;
        let mut timing = None;
        if gate.heavy {
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            timing = parse_heavy_timing(&text);
            // heavy's own limits: 79 no slot in time (the command never ran), 80 the command was ended at its run limit.
            let how = ran.how.clone();
            match how.as_str() {
                "exit 79" if timing.is_some() => ran.how = "queue limit: no slot taken in time".into(),
                "exit 80" if timing.is_some() => {
                    ran.how = format!("timed out after {} s of running", timing.map_or(0.0, |t| t.ran).round())
                }
                // Cut short while queued for the heavy slot: say so, the gate itself never ran (ER-33).
                h if h.starts_with("timed out") && !ran.success => {
                    if let Some(w) = slot_wait(&text) {
                        ran.how = format!("{}: {w}", ran.how);
                    }
                }
                _ => {}
            }
        }
        if let Some(l) = &label
            && !ran.success
            && matches!(ran.how.as_str(), h if h == "cancelled" || h.starts_with("timed out"))
        {
            self.stop_container(l);
        }
        Ok(Done {
            success: ran.success,
            how: ran.how,
            tail: tail(&ran.stderr, 12),
            log: Some(log.display().to_string()),
            timing,
        })
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

    #[test]
    fn heavy_is_found_when_the_variable_is_not_set() {
        let env = HashMap::new();
        assert!(heavy_prefix(&env, &|_| false).is_empty());
        assert_eq!(heavy_prefix(&env, &|p| p == "heavy.sh"), ["heavy.sh"]);
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
                priority: false,
                wrapper: crate::local_sccache::Lazy::off(),
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
            }
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
            let row = decide(&g, &ex, Duration::from_secs(60), &test_subject());
            assert_eq!(row.kind(), Kind::Failed);
            assert!(row.tail.contains("C100 conformance C999 is run by no reachable"), "{}", row.tail);
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
                        run: Duration::from_secs(30),
                        queue: Duration::from_secs(30),
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
                    s.spawn(move || assert!(decide(&g, ex, Duration::from_secs(60), &test_subject()).kind() == Kind::Passed));
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
            let ex = real(d.path(), &lock);
            let g = gate("clippy a", "echo never runs");
            let r = decide(&g, &ex, Duration::from_secs(2), &test_subject());
            assert_eq!(
                r.kind(),
                Kind::NotMeasured,
                "a slot that was never taken is not a pass or a failure: {r:?}"
            );
            assert!(r.reason().contains("waiting for the heavy slot, held by Bob"), "{}", r.reason());
        }

        /// A repository with the heavy tool's inputs committed, and a `cargo` that "builds" a fake heavy (a script that records
        /// its arguments and runs the gate's command), found first on the PATH of the run (ER-14: no real build).
        fn repo_with_fake_cargo(d: &Path, cargo_ok: bool) -> RealExec {
            let git = |args: &[&str]| {
                let ok = paddock_proc::command("git")
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
            let body = if cargo_ok {
                format!(
                    "T=$(printf '%s\\n' \"$@\" | sed -n \"s/^build.target-dir='\\(.*\\)'$/\\1/p\")\nmkdir -p \"$T/release\"\ncat > \"$T/release/heavy\" <<'EOS'\n#!/bin/sh\n[ \"$1\" = --version ] && {{ echo heavy-fake; exit 0; }}\necho \"$@\" >> {m}\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\nEOS\nchmod +x \"$T/release/heavy\"\n",
                    m = d.join("heavy-calls").display()
                )
            } else {
                "echo 'error[E0432]: unresolved import' >&2\nexit 101\n".to_string()
            };
            let cargo = bin.join("cargo");
            paddock_testkit::exe::write_program(&cargo, format!("#!/bin/sh\n{body}")).unwrap();
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
                priority: false,
                wrapper: crate::local_sccache::Lazy::off(),
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
                run: Duration::from_secs(60),
                queue: Duration::from_secs(60),
            };
            let log = d.path().join("g.log");
            let env_of = |ex: &RealExec| ex.call(&g, limits, &log, None).child_env(&ex.env);
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
            let sh = ex.call(&s, limits, &log, None).child_env(&ex.env);
            assert!(sh.iter().all(|(k, _)| k != "RUSTC_WRAPPER"), "{sh:?}");
        }

        #[test]
        fn a_landing_pushs_heavy_step_takes_the_priority_lane() {
            let d = tempfile::tempdir().unwrap();
            let mut ex = repo_with_fake_cargo(d.path(), true);
            ex.priority = true;
            let g = gate("clippy a", "echo ran >> out.txt");
            assert!(decide(&g, &ex, Duration::from_secs(60), &test_subject()).kind() == Kind::Passed);
            let calls = std::fs::read_to_string(d.path().join("heavy-calls")).unwrap();
            assert!(calls.contains("run --high --queue-max 1800 --run-max "), "{calls}");
        }

        #[test]
        fn a_heavy_built_from_the_tree_carries_the_gate_and_is_cached_under_its_hash() {
            let d = tempfile::tempdir().unwrap();
            let ex = repo_with_fake_cargo(d.path(), true);
            let g = gate("clippy a", "echo ran >> out.txt");
            assert!(decide(&g, &ex, Duration::from_secs(60), &test_subject()).kind() == Kind::Passed);
            let calls = std::fs::read_to_string(d.path().join("heavy-calls")).unwrap();
            assert!(
                calls.contains("run --queue-max 1800 --run-max ") && calls.contains(" pre-push --"),
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
                priority: false,
                wrapper: crate::local_sccache::Lazy::off(),
                env: ex.env.clone(),
                root: ex.root.clone(),
                logs: ex.logs.clone(),
                stop: ex.stop.clone(),
            };
            assert!(decide(&g, &ex2, Duration::from_secs(60), &test_subject()).kind() == Kind::Passed);
        }

        #[test]
        fn a_heavy_that_will_not_build_is_not_measured_and_names_the_error() {
            let d = tempfile::tempdir().unwrap();
            let ex = repo_with_fake_cargo(d.path(), false);
            let r = decide(&gate("clippy a", "true"), &ex, Duration::from_secs(60), &test_subject());
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
            let r = decide(&gate("clippy a", "true"), &ex, Duration::from_secs(5), &test_subject());
            assert_eq!(r.kind(), Kind::NotMeasured, "{r:?}");
            assert!(r.reason().contains("OVERRIDE SET"), "{}", r.reason());
        }

        #[test]
        fn the_heavy_build_is_cut_off_by_the_credited_run_time_not_the_wall_clock() {
            let d = tempfile::tempdir().unwrap();
            let ex = repo_with_fake_cargo(d.path(), true);
            // Plenty of credited run time left (the step queued behind others for as long as it liked): the build goes ahead.
            let r = decide(&gate("clippy a", "true"), &ex, Duration::from_secs(300), &test_subject());
            assert_eq!(r.kind(), Kind::Passed, "{r:?}");
        }

        #[test]
        fn a_run_ended_at_its_limit_says_which_limit_ran_out() {
            let g = gate("clippy a", "true");
            let done = Done {
                success: false,
                how: "timed out after 700 s of running".into(),
                tail: String::new(),
                log: None,
                timing: Some(Timing { queued: 5.0, ran: 700.0 }),
            };
            let budget = run_limit_note(
                &g,
                Limits {
                    run: Duration::from_secs(700),
                    queue: QUEUE_MAX,
                },
                &done,
            );
            assert_eq!(budget, "; run budget spent after 700 s of run");
            let own = run_limit_note(
                &g,
                Limits {
                    run: own_run_limit(&g),
                    queue: QUEUE_MAX,
                },
                &done,
            );
            assert!(own.contains("own run limit") && own.contains("after 700 s of run"), "{own}");
            let other = Done {
                how: "exit 1".into(),
                ..done
            };
            assert_eq!(
                run_limit_note(
                    &g,
                    Limits {
                        run: own_run_limit(&g),
                        queue: QUEUE_MAX
                    },
                    &other
                ),
                "",
                "any other ending says nothing"
            );
        }

        #[test]
        fn a_build_after_the_budget_is_not_started() {
            let d = tempfile::tempdir().unwrap();
            let ex = repo_with_fake_cargo(d.path(), true);
            let r = decide(&gate("clippy a", "true"), &ex, Duration::ZERO, &test_subject());
            assert_eq!(r.kind(), Kind::NotMeasured, "{r:?}");
            assert!(r.reason().contains("budget was spent"), "{}", r.reason());
        }

        #[test]
        fn heavy_off_still_runs_without_the_slot_for_hermetic_tests() {
            let d = tempfile::tempdir().unwrap();
            let mut ex = real(d.path(), &d.path().join("l"));
            ex.env.insert("PADDOCK_HEAVY".into(), "off".into());
            ex.env.insert(TEST_HEAVY_OFF.into(), "1".into());
            ex.env.insert("PATH".into(), "/usr/bin:/bin".into());
            assert_eq!(
                decide(&gate("clippy a", "true"), &ex, Duration::from_secs(5), &test_subject()).kind(),
                Kind::Passed
            );
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
