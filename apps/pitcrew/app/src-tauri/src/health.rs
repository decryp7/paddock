//! The Pitcrew service as a shell health service (ADR 0010, rule 19). The event thread reports the connection
//! ([`report`]); the shell shows the status-bar chip and one notification per cause, and its Repair runs [`repair`] on a
//! thread of its own, so it never goes through the service it repairs: on macOS it replaces the launchd job an update
//! left behind (unregister, bootout, wait, register; rule 24), elsewhere it installs the service again (the OS asks).

use crate::dto::ConnectionState;
use crate::service_client::{LocalServiceClient, ServiceClient};
use paddock_health::{Cause, Health, RepairAction, ServiceSpec};
use pitcrew_core::protocol::Request;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Runtime};

/// The service's id in the health registry (and `defineHealthService` in the UI).
pub const SERVICE: &str = "pc.service";
/// How long a repair waits for the service to answer again.
const WAIT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_secs(2);

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// The app's data folder, known once the app is set up (the repair records the version there).
pub fn set_data_dir(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
}

/// The service entry for the shell's builder.
pub fn spec() -> ServiceSpec {
    ServiceSpec::new(SERVICE, "the Pitcrew service")
        .starting()
        .repair(|| repair(DATA_DIR.get().map(PathBuf::as_path)))
}

/// What the connection means for the chip: `None` while connecting (not an outage).
pub fn cause_of(c: &ConnectionState) -> Option<(Cause, RepairAction)> {
    match c {
        ConnectionState::Connecting | ConnectionState::Connected { .. } => None,
        ConnectionState::Unavailable { message } => Some((
            Cause::new("Pitcrew cannot reach its background service, so runners are not being managed.")
                .hint("Choose Repair to register and start the service again.")
                .detail(message.clone())
                .code("pc.service.unavailable"),
            RepairAction::Restart,
        )),
        ConnectionState::Mismatch { message } => Some((
            Cause::new("Pitcrew and its background service are different versions.")
                .hint("Choose Repair to start the service that belongs to this version; if it stays, reinstall Pitcrew.")
                .detail(message.clone())
                .code("pc.service.mismatch"),
            RepairAction::Restart,
        )),
    }
}

/// Tells the registry how the connection is (called by the event thread on every change).
pub fn report<R: Runtime>(app: &AppHandle<R>, c: &ConnectionState) {
    let Some(health) = app.try_state::<Arc<Health>>() else { return };
    match (c, cause_of(c)) {
        (ConnectionState::Connected { .. }, _) => {
            health.ok(SERVICE);
        }
        (_, Some((cause, action))) => {
            health.down(SERVICE, cause, Some(action));
        }
        _ => {}
    }
}

fn repair(data_dir: Option<&std::path::Path>) -> Result<(), Cause> {
    let failed = |m: String| {
        Cause::new("The background service could not be repaired.")
            .hint(m)
            .code("pc.service.repair")
    };
    if crate::service_install::state().installed {
        // Registered but not answering: macOS replaces the job an update left behind (unregister, bootout, wait, register;
        // rule 24); elsewhere the registration is installed again, which also starts it.
        #[cfg(target_os = "macos")]
        {
            let dir = data_dir.ok_or_else(|| failed("Pitcrew has no data folder to record the repair in.".into()))?;
            crate::service_install::request_repair(dir).map_err(|e| failed(e.message))?;
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = data_dir;
            crate::service_install::install().map_err(|e| failed(e.message))?;
        }
    } else {
        // Never installed (a first run): install it; the OS asks for permission.
        crate::service_install::install().map_err(|e| failed(e.message))?;
        if let Some(dir) = data_dir {
            crate::service_install::note_registered(dir);
        }
    }
    wait_until_answers(&LocalServiceClient, WAIT, POLL).then_some(()).ok_or_else(|| {
        failed("The service did not start. Check System Settings > Login Items (macOS) or the Services list, then try again.".into())
    })
}

/// Polls until the service answers, at most `limit`.
pub fn wait_until_answers(client: &dyn ServiceClient, limit: Duration, poll: Duration) -> bool {
    let start = Instant::now();
    loop {
        if client.request(Request::Status).is_ok() {
            return true;
        }
        if start.elapsed() >= limit {
            return false;
        }
        std::thread::sleep(poll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_client::ClientError;
    use pitcrew_core::protocol::Response;

    #[test]
    fn only_a_lost_service_is_an_outage() {
        assert!(cause_of(&ConnectionState::Connecting).is_none());
        assert!(cause_of(&ConnectionState::Connected {
            service_version: "1".into()
        })
        .is_none());
        let (c, a) = cause_of(&ConnectionState::Unavailable { message: "x".into() }).unwrap();
        assert_eq!(a, RepairAction::Restart);
        assert_eq!(c.code.as_deref(), Some("pc.service.unavailable"));
        assert!(cause_of(&ConnectionState::Mismatch { message: "x".into() }).is_some());
    }

    struct Later(std::sync::atomic::AtomicU32, u32);
    impl ServiceClient for Later {
        fn request(&self, _: Request) -> Result<Response, ClientError> {
            if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= self.1 {
                Ok(Response::Ok)
            } else {
                Err(ClientError::Unavailable("down".into()))
            }
        }
    }

    #[test]
    fn a_repair_waits_for_the_service_but_not_forever() {
        assert!(wait_until_answers(
            &Later(Default::default(), 2),
            Duration::from_secs(5),
            Duration::ZERO
        ));
        assert!(!wait_until_answers(
            &Later(Default::default(), u32::MAX),
            Duration::ZERO,
            Duration::ZERO
        ));
    }
}
