//! The test kit's wait for a program a test started (ADR 0071 section 1, `paddock_testkit_start`): until it is ready or has ended, never a fixed count of polls
//! or a deadline that decides the verdict.
//!
//! - [`wait_ready_or_exit`]: returns [`StartOutcome::Ready`] as soon as the test's probe gives a value, or
//!   [`StartOutcome::Exited`] as soon as the program has ended (with its error output when the kit started it,
//!   [`Logged`]). A start that is slow because the host is loaded, the OS checks a fresh binary or a browser is cold is
//!   waited for; only a start that neither answers nor ends fails, after [`HANG_GUARD`] of wall time, and the panic says it
//!   is a hang (the pid, whether the process is alive, stopped or gone, how often it was looked at), never "slow".
//! - [`try_wait_ready_or_exit`]: the same, returning [`HangGuardExpired`] instead of panicking, for a test double that must
//!   hand an error to the code under test.
//! - [`wait_exit`]: waits for a program to end on its own.
//! - [`warm_first_start`] / [`warm_first_starts`]: pay the OS's first-run check of a fresh binary once, before a step whose
//!   bound the test cannot remove ([`paddock_proc::warm_up`], memoised per file identity). Never what makes a test correct.
//! - [`PATIENCE`] and [`HANG_GUARD`]: the start judge's patience for a test, under which only the wall guard ends a wait. A
//!   product driver that takes a [`Patience`] and a guard (Debrief's `chromium_driver::Answers::Judged`) gets these two from
//!   here.
//!
//! The judgement is the product's: [`paddock_proc::StartWatch`] over a probe of the test's (ADR 0031), so there is one
//! "ready or exit" in the family (rule 55). Its tick counting cannot stretch the guard: the pace here ends the wait on the
//! wall clock, so a paused host still ends a real hang before nextest's kill.
//!
//! This crate owns real time for tests, as `paddock_testkit::wait` does (conformance C71 skips both). It is its own crate
//! because `paddock-proc`'s tests use `paddock-testkit`: the kit depending on `paddock-proc` made a publish cycle (ci.yml 7989).

use paddock_proc::{
    ExitInfo, Liveness, Pace, Patience, Presence, ProcessId, RunState, StallCause, StartProbe,
    StartState, StartWatch, Watched,
};
pub use paddock_testkit::HANG_GUARD;
use paddock_testkit::wait::{FIRST, LAST};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How often a wait that goes on says so on the test's stderr (ER-27): a test killed by the runner then shows why.
const NOTE_EVERY: Duration = Duration::from_secs(10);
/// How much of a program's error output an [`ExitInfo`] keeps.
const TAIL: usize = 2048;

/// The start judge's patience in a test: the judge never calls a start stuck (no spin, idle or blind limit is ever reached),
/// so only the wall guard ([`HANG_GUARD`]) ends a wait that never ends. The tick is the longest poll interval of the kit.
pub const PATIENCE: Patience = Patience {
    spin: Duration::MAX,
    idle: Duration::MAX,
    hang_guard: Duration::MAX,
    note_after: 0,
    tick: LAST,
    cpu_every: 1,
};

/// How a wait for a started program ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartOutcome<T> {
    /// The probe gave this value.
    Ready(T),
    /// The program ended first.
    Exited(ExitInfo),
}

impl<T> StartOutcome<T> {
    /// The value, or a panic that says the program ended first, with its exit status and error output.
    pub fn expect_ready(self, what: &str) -> T {
        match self {
            StartOutcome::Ready(v) => v,
            StartOutcome::Exited(info) => {
                panic!("{what} ended before it was ready: {}", describe_exit(&info))
            }
        }
    }
}

/// A wait that neither saw the program ready nor saw it end within the hang guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HangGuardExpired {
    /// What was waited for.
    pub what: String,
    /// The program's pid, when known.
    pub pid: Option<u32>,
    /// What the OS said of the program at the end: running, stopped, gone or unknown.
    pub state: &'static str,
    /// How long the wait lasted on the wall clock.
    pub waited: Duration,
    /// How often the probe was asked.
    pub looks: u64,
}

