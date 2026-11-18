//! A bounded phase of a host step (the port of `conformance/ci/bounded.sh`'s caller side): runs a closure for at most a limit,
//! logs `<step>: <label> ...` before and `<step>: <label>: done|failed (exit 1) in N s` after, and tells a caller that a phase
//! did not finish. The caller decides what a timeout means: a hard step ends in one `::error::` line, a soft one (a step that must
//! never fail its job, `BOUNDED_SOFT=1` in the shell) warns and falls back.
//!
//! A phase that overruns is left behind: its thread is not killable. A program (git) it holds is told to the phase's [`Held`] and is
//! ended with the phase when it is abandoned; the program's own limit through [`ToolCall`](crate::ci_tool::ToolCall) is the phase's
//! limit, except under a clock mark (tests only), where it is the general one and the phase's bound, counted from the mark, ends it
//! (ADR 0103, second amendment). A file-system call that hangs is abandoned and the process ends with the step.

use super::Step;
use paddock_proc::ProcessId;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

/// The programs a phase runs, which its caller may have to give up on: a captured [`ToolCall`](crate::ci_tool::ToolCall)
/// (`held_by`) tells the set who it started, and [`Held::end_all`] ends them all by identity and closes the set, so a program that
/// starts after that is ended at its start. A phase's thread cannot be killed, so a phase that hit its bound is abandoned; the
/// bounded functions below end the set of an abandoned phase before they return, because the thread's own timeout may be late
/// (ending a tree takes time on a loaded machine) and a step that ends with a program still running leaves it behind.
#[derive(Debug, Default)]
pub struct Held {
    state: Mutex<HeldState>,
}

#[derive(Debug, Default)]
struct HeldState {
    closed: bool,
    running: Vec<ProcessId>,
    /// Programs whose start time could not be read: they have no identity, so nothing can end them safely (rule 53).
    unidentified: usize,
}

/// What [`Held::end_all`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct EndedAll {
    /// How many programs (with the ones below them) it ended.
    pub ended: usize,
    /// How long that took.
    pub took: Duration,
    /// Still running when the wait ran out: the OS refused to end them.
    pub left: Vec<ProcessId>,
    /// Programs it could not identify and so did not end.
    pub unidentified: usize,
}

/// How long [`Held::end_all`] waits for the OS to finish ending what it signalled. SIGKILL is delivered at once and the process goes a
/// moment later; the wait is on the process table with this deadline. Measured 2026-11-18 (lessons.md, "An abandoned kept-clone phase
/// let its git outlive the step"): the whole wait took at most a few milliseconds in every run, at load 17 to 28; 10 s is a bound for a
/// machine in trouble (a process stuck in the kernel), not an expected time, and a process that outlasts it is reported, not waited for.
const ENDED_WITHIN: Duration = Duration::from_secs(10);

impl Held {
    /// A program started: `id` is its identity, `None` when its start time could not be read.
    pub(crate) fn note(&self, id: Option<ProcessId>) {
        let ended_at_once = {
            let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            match id {
                Some(id) if !s.closed => s.running.push(id),
                Some(_) => {}
                None => s.unidentified += 1,
            }
            s.closed
        };
        if let (true, Some(id)) = (ended_at_once, id) {
            let _ = paddock_proc::kill_tree(id);
        }
    }

    /// Ends every program noted so far and every one noted later; returns once they are gone (at most [`ENDED_WITHIN`]).
    pub fn end_all(&self) -> EndedAll {
        let began = Instant::now();
        let (running, unidentified) = {
            let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            s.closed = true;
            (std::mem::take(&mut s.running), s.unidentified)
        };
        // Only a program still running is looked at: reading the process table to end a tree costs about a second on a loaded machine
        // (measured, lessons.md), and most programs of a phase have ended by the time it is abandoned. `kill_tree` reads the tree and
        // answers every process it ended, so the table is read once per running program.
        let mut ended_or_ending = Vec::new();
        for id in running.iter().filter(|id| !id.has_ended()) {
            ended_or_ending.extend(paddock_proc::kill_tree(*id).into_iter().map(|(id, _)| id));
        }
        let deadline = Instant::now() + ENDED_WITHIN;
        let mut pause = Duration::from_millis(1);
        let left = loop {
            let left: Vec<_> = ended_or_ending.iter().copied().filter(|id| !id.has_ended()).collect();
            if left.is_empty() || Instant::now() >= deadline {
                break left;
            }
            std::thread::sleep(pause);
            pause = (pause * 2).min(Duration::from_millis(20));
        };
        EndedAll {
            ended: ended_or_ending.len() - left.len(),
            took: began.elapsed(),
            left,
            unidentified,
        }
    }
}

