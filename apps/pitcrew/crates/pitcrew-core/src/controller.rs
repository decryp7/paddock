//! One runner's life as a pure state machine: start it, watch its log and its jobs, restart it with backoff when it
//! exits or stalls, stop, restart and drain it **only between jobs**, and hand it to the update machine.
//!
//! The service feeds [`Input`]s (what happened, what the user asked) with the time, and carries out the
//! [`Action`]s returned. Two rules are absolute:
//!
//! - **A running job is never interrupted.** Stop, restart, drain, a stall restart and moving a runner into the
//!   user's session all wait until [`crate::jobs::JobTracker`] says no job runs. The runner itself is only asked to
//!   stop gracefully; it is terminated only after the stop timeout and only while no job runs.
//! - **The stall rule is the PowerShell watchdog's**: at least `threshold` "failed to fetch task" lines (timed out or
//!   failed otherwise) within the window while no job runs. An idle runner logs nothing for hours; silence is never a
//!   stall.

use crate::health::HealthFacts;
use crate::history::HistoryKind;
use crate::logline::RunnerLogEvent;
use crate::restart::{RestartDecision, RestartPolicy, RestartReason, RestartTracker};
use crate::time::Timestamp;
use crate::watchdog::{StallDetector, StallRule, StallVerdict};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A poll failure older than this no longer counts as "the server cannot be reached".
pub const FAILURE_RECOVERY: Duration = Duration::from_secs(90);

/// What happens when the runner has stopped after a graceful stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AfterStop {
    /// Stay stopped (the user stopped it).
    Stop,
    /// Pause for maintenance.
    Pause,
    /// Start again right away (the user restarted it, or it moves to another session).
    Restart,
    /// Start again after the backoff delay (the watchdog found it stalled).
    StallRestart,
    /// Wait: the update machine decides (see [`Input::Hold`]).
    Hold,
}

/// Something to do between jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Pending {
    /// Stop.
    Stop,
    /// Pause for maintenance (drain).
    Pause,
    /// Restart.
    Restart,
    /// Restart in another session.
    Relocate,
}

impl Pending {
    fn after(self) -> AfterStop {
        match self {
            Pending::Stop => AfterStop::Stop,
            Pending::Pause => AfterStop::Pause,
            Pending::Restart | Pending::Relocate => AfterStop::Restart,
        }
    }
}

/// Where the runner's life is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Life {
    /// Stopped by the user.
    Stopped,
    /// Drained and paused for maintenance.
    Paused,
    /// Held by an update: not running, the update machine starts it.
    Held,
    /// Waiting for the backoff delay before starting.
    WaitingToStart {
        /// When it starts.
        at: Timestamp,
    },
    /// Waiting for a user to log on.
    WaitingForLogon,
    /// Being started (the service is launching the process).
    Starting,
    /// Running.
    Running {
        /// Since when.
        since: Timestamp,
    },
    /// Asked to stop gracefully.
    Stopping {
        /// Since when.
        since: Timestamp,
        /// It was running since.
        started: Timestamp,
        /// What follows.
        then: AfterStop,
        /// It was terminated after the stop timeout.
        killed: bool,
    },
    /// Exited too often; restarts paused.
    CrashLoop {
        /// When Pitcrew tries again by itself.
        until: Timestamp,
    },
}

/// Why a launch did not happen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LaunchError {
    /// It needs a logged-on user and nobody is logged on.
    WaitingForLogon,
    /// It could not be started (missing binary, refused token, …).
    Failed {
        /// Why, for the user.
        reason: String,
    },
}

/// What happened, or what the user asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Time passed.
    Tick,
    /// The process started.
    Launched,
    /// The process could not be started.
    LaunchFailed(LaunchError),
    /// The process exited.
    Exited {
        /// Its exit code.
        code: Option<i32>,
    },
    /// A classified log line.
    Log(RunnerLogEvent),
    /// Whether a job runs now (from the job tracker, before every tick).
    JobRunning(bool),
    /// The user starts it (also clears a crash loop and a pause).
    Start,
    /// The user stops it (after the current job).
    Stop,
    /// The user restarts it (after the current job).
    Restart,
    /// Finish the current job, then pause (maintenance).
    Drain,
    /// Leave maintenance.
    Resume,
    /// Someone logged on.
    LogonAvailable,
    /// It should run in another session (see `session::should_relocate`); done between jobs.
    Relocate,
    /// An update takes over (`true`) or gives it back (`false`). While held nothing restarts by itself.
    Hold(bool),
    /// While held: start now.
    LaunchNow,
    /// While held: stop gracefully now (the update machine checked that no job runs).
    StopNow,
}

