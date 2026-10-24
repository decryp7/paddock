//! The Tauri adapter of `paddock-service` (built with the `tauri` feature; plan X4).
//!
//! - [`Setup`] names the app's [`Manager`], the `paddock-health` entry it keeps current (declared by the app in its
//!   health services) and the start-up checks; [`install`] manages the [`ServiceControl`] and starts them.
//!   `paddock-shell` calls it from `Builder::service`.
//! - [`COMMANDS`](crate::COMMANDS): `service_status` (the look, on the blocking pool), `service_repair` and
//!   `service_install` (return at once; the outcome arrives as [`EVENT`](crate::EVENT_NAME)), `open_login_items`.
//! - Every update (a look, a repair's end, a start-up check) goes to every window on [`EVENT`](crate::EVENT_NAME), and
//!   to the health registry, whose Repair is this service's repair (rule 19).

use crate::control::{
    Request, ServiceCommandError, ServiceControl, ServiceErrorCode, ServiceUpdate,
};
use crate::manager::Manager;
use crate::status::ServiceStatus;
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Emitter, Manager as _, Runtime, State};
use paddock_health::Health;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The event every update is sent on.
pub const EVENT: &str = crate::EVENT_NAME;

/// What the app sets up.
pub struct Setup {
    manager: Manager,
    health: Option<String>,
    checks: Option<(Duration, Duration, u32)>,
}

impl Setup {
    /// The service `manager` controls, with the start-up checks on (3 s after start, then every minute, 8 in all).
    pub fn new(manager: Manager) -> Setup {
        Setup {
            manager,
            health: None,
            checks: Some((Duration::from_secs(3), Duration::from_secs(60), 8)),
        }
    }

    /// Keeps the health entry `service` current and makes this its repair (declare it in the app's health services).
    pub fn health(mut self, service: &str) -> Setup {
        self.health = Some(service.to_string());
        self
    }

    /// Other start-up checks (`times` 0: none; tests).
    pub fn checks(mut self, first: Duration, every: Duration, times: u32) -> Setup {
        self.checks = (times > 0).then_some((first, every, times));
        self
    }
}

/// The managed state: the control and its start-up checks.
pub struct Service {
    control: Arc<ServiceControl>,
    checks: Mutex<Option<crate::control::StartChecks>>,
}

impl Service {
    /// The control (for the app's own code).
    pub fn control(&self) -> &Arc<ServiceControl> {
        &self.control
    }

    /// Stops the start-up checks and asks a running repair to stop where nothing is left half-done (at app exit).
    pub fn shutdown(&self) {
        self.control.cancel();
        if let Some(c) = self
            .checks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            c.stop();
        }
    }
}

/// Manages the service. With [`Setup::health`], the health registry must be managed first (`paddock_health::tauri::init`,
/// which the shell runs); a service it does not declare is reported and left out of health.
pub fn install<R: Runtime>(app: &AppHandle<R>, setup: Setup) -> Arc<Service> {
    if let Some(existing) = app.try_state::<Arc<Service>>() {
        return existing.inner().clone();
    }
    let handle = app.clone();
    let emit: crate::control::Emit = Arc::new(move |u: &ServiceUpdate| {
        // A failed emit means the app is closing; nothing is left to tell.
        let _ = handle.emit(EVENT, u);
    });
    let health = app.try_state::<Arc<Health>>().map(|h| h.inner().clone());
    let Setup {
        manager,
        health: id,
        checks,
    } = setup;
    let declared = match (id, health) {
        (Some(id), Some(h)) if h.get(&id).is_some() => Some((id, h)),
        (Some(id), Some(_)) => {
            log::error!(service = id.as_str(); "the service's health entry is not declared in the app's health services");
            None
        }
        (Some(id), None) => {
            log::warn!(service = id.as_str(); "the service's health entry is set but no health registry is managed");
            None
        }
        (None, _) => None,
    };
    // The id is declared, so wiring cannot be refused; the plain control is the fallback all the same.
    let control = match declared {
        Some((id, h)) => ServiceControl::with_health_or_plain(manager, emit, h, &id),
        None => ServiceControl::new(manager, emit),
    };
    let checks = checks
        .and_then(|(first, every, times)| control.spawn_start_checks(first, every, times).ok());
    let state = Arc::new(Service {
        control,
        checks: Mutex::new(checks),
    });
    app.manage(state.clone());
    state
}

type Managed<'a> = State<'a, Arc<Service>>;

fn not_set_up() -> ServiceCommandError {
    ServiceCommandError {
        code: ServiceErrorCode::NotSetUp,
        message: "This app has no background service.".into(),
        retryable: false,
    }
}

/// The service now (the OS is asked on the blocking pool, with bounded helpers).
#[::tauri::command]
pub async fn service_status(state: Managed<'_>) -> Result<ServiceStatus, ServiceCommandError> {
    let control = state.control.clone();
    ::tauri::async_runtime::spawn_blocking(move || control.status())
        .await
        .ok()
        .flatten()
        .ok_or_else(not_set_up)
}

/// Repairs the registration; returns at once, the outcome arrives as an [`EVENT`].
#[::tauri::command]
pub async fn service_repair(state: Managed<'_>) -> Result<(), ServiceCommandError> {
    state.control.request(Request::Repair)
}

/// Registers the service (the user's consent); returns at once, the outcome arrives as an [`EVENT`].
#[::tauri::command]
pub async fn service_install(state: Managed<'_>) -> Result<bool, ServiceCommandError> {
    state.control.request(Request::Install).map(|()| true)
}

/// Opens System Settings > General > Login Items (macOS); `false` where the OS has no such page. The user switches the
/// item; the app never does.
#[::tauri::command]
pub async fn open_login_items(state: Managed<'_>) -> Result<bool, ServiceCommandError> {
    state.control.open_login_items()
}

/// The invoke handler of [`COMMANDS`](crate::COMMANDS); wrap it in the shell's guard.
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![
        service_status,
        service_repair,
        service_install,
        open_login_items
    ]
}

#[cfg(test)]
mod tests;