/// Ends what an abandoned phase's programs left running, and says so in the step's log (ER-27: how many, and how long it took).
fn end_abandoned(step: &mut Step<'_>, label: &str, held: &Held) {
    let done = held.end_all();
    if done.ended > 0 {
        step.say(format!(
            "{}: {label}: ended {} program(s) of the abandoned phase in {} ms",
            step.name,
            done.ended,
            done.took.as_millis()
        ));
    }
    for id in &done.left {
        step.warn(format!(
            "{label}: {id} was still running {ENDED_WITHIN:?} after the abandoned phase's programs were ended"
        ));
    }
    if done.unidentified > 0 {
        step.warn(format!(
            "{label}: {} program(s) of the abandoned phase have no readable start time and were not ended",
            done.unidentified
        ));
    }
}

/// Why a bounded phase ended without a value.
pub enum Phase {
    /// It did not finish in time.
    Timeout,
    /// It failed with this message.
    Failed(String),
}

/// Runs `f` for at most `limit`, logging `label ...` before and `label: done|failed in N s` after (the shell's `bounded`).
pub fn bounded<T: Send + 'static>(
    step: &mut Step<'_>,
    label: &str,
    limit: Duration,
    f: impl FnOnce(Arc<Held>) -> Result<T, String> + Send + 'static,
) -> Result<T, Phase> {
    bounded_hung(step, label, limit, None, f)
}

/// [`bounded`] for a phase that can end itself at its limit (a program it runs is bounded by the same limit): when `hung` is set at
/// the end of the phase, failed or not, the phase did not fail or finish, it hung, and the caller reports that, so no `failed` or
/// `done` line is logged.
pub fn bounded_hung<T: Send + 'static>(
    step: &mut Step<'_>,
    label: &str,
    limit: Duration,
    hung: Option<&AtomicBool>,
    f: impl FnOnce(Arc<Held>) -> Result<T, String> + Send + 'static,
) -> Result<T, Phase> {
    let start = Instant::now();
    step.say(format!("{}: {label} ...", step.name));
    let (tx, rx) = mpsc::channel();
    let held = Arc::new(Held::default());
    let mine = held.clone();
    std::thread::spawn(move || {
        let _ = tx.send(f(mine));
    });
    let secs = |s: Instant| s.elapsed().as_secs();
    let r = match rx.recv_timeout(limit) {
        // A program of the phase ended at its limit and the phase went on (the port of a shell function that ignores one step's
        // status): the phase still ran for its limit, and the shell's `bounded` ends the whole phase there, so it hung.
        Ok(Ok(_)) if hung.is_some_and(|h| h.load(Ordering::SeqCst)) => Err(Phase::Timeout),
        Ok(Ok(v)) => {
            step.say(format!("{}: {label}: done in {} s", step.name, secs(start)));
            Ok(v)
        }
        Ok(Err(e)) => {
            if hung.is_some_and(|h| h.load(Ordering::SeqCst)) {
                return Err(Phase::Timeout);
            }
            step.say(format!("{}: {label}: failed (exit 1) in {} s", step.name, secs(start)));
            Err(Phase::Failed(e))
        }
        Err(_) => Err(Phase::Timeout),
    };
    if matches!(r, Err(Phase::Timeout)) {
        end_abandoned(step, label, &held);
    }
    r
}