/// What the service must do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Start the process.
    Launch,
    /// Ask it to stop gracefully.
    StopGracefully,
    /// Terminate the process tree (after the stop timeout, no job running).
    Kill,
    /// Record in the history.
    Record(HistoryKind),
}

/// The controller's parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerSettings {
    /// Backoff and crash-loop protection.
    pub restart: RestartPolicy,
    /// The stall rule.
    pub stall: StallRule,
    /// How long a graceful stop may take before the process is terminated (only while no job runs).
    pub stop_timeout: Duration,
}

impl Default for ControllerSettings {
    fn default() -> Self {
        ControllerSettings {
            restart: RestartPolicy::default(),
            stall: StallRule::default(),
            stop_timeout: Duration::from_secs(120),
        }
    }
}

/// One runner's controller.
#[derive(Debug, Clone)]
pub struct Controller {
    settings: ControllerSettings,
    life: Life,
    pending: Option<Pending>,
    held: bool,
    job_running: bool,
    restarts: RestartTracker,
    stall: StallDetector,
    stalled: bool,
    declared: bool,
    rejected: bool,
    failing_since: Option<Timestamp>,
    last_failure: Option<Timestamp>,
    problem: Option<String>,
}

impl Controller {
    /// A controller for a runner that should run (`wanted`), stay stopped, or stay paused (`paused`).
    pub fn new(settings: ControllerSettings, wanted: bool, paused: bool) -> Self {
        let life = if paused {
            Life::Paused
        } else if wanted {
            Life::WaitingToStart { at: Timestamp(0) }
        } else {
            Life::Stopped
        };
        Controller {
            settings,
            life,
            pending: None,
            held: false,
            job_running: false,
            restarts: RestartTracker::new(settings.restart),
            stall: StallDetector::new(settings.stall),
            stalled: false,
            declared: false,
            rejected: false,
            failing_since: None,
            last_failure: None,
            problem: None,
        }
    }

    /// Changes the parameters (settings saved by the user); history is kept.
    pub fn reconfigure(&mut self, settings: ControllerSettings) {
        self.settings = settings;
        self.restarts = RestartTracker::new(settings.restart);
        self.stall = StallDetector::new(settings.stall);
    }

    /// Where it is.
    pub fn life(&self) -> Life {
        self.life
    }

    /// What waits for the current job to end.
    pub fn pending(&self) -> Option<Pending> {
        self.pending
    }

    /// Whether an update holds it.
    pub fn held(&self) -> bool {
        self.held
    }

    /// Whether the process runs.
    pub fn process_alive(&self) -> bool {
        matches!(self.life, Life::Running { .. } | Life::Stopping { .. })
    }

    /// Since when the current process runs.
    pub fn running_since(&self) -> Option<Timestamp> {
        match self.life {
            Life::Running { since } => Some(since),
            Life::Stopping { started, .. } => Some(started),
            _ => None,
        }
    }

    /// Whether the server accepted the runner since it started.
    pub fn declared(&self) -> bool {
        self.declared
    }

    /// The last problem worth showing (a failed start), cleared when the server accepts the runner.
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// Whether the user wants it running (not stopped, not paused).
    pub fn wanted(&self) -> bool {
        !matches!(self.life, Life::Stopped | Life::Paused) && self.pending != Some(Pending::Stop)
    }

    /// Whether it is paused for maintenance, or will be after its job.
    pub fn paused_or_draining(&self) -> bool {
        self.life == Life::Paused || self.pending == Some(Pending::Pause)
    }

