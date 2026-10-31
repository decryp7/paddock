//! Pitwall's service around an update (`paddock_updater::ServiceHooks`). Before an install nothing needs stopping: the server
//! is its own program and restarts itself onto the new one once it is idle (it keeps serving meanwhile). After the first
//! start of a new version macOS may refuse the service registered by the previous build (its launch constraint is the old
//! code identity), so the registration is repaired then; the start-up repair loop in `lib.rs` keeps watching for a later
//! refusal. Windows (the installer's service) and Linux (a user unit) need nothing.

use crate::commands::SharedClient;
use paddock_updater::ServiceHooks;

/// The hooks over the app's client of the server (whether the service answers).
pub struct PitwallService {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    client: SharedClient,
}

impl PitwallService {
    /// The hooks for `client`.
    pub fn new(client: SharedClient) -> PitwallService {
        PitwallService { client }
    }
}

impl ServiceHooks for PitwallService {
    fn on_version_change(&self, previous: Option<&str>, current: &str) -> Result<(), String> {
        eprintln!("Pitwall: first start of {current} (before: {})", previous.unwrap_or("none"));
        #[cfg(target_os = "macos")]
        {
            use crate::service_repair::RepairOutcome;
            let client = self.client.clone();
            let answers =
                || tauri::async_runtime::block_on(crate::commands::forward(client.as_ref(), "overview", serde_json::Value::Null)).is_ok();
            match crate::service_install::repair_registration(&answers) {
                Some(RepairOutcome::Failed(e)) => return Err(format!("the service could not be registered again: {e}")),
                Some(RepairOutcome::Recovered | RepairOutcome::NeedsApproval | RepairOutcome::Cancelled) | None => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server_client::fake::FakeServer;
    use std::sync::Arc;

    /// Before an install nothing is stopped (the server restarts itself onto the new program), and a version change
    /// off macOS succeeds without touching the system.
    #[test]
    fn nothing_is_stopped_before_an_install() {
        let hooks = PitwallService::new(Arc::new(FakeServer::new()));
        assert_eq!(hooks.before_install(), Ok(()));
        #[cfg(not(target_os = "macos"))]
        assert_eq!(hooks.on_version_change(Some("1.0.3"), "1.1.0"), Ok(()));
    }
}
