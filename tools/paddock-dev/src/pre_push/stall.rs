//! The progress watchdog (WP-2 of docs/research/2026-11-19-gate-hang-detection.md): a step is stuck when it has shown no progress for the
//! gap limit while it is not in a wait state. Built on `paddock_proc::StartJudge` (ADR 0031: patience, not a wall clock; rule 42
//! share first) fed one reading per [`WATCH_POLL`]: the progress counter of the step's output ([`super::progress`]), a stopped
//! liveness while cargo waits for another cargo's lock (the judge counts nothing then), and no CPU reading at all. CPU is read by the
//! watch only to word the report (spinning, idle, or hidden by a cache server or a container), never to decide: tree CPU is blind to
//! sccache and to the container row (research note, section 2).
//!
//! Progress is more than output (the audit of 2026-11-23: 11 of 13 report-only calls were healthy steps that print only at the end, so
//! their silence was most of their run). A step whose work the tree shows ([`StepInfo::signals`]) also makes progress when its process
//! tree's memory moves by more than [`MEMORY_STEP`] between two samples or its disk bytes grow ([`paddock_proc::tree_disk_io_from`]):
//! each is one more tick of the same progress counter the judge is fed, so a signal can only DELAY a verdict, never cause one. A step is
//! stuck when every signal is flat for the limit and it is not in a wait state. A step compiled through sccache (the compile runs in the
//! cache server) and the Linux container row (the work runs in a VM) show nothing in the tree: they keep output events and the row cap.
//!
//! The limit is [`GAP`] (15 minutes) unless the gate's own history ([`super::history`]) has seen that kind of step finish, in which case
//! it is tighter. The watchdog starts report-only ([`Mode::ReportOnly`], ER-41): a step the judge calls stuck is logged as "would have
//! stopped" and left running (only the row cap ends it) until [`Mode::Enforce`] is chosen with `PADDOCK_GATE_WATCHDOG=enforce`, which
//! becomes the default after a week of pushes of report-only data (WP-3 of the note; the first green run is recorded where the gate is
//! defined, in docs/benchmarks.md "Push gate run time").

use super::history::{History, StepRecord};
use super::progress::Progress;
use super::watchdog::{Observation, StepInfo, StepWatch, Stuck, WATCH_POLL, Watchdog};
use paddock_proc::{Cpu, Liveness, Patience, Reading, StallCause, StartJudge, StartState};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The longest a step may go without a finished unit (or test line) when the gate has no history of its kind: 5 minutes.
///
/// Measured cause (WP-0, 2026-11-19, docs/benchmarks.md "Push gate stall gap (WP-0)"): the gate's own cargo commands run cold, one
/// workspace at a time through heavy with `CARGO_BUILD_JOBS=2`, no sccache, on the shared Mac mini with other agents running (15-minute
/// load 9 on 14 cores), 11 workspaces, clippy, doctests and, for three, the xwin Clippy: 25 cold runs with 178 to 1085 progress events
/// each. The longest gap between two events in any run was 43.1 s (`cargo test --doc` of apps/pitwall, at 261 s of a 861 s run), then
/// 35.1 s (doctests of apps/debrief) and 22.2 s (apps/dyno); the median of the runs' longest gaps was 10.5 s. 5 minutes is 7 times the
/// largest, the same margin the old 15-minute figure had over the 7.1 s and 20.3 s of the first look. It is still a first value: each
/// workspace was measured once, so the report-only phase checks it against real pushes before the watchdog may enforce (ER-12, ER-41).
pub const GAP: Duration = Duration::from_secs(5 * 60);

/// The least change of the tree's memory between two samples that counts as progress: 1 MiB (a hung process's allocator noise stays below it; a
/// compiler, a node process loading modules or a linker moves it by far more).
pub const MEMORY_STEP: u64 = 1 << 20;

/// Whether the watchdog may end a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Logs "would have stopped" and leaves the step running.
    ReportOnly,
    /// Ends the step.
    Enforce,
}

/// The mode when `PADDOCK_GATE_WATCHDOG` is unset: the one line that flips the watchdog from reporting to ending steps. Change it to
/// [`Mode::Enforce`] only when `./dev gate-watchdog report` says CLEAN and the window is recorded in docs/benchmarks.md, "Push gate
/// watchdog window" (ER-41: the same tool step that records the first clean window flips it, never a later hand edit).
pub const DEFAULT_MODE: Mode = Mode::ReportOnly;

