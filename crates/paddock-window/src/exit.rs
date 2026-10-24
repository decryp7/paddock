//! A bounded quit: a stuck shutdown must never hang Quit.
//!
//! Every step of tearing down (the app's: stop plugins, interrupt jobs, flush notifications, save bounds) runs on a thread
//! of its own and holds the exit up at most [`TEARDOWN_STEP_LIMIT`]; it is logged when it starts and how it ended, so a
//! hang in a log names its step. The exit watchdog ends a quit whose windows never close (seen with WebView2 in a
//! non-interactive session, where closing a webview waited for its driver): [`EXIT_STALL_LIMIT`] after the exit request,
//! unless the normal exit is already tearing down, it logs the windows still open, runs the teardown and ends the
//! process with the requested code.
//!
//! Ported from Debrief's `lifecycle.rs` (`teardown_step`, `arm_exit_watchdog`, `end_stalled_exit`), with the once-only
//! flags in a value instead of process statics, so each app (and each test) has its own.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Longest one teardown step may hold the exit up; it goes on in the background after that, and the app exits anyway.
pub const TEARDOWN_STEP_LIMIT: Duration = Duration::from_secs(3);
/// How long the windows may take to close after an exit request before the exit watchdog ends the process.
pub const EXIT_STALL_LIMIT: Duration = Duration::from_secs(8);

/// How a teardown step ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepEnd {
    Done,
    /// Still running after the limit (left behind).
    TimedOut,
    /// Crashed (the panic hook logs it) or could not start.
    Failed,
}

/// Runs one step of closing the app on a thread of its own and waits at most `limit` for it, logging (at Info) that it
/// started and how it ended. Work that may wait for the main thread never runs on it.
pub fn teardown_step(step: &str, limit: Duration, f: impl FnOnce() + Send + 'static) -> StepEnd {
    log::info!(Step = step; "Teardown: {step}");
    let started = Instant::now();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let spawned = std::thread::Builder::new()
        .name(format!("paddock-teardown-{step}"))
        .spawn(move || {
            f();
            let _ = tx.send(());
        });
    let end = match spawned.map(|_| rx.recv_timeout(limit)) {
        Ok(Ok(())) => StepEnd::Done,
        Ok(Err(std::sync::mpsc::RecvTimeoutError::Timeout)) => StepEnd::TimedOut,
        Ok(Err(std::sync::mpsc::RecvTimeoutError::Disconnected)) | Err(_) => StepEnd::Failed,
    };
    let ms = started.elapsed().as_millis() as u64;
    match end {
        StepEnd::Done => log::info!(Step = step, ElapsedMs = ms; "Teardown: {step} done"),
        StepEnd::TimedOut => {
            log::warn!(Step = step, ElapsedMs = ms; "Teardown: {step} did not finish in time; the app goes on closing")
        }
        StepEnd::Failed => {
            log::warn!(Step = step, ElapsedMs = ms; "Teardown: {step} failed; the app goes on closing")
        }
    }
    end
}

/// The once-only flags of one app's exit: the watchdog is armed once, the teardown runs once (the watchdog and the
/// normal exit path may both get there), and quitting is a state other windows see.
#[derive(Debug, Default)]
pub struct ExitState {
    quitting: AtomicBool,
    armed: AtomicBool,
    teardown: AtomicBool,
}

impl ExitState {
    /// Quitting has begun: closes go through, windows destroyed from now on are not "closed by the user".
    pub fn begin_quit(&self) {
        self.quitting.store(true, Ordering::SeqCst);
    }

    pub fn quitting(&self) -> bool {
        self.quitting.load(Ordering::SeqCst)
    }

    /// `true` the first time only: arm the exit watchdog now.
    pub fn arm(&self) -> bool {
        !self.armed.swap(true, Ordering::SeqCst)
    }

    /// `true` the first time only: tear down now.
    pub fn begin_teardown(&self) -> bool {
        !self.teardown.swap(true, Ordering::SeqCst)
    }

    /// The teardown has begun (the normal exit path owns it).
    pub fn tearing_down(&self) -> bool {
        self.teardown.load(Ordering::SeqCst)
    }
}

/// The exit watchdog's decision once its time is up: nothing if the normal exit is already tearing down; else log the
/// windows still open, tear down and end the process with `code`. Returns whether it ended the exit.
pub fn end_stalled_exit(
    tearing_down: bool,
    open_windows: impl FnOnce() -> Vec<String>,
    teardown: impl FnOnce(),
    end: impl FnOnce(i32),
    code: i32,
    waited: Duration,
) -> bool {
    if tearing_down {
        return false;
    }
    let open = open_windows();
    log::warn!(Windows = open.join(", ").as_str(), WaitedMs = waited.as_millis() as u64; "Exit stalled while closing windows; ending the process");
    teardown();
    end(code);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn teardown_steps_are_bounded_and_say_how_they_ended() {
        let quick = Duration::from_millis(200);
        assert_eq!(teardown_step("quick", quick, || {}), StepEnd::Done);
        let started = Instant::now();
        let (release, stuck_until) = std::sync::mpsc::channel::<()>();
        let stuck = teardown_step("stuck", quick, move || {
            let _ = stuck_until.recv_timeout(Duration::from_secs(5));
        });
        assert_eq!(stuck, StepEnd::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(release);
        // A generous limit: the panic hook can take a while in a debug build; the step ends as soon as it unwound.
        assert_eq!(
            teardown_step("crashing", Duration::from_secs(60), || panic!(
                "a crash in teardown"
            )),
            StepEnd::Failed
        );
    }

    #[test]
    fn the_flags_are_once_only() {
        let s = ExitState::default();
        assert!(!s.quitting() && !s.tearing_down());
        s.begin_quit();
        assert!(s.quitting());
        assert!(s.arm() && !s.arm());
        assert!(s.begin_teardown() && !s.begin_teardown());
        assert!(s.tearing_down());
    }

    /// The exit watchdog tears down and ends a quit whose windows never close, once.
    #[test]
    fn a_stalled_exit_is_ended_after_teardown_unless_the_normal_exit_already_runs() {
        let steps = RefCell::new(Vec::new());
        let open = || vec!["main".to_string()];
        let teardown = || steps.borrow_mut().push("teardown".to_string());
        let end = |code| steps.borrow_mut().push(format!("end {code}"));
        assert!(end_stalled_exit(
            false,
            open,
            teardown,
            end,
            3,
            EXIT_STALL_LIMIT
        ));
        assert_eq!(*steps.borrow(), ["teardown", "end 3"]);
        let ended = end_stalled_exit(true, open, teardown, end, 0, EXIT_STALL_LIMIT);
        assert!(!ended, "the normal exit path owns the teardown");
        assert_eq!(steps.borrow().len(), 2, "nothing ran again");
    }
}
