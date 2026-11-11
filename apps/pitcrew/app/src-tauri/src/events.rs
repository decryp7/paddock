//! Pushed events from the service: one subscription thread forwards them to the UI (`pitcrew://event`), keeps the
//! tray current and shows native notifications. It reconnects with backoff, and the UI and tray say when the
//! service is unavailable. No polling.

use crate::commands::SharedClient;
use crate::dto::{BackendEvent, ConnectionState, StatusReply};
use crate::engines::Statuses;
use crate::service_client::{subscribe, ClientError};
#[cfg(windows)]
use crate::service_client::{subscribe_engine, Engine};
use pitcrew_core::names::RunnerName;
use pitcrew_core::notify::Notification;
use pitcrew_core::protocol::{Request, Response, ServiceEvent};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_notification::NotificationExt;

/// The Tauri event the UI listens to.
pub const EVENT: &str = "pitcrew://event";
/// Asks the UI to show a page (payload: a route such as `/logs/build-pc`).
pub const NAVIGATE: &str = "pitcrew://navigate";

/// The latest status and connection, for commands and the tray.
#[derive(Default)]
pub struct Latest {
    status: Mutex<Option<StatusReply>>,
    connection: Mutex<Option<ConnectionState>>,
}

impl Latest {
    /// The last status pushed.
    pub fn status(&self) -> Option<StatusReply> {
        self.status.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// The connection now.
    pub fn connection(&self) -> ConnectionState {
        self.connection
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .unwrap_or(ConnectionState::Connecting)
    }

    fn set_status(&self, s: StatusReply) {
        *self.status.lock().unwrap_or_else(|p| p.into_inner()) = Some(s);
    }

    fn set_connection(&self, c: ConnectionState) {
        *self.connection.lock().unwrap_or_else(|p| p.into_inner()) = Some(c);
    }
}

/// The page a tray action opens, if it opens one: the runner's logs, processes or configuration.
pub fn page_route(action: &str, runner: &str) -> Option<String> {
    match action {
        "logs" => Some(format!("/logs/{runner}")),
        "processes" => Some(format!("/runners/{runner}/processes")),
        "config" => Some(format!("/runners/{runner}/config")),
        _ => None,
    }
}

/// The last page the shell asked the window to open, until a window takes it (one created by that same action only
/// starts listening after it was asked).
#[derive(Default)]
pub struct PendingNav(Mutex<Option<String>>);

impl PendingNav {
    /// Remembers `route`.
    pub fn set(&self, route: String) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(route);
    }

    /// The remembered route, once.
    pub fn take(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).take()
    }
}

/// The tray's quick actions.
pub struct Actions {
    client: SharedClient,
}

impl Actions {
    /// Actions over `client`.
    pub fn new(client: SharedClient) -> Self {
        Actions { client }
    }

    /// Runs a tray action off the UI thread; failures become a notification.
    pub fn run<R: Runtime>(&self, app: &AppHandle<R>, action: &str, runner: &str) {
        let Ok(name) = RunnerName::parse(runner) else { return };
        if let Some(route) = page_route(action, name.as_str()) {
            // Kept until the window asks: a window the tray just created may not listen yet.
            if let Some(p) = app.try_state::<PendingNav>() {
                p.set(route.clone());
            }
            let _ = app.emit(NAVIGATE, route);
            return;
        }
        let req = match action {
            "start" => Request::Start { runner: name },
            "stop" => Request::Stop { runner: name },
            "restart" => Request::Restart { runner: name },
            "drain" => Request::Drain { runner: name },
            "resume" => Request::Resume { runner: name },
            "applyUpdate" => Request::ApplyUpdate { runner: name },
            "clean" => Request::CleanDisk { runner: name },
            _ => return,
        };
        let client = self.client.clone();
        let app = app.clone();
        std::thread::spawn(move || {
            let result = client.request(req);
            let problem = match result {
                Ok(Response::Error(e)) => Some(e.message),
                Err(e) => Some(e.to_string()),
                Ok(_) => None,
            };
            if let Some(message) = problem {
                let _ = app.notification().builder().title("Pitcrew").body(message).show();
            }
        });
    }
}

fn emit<R: Runtime>(app: &AppHandle<R>, e: BackendEvent) {
    let _ = app.emit(EVENT, e);
}

fn connection<R: Runtime>(app: &AppHandle<R>, c: ConnectionState) {
    let latest = app.state::<Latest>();
    if latest.connection() == c {
        return;
    }
    latest.set_connection(c.clone());
    let connected = matches!(c, ConnectionState::Connected { .. });
    let runners = latest.status().map(|s| s.runners).unwrap_or_default();
    crate::tray::update(app, if connected { &runners } else { &[] }, connected);
    emit(app, BackendEvent::Connection { connection: c });
}

fn notify<R: Runtime>(app: &AppHandle<R>, n: &Notification) {
    let _ = app.notification().builder().title(&n.title).body(&n.body).show();
    if let Ok(dir) = app.path().app_data_dir() {
        crate::channels::dispatch_in_background(dir, n);
    }
}

