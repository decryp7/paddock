//! Ready or exit, never a fixed start budget (ADR 0071): the one wait every test and every product start uses for a program
//! it started.
//!
//! - [`wait_ready_or_exit`] waits while the process is alive until a ready condition holds (a port open, a line seen, a
//!   handshake: whatever the [`StartProbe`] reads) or the process has ended; an end is returned at once with its exit status
//!   and the tail of its error output ([`ExitInfo`]). A stopped process ([`Liveness::Paused`]) is waited for and costs nothing
//!   against the guard. Elapsed time never decides the verdict: the only bound is one generous outer hang guard
//!   ([`HANG_GUARD`] by default), counted in ticks of the [`Pace`] like [`crate::StartJudge`]'s, that restarts whenever the
//!   probe's progress counter grows, so a start that keeps reporting phases is never called hung, and its expiry is
//!   [`Ended::Hung`], not a "slow start". It does not look at CPU: a process the OS holds before its first instruction (the
//!   first run of a fresh binary on macOS) uses none and shows no output, and is exactly the case a CPU judge calls stuck.
//! - [`warm_up`] pays the first run of a program before the start that matters: it runs `exe --version` to its end, with no
//!   budget of its own, so a macOS first-run hold (20 to 40 s on an idle host, minutes on a loaded one) is not charged to a
//!   start guard that follows. Tests of a real browser or a freshly built binary call it once per process.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use crate::{ExitInfo, Liveness, Pace, StartProbe};
use std::ffi::OsStr;
use std::time::{Duration, Instant};

/// The outer bound of a start wait: far over any working start on a starved host (a first run held by the OS took 42 s idle
/// and over 240 s under I/O load), so only a hang reaches it. Callers with their own bound (nextest's slow-timeout) pass
/// theirs.
// conformance: hang-guard
pub const HANG_GUARD: Duration = Duration::from_secs(600);

/// How a wait that did not end ready ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// The process ended before it was ready.
    Exited(ExitInfo),
    /// It was alive and reported no progress for the whole hang guard (counted while it was not stopped).
    Hung {
        /// The guard that ran out.
        guard: Duration,
    },
    /// The pace said the wait was cancelled.
    Cancelled,
}

impl std::fmt::Display for Ended {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ended::Exited(info) => {
                match info.status {
                    Some(code) => write!(f, "ended with exit status {code} before it was ready")?,
                    None => write!(
                        f,
                        "ended (by a signal, or its status is not known) before it was ready"
                    )?,
                }
                match &info.stderr_tail {
                    Some(tail) => write!(f, " (its error output: {tail})"),
                    None => write!(f, " (it wrote no error output)"),
                }
            }
            Ended::Hung { guard } => write!(
                f,
                "alive but neither ready nor making progress for the hang guard of {guard:?}; it is hung, not slow"
            ),
            Ended::Cancelled => write!(f, "the wait was cancelled"),
        }
    }
}

/// Waits a tick on the thread (the production [`Pace`]; tests pass a fake that counts).
#[derive(Debug, Default, Clone, Copy)]
pub struct ThreadPace;

impl Pace for ThreadPace {
    fn wait(&mut self, tick: Duration) -> bool {
        std::thread::sleep(tick);
        true
    }
}

/// Waits until `probe` is ready, ends, or hangs; see the module's description. `tick` is how often the probe is looked at;
/// the ready condition is read first, so a process that became ready and ended in one tick is ready.
///
/// # Errors
/// [`Ended::Exited`] with the exit status and error output the moment the probe reports the process gone,
/// [`Ended::Hung`] when `hang_guard` ticks of no progress passed while it was alive and not stopped, and
/// [`Ended::Cancelled`] when `pace` says so.
pub fn wait_ready_or_exit(
    probe: &mut impl StartProbe,
    pace: &mut impl Pace,
    tick: Duration,
    hang_guard: Duration,
) -> Result<(), Ended> {
    let mut idle = Duration::ZERO;
    let mut seen = probe.progress();
    loop {
        if probe.ready() {
            return Ok(());
        }
        match probe.liveness() {
            Liveness::Exited(info) => return Err(Ended::Exited(info)),
            Liveness::Paused => {}
            Liveness::Running => {
                let now = probe.progress();
                if now == seen {
                    idle = idle.saturating_add(tick);
                } else {
                    seen = now;
                    idle = Duration::ZERO;
                }
                if idle >= hang_guard {
                    return Err(Ended::Hung { guard: hang_guard });
                }
            }
        }
        if !pace.wait(tick) {
            return Err(Ended::Cancelled);
        }
    }
}