    /// The facts for `health::status` (`has_registration` and `updating` come from the service).
    pub fn facts(&self, now: Timestamp, has_registration: bool, updating: bool) -> HealthFacts {
        let last_good_poll = if self.declared {
            Some(self.failing_since.unwrap_or(now))
        } else {
            None
        };
        HealthFacts {
            wanted: self.wanted(),
            process_alive: self.process_alive(),
            has_registration,
            declared: self.declared,
            rejected: self.rejected,
            last_good_poll,
            job_running: self.job_running,
            stalled: self.stalled,
            crash_loop: matches!(self.life, Life::CrashLoop { .. }),
            updating,
            paused: self.life == Life::Paused,
            draining: self.pending.is_some() && self.process_alive() && !matches!(self.life, Life::Stopping { .. }),
            waiting_for_logon: self.life == Life::WaitingForLogon,
        }
    }

    /// Handles one input at `now`.
    pub fn handle(&mut self, input: Input, now: Timestamp) -> Vec<Action> {
        let mut out = vec![];
        match input {
            Input::JobRunning(b) => self.job_running = b,
            Input::Tick => self.tick(now, &mut out),
            Input::Log(event) => self.log(event, now),
            Input::Launched => match self.life {
                Life::Starting => self.started(now, &mut out),
                // Stopped or paused while the launch was on its way: stop what just started.
                Life::Stopped | Life::Paused => {
                    let then = if self.life == Life::Paused {
                        AfterStop::Pause
                    } else {
                        AfterStop::Stop
                    };
                    self.started(now, &mut out);
                    self.stop_now(now, then, &mut out);
                }
                _ => self.started(now, &mut out),
            },
            Input::LaunchFailed(LaunchError::WaitingForLogon) => {
                if self.life == Life::Starting {
                    self.life = Life::WaitingForLogon;
                }
            }
            Input::LaunchFailed(LaunchError::Failed { reason }) => {
                self.problem = Some(reason.clone());
                out.push(Action::Record(HistoryKind::LaunchFailed { reason }));
                if self.life == Life::Starting {
                    if self.held {
                        self.life = Life::Held;
                    } else {
                        self.after_exit(now, now, RestartReason::Exited, &mut out);
                    }
                }
            }
            Input::Exited { code } => self.exited(code, now, &mut out),
            Input::Start => {
                self.restarts.clear();
                self.pending = None;
                match self.life {
                    Life::Running { .. } | Life::Starting => {}
                    Life::Stopping {
                        since, started, killed, ..
                    } => {
                        self.life = Life::Stopping {
                            since,
                            started,
                            then: AfterStop::Restart,
                            killed,
                        };
                    }
                    Life::Held => {}
                    _ => self.launch(&mut out),
                }
            }
            Input::Stop => self.between_jobs(Pending::Stop, now, &mut out),
            Input::Restart => self.between_jobs(Pending::Restart, now, &mut out),
            Input::Drain => self.between_jobs(Pending::Pause, now, &mut out),
            Input::Relocate => {
                if self.pending.is_none() && self.process_alive() {
                    self.between_jobs(Pending::Relocate, now, &mut out);
                }
            }
            Input::Resume => {
                if self.pending == Some(Pending::Pause) {
                    self.pending = None;
                    out.push(Action::Record(HistoryKind::Resumed));
                }
                if self.life == Life::Paused {
                    self.restarts.clear();
                    out.push(Action::Record(HistoryKind::Resumed));
                    self.launch(&mut out);
                }
            }
            Input::LogonAvailable => {
                if self.life == Life::WaitingForLogon {
                    self.launch(&mut out);
                }
            }
            Input::Hold(true) => self.held = true,
            Input::Hold(false) => {
                self.held = false;
                if self.life == Life::Held {
                    self.launch(&mut out);
                }
            }
            Input::LaunchNow => {
                if !self.process_alive() && self.life != Life::Starting {
                    self.launch(&mut out);
                }
            }
            Input::StopNow => self.stop_now(now, AfterStop::Hold, &mut out),
        }
        out
    }