impl std::fmt::Display for HangGuardExpired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pid = self
            .pid
            .map_or_else(|| "pid unknown".to_string(), |p| format!("pid {p}"));
        write!(
            f,
            "{} ({pid}) never became ready or ended: a hang, not a slow start (the {:.0} s hang guard ran out after {:.1} s and {} \
             looks; the process is {})",
            self.what,
            HANG_GUARD.as_secs_f64(),
            self.waited.as_secs_f64(),
            self.looks,
            self.state
        )
    }
}

impl std::error::Error for HangGuardExpired {}

/// A program a test waits for: whether it still runs and, once it ended, how.
pub trait Started {
    /// The program, by identity, when known: a stopped program is told from a slow one by its run state.
    fn pid(&self) -> Option<ProcessId>;
    /// Running (or stopped) while it is there; [`Liveness::Exited`] once it ended.
    fn liveness(&mut self) -> Liveness;
}

impl Started for Child {
    fn pid(&self) -> Option<ProcessId> {
        ProcessId::of(self.id())
    }

    fn liveness(&mut self) -> Liveness {
        match self.try_wait() {
            Ok(Some(status)) => Liveness::Exited(ExitInfo {
                status: status.code(),
                stderr_tail: None,
            }),
            // An error reading the status says nothing either way: the guard bounds the wait.
            Ok(None) | Err(_) => Liveness::Running,
        }
    }
}

/// A process the test did not start itself (a browser a product launched), by identity. A process table that cannot be
/// read counts as running: the guard bounds the wait, and "gone" is never guessed (rule 53).
impl Started for ProcessId {
    fn pid(&self) -> Option<ProcessId> {
        Some(*self)
    }

    fn liveness(&mut self) -> Liveness {
        match self.presence() {
            Presence::Gone => Liveness::Exited(ExitInfo {
                status: None,
                stderr_tail: None,
            }),
            Presence::Running | Presence::Unknown => Liveness::Running,
        }
    }
}

/// A child whose error output goes to a file, so an end is reported with the end of what it printed.
#[derive(Debug)]
pub struct Logged {
    child: Child,
    stderr: PathBuf,
}

impl Logged {
    /// Starts `cmd` with its error output in the file `stderr` (created or emptied); the rest of its set-up is the caller's.
    /// Dropping it ends the child.
    ///
    /// # Errors
    /// The file could not be created or the program could not be started.
    pub fn spawn(cmd: &mut Command, stderr: &Path) -> std::io::Result<Logged> {
        let file = std::fs::File::create(stderr)?;
        let child = cmd.stderr(Stdio::from(file)).spawn()?;
        Ok(Logged {
            child,
            stderr: stderr.to_path_buf(),
        })
    }

    /// [`Logged::spawn`] after the program's first start was paid ([`warm_first_start`] with `warm_args`, `--version` by
    /// convention): the OS's check of a fresh binary is then not charged to the start the test waits on. The program must end
    /// on its own with `warm_args` (a program that serves forever would block here), so this is a choice, never the default.
    ///
    /// # Errors
    /// As [`Logged::spawn`]; a program that fails its warm-up panics, as [`warm_first_start`] does.
    pub fn spawn_warmed(
        cmd: &mut Command,
        stderr: &Path,
        warm_args: &[&str],
    ) -> std::io::Result<Logged> {
        warm_first_start(Path::new(cmd.get_program()), warm_args);
        Logged::spawn(cmd, stderr)
    }

