//! Stopping a supervised job without removing its registration (the "stop" half of `paddock-service`): the job stays registered, so
//! the next login (or [`crate::Manager::repair`]) starts it again. Every call goes through the [`System`] port and ends with a read-back
//! of the OS (ADR 0054 section 1.1): a stop whose read-back still shows the job is never `Ok`.
//!
//! - launchd: `launchctl bootout <domain>/<label>`. The registration (`SMAppService`) is untouched; `KeepAlive` cannot restart a job
//!   launchd no longer holds. The read-back is `launchctl print`: launchd must no longer know the job.
//! - systemd (user): `systemctl --user stop <unit>`, never `disable`. The read-back is `is-active`.

use crate::launchctl::{bootout, job_report};
use crate::paths::SYSTEMCTL;
use crate::port::System;
use std::time::Duration;

/// What to stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopMechanism {
    /// A launchd job: `gui/<uid>/<label>`.
    Launchd {
        /// The job's launchd target.
        target: String,
    },
    /// A systemd user unit (`dyno-agent.service`).
    SystemdUser {
        /// The unit's name.
        unit: String,
    },
}

/// How long the read-back waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopTimings {
    /// The longest wait for the OS to drop the job (the job's own exit grace comes first).
    pub gone_wait: Duration,
    /// How often the OS is asked meanwhile.
    pub poll: Duration,
}

impl Default for StopTimings {
    fn default() -> Self {
        StopTimings {
            gone_wait: Duration::from_secs(30),
            poll: Duration::from_millis(250),
        }
    }
}

/// How a stop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// The OS held the job and now confirms it is gone.
    Stopped,
    /// The OS did not hold a running job to begin with.
    AlreadyStopped,
}

/// Why a stop did not end with the OS confirming the job gone. The text says what happened and the next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopError(pub String);

impl std::fmt::Display for StopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StopError {}

/// What the OS says about the job now: `Some(true)` it holds it, `Some(false)` it does not, `None` it could not be asked or
/// said something unexpected (unknown is never "gone").
fn held(sys: &mut dyn System, m: &StopMechanism) -> Option<bool> {
    match m {
        StopMechanism::Launchd { target } => job_report(sys, target).map(|j| j.found),
        StopMechanism::SystemdUser { unit } => {
            let out = sys.run(SYSTEMCTL, &["--user", "is-active", unit]).ok()?;
            match out.stdout.trim() {
                "active" | "activating" | "deactivating" | "reloading" => Some(true),
                "inactive" | "failed" => Some(false),
                _ => None,
            }
        }
    }
}

/// Stops the job, keeps its registration and waits until the OS confirms it is gone.
///
/// # Errors
/// [`StopError`] when the command failed or the OS still holds the job after `timings.gone_wait`.
pub fn stop(
    sys: &mut dyn System,
    m: &StopMechanism,
    timings: StopTimings,
) -> Result<Stopped, StopError> {
    if held(sys, m) == Some(false) {
        return Ok(Stopped::AlreadyStopped);
    }
    // A command that failed or timed out may still have done its work (launchd's exit grace is longer than a command's
    // timeout): the read-back decides, and the command's own words stay in the message if it does not confirm.
    let issued: Result<(), String> = match m {
        StopMechanism::Launchd { target } => bootout(sys, target),
        StopMechanism::SystemdUser { unit } => {
            match sys.run(SYSTEMCTL, &["--user", "stop", unit]) {
                Ok(out) if out.success() => Ok(()),
                Ok(out) => Err(format!("systemctl stop: {}", out.reason())),
                Err(e) => Err(format!("systemctl stop {e}")),
            }
        }
    };
    let mut waited = Duration::ZERO;
    loop {
        if held(sys, m) == Some(false) {
            return Ok(Stopped::Stopped);
        }
        if waited >= timings.gone_wait {
            let cause = issued.err().map(|e| format!(" ({e})")).unwrap_or_default();
            return Err(StopError(format!(
                "The operating system still reports the agent after {} s{cause}. {}",
                waited.as_secs(),
                next_step(m)
            )));
        }
        sys.pause(timings.poll);
        waited += timings.poll;
    }
}

