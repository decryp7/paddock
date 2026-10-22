//! The runner update as a state machine: download, verify, wait for the running job, swap, restart, confirm, and
//! roll back automatically when the new binary does not come up.
//!
//! The machine is pure: the service feeds it [`UpdateEvent`]s (what happened) and carries out the
//! [`UpdateCommand`]s it returns (what to do next). It never kills a running job: if the runner stays busy past the
//! drain timeout, the update is postponed to the next check. See `docs/adr/0002-runner-auto-update.md`.

use crate::time::Timestamp;
use crate::version::RunnerVersion;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How long each waiting phase may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateTimeouts {
    /// Waiting for the running job to finish, in seconds; then the update is postponed.
    pub drain_secs: u64,
    /// Waiting for the idle runner to stop after the graceful stop, in seconds; then the update is postponed.
    pub stop_secs: u64,
    /// Waiting for the new runner to be accepted by the server, in seconds; then it is rolled back.
    pub confirm_secs: u64,
}

impl Default for UpdateTimeouts {
    fn default() -> Self {
        UpdateTimeouts {
            drain_secs: 3 * 3600,
            stop_secs: 120,
            confirm_secs: 300,
        }
    }
}

/// The versions an update goes between.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePlan {
    /// The installed version (`None`: first install, nothing to roll back to).
    pub from: Option<RunnerVersion>,
    /// The version being installed.
    pub to: RunnerVersion,
}

/// Where an update is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum UpdatePhase {
    /// No update in progress.
    Idle,
    /// Downloading the new binary next to the installed one.
    Downloading {
        /// The update.
        plan: UpdatePlan,
    },
    /// Checking the download against the published checksum (and signature, when there is one).
    Verifying {
        /// The update.
        plan: UpdatePlan,
    },
    /// Waiting for the running job to finish (the runner keeps working meanwhile).
    Draining {
        /// The update.
        plan: UpdatePlan,
        /// Since when.
        since: Timestamp,
    },
    /// The idle runner was asked to stop gracefully.
    Stopping {
        /// The update.
        plan: UpdatePlan,
        /// Since when.
        since: Timestamp,
    },
    /// Moving the installed binary aside (kept as the previous one) and the new one in place.
    Swapping {
        /// The update.
        plan: UpdatePlan,
    },
    /// The new runner was started; waiting for the server to accept it.
    Confirming {
        /// The update.
        plan: UpdatePlan,
        /// Since when.
        since: Timestamp,
    },
    /// Putting the previous binary back and starting it.
    RollingBack {
        /// The update.
        plan: UpdatePlan,
        /// Why.
        reason: RollbackReason,
    },
}

/// Why an update was rolled back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RollbackReason {
    /// The new runner exited before the server accepted it.
    ExitedEarly,
    /// The server did not accept the new runner in time.
    NotAcceptedInTime,
}

/// Why an update was postponed (it is tried again at the next check).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PostponeReason {
    /// A job ran longer than the drain timeout.
    Busy,
    /// The runner did not stop in time after the graceful stop.
    DidNotStop,
}

/// How an update ended (kept as "last update" and shown with the current and latest version).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum UpdateOutcome {
    /// The new version runs and was accepted by the server.
    Updated {
        /// The update.
        plan: UpdatePlan,
    },
    /// The new version failed; the previous one runs again.
    RolledBack {
        /// The update.
        plan: UpdatePlan,
        /// Why.
        reason: RollbackReason,
    },
    /// Not installed this time; tried again later.
    Postponed {
        /// The update.
        plan: UpdatePlan,
        /// Why.
        reason: PostponeReason,
    },
    /// Cancelled by the user.
    Cancelled {
        /// The update.
        plan: UpdatePlan,
    },
    /// Failed; the installed version was not changed, or (when even the roll-back failed) the runner is stopped
    /// and needs the user.
    Failed {
        /// The update.
        plan: UpdatePlan,
        /// What went wrong, for the user.
        reason: String,
        /// Whether the runner needs the user (roll-back failed or no previous version).
        needs_attention: bool,
    },
}