/// A clock mark (ADR 0103, "Hang-point signal"; for tests): the phase's limit counts from the moment `file` appears, and until then the
/// phase is bounded by `wait`. The port of `bounded.sh`'s `clock_mark` and `clock_wait`.
pub struct ClockMark<'a> {
    /// The file the test's stand-in writes when it enters its hang (removed when the phase starts).
    pub file: &'a std::path::Path,
    /// The bound before the mark appears.
    pub wait: Duration,
}

/// The general limit of a hung-phase test (the one that bounds every other phase, and the hanging one until its mark): 20 s because the
/// slowest whole hung-phase test took 17.4 s (300 runs of the six, 6 burners, 1-minute load 18 to 68, 2026-11-15;
/// `fix/kept-clone-phase-state`'s 50-run stress, ADR 0103), and no phase of a test outlasts the test; a git that misses its mark then
/// ends at 20 s, not at nextest's 60 s. The one value of every hung-phase test (ER-22): the unit tests, `stable_workspace`'s and the
/// parity tests' `kept_hang_env`.
pub const HUNG_TEST_GENERAL_LIMIT: Duration = Duration::from_secs(20);

/// How often a phase with a clock mark looks for it: the shell's first naps (0.05 s).
const MARK_LOOK: Duration = Duration::from_millis(50);