    fn tick(&mut self, now: Timestamp, out: &mut Vec<Action>) {
        if self.last_failure.is_some_and(|t| now.since(t) >= FAILURE_RECOVERY) {
            self.failing_since = None;
            self.last_failure = None;
        }
        match self.life {
            Life::Running { .. } if !self.held => {
                if let Some(p) = self.pending {
                    if !self.job_running {
                        self.stop_now(now, p.after(), out);
                        return;
                    }
                }
                if let StallVerdict::Stalled { timeouts } = self.stall.evaluate(now, self.job_running) {
                    self.stalled = true;
                    out.push(Action::Record(HistoryKind::Stalled { timeouts }));
                    self.stop_now(now, AfterStop::StallRestart, out);
                }
            }
            Life::Stopping {
                since,
                started,
                then,
                killed: false,
            } if now.since(since) >= self.settings.stop_timeout && !self.job_running => {
                self.life = Life::Stopping {
                    since,
                    started,
                    then,
                    killed: true,
                };
                out.push(Action::Kill);
            }
            Life::WaitingToStart { at } if now >= at && !self.held => self.launch(out),
            Life::CrashLoop { until } if now >= until && !self.held => {
                self.restarts.clear();
                self.launch(out);
            }
            _ => {}
        }
    }

    fn log(&mut self, event: RunnerLogEvent, now: Timestamp) {
        match event {
            RunnerLogEvent::Declared => {
                self.declared = true;
                self.rejected = false;
                self.problem = None;
            }
            RunnerLogEvent::DaemonStarting => self.declared = false,
            RunnerLogEvent::NotRegistered => self.rejected = true,
            RunnerLogEvent::FetchTimedOut | RunnerLogEvent::FetchFailed => {
                self.stall.record_timeout(now);
                self.last_failure = Some(now);
                self.failing_since.get_or_insert(now);
            }
            RunnerLogEvent::TaskStarted => self.job_running = true,
            RunnerLogEvent::ShuttingDown | RunnerLogEvent::Other => {}
        }
    }

    fn between_jobs(&mut self, what: Pending, now: Timestamp, out: &mut Vec<Action>) {
        match self.life {
            Life::Running { .. } => {
                self.pending = Some(what);
                if !self.job_running && !self.held {
                    self.stop_now(now, what.after(), out);
                }
            }
            Life::Stopping {
                since, started, killed, ..
            } => {
                self.life = Life::Stopping {
                    since,
                    started,
                    then: what.after(),
                    killed,
                };
            }
            // Not running: stop and pause take effect at once; a restart is a start.
            _ => match what {
                Pending::Stop => {
                    if self.life != Life::Stopped {
                        out.push(Action::Record(HistoryKind::Stopped));
                    }
                    self.pending = None;
                    self.life = Life::Stopped;
                }
                Pending::Pause => {
                    if self.life != Life::Paused {
                        out.push(Action::Record(HistoryKind::Paused));
                    }
                    self.pending = None;
                    self.life = Life::Paused;
                }
                Pending::Restart | Pending::Relocate => {
                    if !matches!(self.life, Life::Starting | Life::Held) {
                        self.restarts.clear();
                        self.launch(out);
                    }
                }
            },
        }
    }

    fn launch(&mut self, out: &mut Vec<Action>) {
        self.life = Life::Starting;
        out.push(Action::Launch);
    }

    fn started(&mut self, now: Timestamp, out: &mut Vec<Action>) {
        self.life = Life::Running { since: now };
        self.declared = false;
        self.stalled = false;
        self.failing_since = None;
        self.last_failure = None;
        self.stall.reset();
        out.push(Action::Record(HistoryKind::Started));
    }

    fn stop_now(&mut self, now: Timestamp, then: AfterStop, out: &mut Vec<Action>) {
        if let Life::Running { since } = self.life {
            self.life = Life::Stopping {
                since: now,
                started: since,
                then,
                killed: false,
            };
            out.push(Action::StopGracefully);
        }
    }