/// Something that happened, reported by the service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum UpdateEvent {
    /// Start an update.
    Begin {
        /// The update.
        plan: UpdatePlan,
    },
    /// The download finished.
    Downloaded,
    /// The download failed.
    DownloadFailed {
        /// Why.
        reason: String,
    },
    /// The download matches its checksum (and signature).
    Verified,
    /// The download does not match, or no trustworthy checksum could be obtained.
    VerificationFailed {
        /// Why.
        reason: String,
    },
    /// The runner has no job running.
    RunnerIdle,
    /// The runner process has exited.
    RunnerStopped,
    /// The new binary is in place; the previous one is kept.
    Swapped,
    /// Swapping failed; the installed binary is unchanged.
    SwapFailed {
        /// Why.
        reason: String,
    },
    /// The server accepted the runner.
    Accepted,
    /// The runner exited.
    RunnerExited,
    /// The previous binary is back in place and running.
    RolledBack,
    /// Rolling back failed.
    RollbackFailed {
        /// Why.
        reason: String,
    },
    /// Time passed (timeouts are checked).
    Tick,
    /// The user cancelled.
    Cancel,
}

/// What the service must do next.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "camelCase")]
pub enum UpdateCommand {
    /// Download `version` next to the installed binary.
    Download {
        /// The version.
        version: RunnerVersion,
    },
    /// Verify the download.
    Verify,
    /// Report [`UpdateEvent::RunnerIdle`] as soon as no job runs.
    WatchForIdle,
    /// Ask the runner to stop gracefully (it finishes a job it may just have taken).
    StopGracefully,
    /// Move the installed binary aside and the download in place.
    Swap,
    /// Start the runner (and report [`UpdateEvent::Accepted`] or [`UpdateEvent::RunnerExited`]).
    StartRunner,
    /// Stop the runner (it is not accepted and runs no job).
    StopRunner,
    /// Put the previous binary back, start it and report [`UpdateEvent::RolledBack`].
    RestorePrevious,
    /// Delete the download.
    DiscardDownload,
    /// Record the outcome and tell the user.
    Finish {
        /// The outcome.
        outcome: UpdateOutcome,
    },
}

/// An event that makes no sense in the current phase (a bug in the caller; reported, never a panic).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("update event {event} is not expected while {phase}")]
pub struct TransitionError {
    /// The phase's name.
    pub phase: &'static str,
    /// The event's name.
    pub event: &'static str,
}

/// One runner's update state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateMachine {
    phase: UpdatePhase,
    timeouts: UpdateTimeouts,
}

impl UpdateMachine {
    /// An idle machine.
    pub fn new(timeouts: UpdateTimeouts) -> Self {
        UpdateMachine {
            phase: UpdatePhase::Idle,
            timeouts,
        }
    }

    /// The current phase.
    pub fn phase(&self) -> &UpdatePhase {
        &self.phase
    }