impl Mode {
    /// `PADDOCK_GATE_WATCHDOG`: `enforce`, or `report` / unset for the default ([`DEFAULT_MODE`], report-only for now). Any other value is an error
    /// that names the variable and the choices: a typo (`enforced`) must never leave a person thinking the watchdog kills.
    pub fn from_env(value: Option<&str>) -> Result<Mode, String> {
        match value.map(str::trim) {
            None | Some("") => Ok(DEFAULT_MODE),
            Some("report") => Ok(Mode::ReportOnly),
            Some("enforce") => Ok(Mode::Enforce),
            Some(other) => Err(format!(
                "PADDOCK_GATE_WATCHDOG is `{other}`: it must be `enforce` or `report` (or unset, which is the default of the build)"
            )),
        }
    }
}

/// The patience of the judge: only the hang guard decides (no CPU is ever read), counted in ticks of [`WATCH_POLL`].
fn patience(gap: Duration) -> Patience {
    Patience {
        spin: Duration::MAX,
        idle: Duration::MAX,
        hang_guard: gap,
        note_after: 0,
        tick: WATCH_POLL,
        // Never read (every reading is `NotRead`): the judge counts the ticks past twice this toward its hang guard, so the gap
        // is the limit plus two ticks.
        cpu_every: 1,
    }
}

/// How many finished units a cargo step shows before it is classed cold or warm from the share of them really compiled.
pub const CLASSIFY_AFTER_UNITS: u64 = 20;

/// Cold when at least half of the finished units were really compiled (`"fresh":false`): a warm build finds nearly all of them in the
/// build folder (review P2-3: whether `debug/` exists says nothing about what the step does).
pub fn is_cold(units: u64, compiled: u64) -> bool {
    units > 0 && compiled * 2 >= units
}

/// The progress watchdog: a [`StepWatch`] for every step.
pub struct StallWatchdog {
    mode: Mode,
    gap: Duration,
    history: Option<Arc<History>>,
}

impl StallWatchdog {
    /// A watchdog in `mode` with the fallback `gap`, learning from `history` when there is one.
    pub fn new(mode: Mode, gap: Duration, history: Option<Arc<History>>) -> StallWatchdog {
        StallWatchdog { mode, gap, history }
    }
}

impl Watchdog for StallWatchdog {
    fn name(&self) -> &str {
        match self.mode {
            Mode::ReportOnly => "progress watchdog, report-only",
            Mode::Enforce => "progress watchdog",
        }
    }

    fn watch(&self, step: &StepInfo<'_>) -> Option<Box<dyn StepWatch>> {
        let mut w = StallWatch {
            judge: StartJudge::new(patience(self.gap), step.id),
            id: step.id.to_string(),
            mode: self.mode,
            gap: self.gap,
            limit: self.gap,
            step: (step.kind.to_string(), step.ws.to_string()),
            class: None,
            units: (0, 0),
            history: self.history.clone(),
            started: None,
            last_progress: 0,
            last_change: None,
            cpu_then: None,
            max_gap: Duration::ZERO,
            events: 0,
            would_stop: None,
            signals: step.signals,
            mem_then: None,
            disk_then: None,
            moves: Moves::default(),
            would_stop_at: None,
        };
        // A step that is not cargo's JSON has no units to classify by: it is one class (warm, "not a build").
        if step.progress == Progress::Lines {
            w.apply_class(false);
        }
        Some(Box::new(w))
    }
}

struct StallWatch {
    judge: StartJudge,
    id: String,
    mode: Mode,
    /// The fallback gap and the limit in force (the history's, once the class is known and it has one).
    gap: Duration,
    limit: Duration,
    step: (String, String),
    /// Cold or warm, known after [`CLASSIFY_AFTER_UNITS`] finished units (or at the end).
    class: Option<bool>,
    units: (u64, u64),
    history: Option<Arc<History>>,
    started: Option<Instant>,
    last_progress: u64,
    last_change: Option<Instant>,
    cpu_then: Option<Duration>,
    max_gap: Duration,
    events: u64,
    would_stop: Option<String>,
    /// Whether the memory and disk of the tree are read as progress for this step ([`StepInfo::signals`]).
    signals: bool,
    mem_then: Option<u64>,
    disk_then: Option<u64>,
    moves: Moves,
    /// Seconds into the run at which the first "would have stopped" was said.
    would_stop_at: Option<f64>,
}

