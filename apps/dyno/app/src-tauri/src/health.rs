//! Dyno's service for the shell's service health (ADR 0010, rule 19): the Dyno agent, which measures everything. The status
//! bar chip lists it; the link to the agent reconnects by itself, so its repair only asks for a fresh look.
//!
//! The decision (what a status means for the chip) is a pure function over the link's [`AgentStatus`]; the hub only reports
//! it.

use crate::dto::{AgentState, AgentStatus};
use crate::link::{AgentLink, Stall};
use paddock_health::{Cause, Health, RepairAction, ServiceSpec};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

/// The Dyno agent.
pub const AGENT: &str = "dy.agent";

/// How long a repair waits for the link to come back before it says it did not.
const REPAIR_WAIT: Duration = Duration::from_secs(8);

/// Why the agent is down, or `None` when the status is not an outage (running, or still starting up).
pub fn agent_problem(status: &AgentStatus) -> Option<Cause> {
    match status.state {
        AgentState::Running | AgentState::Starting => None,
        AgentState::Restarting => Some(
            Cause::new("Dyno lost the connection to its agent and is reconnecting.")
                .hint("Measurements resume by themselves. If this stays, restart Dyno.")
                .detail(status.message.clone().unwrap_or_default())
                .code("dy.agent.restarting"),
        ),
        AgentState::Failed => Some(
            Cause::new("The Dyno agent is not running.")
                .hint("Dyno keeps trying to start it. If it does not come back, reinstall Dyno.")
                .detail(status.message.clone().unwrap_or_default())
                .code("dy.agent.failed"),
        ),
    }
}

/// Why a long wait for the agent is shown on the chip (a "still starting" with its cause; Retry acts on it,
/// [`AgentLink::retry`]). Not an error: the wait goes on, this is information and an action (ADR 0031).
pub fn stall_problem(stall: Stall) -> Cause {
    match stall {
        Stall::WaitingForService => Cause::new("Dyno is waiting for the agent that your system starts at login.")
            .hint(
                "If it does not come, choose Retry to start Dyno's own agent, and check Dyno in System Settings, Login Items (macOS) \
                 or the dyno-agent user service (Linux).",
            )
            .code("dy.agent.waiting-service"),
        Stall::AgentNotAnswering => Cause::new("The Dyno agent is still starting.")
            .hint("A slow disk or a busy computer can take a minute. If it stays, choose Retry to end it and start it again.")
            .code("dy.agent.starting"),
        Stall::ServiceNotAnswering => Cause::new("Dyno is waiting for the system to answer.")
            .hint("The login service or the agent's files are not answering. Choose Retry to look again.")
            .code("dy.agent.system-silent"),
        Stall::AgentKeepsFailing => Cause::new("The Dyno agent keeps failing to start.")
            .hint("Its log is in the Dyno data folder. Choose Retry to try again now; if it stays, reinstall Dyno.")
            .code("dy.agent.keeps-failing"),
        Stall::AgentPaused => Cause::new("The Dyno agent is paused. Paused by macOS or another app.")
            .hint("It continues by itself when it is allowed to run again (an app such as App Tamer or the system stopped it). Dyno waits and does not restart it.")
            .code("dy.agent.paused"),
        Stall::OtherAgentHolds => Cause::new("Another Dyno agent holds the data folder and has not answered.")
            .hint(
                "It may still be starting. If it stays, choose Retry; if the data folder is shared with another computer, quit Dyno there.",
            )
            .code("dy.agent.other-holds"),
    }
}

/// What the chip offers for a stall: Retry, except for a paused agent (nothing to retry: it continues by itself, ADR 0031).
pub fn stall_repair(stall: Stall) -> Option<RepairAction> {
    match stall {
        Stall::AgentPaused => None,
        _ => Some(RepairAction::Retry),
    }
}

/// What the repair reaches: the app, once it is set up (the specs are built before the app exists).
pub struct Hub<R: Runtime> {
    app: Arc<OnceLock<AppHandle<R>>>,
}

impl<R: Runtime> Clone for Hub<R> {
    fn clone(&self) -> Self {
        Hub { app: self.app.clone() }
    }
}

impl<R: Runtime> Default for Hub<R> {
    fn default() -> Self {
        Hub {
            app: Arc::new(OnceLock::new()),
        }
    }
}

impl<R: Runtime> Hub<R> {
    /// Connects the repair and the reports to the running app.
    pub fn attach(&self, app: &AppHandle<R>) {
        let _ = self.app.set(app.clone());
    }

    /// The services, in the order the status bar lists them.
    pub fn services(&self) -> Vec<ServiceSpec> {
        let hub = self.clone();
        vec![ServiceSpec::new(AGENT, "the Dyno agent").repair(move || hub.reconnect())]
    }