    /// Handles one event at `now`, returning the commands to carry out in order. On an unexpected event the phase
    /// does not change.
    pub fn handle(&mut self, event: UpdateEvent, now: Timestamp) -> Result<Vec<UpdateCommand>, TransitionError> {
        use UpdateCommand as C;
        use UpdateEvent as E;
        use UpdatePhase as P;
        let t = self.timeouts;
        let expired = |since: Timestamp, secs: u64| now.since(since) >= Duration::from_secs(secs);
        let unexpected = TransitionError {
            phase: phase_name(&self.phase),
            event: event_name(&event),
        };

        let (next, commands) = match (self.phase.clone(), event) {
            (_, E::Tick) if !self.has_timeout() => return Ok(vec![]),
            (P::Idle, E::Begin { plan }) => {
                let version = plan.to.clone();
                (P::Downloading { plan }, vec![C::Download { version }])
            }
            (P::Downloading { plan }, E::Downloaded) => (P::Verifying { plan }, vec![C::Verify]),
            (P::Downloading { plan }, E::DownloadFailed { reason }) => (P::Idle, vec![C::DiscardDownload, failed(plan, reason, false)]),
            (P::Verifying { plan }, E::Verified) => (P::Draining { plan, since: now }, vec![C::WatchForIdle]),
            (P::Verifying { plan }, E::VerificationFailed { reason }) => (P::Idle, vec![C::DiscardDownload, failed(plan, reason, false)]),
            (P::Draining { plan, .. }, E::RunnerIdle) => (P::Stopping { plan, since: now }, vec![C::StopGracefully]),
            // A runner that was not running (stopped by the user, or crashed) needs no draining.
            (P::Draining { plan, .. }, E::RunnerStopped) => (P::Swapping { plan }, vec![C::Swap]),
            (P::Draining { plan, since }, E::Tick) if expired(since, t.drain_secs) => (
                P::Idle,
                vec![
                    C::DiscardDownload,
                    C::Finish {
                        outcome: UpdateOutcome::Postponed {
                            plan,
                            reason: PostponeReason::Busy,
                        },
                    },
                ],
            ),
            (P::Stopping { plan, .. }, E::RunnerStopped) => (P::Swapping { plan }, vec![C::Swap]),
            // The runner keeps finishing a job it took just before the stop; the supervisor restarts the installed
            // version once it exits. Nothing is killed.
            (P::Stopping { plan, since }, E::Tick) if expired(since, t.stop_secs) => (
                P::Idle,
                vec![
                    C::DiscardDownload,
                    C::Finish {
                        outcome: UpdateOutcome::Postponed {
                            plan,
                            reason: PostponeReason::DidNotStop,
                        },
                    },
                ],
            ),
            (P::Swapping { plan }, E::Swapped) => (P::Confirming { plan, since: now }, vec![C::StartRunner]),
            (P::Swapping { plan }, E::SwapFailed { reason }) => (
                P::Idle,
                vec![
                    C::StartRunner,
                    C::DiscardDownload,
                    failed(plan, format!("the new binary could not be put in place: {reason}"), false),
                ],
            ),
            (P::Confirming { plan, .. }, E::Accepted) => (
                P::Idle,
                vec![C::Finish {
                    outcome: UpdateOutcome::Updated { plan },
                }],
            ),
            (P::Confirming { plan, .. }, E::RunnerExited) => roll_back(plan, RollbackReason::ExitedEarly, false),
            (P::Confirming { plan, since }, E::Tick) if expired(since, t.confirm_secs) => {
                roll_back(plan, RollbackReason::NotAcceptedInTime, true)
            }
            (P::RollingBack { plan, reason }, E::RolledBack) => (
                P::Idle,
                vec![C::Finish {
                    outcome: UpdateOutcome::RolledBack { plan, reason },
                }],
            ),
            (P::RollingBack { plan, .. }, E::RollbackFailed { reason }) => (
                P::Idle,
                vec![failed(plan, format!("the previous version could not be restored: {reason}"), true)],
            ),
            (P::Downloading { plan } | P::Verifying { plan } | P::Draining { plan, .. }, E::Cancel) => (
                P::Idle,
                vec![
                    C::DiscardDownload,
                    C::Finish {
                        outcome: UpdateOutcome::Cancelled { plan },
                    },
                ],
            ),
            // The graceful stop cannot be taken back; the supervisor restarts the installed version when it exits.
            (P::Stopping { plan, .. }, E::Cancel) => (
                P::Idle,
                vec![
                    C::DiscardDownload,
                    C::Finish {
                        outcome: UpdateOutcome::Cancelled { plan },
                    },
                ],
            ),
            (phase, E::Tick) => (phase, vec![]),
            _ => return Err(unexpected),
        };
        self.phase = next;
        Ok(commands)
    }

    fn has_timeout(&self) -> bool {
        matches!(
            self.phase,
            UpdatePhase::Draining { .. } | UpdatePhase::Stopping { .. } | UpdatePhase::Confirming { .. }
        )
    }
}

fn failed(plan: UpdatePlan, reason: String, needs_attention: bool) -> UpdateCommand {
    UpdateCommand::Finish {
        outcome: UpdateOutcome::Failed {
            plan,
            reason,
            needs_attention,
        },
    }
}

