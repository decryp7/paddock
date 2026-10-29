//! Dyno's service for the shell's service health (ADR 0010, rule 19): the Dyno agent, which measures everything. The status
//! bar chip lists it; the link to the agent reconnects by itself, so its repair only asks for a fresh look.
//!
//! The decision (what a status means for the chip) is a pure function over the link's [`AgentStatus`]; the hub only reports
//! it.

use crate::dto::{AgentState, AgentStatus};
use crate::link::AgentLink;
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
        let end = std::time::Instant::now() + REPAIR_WAIT;
        while std::time::Instant::now() < end {
            if link.connected() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Err(Cause::new("The Dyno agent did not answer.")
            .hint("Dyno keeps trying to start it. If it does not come back, reinstall Dyno.")
            .code("dy.agent.failed"))
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
    fn a_lost_or_failed_agent_tells_the_user_what_to_do() {
        let lost = agent_problem(&status(AgentState::Restarting, Some("pipe closed"))).unwrap();
        assert!(lost.hint.as_deref().is_some_and(|h| h.contains("restart Dyno")));
        assert_eq!(lost.detail.as_deref(), Some("pipe closed"));
        let failed = agent_problem(&status(AgentState::Failed, None)).unwrap();
        assert!(failed.hint.as_deref().is_some_and(|h| h.contains("reinstall")));
    }
}