/// How many times each signal moved: the ticks the judge was given beyond the output events.
#[derive(Debug, Default, Clone, Copy)]
struct Moves {
    mem: u64,
    disk: u64,
    mem_now: u64,
    disk_now: u64,
}

impl Moves {
    /// The progress the signals added.
    fn ticks(&self) -> u64 {
        self.mem + self.disk
    }
}

impl StallWatch {
    /// Reads the sampled memory and disk of the tree: a change of more than [`MEMORY_STEP`], or any growth of the disk bytes, since the
    /// last look is one tick of progress. A first sample only becomes the baseline. Nothing is read for a step without signals.
    fn read_signals(&mut self, o: &Observation<'_>) {
        if !self.signals {
            return;
        }
        if let Some(m) = o.mem {
            if self.mem_then.is_some_and(|p| m.abs_diff(p) > MEMORY_STEP) {
                self.moves.mem += 1;
            }
            self.mem_then = Some(m);
            self.moves.mem_now = m;
        }
        if let Some(d) = o.disk {
            if self.disk_then.is_some_and(|p| d > p) {
                self.moves.disk += 1;
            }
            self.disk_then = Some(d);
            self.moves.disk_now = d;
        }
    }

    /// The readings in words, for the report and the phase line.
    fn signal_words(&self, events: u64) -> String {
        if !self.signals {
            return format!("signals: none read (this step's work is not visible in its process tree); {events} output events");
        }
        format!(
            "signals: tree memory {} MiB ({} moves over 1 MiB), disk {} MiB ({} growths), {events} output events",
            self.moves.mem_now >> 20,
            self.moves.mem,
            self.moves.disk_now >> 20,
            self.moves.disk
        )
    }

    /// The step is cold (`true`) or warm: its limit is the history's for that class, when the history has one.
    fn apply_class(&mut self, cold: bool) {
        self.class = Some(cold);
        let limit = self
            .history
            .as_ref()
            .and_then(|h| h.limit(&self.step.0, &self.step.1, cold, self.signals, self.gap))
            .unwrap_or(self.gap);
        if limit != self.limit {
            self.limit = limit;
            // A new judge: the class is decided right after a unit finished, so nothing it counted so far is lost.
            self.judge = StartJudge::new(patience(limit), &self.id);
        }
    }
}

/// How the CPU reading words the report: it never decides.
fn cpu_words(then: Option<Duration>, now: Option<Duration>) -> String {
    match (then, now) {
        (Some(a), Some(b)) if b > a => format!(
            "the process tree used {:.1} s of CPU meanwhile (busy, or hidden work: a cache server, a container)",
            (b - a).as_secs_f64()
        ),
        (Some(_), Some(_)) => "the process tree used no CPU meanwhile (idle)".to_string(),
        _ => "CPU is not visible for this step".to_string(),
    }
}