/// A key of what the tray menu shows, so it is rebuilt only when that changes (rebuilding closes an open menu).
fn menu_key(s: &StatusReply) -> String {
    s.runners
        .iter()
        .map(|r| {
            format!(
                "{}|{:?}|{:?}|{}|{}|{:?}",
                r.name,
                r.status,
                r.current_job.as_ref().map(|j| (&j.info.repository, &j.info.job)),
                r.update_target.is_some(),
                r.running_since.is_some(),
                r.disk.map(|d| d.low)
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Shows a merged status: the tray (rebuilt only when what it shows changed), the latest status and the window.
fn publish_status<R: Runtime>(a: &AppHandle<R>, status: StatusReply) {
    let key = menu_key(&status);
    if a.state::<Statuses>().menu_changed(&key) {
        crate::tray::update(a, &status.runners, true);
    }
    a.state::<Latest>().set_status(status.clone());
    emit(a, BackendEvent::Status { status });
}

/// The per-user engine's subscription (Windows): its runners join the list; it not running is not an error (nobody has runners
/// there yet), so it never changes the connection state, and it is started again whenever it is found not running.
#[cfg(windows)]
fn start_user_engine_events<R: Runtime>(app: AppHandle<R>) {
    std::thread::Builder::new()
        .name("pitcrew-user-engine-events".into())
        .spawn(move || {
            let mut backoff = Duration::from_secs(2);
            loop {
                // What is wrong with the engine, if anything, goes to the window with the service's other problems.
                let mut problem = crate::service_install::start_user_engine().err().map(|e| e.message);
                let a = app.clone();
                let result = subscribe_engine(
                    Engine::User,
                    |_| {},
                    |r| {
                        if let Response::Event { event } = r {
                            match event {
                                ServiceEvent::Status { runners, service } => {
                                    let merged = a.state::<Statuses>().set_user(Some(StatusReply { runners, service }));
                                    if let Some(merged) = merged {
                                        publish_status(&a, merged);
                                    }
                                }
                                ServiceEvent::Notification { notification } => {
                                    notify(&a, &notification);
                                    emit(&a, BackendEvent::Notification { notification });
                                }
                            }
                        }
                        true
                    },
                );
                if let Err(ClientError::Unavailable(m)) = &result {
                    if m != crate::service_client::USER_ENGINE_NOT_RUNNING {
                        problem.get_or_insert_with(|| m.clone());
                    }
                }
                let merged = {
                    let statuses = app.state::<Statuses>();
                    statuses.set_user(None);
                    statuses.set_user_problem(problem)
                };
                if let Some(merged) = merged {
                    publish_status(&app, merged);
                }
                if result.is_ok() {
                    backoff = Duration::from_secs(2);
                }
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        })
        .ok();
}

/// Starts the subscription thread.
pub fn start<R: Runtime>(app: AppHandle<R>) {
    #[cfg(windows)]
    start_user_engine_events(app.clone());
    std::thread::Builder::new()
        .name("pitcrew-events".into())
        .spawn(move || {
            let mut backoff = Duration::from_secs(1);
            loop {
                connection(&app, ConnectionState::Connecting);
                let a = app.clone();
                let result = subscribe(
                    |v| {
                        connection(&a, ConnectionState::Connected { service_version: v });
                    },
                    |r| {
                        if let Response::Event { event } = r {
                            match event {
                                ServiceEvent::Status { runners, service } => {
                                    let merged = a.state::<Statuses>().set_system(StatusReply { runners, service });
                                    publish_status(&a, merged);
                                }
                                ServiceEvent::Notification { notification } => {
                                    notify(&a, &notification);
                                    emit(&a, BackendEvent::Notification { notification });
                                }
                            }
                        }
                        true
                    },
                );
                app.state::<Statuses>().reset_menu();
                match result {
                    Err(ClientError::Mismatch(message)) => connection(&app, ConnectionState::Mismatch { message }),
                    Err(e) => connection(&app, ConnectionState::Unavailable { message: e.to_string() }),
                    Ok(()) => {}
                }
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(15));
                if matches!(app.state::<Latest>().connection(), ConnectionState::Connected { .. }) {
                    backoff = Duration::from_secs(1);
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_status_survives_a_poisoned_lock() {
        let l = Latest::default();
        assert_eq!(l.connection(), ConnectionState::Connecting);
        let l = std::sync::Arc::new(l);
        let l2 = l.clone();
        let _ = std::thread::spawn(move || {
            let _g = l2.connection.lock().unwrap();
            panic!("poison it");
        })
        .join();
        l.set_connection(ConnectionState::Unavailable { message: "x".into() });
        assert!(matches!(l.connection(), ConnectionState::Unavailable { .. }));
        assert!(l.status().is_none());
    }
}

#[cfg(test)]
mod nav_tests {
    use super::*;

    #[test]
    fn tray_pages_have_routes_and_a_pending_route_is_taken_once() {
        assert_eq!(page_route("logs", "mac").as_deref(), Some("/logs/mac"));
        assert_eq!(page_route("processes", "mac").as_deref(), Some("/runners/mac/processes"));
        assert_eq!(page_route("config", "mac").as_deref(), Some("/runners/mac/config"));
        assert_eq!(page_route("clean", "mac"), None);
        let p = PendingNav::default();
        p.set("/logs/mac".into());
        assert_eq!(p.take().as_deref(), Some("/logs/mac"));
        assert_eq!(p.take(), None);
    }
}