    fn exited(&mut self, code: Option<i32>, now: Timestamp, out: &mut Vec<Action>) {
        self.declared = false;
        self.job_running = false;
        match self.life {
            Life::Stopping { started, then, .. } => {
                out.push(Action::Record(HistoryKind::Exited { code, expected: true }));
                self.pending = None;
                match then {
                    AfterStop::Stop => {
                        out.push(Action::Record(HistoryKind::Stopped));
                        self.life = Life::Stopped;
                    }
                    AfterStop::Pause => {
                        out.push(Action::Record(HistoryKind::Paused));
                        self.life = Life::Paused;
                    }
                    AfterStop::Restart => self.launch(out),
                    AfterStop::StallRestart => self.after_exit(started, now, RestartReason::Stalled, out),
                    AfterStop::Hold => self.life = Life::Held,
                }
            }
            Life::Running { since } => {
                out.push(Action::Record(HistoryKind::Exited { code, expected: false }));
                if self.held {
                    self.life = Life::Held;
                } else {
                    self.after_exit(since, now, RestartReason::Exited, out);
                }
            }
            _ => {}
        }
    }

    fn after_exit(&mut self, started: Timestamp, now: Timestamp, reason: RestartReason, out: &mut Vec<Action>) {
        // A pending stop or pause wins over the restart (the runner exited on its own before its job ended).
        match self.pending.take() {
            Some(Pending::Stop) => {
                out.push(Action::Record(HistoryKind::Stopped));
                self.life = Life::Stopped;
                return;
            }
            Some(Pending::Pause) => {
                out.push(Action::Record(HistoryKind::Paused));
                self.life = Life::Paused;
                return;
            }
            _ => {}
        }
        match self.restarts.on_exit(started, now, reason) {
            RestartDecision::RestartAt { at, .. } => self.life = Life::WaitingToStart { at },
            RestartDecision::CrashLoop { until, exits } => {
                out.push(Action::Record(HistoryKind::CrashLoop { exits }));
                self.life = Life::CrashLoop { until };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: u64) -> Timestamp {
        Timestamp(s * 1000)
    }

    fn running() -> Controller {
        let mut c = Controller::new(ControllerSettings::default(), true, false);
        assert_eq!(c.handle(Input::Tick, t(0)), vec![Action::Launch]);
        assert_eq!(c.handle(Input::Launched, t(0)), vec![Action::Record(HistoryKind::Started)]);
        c.handle(Input::Log(RunnerLogEvent::Declared), t(1));
        c
    }

    fn records(a: &[Action]) -> Vec<&HistoryKind> {
        a.iter()
            .filter_map(|a| match a {
                Action::Record(k) => Some(k),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn starts_and_reports_health() {
        let c = running();
        assert_eq!(c.life(), Life::Running { since: t(0) });
        let f = c.facts(t(5), true, false);
        assert!(f.wanted && f.process_alive && f.declared);
        assert_eq!(f.last_good_poll, Some(t(5)));
        assert_eq!(c.running_since(), Some(t(0)));
        assert!(c.declared());
        let stopped = Controller::new(ControllerSettings::default(), false, false);
        assert_eq!(stopped.life(), Life::Stopped);
        assert!(!stopped.wanted());
        let paused = Controller::new(ControllerSettings::default(), true, true);
        assert!(paused.facts(t(0), true, false).paused);
    }

    #[test]
    fn exits_back_off_and_crash_loops_pause_restarts() {
        let mut c = running();
        let a = c.handle(Input::Exited { code: Some(1) }, t(5));
        assert_eq!(
            records(&a),
            vec![&HistoryKind::Exited {
                code: Some(1),
                expected: false
            }]
        );
        assert_eq!(c.life(), Life::WaitingToStart { at: t(15) });
        assert!(c.handle(Input::Tick, t(10)).is_empty());
        assert_eq!(c.handle(Input::Tick, t(15)), vec![Action::Launch]);
        let mut now = 15;
        for _ in 0..4 {
            c.handle(Input::Launched, t(now));
            now += 1;
            c.handle(Input::Exited { code: Some(1) }, t(now));
            if let Life::WaitingToStart { at } = c.life() {
                now = at.0 / 1000;
                c.handle(Input::Tick, t(now));
            }
        }
        assert!(matches!(c.life(), Life::CrashLoop { .. }), "{:?}", c.life());
        assert!(c.facts(t(now), true, false).crash_loop);
        // A manual start clears it.
        assert_eq!(c.handle(Input::Start, t(now)), vec![Action::Launch]);
        assert_eq!(c.life(), Life::Starting);
        // So does the end of the cool-down.
        let mut c = running();
        c.restarts = RestartTracker::new(RestartPolicy {
            crash_loop_restarts: 1,
            ..RestartPolicy::default()
        });
        c.handle(Input::Exited { code: None }, t(5));
        let Life::CrashLoop { until } = c.life() else { panic!() };
        assert!(c.handle(Input::Tick, t(6)).is_empty());
        assert_eq!(c.handle(Input::Tick, until), vec![Action::Launch]);
    }

    #[test]
    fn stop_restart_and_drain_wait_for_the_job() {
        let mut c = running();
        c.handle(Input::JobRunning(true), t(2));
        assert!(c.handle(Input::Stop, t(2)).is_empty(), "never during a job");
        assert_eq!(c.pending(), Some(Pending::Stop));
        assert!(!c.wanted());
        assert!(c.facts(t(2), true, false).draining);
        assert!(c.handle(Input::Tick, t(3)).is_empty());
        c.handle(Input::JobRunning(false), t(4));
        assert_eq!(c.handle(Input::Tick, t(4)), vec![Action::StopGracefully]);
        let a = c.handle(Input::Exited { code: Some(0) }, t(5));
        assert_eq!(
            records(&a),
            vec![
                &HistoryKind::Exited {
                    code: Some(0),
                    expected: true
                },
                &HistoryKind::Stopped
            ]
        );
        assert_eq!(c.life(), Life::Stopped);
        assert_eq!(c.handle(Input::Start, t(6)), vec![Action::Launch]);

        // Restart when idle is immediate, and starts right after the exit.
        let mut c = running();
        assert_eq!(c.handle(Input::Restart, t(2)), vec![Action::StopGracefully]);
        assert_eq!(c.handle(Input::Exited { code: Some(0) }, t(3))[1], Action::Launch);

        // Drain pauses; resume starts again.
        let mut c = running();
        c.handle(Input::JobRunning(true), t(2));
        c.handle(Input::Drain, t(2));
        assert!(c.paused_or_draining());
        c.handle(Input::JobRunning(false), t(9));
        assert_eq!(c.handle(Input::Tick, t(9)), vec![Action::StopGracefully]);
        c.handle(Input::Exited { code: Some(0) }, t(10));
        assert_eq!(c.life(), Life::Paused);
        let a = c.handle(Input::Resume, t(11));
        assert_eq!(a, vec![Action::Record(HistoryKind::Resumed), Action::Launch]);
    }

    #[test]
    fn resume_cancels_a_drain_that_has_not_finished() {
        let mut c = running();
        c.handle(Input::JobRunning(true), t(2));
        c.handle(Input::Drain, t(2));
        assert_eq!(c.handle(Input::Resume, t(3)), vec![Action::Record(HistoryKind::Resumed)]);
        assert_eq!(c.pending(), None);
        assert!(c.handle(Input::Resume, t(3)).is_empty());
    }

    #[test]
    fn commands_while_not_running_take_effect_at_once() {
        let mut c = Controller::new(ControllerSettings::default(), true, false);
        assert_eq!(records(&c.handle(Input::Stop, t(0))), vec![&HistoryKind::Stopped]);
        assert!(c.handle(Input::Stop, t(0)).is_empty());
        assert_eq!(records(&c.handle(Input::Drain, t(0))), vec![&HistoryKind::Paused]);
        assert!(c.handle(Input::Drain, t(0)).is_empty());
        assert_eq!(c.handle(Input::Restart, t(1)), vec![Action::Launch]);
        assert!(c.handle(Input::Restart, t(1)).is_empty(), "already starting");
        assert!(c.handle(Input::Start, t(1)).is_empty());
        // Stopped while the launch was on its way.
        c.handle(Input::Stop, t(1));
        let a = c.handle(Input::Launched, t(2));
        assert_eq!(a.last(), Some(&Action::StopGracefully));
        c.handle(Input::Exited { code: None }, t(3));
        assert_eq!(c.life(), Life::Stopped);
        let mut p = Controller::new(ControllerSettings::default(), true, true);
        let a = p.handle(Input::Launched, t(0));
        assert_eq!(a.last(), Some(&Action::StopGracefully));
        p.handle(Input::Exited { code: None }, t(1));
        assert_eq!(p.life(), Life::Paused);
    }

    #[test]
    fn commands_during_a_graceful_stop_change_what_follows() {
        let mut c = running();
        c.handle(Input::Stop, t(2));
        c.handle(Input::Start, t(3));
        c.handle(Input::Exited { code: None }, t(4));
        assert_eq!(c.life(), Life::Starting, "start after stop means come back");
        let mut c = running();
        c.handle(Input::Restart, t(2));
        c.handle(Input::Drain, t(3));
        c.handle(Input::Exited { code: None }, t(4));
        assert_eq!(c.life(), Life::Paused);
    }

    #[test]
    fn stalls_restart_only_an_idle_runner_and_count_all_fetch_failures() {
        let mut c = running();
        c.handle(Input::JobRunning(true), t(2));
        for i in 0..6 {
            let e = if i % 2 == 0 {
                RunnerLogEvent::FetchFailed
            } else {
                RunnerLogEvent::FetchTimedOut
            };
            c.handle(Input::Log(e), t(10 + i * 30));
        }
        assert!(c.handle(Input::Tick, t(200)).is_empty(), "busy wins");
        c.handle(Input::JobRunning(false), t(201));
        let a = c.handle(Input::Tick, t(201));
        assert_eq!(
            a,
            vec![Action::Record(HistoryKind::Stalled { timeouts: 6 }), Action::StopGracefully]
        );
        assert!(c.facts(t(201), true, false).stalled);
        c.handle(Input::Exited { code: Some(0) }, t(205));
        assert_eq!(c.life(), Life::WaitingToStart { at: t(215) });
        c.handle(Input::Tick, t(215));
        c.handle(Input::Launched, t(215));
        assert!(!c.facts(t(216), true, false).stalled);
    }

    #[test]
    fn poll_failures_degrade_until_they_stop() {
        let mut c = running();
        c.handle(Input::Log(RunnerLogEvent::FetchFailed), t(10));
        assert_eq!(c.facts(t(20), true, false).last_good_poll, Some(t(10)));
        c.handle(Input::Tick, t(10 + 91));
        assert_eq!(c.facts(t(200), true, false).last_good_poll, Some(t(200)));
        c.handle(Input::Log(RunnerLogEvent::NotRegistered), t(201));
        assert!(c.facts(t(201), true, false).rejected);
        c.handle(Input::Log(RunnerLogEvent::Declared), t(202));
        assert!(!c.facts(t(202), true, false).rejected);
        c.handle(Input::Log(RunnerLogEvent::DaemonStarting), t(203));
        assert_eq!(c.facts(t(203), true, false).last_good_poll, None);
        c.handle(Input::Log(RunnerLogEvent::ShuttingDown), t(204));
        c.handle(Input::Log(RunnerLogEvent::TaskStarted), t(205));
        assert!(c.facts(t(205), true, false).job_running);
    }

    #[test]
    fn a_hanging_stop_is_ended_only_without_a_job() {
        let mut c = running();
        c.handle(Input::Stop, t(2));
        c.handle(Input::JobRunning(true), t(3));
        assert!(c.handle(Input::Tick, t(200)).is_empty());
        c.handle(Input::JobRunning(false), t(201));
        assert_eq!(c.handle(Input::Tick, t(201)), vec![Action::Kill]);
        assert!(c.handle(Input::Tick, t(300)).is_empty(), "once");
        c.handle(Input::Start, t(301));
        c.handle(Input::Exited { code: None }, t(302));
        assert_eq!(c.life(), Life::Starting);
    }

    #[test]
    fn launch_failures_back_off_and_logon_waits() {
        let mut c = Controller::new(ControllerSettings::default(), true, false);
        c.handle(Input::Tick, t(0));
        let a = c.handle(
            Input::LaunchFailed(LaunchError::Failed {
                reason: "no binary".into(),
            }),
            t(0),
        );
        assert_eq!(
            records(&a),
            vec![&HistoryKind::LaunchFailed {
                reason: "no binary".into()
            }]
        );
        assert_eq!(c.problem(), Some("no binary"));
        assert_eq!(c.life(), Life::WaitingToStart { at: t(10) });
        c.handle(Input::Tick, t(10));
        c.handle(Input::LaunchFailed(LaunchError::WaitingForLogon), t(10));
        assert_eq!(c.life(), Life::WaitingForLogon);
        assert!(c.facts(t(10), true, false).waiting_for_logon);
        c.handle(Input::LaunchFailed(LaunchError::WaitingForLogon), t(11));
        assert_eq!(c.handle(Input::LogonAvailable, t(12)), vec![Action::Launch]);
        assert!(c.handle(Input::LogonAvailable, t(12)).is_empty());
        c.handle(Input::Launched, t(13));
        c.handle(Input::Log(RunnerLogEvent::Declared), t(14));
        assert_eq!(c.problem(), None);
    }

    #[test]
    fn relocation_waits_for_the_job_and_restarts_at_once() {
        let mut c = running();
        c.handle(Input::JobRunning(true), t(2));
        assert!(c.handle(Input::Relocate, t(2)).is_empty());
        assert_eq!(c.pending(), Some(Pending::Relocate));
        c.handle(Input::JobRunning(false), t(30));
        assert_eq!(c.handle(Input::Tick, t(30)), vec![Action::StopGracefully]);
        c.handle(Input::Exited { code: None }, t(31));
        assert_eq!(c.life(), Life::Starting);
        let mut idle = Controller::new(ControllerSettings::default(), false, false);
        assert!(idle.handle(Input::Relocate, t(0)).is_empty(), "nothing to move");
    }

    #[test]
    fn an_update_holds_the_runner() {
        let mut c = running();
        c.handle(Input::Hold(true), t(2));
        assert!(c.held());
        c.handle(Input::Stop, t(2));
        assert!(c.handle(Input::Tick, t(3)).is_empty(), "pending work waits for the update");
        c.pending = None;
        assert_eq!(c.handle(Input::StopNow, t(3)), vec![Action::StopGracefully]);
        c.handle(Input::Exited { code: Some(0) }, t(4));
        assert_eq!(c.life(), Life::Held);
        assert!(c.handle(Input::Tick, t(100)).is_empty());
        assert!(c.handle(Input::Start, t(100)).is_empty(), "the update starts it");
        assert_eq!(c.handle(Input::LaunchNow, t(101)), vec![Action::Launch]);
        assert!(c.handle(Input::LaunchNow, t(101)).is_empty());
        c.handle(Input::Launched, t(102));
        // The new binary exits early: held, no restart.
        c.handle(Input::Exited { code: Some(2) }, t(103));
        assert_eq!(c.life(), Life::Held);
        c.handle(Input::LaunchNow, t(104));
        c.handle(Input::LaunchFailed(LaunchError::Failed { reason: "gone".into() }), t(104));
        assert_eq!(c.life(), Life::Held);
        assert_eq!(c.handle(Input::Hold(false), t(105)), vec![Action::Launch]);
        assert!(!c.held());
        let mut free = running();
        assert!(free.handle(Input::Hold(false), t(2)).is_empty());
        // Held while running: an exit is held too.
        free.handle(Input::Hold(true), t(3));
        free.handle(Input::Exited { code: None }, t(4));
        assert_eq!(free.life(), Life::Held);
    }

    #[test]
    fn a_runner_that_exits_during_a_drain_pauses_instead_of_restarting() {
        let mut c = running();
        c.handle(Input::JobRunning(true), t(1));
        c.handle(Input::Drain, t(1));
        c.handle(Input::Exited { code: Some(1) }, t(5));
        assert_eq!(c.life(), Life::Paused);
        let mut c = running();
        c.handle(Input::JobRunning(true), t(1));
        c.handle(Input::Stop, t(1));
        c.handle(Input::Exited { code: Some(1) }, t(5));
        assert_eq!(c.life(), Life::Stopped);
        // Exits outside running are ignored.
        let mut s = Controller::new(ControllerSettings::default(), false, false);
        assert!(s.handle(Input::Exited { code: None }, t(1)).is_empty());
        s.reconfigure(ControllerSettings::default());
        assert!(s.handle(Input::StopNow, t(1)).is_empty());
    }
}