fn next_step(m: &StopMechanism) -> String {
    match m {
        StopMechanism::Launchd { target } => {
            format!("Run `launchctl bootout {target}` in Terminal, or restart the Mac.")
        }
        StopMechanism::SystemdUser { unit } => {
            format!("Run `systemctl --user stop {unit}` in a terminal, or log out and in.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::port::{Output, RunError};
    use crate::testing::FakeSystem;
    use std::sync::{Arc, Mutex};

    const T: &str = "gui/501/net.example.agent";
    const FAST: StopTimings = StopTimings {
        gone_wait: Duration::from_secs(2),
        poll: Duration::from_millis(500),
    };

    fn launchd() -> StopMechanism {
        StopMechanism::Launchd { target: T.into() }
    }

    fn systemd() -> StopMechanism {
        StopMechanism::SystemdUser {
            unit: "x.service".into(),
        }
    }

    /// A fake launchd whose job disappears after `bootout`, or never does.
    fn fake_launchd(drops: bool) -> FakeSystem {
        let sys = FakeSystem::new();
        let up = Arc::new(Mutex::new(true));
        let (a, b) = (up.clone(), up);
        sys.on("/bin/launchctl print", move |_, _| {
            Ok(if *a.lock().unwrap() {
                Output::ok("state = running\n\tpid = 7\n")
            } else {
                Output::code(113)
            })
        });
        sys.on("/bin/launchctl bootout", move |_, _| {
            if drops {
                *b.lock().unwrap() = false;
            }
            Ok(Output::code(0))
        });
        sys
    }

    #[test]
    fn launchd_is_booted_out_and_the_read_back_confirms_it_is_gone() {
        let sys = fake_launchd(true);
        assert_eq!(
            stop(&mut sys.clone(), &launchd(), FAST),
            Ok(Stopped::Stopped)
        );
        let cmds = sys.commands();
        assert!(
            cmds.contains(&format!("/bin/launchctl bootout {T}")),
            "{cmds:?}"
        );
        assert!(
            cmds.last().unwrap().starts_with("/bin/launchctl print"),
            "the last call is the read-back: {cmds:?}"
        );
        assert!(
            cmds.iter()
                .all(|c| !c.contains("disable") && !c.contains("remove")),
            "the registration stays: {cmds:?}"
        );
    }

    #[test]
    fn a_launchd_job_that_stays_after_bootout_is_reported_with_the_next_step() {
        let sys = fake_launchd(false);
        let e = stop(&mut sys.clone(), &launchd(), FAST).unwrap_err();
        assert!(
            e.0.contains("still reports the agent")
                && e.0.contains(&format!("launchctl bootout {T}")),
            "{e}"
        );
    }

    #[test]
    fn a_job_launchd_does_not_hold_is_already_stopped_and_nothing_is_run() {
        let sys = FakeSystem::new();
        sys.answer("/bin/launchctl print", Output::code(113));
        assert_eq!(
            stop(&mut sys.clone(), &launchd(), FAST),
            Ok(Stopped::AlreadyStopped)
        );
        assert!(!sys.commands().iter().any(|c| c.contains("bootout")));
    }

    #[test]
    fn a_launchd_that_cannot_be_asked_is_not_taken_for_gone() {
        let sys = FakeSystem::new();
        sys.on("/bin/launchctl print", |_, _| {
            Err(RunError::TimedOut("launchctl".into()))
        });
        assert!(stop(&mut sys.clone(), &launchd(), FAST).is_err());
    }

    #[test]
    fn a_stop_command_that_timed_out_still_gets_its_read_back() {
        // launchd needs longer than the command's own timeout to end the job: the read-back decides, not the timeout.
        let sys = FakeSystem::new();
        let up = Arc::new(Mutex::new(true));
        let (a, b) = (up.clone(), up);
        sys.on("/bin/launchctl print", move |_, _| {
            Ok(if *a.lock().unwrap() {
                Output::ok("state = running\n")
            } else {
                Output::code(113)
            })
        });
        sys.on("/bin/launchctl bootout", move |_, _| {
            *b.lock().unwrap() = false;
            Err(RunError::TimedOut("launchctl".into()))
        });
        assert_eq!(
            stop(&mut sys.clone(), &launchd(), FAST),
            Ok(Stopped::Stopped)
        );
        let sys = FakeSystem::new();
        sys.answer("systemctl --user is-active", Output::ok("active\n"));
        sys.on("systemctl --user stop", |_, _| {
            Err(RunError::TimedOut("systemctl".into()))
        });
        let e = stop(&mut sys.clone(), &systemd(), FAST).unwrap_err();
        assert!(
            e.0.contains("did not finish in time"),
            "the cause stays in the message: {e}"
        );
    }

    #[test]
    fn a_failing_bootout_is_reported() {
        let sys = FakeSystem::new();
        sys.answer("/bin/launchctl print", Output::ok("state = running\n"));
        sys.answer(
            "/bin/launchctl bootout",
            Output {
                code: Some(5),
                stderr: "Operation not permitted".into(),
                ..Output::default()
            },
        );
        let e = stop(&mut sys.clone(), &launchd(), FAST).unwrap_err();
        assert!(
            e.0.contains("Operation not permitted") && e.0.contains("Terminal"),
            "{e}"
        );
    }

    fn fake_systemd(drops: bool) -> FakeSystem {
        let sys = FakeSystem::new();
        let up = Arc::new(Mutex::new(true));
        let (a, b) = (up.clone(), up);
        sys.on("systemctl --user is-active", move |_, _| {
            Ok(if *a.lock().unwrap() {
                Output::ok("active\n")
            } else {
                Output {
                    code: Some(3),
                    stdout: "inactive\n".into(),
                    ..Output::default()
                }
            })
        });
        sys.on("systemctl --user stop", move |_, _| {
            if drops {
                *b.lock().unwrap() = false;
            }
            Ok(Output::code(0))
        });
        sys
    }

    #[test]
    fn systemd_is_stopped_never_disabled_and_read_back() {
        let sys = fake_systemd(true);
        assert_eq!(
            stop(&mut sys.clone(), &systemd(), FAST),
            Ok(Stopped::Stopped)
        );
        let cmds = sys.commands();
        assert!(
            cmds.contains(&"systemctl --user stop x.service".to_string()),
            "{cmds:?}"
        );
        assert!(cmds.iter().all(|c| !c.contains("disable")), "{cmds:?}");
        assert!(cmds.last().unwrap().contains("is-active"), "{cmds:?}");
    }

    #[test]
    fn a_systemd_unit_that_stays_active_is_reported() {
        let sys = fake_systemd(false);
        let e = stop(&mut sys.clone(), &systemd(), FAST).unwrap_err();
        assert!(
            e.0.contains("still reports") && e.0.contains("systemctl --user stop x.service"),
            "{e}"
        );
    }

    #[test]
    fn systemd_already_inactive_is_already_stopped_and_a_failing_stop_or_unknown_answer_is_not_gone()
     {
        let sys = fake_systemd(true);
        sys.answer(
            "systemctl --user is-active",
            Output {
                code: Some(3),
                stdout: "inactive\n".into(),
                ..Output::default()
            },
        );
        assert_eq!(
            stop(&mut sys.clone(), &systemd(), FAST),
            Ok(Stopped::AlreadyStopped)
        );
        let sys = fake_systemd(true);
        sys.answer(
            "systemctl --user stop",
            Output {
                code: Some(1),
                stderr: "Failed to connect to bus".into(),
                ..Output::default()
            },
        );
        assert!(
            stop(&mut sys.clone(), &systemd(), FAST)
                .unwrap_err()
                .0
                .contains("Failed to connect to bus")
        );
        let sys = fake_systemd(true);
        sys.on("systemctl --user stop", |_, _| {
            Err(RunError::NotStarted("no systemctl".into()))
        });
        assert!(
            stop(&mut sys.clone(), &systemd(), FAST)
                .unwrap_err()
                .0
                .contains("no systemctl")
        );
        let sys = FakeSystem::new();
        sys.answer("systemctl --user is-active", Output::ok("unknown\n"));
        assert!(
            stop(&mut sys.clone(), &systemd(), FAST).is_err(),
            "an answer it does not know is not 'gone'"
        );
    }

    #[test]
    fn the_defaults_and_messages_are_sane() {
        assert!(StopTimings::default().gone_wait >= Duration::from_secs(10));
        assert_eq!(StopError("x".into()).to_string(), "x");
    }
}
