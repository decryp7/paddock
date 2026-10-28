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
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

/// The service's id in the health registry (and `defineHealthService` in the UI).
pub const SERVICE: &str = "pc.service";
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
///
/// `installed` is whether the OS has the service registered: a service that was never installed is a different outage from one
/// that does not answer, and its repair names the prompt the install brings.
pub fn cause_of(c: &ConnectionState, installed: bool) -> Option<(Cause, RepairAction)> {
    match c {
        ConnectionState::Connecting | ConnectionState::Connected { .. } => None,
        ConnectionState::Unavailable { message } if !installed => Some((
            Cause::new("The Pitcrew service is not installed, so runners are not being managed.")
                .hint(install_hint())
                .detail(message.clone())
                .code("pc.service.not-installed"),
            RepairAction::Restart,
        )),
        ConnectionState::Unavailable { message } => Some((
            Cause::new("Pitcrew cannot reach its background service, so runners are not being managed.")
                .hint("Choose Restart to register and start the service again (it is installed first when it never was).")
                .detail(message.clone())
                .code("pc.service.unavailable"),
            RepairAction::Restart,
        )),
        ConnectionState::Mismatch { message } => Some((
            Cause::new("Pitcrew and its background service are different versions.")
                .hint("Choose Restart to start the service that belongs to this version; if it stays, reinstall Pitcrew.")
                .detail(message.clone())
                .code("pc.service.mismatch"),
            RepairAction::Restart,
        )),
    }
}

/// What Restart does for a service that was never installed, and the OS prompt it brings.
fn install_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "Choose Restart to install it; macOS asks you to approve Pitcrew in System Settings > General > Login Items."
    } else {
        "Choose Restart to install it; Windows asks for administrator permission (a UAC prompt)."
    }
}

/// Tells the registry how the connection is (called by the event thread on every change).
pub fn report<R: Runtime>(app: &AppHandle<R>, c: &ConnectionState) {
    let installed = !matches!(c, ConnectionState::Unavailable { .. }) || crate::service_install::state().installed;
    report_with(app, c, installed);
}

/// [`report`] with the OS's answer to "is the service registered" given (tests never ask the machine, rule 14).
pub fn report_with<R: Runtime>(app: &AppHandle<R>, c: &ConnectionState, installed: bool) {
    let Some(health) = app.try_state::<Arc<Health>>() else { return };
    match (c, cause_of(c, installed)) {
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
    wait_until_answers(&LocalServiceClient, || crate::service_install::state().installed, POLL)
        .then_some(())
        .ok_or_else(|| {
            failed(
                "The service is not registered any more. Check System Settings > Login Items (macOS) or the Services list, then try again."
                    .into(),
            )
        })
}

/// Polls until the service answers. There is no time limit (rule 19: a running repair is never abandoned; the shell shows
/// "Still working"); it ends early only when `can_still_come_up` says the registration is gone.
pub fn wait_until_answers(client: &dyn ServiceClient, can_still_come_up: impl Fn() -> bool, poll: Duration) -> bool {
    loop {
        if client.request(Request::Status).is_ok() {
            return true;
        }
        if !can_still_come_up() {
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
        assert!(cause_of(&ConnectionState::Connecting, true).is_none());
        assert!(cause_of(
            &ConnectionState::Connected {
                service_version: "1".into()
            },
            true
        )
        .is_none());
        let (c, a) = cause_of(&ConnectionState::Unavailable { message: "x".into() }, true).unwrap();
        assert_eq!(a, RepairAction::Restart);
        assert_eq!(c.code.as_deref(), Some("pc.service.unavailable"));
        assert!(cause_of(&ConnectionState::Mismatch { message: "x".into() }, true).is_some());
    }

    #[test]
    fn a_service_that_was_never_installed_says_so_and_names_the_prompt_the_install_brings() {
        let (c, a) = cause_of(&ConnectionState::Unavailable { message: "x".into() }, false).unwrap();
        assert_eq!(a, RepairAction::Restart);
        assert_eq!(c.code.as_deref(), Some("pc.service.not-installed"));
        let text = format!("{c:?}");
        assert!(text.contains("not installed"), "{text}");
        assert!(text.contains("UAC") || text.contains("System Settings"), "{text}");
        // An installed service that does not answer is not "not installed".
        let (c, _) = cause_of(&ConnectionState::Unavailable { message: "x".into() }, true).unwrap();
        assert_eq!(c.code.as_deref(), Some("pc.service.unavailable"));
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
    fn a_repair_waits_for_the_service_for_as_long_as_it_can_still_come_up() {
        // No time limit (ER-19: the UI says "Still working", it never gives up on a running repair): it ends when the
        // service answers, or when there is nothing left to wait for (the registration is gone).
        assert!(wait_until_answers(&Later(Default::default(), 2), || true, Duration::ZERO));
        assert!(!wait_until_answers(&Later(Default::default(), u32::MAX), || false, Duration::ZERO));
    }
}