impl StepWatch for StallWatch {
    fn check(&mut self, o: &Observation<'_>) -> Option<Stuck> {
        let started = *self.started.get_or_insert(o.now);
        let last_change = *self.last_change.get_or_insert(o.now);
        if self.started == Some(o.now) {
            // the first look: the CPU the step had then is what the report compares with
            self.cpu_then = o.cpu;
        }
        self.read_signals(o);
        // One counter for the judge: the output's events plus the ticks the signals added. It only ever grows.
        let activity = o.progress + self.moves.ticks();
        if activity > self.last_progress {
            self.max_gap = self.max_gap.max(o.now.saturating_duration_since(last_change));
            self.last_progress = activity;
            self.last_change = Some(o.now);
            self.cpu_then = o.cpu;
        }
        self.events = o.progress;
        self.units = (o.units, o.compiled);
        if self.class.is_none() && o.units >= CLASSIFY_AFTER_UNITS {
            self.apply_class(is_cold(o.units, o.compiled));
        }
        let reading = Reading {
            // A wait for another cargo's lock is a state of the step, not a stall: the judge counts nothing while it lasts.
            liveness: if o.waiting_on.is_some() {
                Liveness::Paused
            } else {
                Liveness::Running
            },
            ready: false,
            progress: activity,
            cpu: Cpu::NotRead,
        };
        let StartState::Stalled(report) = self.judge.observe(&reading) else {
            return None;
        };
        let StallCause::Hung(_) = report.cause else { return None };
        let quiet = o.now.saturating_duration_since(self.last_change.unwrap_or(started));
        let stuck = Stuck {
            guard: format!("progress watchdog, limit {} s", self.limit.as_secs()),
            reason: format!(
                "no unit or line finished and the tree's memory and disk were flat for {} s while not waiting on a lock; last event: {}; {}; {}",
                quiet.as_secs(),
                if o.last_event.is_empty() { "(none)" } else { o.last_event },
                self.signal_words(o.progress),
                cpu_words(self.cpu_then, o.cpu)
            ),
            quiet,
        };
        match self.mode {
            Mode::Enforce => Some(stuck),
            Mode::ReportOnly => {
                if self.would_stop.is_none() {
                    eprintln!("pre-push: {}: WOULD HAVE STOPPED (report-only): {}", self.step.0, stuck.reason);
                    self.would_stop_at = Some(o.active_for.as_secs_f64());
                    self.would_stop = Some(format!(
                        "would have stopped at {} s of run: {}",
                        o.active_for.as_secs(),
                        stuck.reason
                    ));
                }
                None
            }
        }
    }