fn roll_back(plan: UpdatePlan, reason: RollbackReason, still_running: bool) -> (UpdatePhase, Vec<UpdateCommand>) {
    let mut commands = if still_running { vec![UpdateCommand::StopRunner] } else { vec![] };
    if plan.from.is_none() {
        let why = match reason {
            RollbackReason::ExitedEarly => "the new runner exited before the server accepted it",
            RollbackReason::NotAcceptedInTime => "the server did not accept the new runner in time",
        };
        commands.push(failed(plan, format!("{why}, and there is no previous version to go back to"), true));
        return (UpdatePhase::Idle, commands);
    }
    commands.push(UpdateCommand::RestorePrevious);
    (UpdatePhase::RollingBack { plan, reason }, commands)
}

fn phase_name(p: &UpdatePhase) -> &'static str {
    match p {
        UpdatePhase::Idle => "idle",
        UpdatePhase::Downloading { .. } => "downloading",
        UpdatePhase::Verifying { .. } => "verifying",
        UpdatePhase::Draining { .. } => "draining",
        UpdatePhase::Stopping { .. } => "stopping",
        UpdatePhase::Swapping { .. } => "swapping",
        UpdatePhase::Confirming { .. } => "confirming",
        UpdatePhase::RollingBack { .. } => "rolling back",
    }
}

