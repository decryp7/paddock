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