    fn finish(&mut self, ran: f64, ok: bool, ended_by: Option<&str>) -> Option<String> {
        if let (Some(h), Some(last)) = (&self.history, self.last_change) {
            // The gap from the last event to the end counts too (a step that spends its last minutes in one unit).
            let tail = self
                .started
                .map_or(Duration::ZERO, |_| Instant::now().saturating_duration_since(last));
            h.append(&StepRecord {
                kind: self.step.0.clone(),
                ws: self.step.1.clone(),
                cold: self.class.unwrap_or_else(|| is_cold(self.units.0, self.units.1)),
                ok,
                ran_secs: ran,
                // A step cannot have been quiet for longer than it ran (the load-gate wait of heavy was once counted as run time).
                max_gap_secs: self.max_gap.max(tail).as_secs_f64().min(ran),
                events: self.events,
                signals: self.signals,
                at: super::history::now_secs(),
                tool: super::history::TOOL.to_string(),
                rev: super::history::REV,
                ended_by: ended_by.map(str::to_string),
                would_stop_secs: self.would_stop_at,
            });
        }
        // The signal readings go with the note (ER-27), so the next report-only window can be judged from the files alone.
        self.would_stop
            .take()
            .map(|n| format!("{n}; at the end {}", self.signal_words(self.events)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pre_push::progress::Progress;
    use paddock_proc::ProcessId;

    fn step<'a>() -> StepInfo<'a> {
        StepInfo {
            id: "doctest crates",
            heavy: true,
            progress: Progress::CargoJson,
            kind: "doctest",
            ws: "crates",
            signals: false,
        }
    }

    /// A row that prints only at its end (conformance, a fixed-size check): output lines, tree signals read.
    fn quiet_row<'a>() -> StepInfo<'a> {
        StepInfo {
            id: "conformance",
            heavy: false,
            progress: Progress::Lines,
            kind: "conformance",
            ws: "",
            signals: true,
        }
    }

    fn rec_tail() -> StepRecord {
        StepRecord {
            kind: String::new(),
            ws: String::new(),
            cold: false,
            ok: true,
            ran_secs: 0.0,
            max_gap_secs: 0.0,
            events: 0,
            signals: false,
            at: super::super::history::now_secs(),
            tool: super::super::history::TOOL.to_string(),
            rev: super::super::history::REV,
            ended_by: None,
            would_stop_secs: None,
        }
    }

    /// One observation per second of a fake clock; `me` is a real process identity (this test process) for the `root` field.
    struct Run {
        w: Box<dyn StepWatch>,
        t0: Instant,
        secs: u64,
        me: ProcessId,
    }

    impl Run {
        fn new(dog: &StallWatchdog) -> Run {
            Run::of(dog, &step())
        }

        fn of(dog: &StallWatchdog, info: &StepInfo<'_>) -> Run {
            Run {
                w: dog.watch(info).unwrap(),
                t0: Instant::now(),
                secs: 0,
                me: ProcessId::of(std::process::id()).expect("this process"),
            }
        }

        /// One second passes; the step shows `progress` events and `waiting` for a lock.
        fn tick(&mut self, progress: u64, waiting: Option<&str>, cpu: Option<u64>) -> Option<Stuck> {
            self.tick_with(progress, waiting, cpu, None, None)
        }

        /// As [`Run::tick`], with the tree's memory and disk bytes as the executor's last sample.
        fn tick_with(
            &mut self,
            progress: u64,
            waiting: Option<&str>,
            cpu: Option<u64>,
            mem: Option<u64>,
            disk: Option<u64>,
        ) -> Option<Stuck> {
            self.secs += 1;
            self.w.check(&Observation {
                now: self.t0 + Duration::from_secs(self.secs),
                active_for: Duration::from_secs(self.secs),
                log_len: 0,
                last_line: "",
                progress,
                // a cold build: every unit really compiled
                units: progress.max(30),
                compiled: progress.max(30),
                last_event: "compiler-artifact x",
                waiting_on: waiting,
                cpu: cpu.map(Duration::from_secs),
                mem,
                disk,
                root: self.me,
            })
        }
    }

    fn dog(mode: Mode, gap: u64) -> StallWatchdog {
        StallWatchdog::new(mode, Duration::from_secs(gap), None)
    }

    /// The positive control: a step that shows no progress for the gap is stuck, and the report names the last event and the CPU words.
    #[test]
    fn a_step_without_progress_for_the_gap_is_stuck_and_the_report_says_so() {
        let mut r = Run::new(&dog(Mode::Enforce, 60));
        let mut hit = None;
        for _ in 0..200 {
            hit = r.tick(0, None, Some(5)).or(hit);
            if hit.is_some() {
                break;
            }
        }
        let s = hit.expect("stuck");
        assert!(s.guard.starts_with("progress watchdog"), "{}", s.guard);
        assert!(
            s.reason.contains("no unit or line finished") && s.reason.contains("idle"),
            "{}",
            s.reason
        );
        assert!((60..=64).contains(&s.quiet.as_secs()), "{:?}", s.quiet);
    }

    #[test]
    fn steady_progress_is_never_stuck_however_long_it_runs() {
        let mut r = Run::new(&dog(Mode::Enforce, 60));
        // a unit every 50 s for two hours of fake time: far past any total, never stuck
        for i in 0..7200u64 {
            assert!(r.tick(i / 50 + 1, None, None).is_none(), "second {i}");
        }
    }

    #[test]
    fn a_wait_for_a_file_lock_is_not_a_stall_and_the_count_resumes_after_it() {
        let mut r = Run::new(&dog(Mode::Enforce, 60));
        for _ in 0..30 {
            assert!(r.tick(1, None, None).is_none());
        }
        // a lock wait of an hour: nothing counts
        for i in 0..3600 {
            assert!(
                r.tick(1, Some("Blocking waiting for file lock on build directory"), Some(0))
                    .is_none(),
                "{i}"
            );
        }
        // the wait ends and the step goes quiet: the 30 s before the wait still count, so it is stuck after the rest of the gap
        let mut stuck_after = None;
        for i in 0..100 {
            if r.tick(1, None, None).is_some() {
                stuck_after = Some(i);
                break;
            }
        }
        assert!(stuck_after.is_some_and(|i| (25..=35).contains(&i)), "{stuck_after:?}");
    }

    #[test]
    fn the_report_only_mode_logs_and_never_ends_the_step() {
        let mut r = Run::new(&dog(Mode::ReportOnly, 60));
        for _ in 0..300 {
            assert!(r.tick(0, None, None).is_none());
        }
        let note = r.w.finish(300.0, true, None).expect("a note for the phase line");
        assert!(note.starts_with("would have stopped at"), "{note}");
    }

    #[test]
    fn cpu_only_words_the_report_it_never_decides() {
        // CPU growing and CPU flat give the same verdict at the same time; only the words differ
        let stuck = |cpu_step: u64| {
            let mut r = Run::new(&dog(Mode::Enforce, 60));
            r.tick(1, None, Some(0));
            (0..200)
                .find_map(|i| r.tick(1, None, Some(i * cpu_step)))
                .map(|s| (s.quiet, s.reason))
        };
        let (busy_q, busy) = stuck(2).expect("busy but silent is stuck too");
        let (idle_q, idle) = stuck(0).expect("idle is stuck");
        assert_eq!(busy_q, idle_q);
        assert!(busy.contains("used") && busy.contains("cache server"), "{busy}");
        assert!(idle.contains("(idle)"), "{idle}");
        assert_eq!(cpu_words(Some(Duration::ZERO), None), "CPU is not visible for this step");
    }

    #[test]
    fn the_history_tightens_the_limit_only_with_enough_passed_steps() {
        let d = tempfile::tempdir().unwrap();
        let h = Arc::new(History::new(d.path().join("h.jsonl")));
        let make = || StallWatchdog::new(Mode::Enforce, Duration::from_secs(900), Some(Arc::clone(&h)));
        let limit_of = |dog: &StallWatchdog| {
            let mut r = Run::new(dog);
            (0..2000).find_map(|_| r.tick(0, None, None)).map(|s| s.quiet.as_secs())
        };
        assert!(limit_of(&make()).is_some_and(|q| q >= 900), "no history: the fallback");
        for _ in 0..super::super::history::MIN_SAMPLES {
            h.append(&StepRecord {
                kind: "doctest".into(),
                ws: "crates".into(),
                cold: true,
                ok: true,
                ran_secs: 90.0,
                max_gap_secs: 10.0,
                events: 12,
                ..rec_tail()
            });
        }
        let tight = limit_of(&make()).expect("stuck");
        assert!((120..=124).contains(&tight), "the floor of 120 s: {tight}");
    }

    #[test]
    fn the_mode_is_report_only_unless_enforce_is_asked_for() {
        // unset is the one constant that the flip changes; an explicit value always wins over it
        assert_eq!(Mode::from_env(None), Ok(DEFAULT_MODE));
        assert_eq!(Mode::from_env(Some("")), Ok(DEFAULT_MODE));
        assert_eq!(Mode::from_env(Some("report")), Ok(Mode::ReportOnly));
        assert_eq!(Mode::from_env(Some(" enforce ")), Ok(Mode::Enforce));
        // a typo is an error, never a silent report-only
        let why = Mode::from_env(Some("enforced")).unwrap_err();
        assert!(
            why.contains("PADDOCK_GATE_WATCHDOG") && why.contains("`enforced`") && why.contains("`enforce`"),
            "{why}"
        );
    }

    /// Cold or warm comes from the share of units really compiled, not from a folder (review P2-3).
    #[test]
    fn a_step_is_cold_when_most_of_its_units_were_really_compiled() {
        assert!(is_cold(100, 100));
        assert!(is_cold(40, 20), "half or more");
        assert!(!is_cold(100, 3), "a warm build finds nearly every unit in the build folder");
        assert!(!is_cold(0, 0), "no units: not a build");
    }

    /// The class decides which history limits apply: a warm step with a history of quick warm steps gets the tight limit, the same
    /// step cold (the history of cold steps is slow) keeps a long one.
    #[test]
    fn the_history_that_applies_is_the_one_of_the_steps_class() {
        let d = tempfile::tempdir().unwrap();
        let h = Arc::new(History::new(d.path().join("h.jsonl")));
        let add = |cold: bool, gap: f64| {
            for _ in 0..super::super::history::MIN_SAMPLES {
                h.append(&StepRecord {
                    kind: "doctest".into(),
                    ws: "crates".into(),
                    cold,
                    ok: true,
                    ran_secs: 90.0,
                    max_gap_secs: gap,
                    events: 40,
                    ..rec_tail()
                });
            }
        };
        add(false, 2.0);
        add(true, 60.0);
        let limit = |compiled: u64| {
            let dog = StallWatchdog::new(Mode::Enforce, Duration::from_secs(900), Some(Arc::clone(&h)));
            let mut w = dog.watch(&step()).unwrap();
            let t0 = Instant::now();
            let me = ProcessId::of(std::process::id()).unwrap();
            (1..3000u64).find_map(|i| {
                w.check(&Observation {
                    now: t0 + Duration::from_secs(i),
                    active_for: Duration::from_secs(i),
                    log_len: 0,
                    last_line: "",
                    progress: 30,
                    units: 30,
                    compiled,
                    last_event: "",
                    waiting_on: None,
                    cpu: None,
                    mem: None,
                    disk: None,
                    root: me,
                })
                .map(|s| s.quiet.as_secs())
            })
        };
        assert!(
            limit(0).is_some_and(|q| (120..=124).contains(&q)),
            "warm: the floor, {:?}",
            limit(0)
        );
        assert!(
            limit(30).is_some_and(|q| (240..=244).contains(&q)),
            "cold: 4 x 60 s, {:?}",
            limit(30)
        );
    }

    const MIB: u64 = 1 << 20;

    /// The seconds until a quiet-row run is called stuck, `None` when it never is in `limit` seconds. `sample` gives the (memory, disk)
    /// the tree shows at each second.
    fn stuck_after(info: &StepInfo<'_>, gap: u64, limit: u64, sample: impl Fn(u64) -> (Option<u64>, Option<u64>)) -> Option<u64> {
        let mut r = Run::of(&dog(Mode::Enforce, gap), info);
        (0..limit).find_map(|i| {
            let (mem, disk) = sample(i);
            r.tick_with(0, None, None, mem, disk).map(|_| i)
        })
    }

    /// Cause A, first half: a row that prints one line per check finished is never stuck however long it runs, and the same row
    /// printing nothing is (the positive control: the lines are what keep it alive).
    #[test]
    fn a_row_that_prints_a_line_per_check_is_not_stuck_and_the_silent_row_is() {
        let mut r = Run::of(&dog(Mode::Enforce, 60), &quiet_row());
        // a check every 40 s for two hours of fake time, the tree's memory and disk flat all the while
        for i in 0..7200u64 {
            assert!(r.tick_with(i / 40 + 1, None, None, Some(50 * MIB), Some(0)).is_none(), "second {i}");
        }
        let silent = stuck_after(&quiet_row(), 60, 200, |_| (Some(50 * MIB), Some(0)));
        assert!(silent.is_some_and(|i| (60..=64).contains(&i)), "{silent:?}");
    }

    /// Memory growth of the tree delays the verdict for as long as it goes on; flat memory gives the verdict one gap later. A change
    /// under the step (512 KiB) is noise and is no progress.
    #[test]
    fn memory_growth_delays_a_verdict_and_small_changes_do_not() {
        // 2 MiB more every 10 s for 600 s, then flat
        let growing = |i: u64| (Some(100 * MIB + if i < 600 { (i / 10) * 2 * MIB } else { 120 * MIB }), Some(0));
        let late = stuck_after(&quiet_row(), 60, 2000, growing).expect("stuck once memory is flat");
        assert!((655..=670).contains(&late), "growth to 600 s, then one gap: {late}");
        // control: the same readings with nothing moving are stuck at the gap
        let flat = stuck_after(&quiet_row(), 60, 2000, |_| (Some(100 * MIB), Some(0))).expect("flat is stuck");
        assert!((60..=64).contains(&flat), "{flat}");
        // control: 512 KiB steps are below the threshold
        let noise = stuck_after(&quiet_row(), 60, 2000, |i| (Some(100 * MIB + (i / 10) * MIB / 2), Some(0))).expect("noise is stuck");
        assert!((60..=64).contains(&noise), "{noise}");
        // and a fall of memory counts like a rise (a process released what it held: it is working)
        let falling = |i: u64| (Some(900 * MIB - (i / 10).min(60) * 2 * MIB), Some(0));
        let after_fall = stuck_after(&quiet_row(), 60, 2000, falling).expect("stuck once it stops");
        assert!((655..=670).contains(&after_fall), "{after_fall}");
    }

    /// Disk bytes that grow are progress too, by any amount; a counter that stays is not.
    #[test]
    fn disk_growth_delays_a_verdict() {
        let writing = |i: u64| (Some(10 * MIB), Some(if i < 300 { i * 4096 } else { 300 * 4096 }));
        let late = stuck_after(&quiet_row(), 60, 2000, writing).expect("stuck once the disk is flat");
        assert!((355..=366).contains(&late), "{late}");
        // a readings drop (an exited child takes its bytes out of the sum) is no progress
        let dropping = |i: u64| (Some(10 * MIB), Some(1_000_000_000u64.saturating_sub(i * 4096)));
        let dropped = stuck_after(&quiet_row(), 60, 2000, dropping).expect("a falling sum is not progress");
        assert!((60..=64).contains(&dropped), "{dropped}");
    }

    /// A signal only delays: with every reading flat (or none at all) the verdict is the old one, and a step whose work the tree does not
    /// show (sccache, the container) keeps the old rule whatever its tree does.
    #[test]
    fn a_step_without_signals_ignores_the_tree() {
        let blind = StepInfo {
            signals: false,
            ..quiet_row()
        };
        let ignores = stuck_after(&blind, 60, 2000, |i| (Some(100 * MIB + i * 10 * MIB), Some(i * MIB))).expect("stuck");
        assert!(
            (60..=64).contains(&ignores),
            "readings of a step without signals are never used: {ignores}"
        );
        let none = stuck_after(&quiet_row(), 60, 2000, |_| (None, None)).expect("no readings: only the output counts");
        assert!((60..=64).contains(&none), "{none}");
    }

    /// An idle wait is not judged: a step in a lock wait with every signal flat is never stuck, and it counts again after the wait.
    #[test]
    fn an_idle_wait_with_flat_signals_is_not_judged() {
        let mut r = Run::of(&dog(Mode::Enforce, 60), &quiet_row());
        for i in 0..7200 {
            assert!(
                r.tick_with(
                    1,
                    Some("Blocking waiting for file lock on build directory"),
                    None,
                    Some(5 * MIB),
                    Some(0)
                )
                .is_none(),
                "second {i}"
            );
        }
        // control: the same flat readings outside the wait are stuck
        let stuck = (0..200).find_map(|i| r.tick_with(1, None, None, Some(5 * MIB), Some(0)).map(|_| i));
        assert!(stuck.is_some_and(|i| (58..=64).contains(&i)), "{stuck:?}");
    }

    /// A blocked read: output flat, memory flat, disk flat, CPU idle. It is stuck, and the report says which signals were flat.
    #[test]
    fn a_blocked_read_with_flat_signals_is_stuck_and_the_report_gives_the_readings() {
        let mut r = Run::of(&dog(Mode::Enforce, 60), &quiet_row());
        let hit = (0..200)
            .find_map(|_| r.tick_with(3, None, Some(1), Some(80 * MIB), Some(7 * MIB)))
            .expect("stuck");
        assert!(hit.reason.contains("memory and disk were flat"), "{}", hit.reason);
        assert!(
            hit.reason.contains("tree memory 80 MiB") && hit.reason.contains("disk 7 MiB"),
            "{}",
            hit.reason
        );
        assert!(
            hit.reason.contains("3 output events") && hit.reason.contains("(idle)"),
            "{}",
            hit.reason
        );
    }

    /// Observability (ER-27): the report-only note has the readings, and the history record has the time of the call and the end.
    #[test]
    fn the_would_have_stopped_note_and_the_record_carry_the_readings() {
        let d = tempfile::tempdir().unwrap();
        let h = Arc::new(History::new(d.path().join("h.jsonl")));
        let dog = StallWatchdog::new(Mode::ReportOnly, Duration::from_secs(60), Some(Arc::clone(&h)));
        let mut r = Run::of(&dog, &quiet_row());
        for i in 0..300 {
            assert!(r.tick_with(1, None, None, Some(64 * MIB + (i / 100) * 4 * MIB), Some(0)).is_none());
        }
        let note = r.w.finish(300.0, true, Some("row cap")).expect("a note");
        assert!(
            note.starts_with("would have stopped at") && note.contains("signals: tree memory"),
            "{note}"
        );
        let recs = h.records();
        assert_eq!(recs.len(), 1, "{recs:?}");
        let rec = &recs[0];
        assert!(rec.signals && rec.ended_by.as_deref() == Some("row cap"), "{rec:?}");
        assert!(rec.would_stop_secs.is_some_and(|s| s > 60.0), "{rec:?}");
        assert!(rec.at > 0 && rec.tool.starts_with("paddock-dev "), "{rec:?}");
        assert!(rec.max_gap_secs <= rec.ran_secs, "a gap is never longer than the run: {rec:?}");
    }
}