    /// The child, for what the test does with it (its pid, its standard output, a kill).
    pub fn child(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl Started for Logged {
    fn pid(&self) -> Option<ProcessId> {
        self.child.pid()
    }

    fn liveness(&mut self) -> Liveness {
        match self.child.liveness() {
            Liveness::Exited(info) => Liveness::Exited(ExitInfo {
                stderr_tail: paddock_proc::tail_of(&self.stderr, TAIL),
                ..info
            }),
            other => other,
        }
    }
}

impl Drop for Logged {
    fn drop(&mut self) {
        // A child the test did not wait for ends with the test; one that ended already is only reaped.
        drop(self.child.kill());
        drop(self.child.wait());
    }
}

impl<S: Started + ?Sized> Started for &mut S {
    fn pid(&self) -> Option<ProcessId> {
        (**self).pid()
    }

    fn liveness(&mut self) -> Liveness {
        (**self).liveness()
    }
}

/// Waits until `probe` gives a value (the program is ready) or `child` has ended, and returns which came first. A start
/// that takes long is waited for; a wait that sees neither for [`HANG_GUARD`] of wall time panics, naming `what` and calling
/// it a hang.
pub fn wait_ready_or_exit<S: Started, T>(
    child: S,
    what: &str,
    probe: impl FnMut() -> Option<T>,
) -> StartOutcome<T> {
    try_wait_ready_or_exit(child, what, probe).unwrap_or_else(|hang| panic!("{hang}"))
}

/// [`wait_ready_or_exit`] that returns the hang instead of panicking.
///
/// # Errors
/// [`HangGuardExpired`] when the program neither became ready nor ended within [`HANG_GUARD`] of wall time.
pub fn try_wait_ready_or_exit<S: Started, T>(
    child: S,
    what: &str,
    probe: impl FnMut() -> Option<T>,
) -> Result<StartOutcome<T>, HangGuardExpired> {
    let clock = RealClock(Instant::now());
    wait_on(child, what, HANG_GUARD, &clock, probe)
}

/// Waits for `child` to end on its own and returns how it ended; a program that does not end within [`HANG_GUARD`] of wall
/// time panics, naming `what`.
pub fn wait_exit<S: Started>(child: S, what: &str) -> ExitInfo {
    match wait_ready_or_exit(child, what, || None::<()>) {
        StartOutcome::Exited(info) => info,
        StartOutcome::Ready(()) => {
            unreachable!("a probe that never gives a value cannot make the wait ready")
        }
    }
}

/// Runs `exe args` (`--version` by convention) once per file identity in this process, before the test's first real start
/// of it, so the OS's check of a fresh binary is not charged to a step with a bound the test cannot remove. Prints how long
/// that first start took. Panics when the program cannot run or fails: a program that cannot print its version is broken.
pub fn warm_first_start(exe: &Path, args: &[&str]) -> Duration {
    paddock_proc::warm_up(exe, args).unwrap_or_else(|e| panic!("{e}"))
}

/// [`warm_first_start`] for several programs at once, a thread each: the OS checks different binaries side by side, so a test
/// that starts several fresh programs pays about one check instead of one after another.
pub fn warm_first_starts(programs: &[(&Path, &[&str])]) -> Vec<Duration> {
    std::thread::scope(|s| {
        let handles: Vec<_> = programs
            .iter()
            .map(|(exe, args)| s.spawn(move || warm_first_start(exe, args)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|p| std::panic::resume_unwind(p)))
            .collect()
    })
}

/// "status 3; its error output: ..." for an end.
fn describe_exit(info: &ExitInfo) -> String {
    let status = info.status.map_or_else(
        || "no exit code (a signal, or not known)".to_string(),
        |c| format!("exit code {c}"),
    );
    match &info.stderr_tail {
        Some(t) => format!("{status}; its error output ends with: {t}"),
        None => format!("{status}; no error output kept"),
    }
}

/// The wall clock of a wait, a seam so the guard is tested without waiting 40 s.
pub(crate) trait Clock {
    /// Time since the wait began.
    fn elapsed(&self) -> Duration;
    /// Waits `d`.
    fn pause(&self, d: Duration);
}

struct RealClock(Instant);

impl Clock for RealClock {
    fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }

    fn pause(&self, d: Duration) {
        // pace: the poll interval of the wait (200 µs doubling to 20 ms); the guard below bounds it, not this
        std::thread::sleep(d);
    }
}

/// What the judge looks at: the child and the test's probe, which is asked until it gives a value.
struct Look<S, P, T> {
    child: S,
    probe: P,
    value: Option<T>,
    looks: u64,
}

impl<S: Started, P: FnMut() -> Option<T>, T> StartProbe for Look<S, P, T> {
    fn liveness(&mut self) -> Liveness {
        self.child.liveness()
    }

