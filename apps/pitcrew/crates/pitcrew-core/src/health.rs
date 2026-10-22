//! A runner's health: the facts the service observes, summed up as one status for the tray and the window.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// What the service knows about one runner at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthFacts {
    /// Pitcrew wants this runner running (it was not stopped by the user).
    pub wanted: bool,
    /// The runner process is alive.
    pub process_alive: bool,
    /// The runner has a registration (its `.runner` file exists; Pitcrew never reads its contents).
    pub has_registration: bool,
    /// The server accepted the runner since it last started ("declare successfully" in its log).
    pub declared: bool,
    /// The server said it does not know this runner.
    pub rejected: bool,
    /// The last time a poll for tasks did not fail.
    pub last_good_poll: Option<Timestamp>,
    /// A job is running (the runner has child processes, or it reported a task that has not ended).
    pub job_running: bool,
    /// The watchdog found it stalled and a restart is pending.
    pub stalled: bool,
    /// Restarts are paused after a crash loop.
    pub crash_loop: bool,
    /// An update is in progress.
    pub updating: bool,
}

/// The one-word status of a runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunnerStatus {
    /// Stopped by the user.
    Stopped,
    /// Not registered with a server yet (or the server forgot it).
    NotRegistered,
    /// Exited too often; restarts are paused.
    CrashLoop,
    /// An update is being installed.
    Updating,
    /// Running a job.
    Busy,
    /// Stalled: polls time out and no job runs; a restart is pending.
    Stalled,
    /// Waiting to be (re)started, or started but not yet accepted by the server.
    Starting,
    /// Running but its polls fail (server or network unreachable).
    Degraded,
    /// Running, accepted by the server and polling.
    Idle,
}

impl RunnerStatus {
    /// Whether the user should be told (tray badge, notification).
    pub fn needs_attention(self) -> bool {
        matches!(
            self,
            RunnerStatus::NotRegistered | RunnerStatus::CrashLoop | RunnerStatus::Stalled | RunnerStatus::Degraded
        )
    }
}

/// Polls older than this make a running runner [`RunnerStatus::Degraded`] (several poll intervals with a 30 s fetch
/// timeout).
pub const POLL_GRACE: Duration = Duration::from_secs(5 * 60);

/// Sums up the facts. The order of the checks is the order of importance: the most actionable state wins.
pub fn status(f: &HealthFacts, now: Timestamp) -> RunnerStatus {
    if !f.wanted && !f.process_alive {
        return RunnerStatus::Stopped;
    }
    if !f.has_registration || f.rejected {
        return RunnerStatus::NotRegistered;
    }
    if f.updating {
        return RunnerStatus::Updating;
    }
    if f.crash_loop {
        return RunnerStatus::CrashLoop;
    }
    if !f.process_alive {
        return RunnerStatus::Starting;
    }
    if f.job_running {
        return RunnerStatus::Busy;
    }
    if f.stalled {
        return RunnerStatus::Stalled;
    }
    if !f.declared {
        return RunnerStatus::Starting;
    }
    match f.last_good_poll {
        Some(t) if now.since(t) <= POLL_GRACE => RunnerStatus::Idle,
        _ => RunnerStatus::Degraded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> HealthFacts {
        HealthFacts {
            wanted: true,
            process_alive: true,
            has_registration: true,
            declared: true,
            last_good_poll: Some(Timestamp(0)),
            ..HealthFacts::default()
        }
    }

    #[test]
    fn a_registered_polling_runner_is_idle_or_busy() {
        assert_eq!(status(&healthy(), Timestamp(1000)), RunnerStatus::Idle);
        assert_eq!(
            status(
                &HealthFacts {
                    job_running: true,
                    ..healthy()
                },
                Timestamp(1000)
            ),
            RunnerStatus::Busy
        );
    }

    #[test]
    fn the_most_actionable_state_wins() {
        let now = Timestamp(1000);
        assert_eq!(
            status(
                &HealthFacts {
                    wanted: false,
                    process_alive: false,
                    ..healthy()
                },
                now
            ),
            RunnerStatus::Stopped
        );
        assert_eq!(
            status(
                &HealthFacts {
                    has_registration: false,
                    ..healthy()
                },
                now
            ),
            RunnerStatus::NotRegistered
        );
        assert_eq!(
            status(
                &HealthFacts {
                    rejected: true,
                    job_running: true,
                    ..healthy()
                },
                now
            ),
            RunnerStatus::NotRegistered
        );
        assert_eq!(
            status(
                &HealthFacts {
                    updating: true,
                    ..healthy()
                },
                now
            ),
            RunnerStatus::Updating
        );
        assert_eq!(
            status(
                &HealthFacts {
                    crash_loop: true,
                    process_alive: false,
                    ..healthy()
                },
                now
            ),
            RunnerStatus::CrashLoop
        );
        assert_eq!(
            status(
                &HealthFacts {
                    process_alive: false,
                    ..healthy()
                },
                now
            ),
            RunnerStatus::Starting
        );
        assert_eq!(
            status(
                &HealthFacts {
                    stalled: true,
                    ..healthy()
                },
                now
            ),
            RunnerStatus::Stalled
        );
        assert_eq!(
            status(
                &HealthFacts {
                    stalled: true,
                    job_running: true,
                    ..healthy()
                },
                now
            ),
            RunnerStatus::Busy
        );
        assert_eq!(
            status(
                &HealthFacts {
                    declared: false,
                    ..healthy()
                },
                now
            ),
            RunnerStatus::Starting
        );
    }

    #[test]
    fn stale_or_missing_polls_are_degraded() {
        assert_eq!(
            status(
                &HealthFacts {
                    last_good_poll: None,
                    ..healthy()
                },
                Timestamp(0)
            ),
            RunnerStatus::Degraded
        );
        let late = Timestamp(0).plus(POLL_GRACE).plus(Duration::from_millis(1));
        assert_eq!(status(&healthy(), late), RunnerStatus::Degraded);
        assert!(RunnerStatus::Degraded.needs_attention());
        assert!(!RunnerStatus::Idle.needs_attention());
    }
}