/// [`bounded_hung`] with an optional [`ClockMark`]; also returns whether the phase ended before its mark (so the mark's `wait`, not
/// `limit`, was the bound: the caller's "did not finish within" line names it). The `done in` and `failed in` times count from the
/// mark once it was seen, as the shell's do (its clock starts again at the mark).
pub fn bounded_marked<T: Send + 'static>(
    step: &mut Step<'_>,
    label: &str,
    limit: Duration,
    hung: Option<&AtomicBool>,
    mark: Option<ClockMark<'_>>,
    f: impl FnOnce(Arc<Held>) -> Result<T, String> + Send + 'static,
) -> (Result<T, Phase>, bool) {
    let Some(mark) = mark else {
        return (bounded_hung(step, label, limit, hung, f), false);
    };
    let _ = std::fs::remove_file(mark.file);
    let start = Instant::now();
    step.say(format!("{}: {label} ...", step.name));
    let (tx, rx) = mpsc::channel();
    let held = Arc::new(Held::default());
    let mine = held.clone();
    std::thread::spawn(move || {
        let _ = tx.send(f(mine));
    });
    // Before the mark: look for it between waits for the result, up to `wait`; from the mark on, `limit`.
    let (mut from, mut bound, mut seen) = (start, mark.wait, false);
    let got = loop {
        if !seen && mark.file.exists() {
            (from, bound, seen) = (Instant::now(), limit, true);
        }
        let left = bound.saturating_sub(from.elapsed());
        if left.is_zero() {
            break None;
        }
        let slice = if seen { left } else { left.min(MARK_LOOK) };
        match rx.recv_timeout(slice) {
            Ok(r) => break Some(r),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break Some(Err("the phase's thread ended without a result".into())),
        }
    };
    let is_hung = || hung.is_some_and(|h| h.load(Ordering::SeqCst));
    let secs = from.elapsed().as_secs();
    let r = match got {
        Some(Ok(_)) if is_hung() => Err(Phase::Timeout),
        Some(Ok(v)) => {
            step.say(format!("{}: {label}: done in {secs} s", step.name));
            Ok(v)
        }
        Some(Err(_)) if is_hung() => Err(Phase::Timeout),
        Some(Err(e)) => {
            step.say(format!("{}: {label}: failed (exit 1) in {secs} s", step.name));
            Err(Phase::Failed(e))
        }
        None => Err(Phase::Timeout),
    };
    if matches!(r, Err(Phase::Timeout)) {
        end_abandoned(step, label, &held);
    }
    (r, !seen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Runs `f` as a phase with a 1 s limit and a clock mark at `file`; returns the result and the step's lines.
    fn marked(
        file: &std::path::Path,
        wait: Duration,
        f: impl FnOnce(Arc<Held>) -> Result<(), String> + Send + 'static,
    ) -> (Result<(), Phase>, bool, String) {
        let env = HashMap::new();
        let mut out = Vec::new();
        let mut step = Step {
            env: &env,
            out: &mut out,
            windows: false,
            name: "t",
        };
        let mark = ClockMark { file, wait };
        let (r, before_mark) = bounded_marked(&mut step, "p", Duration::from_secs(1), None, Some(mark), f);
        (r, before_mark, String::from_utf8_lossy(&out).into_owned())
    }

    #[cfg(unix)]
    #[test]
    fn a_marked_phase_counts_its_limit_from_the_mark() {
        // Work before the hang (1.5 s, past the 1 s limit), then the mark, then the hang: the phase is ended only after the mark, so
        // it is the hang that meets the limit. A stale mark from before the phase is removed when it starts.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mark");
        std::fs::write(&file, "stale").unwrap();
        let f2 = file.clone();
        let (r, before_mark, _) = marked(&file, Duration::from_secs(60), move |_| {
            // The phase's slow work before its hang: a program that runs longer than the limit (as a slow `git config` would), waited
            // for as a phase waits for its programs.
            let _ = paddock_proc::command("/bin/sleep").arg("1.5").status();
            std::fs::write(&f2, "hang").unwrap();
            loop {
                std::thread::park(); // the hang: nothing unparks it
            }
        });
        assert!(matches!(r, Err(Phase::Timeout)), "the hang meets the limit");
        assert!(!before_mark, "the limit applied, counted from the mark");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap_or_default(),
            "hang",
            "the phase was ended after its mark, not in its work before the hang"
        );
    }

    /// A sleeper stand-in (30 s) started the way a phase's program is: the caller waits for it in a thread, and `held` is told who it is.
    fn sleeper(dir: &std::path::Path, held: &Held) -> std::thread::JoinHandle<std::process::ExitStatus> {
        let program = paddock_testkit::StandIn::new(dir, "sleeper")
            .sleeps(Duration::from_secs(30))
            .install()
            .unwrap();
        let mut cmd = paddock_proc::command(program.program());
        let mut child = cmd.spawn().unwrap();
        held.note(ProcessId::of(child.id()));
        std::thread::spawn(move || child.wait().unwrap())
    }

    #[test]
    fn a_held_set_ends_the_programs_it_was_told_of_and_ends_a_later_one_at_its_start() {
        let dir = tempfile::tempdir().unwrap();
        let held = Held::default();
        let first = sleeper(dir.path(), &held);
        let done = held.end_all();
        assert_eq!(
            (done.ended >= 1, done.left.is_empty(), done.unidentified),
            (true, true, 0),
            "{done:?}"
        );
        assert!(!first.join().unwrap().success(), "the first program was ended");
        // The set is closed: a program that starts after it is ended at its start (it would sleep 30 s otherwise).
        let late = sleeper(dir.path(), &held);
        assert!(!late.join().unwrap().success(), "the later program was ended at its start");
    }

    #[test]
    fn a_program_without_a_readable_start_time_is_counted_and_never_ended_by_a_guess() {
        let held = Held::default();
        held.note(None);
        let done = held.end_all();
        assert_eq!((done.ended, done.unidentified), (0, 1));
    }

    #[test]
    fn a_marked_phase_that_finishes_before_its_mark_is_done() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mark");
        let (r, before_mark, out) = marked(&file, Duration::from_secs(60), |_| Ok(()));
        assert!(r.is_ok());
        assert!(before_mark, "it ended before any mark");
        assert!(out.contains("t: p: done in "), "{out}");
    }

    #[test]
    fn a_marked_phase_whose_mark_never_appears_ends_at_the_wait() {
        // A stand-in that hangs without writing its mark (a test whose git misses the mark): the phase ends at the mark's wait, and
        // the caller is told it ended before its mark, so its line names that bound instead of the phase's limit.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mark");
        let (r, before_mark, _) = marked(&file, Duration::from_secs(2), |_| {
            loop {
                std::thread::park(); // the hang, with no mark: nothing unparks it
            }
        });
        assert!(matches!(r, Err(Phase::Timeout)), "the phase ends at its wait");
        assert!(before_mark, "it ended before its mark");
        assert!(!file.exists(), "no mark was written");
    }

    /// The functions of a test module's text that bound work with a phase (`.phase(`, `bounded`, `bounded_hung`, `bounded_marked`,
    /// `run_with`) and neither use a clock mark (`ClockMark`, `hang_flow(`, `hang_env(`) nor carry the exemption line
    /// `// conformance: no-clock-mark - <reason>`: such a test times its limit from the phase's start, so setup work before its hang
    /// (a stand-in git that is slow to start under load) can meet the limit. The class seen four times (ADR 0103, second amendment).
    fn unmarked_phase_tests(text: &str) -> Vec<String> {
        let trigger = regex::Regex::new(concat!(r"\.pha", r"se\(|\bbound", r"ed(_hung|_marked)?\(|\brun_", r"with\(")).unwrap();
        let marked = regex::Regex::new(concat!(
            r"Clock",
            r"Mark|hang_",
            r"flow\(|hang_",
            r"env\(|conformance: no-clock",
            r"-mark - \S"
        ))
        .unwrap();
        let Some((_, tests)) = text.split_once("mod tests {") else {
            return Vec::new();
        };
        let head = regex::Regex::new(r"(?m)^    (?:async )?fn ([a-z0-9_]+)").unwrap();
        let starts: Vec<(usize, String)> = head
            .captures_iter(tests)
            .map(|c| (c.get(0).unwrap().start(), c[1].to_string()))
            .collect();
        let mut bad = Vec::new();
        for (i, (at, name)) in starts.iter().enumerate() {
            let end = starts.get(i + 1).map_or(tests.len(), |n| n.0);
            let body = &tests[*at..end];
            if trigger.is_match(body) && !marked.is_match(body) {
                bad.push(name.clone());
            }
        }
        bad
    }

    #[test]
    fn the_audit_of_unmarked_phase_tests_sees_one_and_accepts_a_mark_or_a_reason() {
        // The positive control (ER-33): the audit finds a test that times a phase from its start, and passes the same test once it
        // uses a mark or carries its reason.
        let plain = "mod tests {\n    fn slow_git() {\n        flow.phase(step, id, \"x\", f);\n    }\n}\n";
        assert_eq!(unmarked_phase_tests(plain), ["slow_git"]);
        let marked = plain.replace("flow.phase", "let flow = hang_flow(&bin, &mark);\n        flow.phase");
        assert!(unmarked_phase_tests(&marked).is_empty());
        let exempt = plain.replace(
            "flow.phase",
            "// conformance: no-clock-mark - fake operations, no program\n        flow.phase",
        );
        assert!(unmarked_phase_tests(&exempt).is_empty());
        let bare = plain.replace("flow.phase", "// conformance: no-clock-mark -\n        flow.phase");
        assert_eq!(unmarked_phase_tests(&bare), ["slow_git"], "an exemption names its reason");
    }

    /// Every test of the phase machinery (the files that use `ci_host::phase`) that bounds work with a phase uses a clock mark or says
    /// why it need not (the class "work before the hang point of a hung-phase test meets its limit under load", ADR 0103).
    #[test]
    fn no_source_file_times_a_phase_test_from_its_start_without_a_clock_mark_or_a_reason() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ci_host");
        let mut seen = Vec::new();
        let mut bad = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|x| x != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            if !text.contains("phase::{")
                && !text.contains("use super::phase")
                && !text.contains("mod phase_tests")
                && !path.ends_with("phase.rs")
            {
                continue;
            }
            seen.push(path.file_name().unwrap().to_string_lossy().into_owned());
            for name in unmarked_phase_tests(&text) {
                bad.push(format!("{}::{name}", path.file_name().unwrap().to_string_lossy()));
            }
        }
        assert!(
            ["kept_clone.rs", "stable_workspace.rs", "phase.rs"]
                .iter()
                .all(|f| seen.iter().any(|s| s == f)),
            "the scan read {seen:?}: wrong folder?"
        );
        assert!(
            bad.is_empty(),
            "these tests bound work with a phase from its start: use `bounded_marked` with a ClockMark (`hang_flow` in kept_clone.rs) so a \
             slow-starting stand-in cannot meet the limit, or add `// conformance: no-clock-mark - <reason>` inside the test: {bad:?}"
        );
    }
}
