//! A bounded phase of a host step (the port of `conformance/ci/bounded.sh`'s caller side): runs a closure for at most a limit,
//! logs `<step>: <label> ...` before and `<step>: <label>: done|failed (exit 1) in N s` after, and tells a caller that a phase
//! did not finish. The caller decides what a timeout means: a hard step ends in one `::error::` line, a soft one (a step that must
//! never fail its job, `BOUNDED_SOFT=1` in the shell) warns and falls back.
//!
//! A phase that overruns is left behind: its thread is not killable, so a phase that holds a program (git) gives that program the
//! same limit through [`ToolCall`](crate::ci_tool::ToolCall), which ends the program's whole tree at the limit. A file-system call
//! that hangs is abandoned and the process ends with the step.

use super::Step;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
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
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, Phase> {
    let start = Instant::now();
    step.say(format!("{}: {label} ...", step.name));
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    let secs = |s: Instant| s.elapsed().as_secs();
    match rx.recv_timeout(limit) {
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
    }
}

/// A clock mark (ADR 0103, "Hang-point signal"; for tests): the phase's limit counts from the moment `file` appears, and until then the
/// phase is bounded by `wait`. The port of `bounded.sh`'s `clock_mark` and `clock_wait`.
pub struct ClockMark<'a> {
    /// The file the test's stand-in writes when it enters its hang (removed when the phase starts).
    pub file: &'a std::path::Path,
    /// The bound before the mark appears.
    pub wait: Duration,
}

/// How often a phase with a clock mark looks for it: the shell's first naps (0.05 s).
const MARK_LOOK: Duration = Duration::from_millis(50);

/// [`bounded_hung`] with an optional [`ClockMark`]; also returns the bound that applied when the phase ended (`limit` once the mark
/// was seen or without one, else the mark's `wait`), for the caller's "did not finish within" line.
pub fn bounded_marked<T: Send + 'static>(
    step: &mut Step<'_>,
    label: &str,
    limit: Duration,
    hung: Option<&AtomicBool>,
    mark: Option<ClockMark<'_>>,
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> (Result<T, Phase>, Duration) {
    let Some(mark) = mark else {
        return (bounded_hung(step, label, limit, hung, f), limit);
    };
    let _ = std::fs::remove_file(mark.file);
    let start = Instant::now();
    step.say(format!("{}: {label} ...", step.name));
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
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
    let secs = start.elapsed().as_secs();
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
    (r, bound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Runs `f` as a phase with a 1 s limit and a clock mark at `file`; returns the result and the step's lines.
    fn marked(file: &std::path::Path, f: impl FnOnce() -> Result<(), String> + Send + 'static) -> (Result<(), Phase>, Duration, String) {
        let env = HashMap::new();
        let mut out = Vec::new();
        let mut step = Step {
            env: &env,
            out: &mut out,
            windows: false,
            name: "t",
        };
        let mark = ClockMark {
            file,
            wait: Duration::from_secs(60),
        };
        let (r, bound) = bounded_marked(&mut step, "p", Duration::from_secs(1), None, Some(mark), f);
        (r, bound, String::from_utf8_lossy(&out).into_owned())
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
        let (r, bound, _) = marked(&file, move || {
            // The phase's slow work before its hang: a program that runs longer than the limit (as a slow `git config` would), waited
            // for as a phase waits for its programs.
            let _ = paddock_proc::command("/bin/sleep").arg("1.5").status();
            std::fs::write(&f2, "hang").unwrap();
            loop {
                std::thread::park(); // the hang: nothing unparks it
            }
        });
        assert!(matches!(r, Err(Phase::Timeout)), "the hang meets the limit");
        assert_eq!(bound, Duration::from_secs(1), "the limit applied, counted from the mark");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap_or_default(),
            "hang",
            "the phase was ended after its mark, not in its work before the hang"
        );
    }

    #[test]
    fn a_marked_phase_that_finishes_before_its_mark_is_done() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mark");
        let (r, bound, out) = marked(&file, || Ok(()));
        assert!(r.is_ok());
        assert_eq!(bound, Duration::from_secs(60), "the bound before the mark");
        assert!(out.contains("t: p: done in "), "{out}");
    }
}