    fn pid(&self) -> Option<ProcessId> {
        self.child.pid()
    }

    fn ready(&mut self) -> bool {
        if self.value.is_none() {
            self.looks += 1;
            self.value = (self.probe)();
        }
        self.value.is_some()
    }

    fn cpu_time(&mut self) -> Option<Duration> {
        // Not read: under [`PATIENCE`] the judge decides only ready or exit, and the wall guard ends a hang.
        None
    }
}

/// The kit's pace: the backoff of `paddock_testkit::wait`, a note every [`NOTE_EVERY`], and the end of the wait once the guard has
/// passed on the wall clock (the judge counts ticks, which a paused host would stretch).
struct Deadline<'a> {
    clock: &'a dyn Clock,
    guard: Duration,
    pause: Duration,
    next_note: Duration,
    what: &'a str,
    pid: Option<u32>,
}

impl Pace for Deadline<'_> {
    fn wait(&mut self, _tick: Duration) -> bool {
        let now = self.clock.elapsed();
        if now >= self.guard {
            return false;
        }
        if now >= self.next_note {
            let pid = self
                .pid
                .map_or_else(|| "pid unknown".to_string(), |p| format!("pid {p}"));
            eprintln!(
                "{} ({pid}) is still starting after {:.0} s",
                self.what,
                now.as_secs_f64()
            );
            self.next_note = now + NOTE_EVERY;
        }
        self.clock.pause(self.pause.min(self.guard - now));
        self.pause = (self.pause * 2).min(LAST);
        true
    }
}

/// The wait itself, on `clock`, ended as a hang after `guard`.
pub(crate) fn wait_on<S: Started, T>(
    child: S,
    what: &str,
    guard: Duration,
    clock: &dyn Clock,
    probe: impl FnMut() -> Option<T>,
) -> Result<StartOutcome<T>, HangGuardExpired> {
    let id = child.pid();
    let pace = Deadline {
        clock,
        guard,
        pause: FIRST,
        next_note: NOTE_EVERY,
        what,
        pid: id.map(ProcessId::pid),
    };
    let look = Look {
        child,
        probe,
        value: None,
        looks: 0,
    };
    let mut watch = StartWatch::new(PATIENCE, what, look, pace);
    let ended = loop {
        match watch.wait() {
            // A stopped program is not stuck (the OS or a signal holds it): the wait goes on and the wall guard runs.
            Watched::State(StartState::Stalled(r)) if matches!(r.cause, StallCause::Paused) => {}
            // The judge never answers `Starting`; no other stall can come under [`PATIENCE`], and one that did is a hang.
            Watched::State(StartState::Starting) => {}
            other => break other,
        }
    };
    let mut look = watch.into_probe();
    match ended {
        Watched::State(StartState::Ready) => match look.value.take() {
            Some(v) => Ok(StartOutcome::Ready(v)),
            None => unreachable!("the judge says ready only when the probe gave a value"),
        },
        Watched::State(StartState::Exited(info)) => Ok(StartOutcome::Exited(info)),
        Watched::State(StartState::Stalled(_) | StartState::Starting) | Watched::Cancelled => {
            Err(HangGuardExpired {
                what: what.to_string(),
                pid: id.map(ProcessId::pid),
                state: state_of(&mut look.child, id),
                waited: clock.elapsed(),
                looks: look.looks,
            })
        }
    }
}

fn state_of<S: Started>(child: &mut S, id: Option<ProcessId>) -> &'static str {
    if matches!(child.liveness(), Liveness::Exited(_)) {
        return "gone";
    }
    match id.map(paddock_proc::run_state) {
        Some(RunState::Stopped) => "stopped (paused by a signal or the OS)",
        Some(RunState::Running) => "alive",
        Some(RunState::Exited) => "gone",
        Some(RunState::Unknown) | None => {
            "alive as far as the OS says (its run state cannot be read here)"
        }
    }
}

#[cfg(test)]
mod tests;