/// Runs `exe args` to its end and returns how long it took: the first execution of a program on this machine, made on its
/// own so that a start that follows is not charged for it. There is no time limit here: the caller's own bound (a test
/// runner's slow-timeout) is the guard, because a process the OS holds before its first instruction can be told from a hang by
/// neither CPU nor output.
///
/// # Errors
/// A message naming the program when it could not be run, or its exit status and the tail of its output when it failed.
pub fn warm_up(exe: impl AsRef<OsStr>, args: &[&str]) -> Result<Duration, String> {
    let began = Instant::now();
    let name = exe.as_ref().to_string_lossy().into_owned();
    let out = crate::command(exe.as_ref())
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("{name} could not be run for its first start: {e}"))?;
    if !out.status.success() {
        let tail = String::from_utf8_lossy(&out.stderr);
        let tail = tail.trim();
        let tail: String = tail
            .chars()
            .rev()
            .take(400)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return Err(format!(
            "{name}'s first run ended with {} (its output: {})",
            out.status,
            if tail.is_empty() { "none" } else { &tail }
        ));
    }
    Ok(began.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    const TICK: Duration = Duration::from_secs(1);

    /// A start the old fixed budgets (15 s, 20 s, 60 s of idle) would have given up on, played tick by tick: the pace advances
    /// the shared tick count, so no test sleeps and none depends on the host's speed.
    struct Script {
        ticks: Rc<Cell<u32>>,
        ready_at: Option<u32>,
        exit_at: Option<u32>,
        paused: Option<(u32, u32)>,
        progress_every: Option<u32>,
    }
    impl Script {
        fn new() -> (Script, Ticker) {
            let ticks = Rc::new(Cell::new(0));
            (
                Script {
                    ticks: ticks.clone(),
                    ready_at: None,
                    exit_at: None,
                    paused: None,
                    progress_every: None,
                },
                Ticker(ticks),
            )
        }
    }
    impl StartProbe for Script {
        fn liveness(&mut self) -> Liveness {
            let t = self.ticks.get();
            if self.exit_at.is_some_and(|e| t >= e) {
                return Liveness::Exited(ExitInfo {
                    status: Some(7),
                    stderr_tail: Some("dyld: bundle damaged".into()),
                });
            }
            if self.paused.is_some_and(|(a, b)| (a..b).contains(&t)) {
                return Liveness::Paused;
            }
            Liveness::Running
        }
        fn ready(&mut self) -> bool {
            self.ready_at.is_some_and(|r| self.ticks.get() >= r)
        }
        fn progress(&mut self) -> u64 {
            self.progress_every
                .map_or(0, |n| u64::from(self.ticks.get() / n))
        }
        fn cpu_time(&mut self) -> Option<Duration> {
            None
        }
    }
    struct Ticker(Rc<Cell<u32>>);
    impl Pace for Ticker {
        fn wait(&mut self, _tick: Duration) -> bool {
            self.0.set(self.0.get() + 1);
            true
        }
    }
    const GUARD: Duration = Duration::from_secs(600);

    #[test]
    fn a_start_that_becomes_ready_long_after_any_old_budget_is_waited_for() {
        let (mut s, mut pace) = Script::new();
        s.ready_at = Some(300);
        assert_eq!(wait_ready_or_exit(&mut s, &mut pace, TICK, GUARD), Ok(()));
        assert_eq!(
            s.ticks.get(),
            300,
            "it waited exactly until ready: 300 s of ticks, over 15 s, 20 s and 60 s"
        );
    }

    #[test]
    fn an_exit_ends_the_wait_at_once_with_its_status_and_output() {
        let (mut s, mut pace) = Script::new();
        s.exit_at = Some(3);
        s.ready_at = Some(100);
        let e = wait_ready_or_exit(&mut s, &mut pace, TICK, GUARD).unwrap_err();
        assert_eq!(
            s.ticks.get(),
            3,
            "no waiting out a guard for a process that is gone"
        );
        let text = e.to_string();
        assert!(
            text.contains("exit status 7") && text.contains("bundle damaged"),
            "{text}"
        );
        assert!(matches!(e, Ended::Exited(_)));
    }

    #[test]
    fn a_process_that_is_ready_and_gone_in_the_same_tick_is_ready() {
        let (mut s, mut pace) = Script::new();
        s.ready_at = Some(2);
        s.exit_at = Some(2);
        assert_eq!(wait_ready_or_exit(&mut s, &mut pace, TICK, GUARD), Ok(()));
    }

    #[test]
    fn only_a_process_that_neither_becomes_ready_nor_progresses_is_hung_after_the_guard() {
        let (mut s, mut pace) = Script::new();
        assert_eq!(
            wait_ready_or_exit(&mut s, &mut pace, TICK, Duration::from_secs(50)),
            Err(Ended::Hung {
                guard: Duration::from_secs(50)
            })
        );
        assert_eq!(
            s.ticks.get(),
            49,
            "hung is called at the 50th look without progress, after 49 waits"
        );
        assert!(
            Ended::Hung { guard: GUARD }
                .to_string()
                .contains("hung, not slow")
        );
    }

    #[test]
    fn progress_restarts_the_guard_so_a_start_that_keeps_reporting_phases_is_never_called_hung() {
        let (mut s, mut pace) = Script::new();
        s.progress_every = Some(40);
        s.ready_at = Some(400);
        assert_eq!(
            wait_ready_or_exit(&mut s, &mut pace, TICK, Duration::from_secs(50)),
            Ok(())
        );
    }

    #[test]
    fn a_stopped_process_costs_nothing_against_the_guard() {
        let (mut s, mut pace) = Script::new();
        s.paused = Some((0, 500));
        s.ready_at = Some(520);
        assert_eq!(
            wait_ready_or_exit(&mut s, &mut pace, TICK, Duration::from_secs(50)),
            Ok(())
        );
    }

    #[test]
    fn a_cancelled_pace_ends_the_wait() {
        struct Cancel;
        impl Pace for Cancel {
            fn wait(&mut self, _tick: Duration) -> bool {
                false
            }
        }
        let (mut s, _) = Script::new();
        assert_eq!(
            wait_ready_or_exit(&mut s, &mut Cancel, TICK, GUARD),
            Err(Ended::Cancelled)
        );
    }

    /// The first run of a program is waited for to its end: the stand-in answers only after the gate file exists, which the
    /// test creates from another thread after the program has been seen running (no sleep decides anything).
    #[cfg(unix)]
    #[test]
    fn warm_up_waits_for_a_program_that_answers_only_when_released_and_names_a_failure() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let gate = dir.path().join("gate");
        let started = dir.path().join("started");
        let exe = dir.path().join("slow.sh");
        std::fs::write(
            &exe,
            format!(
                "#!/bin/sh\n: > '{}'\nwhile [ ! -e '{}' ]; do :; done\necho 'Fake 1.0'\n",
                started.display(),
                gate.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let took = std::thread::scope(|s| {
            let run = s.spawn(|| warm_up(&exe, &["--version"]));
            while !started.exists() {
                std::thread::yield_now();
            }
            // Running and not finished: warm_up has not returned, whatever a budget would have said.
            assert!(!run.is_finished());
            std::fs::write(&gate, "").unwrap();
            run.join().unwrap()
        });
        assert!(took.is_ok(), "{took:?}");

        let bad = dir.path().join("bad.sh");
        std::fs::write(&bad, "#!/bin/sh\necho 'bundle damaged' >&2\nexit 5\n").unwrap();
        std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(0o755)).unwrap();
        let e = warm_up(&bad, &["--version"]).unwrap_err();
        assert!(
            e.contains("exit status: 5") && e.contains("bundle damaged"),
            "{e}"
        );
        let e = warm_up(dir.path().join("nope"), &[]).unwrap_err();
        assert!(e.contains("could not be run"), "{e}");
    }
}