    /// The repair: the link reconnects with backoff on its own, so this waits (bounded) for it and says what it found.
    fn reconnect(&self) -> Result<(), Cause> {
        let link = self
            .app
            .get()
            .and_then(|a| a.try_state::<Arc<AgentLink>>().map(|l| l.inner().clone()))
            .ok_or_else(|| Cause::new("Dyno is still starting.").hint("Try again in a moment."))?;
        // A wait in progress (starting, the login service, another agent) acts on it; otherwise the link retries by itself.
        link.retry();
        // The wait is on the link's published state, not a poll. cap: a repair says "did not answer" after this long.
        if link.wait_connected(REPAIR_WAIT) {
            return Ok(());
        }
        Err(Cause::new("The Dyno agent did not answer.")
            .hint("Dyno keeps trying to start it. If it does not come back, reinstall Dyno.")
            .code("dy.agent.failed"))
    }

    /// Reports a long wait to the chip (with Retry), or that it is over.
    pub fn report_stall(&self, stall: Option<Stall>) {
        let Some(health) = self.app.get().and_then(|a| a.try_state::<Arc<Health>>().map(|h| h.inner().clone())) else {
            return;
        };
        match stall {
            Some(stall) => {
                health.down(AGENT, stall_problem(stall), stall_repair(stall));
            }
            None => {
                if health.is_in_outage(AGENT) {
                    health.ok(AGENT);
                }
            }
        }
    }

    /// Reports the link's status to the chip: an outage with its cause, or the recovery.
    pub fn report(&self, status: &AgentStatus) {
        let Some(health) = self.app.get().and_then(|a| a.try_state::<Arc<Health>>().map(|h| h.inner().clone())) else {
            return;
        };
        match agent_problem(status) {
            Some(cause) => {
                health.down(AGENT, cause, Some(RepairAction::Retry));
            }
            None => {
                if health.is_in_outage(AGENT) {
                    health.ok(AGENT);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(state: AgentState, message: Option<&str>) -> AgentStatus {
        AgentStatus::new(state, message.map(str::to_string))
    }

    #[test]
    fn a_running_or_starting_agent_is_not_an_outage() {
        assert!(agent_problem(&status(AgentState::Running, None)).is_none());
        assert!(agent_problem(&status(AgentState::Starting, None)).is_none());
    }

    #[test]
    fn a_long_wait_is_shown_with_its_cause_and_a_way_out_but_is_not_an_error_state_of_its_own() {
        for (stall, code, word) in [
            (Stall::WaitingForService, "dy.agent.waiting-service", "login"),
            (Stall::AgentNotAnswering, "dy.agent.starting", "Retry"),
            (Stall::OtherAgentHolds, "dy.agent.other-holds", "data folder"),
            (Stall::ServiceNotAnswering, "dy.agent.system-silent", "system"),
            (Stall::AgentKeepsFailing, "dy.agent.keeps-failing", "failing"),
        ] {
            let cause = stall_problem(stall);
            assert_eq!(cause.code.as_deref(), Some(code));
            assert!(
                cause.message.contains(word) || cause.hint.as_deref().is_some_and(|h| h.contains(word)),
                "{cause:?}"
            );
            assert!(cause.hint.as_deref().is_some_and(|h| h.contains("Retry")), "{cause:?}");
        }
        // A stopped agent is paused by macOS or another app: said so, and nothing to retry (ADR 0031).
        let paused = stall_problem(Stall::AgentPaused);
        assert_eq!(paused.code.as_deref(), Some("dy.agent.paused"));
        assert!(paused.message.contains("Paused by macOS or another app"), "{paused:?}");
        assert!(!paused.hint.as_deref().unwrap_or("").contains("Retry"), "{paused:?}");
        assert_eq!(stall_repair(Stall::AgentPaused), None);
        assert_eq!(stall_repair(Stall::AgentNotAnswering), Some(RepairAction::Retry));
        // The plain "still starting" status stays no outage: only the stall the link reports after a while is one.
        assert!(agent_problem(&status(AgentState::Starting, Some("still starting"))).is_none());
    }

    #[test]
    fn a_lost_or_failed_agent_tells_the_user_what_to_do() {
        let lost = agent_problem(&status(AgentState::Restarting, Some("pipe closed"))).unwrap();
        assert!(lost.hint.as_deref().is_some_and(|h| h.contains("restart Dyno")));
        assert_eq!(lost.detail.as_deref(), Some("pipe closed"));
        let failed = agent_problem(&status(AgentState::Failed, None)).unwrap();
        assert!(failed.hint.as_deref().is_some_and(|h| h.contains("reinstall")));
    }
}