fn event_name(e: &UpdateEvent) -> &'static str {
    match e {
        UpdateEvent::Begin { .. } => "begin",
        UpdateEvent::Downloaded => "downloaded",
        UpdateEvent::DownloadFailed { .. } => "download failed",
        UpdateEvent::Verified => "verified",
        UpdateEvent::VerificationFailed { .. } => "verification failed",
        UpdateEvent::RunnerIdle => "runner idle",
        UpdateEvent::RunnerStopped => "runner stopped",
        UpdateEvent::Swapped => "swapped",
        UpdateEvent::SwapFailed { .. } => "swap failed",
        UpdateEvent::Accepted => "accepted",
        UpdateEvent::RunnerExited => "runner exited",
        UpdateEvent::RolledBack => "rolled back",
        UpdateEvent::RollbackFailed { .. } => "rollback failed",
        UpdateEvent::Tick => "tick",
        UpdateEvent::Cancel => "cancel",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use UpdateCommand as C;
    use UpdateEvent as E;

    fn plan() -> UpdatePlan {
        UpdatePlan {
            from: Some(RunnerVersion::parse("5.1.0").unwrap()),
            to: RunnerVersion::parse("5.2.0").unwrap(),
        }
    }

    fn s(secs: u64) -> Timestamp {
        Timestamp(secs * 1000)
    }

    /// A machine that got to `Confirming` at t = 10 s.
    fn confirming(p: UpdatePlan) -> UpdateMachine {
        let mut m = UpdateMachine::new(UpdateTimeouts::default());
        m.handle(E::Begin { plan: p }, s(0)).unwrap();
        m.handle(E::Downloaded, s(1)).unwrap();
        m.handle(E::Verified, s(2)).unwrap();
        m.handle(E::RunnerIdle, s(3)).unwrap();
        m.handle(E::RunnerStopped, s(4)).unwrap();
        assert_eq!(m.handle(E::Swapped, s(10)).unwrap(), vec![C::StartRunner]);
        m
    }

    #[test]
    fn a_good_update_runs_every_step_in_order() {
        let mut m = UpdateMachine::new(UpdateTimeouts::default());
        let to = plan().to;
        assert_eq!(
            m.handle(E::Begin { plan: plan() }, s(0)).unwrap(),
            vec![C::Download { version: to }]
        );
        assert_eq!(m.handle(E::Downloaded, s(1)).unwrap(), vec![C::Verify]);
        assert_eq!(m.handle(E::Verified, s(2)).unwrap(), vec![C::WatchForIdle]);
        assert!(matches!(m.phase(), UpdatePhase::Draining { .. }));
        assert_eq!(m.handle(E::RunnerIdle, s(3)).unwrap(), vec![C::StopGracefully]);
        assert_eq!(m.handle(E::RunnerStopped, s(4)).unwrap(), vec![C::Swap]);
        assert_eq!(m.handle(E::Swapped, s(5)).unwrap(), vec![C::StartRunner]);
        assert_eq!(
            m.handle(E::Accepted, s(30)).unwrap(),
            vec![C::Finish {
                outcome: UpdateOutcome::Updated { plan: plan() }
            }]
        );
        assert_eq!(m.phase(), &UpdatePhase::Idle);
    }

    #[test]
    fn a_new_runner_that_exits_is_rolled_back() {
        let mut m = confirming(plan());
        assert_eq!(m.handle(E::RunnerExited, s(20)).unwrap(), vec![C::RestorePrevious]);
        assert_eq!(
            m.handle(E::RolledBack, s(25)).unwrap(),
            vec![C::Finish {
                outcome: UpdateOutcome::RolledBack {
                    plan: plan(),
                    reason: RollbackReason::ExitedEarly
                }
            }]
        );
    }

    #[test]
    fn a_new_runner_not_accepted_in_time_is_stopped_and_rolled_back() {
        let mut m = confirming(plan());
        assert_eq!(m.handle(E::Tick, s(10 + 299)).unwrap(), vec![]);
        assert_eq!(m.handle(E::Tick, s(10 + 300)).unwrap(), vec![C::StopRunner, C::RestorePrevious]);
        assert!(matches!(
            m.phase(),
            UpdatePhase::RollingBack {
                reason: RollbackReason::NotAcceptedInTime,
                ..
            }
        ));
    }

    #[test]
    fn a_failed_roll_back_needs_the_user() {
        let mut m = confirming(plan());
        m.handle(E::RunnerExited, s(20)).unwrap();
        let out = m
            .handle(
                E::RollbackFailed {
                    reason: "file locked".into(),
                },
                s(21),
            )
            .unwrap();
        assert!(
            matches!(&out[..], [C::Finish { outcome: UpdateOutcome::Failed { needs_attention: true, reason, .. } }] if reason.contains("file locked"))
        );
    }

    #[test]
    fn a_first_install_has_nothing_to_roll_back_to() {
        let mut m = confirming(UpdatePlan { from: None, ..plan() });
        let out = m.handle(E::RunnerExited, s(20)).unwrap();
        assert!(matches!(
            &out[..],
            [C::Finish {
                outcome: UpdateOutcome::Failed { needs_attention: true, .. }
            }]
        ));
        assert_eq!(m.phase(), &UpdatePhase::Idle);
    }

    #[test]
    fn bad_downloads_never_reach_the_runner() {
        let mut m = UpdateMachine::new(UpdateTimeouts::default());
        m.handle(E::Begin { plan: plan() }, s(0)).unwrap();
        m.handle(E::Downloaded, s(1)).unwrap();
        let out = m
            .handle(
                E::VerificationFailed {
                    reason: "checksum mismatch".into(),
                },
                s(2),
            )
            .unwrap();
        assert_eq!(out[0], C::DiscardDownload);
        assert!(matches!(
            &out[1],
            C::Finish {
                outcome: UpdateOutcome::Failed {
                    needs_attention: false,
                    ..
                }
            }
        ));
        assert_eq!(m.phase(), &UpdatePhase::Idle);

        m.handle(E::Begin { plan: plan() }, s(3)).unwrap();
        let out = m.handle(E::DownloadFailed { reason: "timeout".into() }, s(4)).unwrap();
        assert_eq!(out[0], C::DiscardDownload);
    }

    #[test]
    fn a_long_job_postpones_the_update_without_killing_it() {
        let mut m = UpdateMachine::new(UpdateTimeouts {
            drain_secs: 60,
            ..UpdateTimeouts::default()
        });
        m.handle(E::Begin { plan: plan() }, s(0)).unwrap();
        m.handle(E::Downloaded, s(1)).unwrap();
        m.handle(E::Verified, s(2)).unwrap();
        assert_eq!(m.handle(E::Tick, s(61)).unwrap(), vec![]);
        let out = m.handle(E::Tick, s(62)).unwrap();
        assert_eq!(
            out,
            vec![
                C::DiscardDownload,
                C::Finish {
                    outcome: UpdateOutcome::Postponed {
                        plan: plan(),
                        reason: PostponeReason::Busy
                    }
                }
            ]
        );
        assert!(!out.contains(&C::StopRunner));
    }

    #[test]
    fn a_runner_that_does_not_stop_postpones_the_update() {
        let mut m = UpdateMachine::new(UpdateTimeouts::default());
        m.handle(E::Begin { plan: plan() }, s(0)).unwrap();
        m.handle(E::Downloaded, s(1)).unwrap();
        m.handle(E::Verified, s(2)).unwrap();
        m.handle(E::RunnerIdle, s(3)).unwrap();
        let out = m.handle(E::Tick, s(3 + 120)).unwrap();
        assert!(matches!(
            &out[1],
            C::Finish {
                outcome: UpdateOutcome::Postponed {
                    reason: PostponeReason::DidNotStop,
                    ..
                }
            }
        ));
    }

    #[test]
    fn a_stopped_runner_skips_draining_and_a_failed_swap_restarts_the_old_one() {
        let mut m = UpdateMachine::new(UpdateTimeouts::default());
        m.handle(E::Begin { plan: plan() }, s(0)).unwrap();
        m.handle(E::Downloaded, s(1)).unwrap();
        m.handle(E::Verified, s(2)).unwrap();
        assert_eq!(m.handle(E::RunnerStopped, s(3)).unwrap(), vec![C::Swap]);
        let out = m
            .handle(
                E::SwapFailed {
                    reason: "access denied".into(),
                },
                s(4),
            )
            .unwrap();
        assert_eq!(out[0], C::StartRunner);
        assert!(matches!(&out[2], C::Finish { outcome: UpdateOutcome::Failed { reason, .. } } if reason.contains("access denied")));
    }

    #[test]
    fn cancel_works_until_the_swap() {
        for steps in [0usize, 1, 2, 3] {
            let mut m = UpdateMachine::new(UpdateTimeouts::default());
            m.handle(E::Begin { plan: plan() }, s(0)).unwrap();
            let path = [E::Downloaded, E::Verified, E::RunnerIdle];
            for e in path.into_iter().take(steps) {
                m.handle(e, s(1)).unwrap();
            }
            let out = m.handle(E::Cancel, s(2)).unwrap();
            assert_eq!(
                out.last(),
                Some(&C::Finish {
                    outcome: UpdateOutcome::Cancelled { plan: plan() }
                }),
                "after {steps} steps"
            );
        }
        let mut m = confirming(plan());
        let err = m.handle(E::Cancel, s(11)).unwrap_err();
        assert_eq!(err.to_string(), "update event cancel is not expected while confirming");
        assert!(
            matches!(m.phase(), UpdatePhase::Confirming { .. }),
            "unchanged after an unexpected event"
        );
    }

    #[test]
    fn unexpected_events_are_errors_and_ticks_are_harmless() {
        let mut m = UpdateMachine::new(UpdateTimeouts::default());
        assert_eq!(m.handle(E::Tick, s(0)).unwrap(), vec![]);
        assert!(m.handle(E::Accepted, s(0)).is_err());
        m.handle(E::Begin { plan: plan() }, s(0)).unwrap();
        assert!(m.handle(E::Begin { plan: plan() }, s(0)).is_err(), "one update at a time");
        assert_eq!(
            m.handle(E::Tick, s(1_000_000)).unwrap(),
            vec![],
            "downloads time out in the downloader, not here"
        );
    }

    #[test]
    fn every_event_has_a_name_for_error_messages() {
        let events = [
            E::Begin { plan: plan() },
            E::Downloaded,
            E::DownloadFailed { reason: String::new() },
            E::Verified,
            E::VerificationFailed { reason: String::new() },
            E::RunnerIdle,
            E::RunnerStopped,
            E::Swapped,
            E::SwapFailed { reason: String::new() },
            E::Accepted,
            E::RunnerExited,
            E::RolledBack,
            E::RollbackFailed { reason: String::new() },
            E::Tick,
            E::Cancel,
        ];
        let names: std::collections::HashSet<_> = events.iter().map(event_name).collect();
        assert_eq!(names.len(), events.len());
        let mut m = confirming(plan());
        m.handle(E::RunnerExited, s(20)).unwrap();
        assert_eq!(phase_name(m.phase()), "rolling back");
        for p in [
            UpdatePhase::Idle,
            UpdatePhase::Swapping { plan: plan() },
            UpdatePhase::Verifying { plan: plan() },
        ] {
            assert!(!phase_name(&p).is_empty());
        }
    }
}
