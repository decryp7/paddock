//! Two engines on Windows (ADR 0004, second amendment): the SYSTEM **service** manages the runners whose folders only
//! administrators can change (`session = service`), and the **per-user engine**, the same engine started in the user's own
//! session, manages the runners in folders the user can change (`session = interactive`). The app talks to both and the window sees
//! one list. This module is the pure part: where a request goes ([`RoutingClient`]) and how the two statuses become one
//! ([`Statuses`]). Elsewhere there is one engine and none of this is used.

use crate::dto::StatusReply;
use crate::service_client::{ClientError, ServiceClient};
use pitcrew_core::config::{Config, SessionMode};
use pitcrew_core::names::RunnerName;
use pitcrew_core::protocol::{ErrorCode, Request, Response};
use std::sync::{Arc, Mutex};

/// Sends each request to the engine that owns what it is about, and merges the answers that are about both.
///
/// - `AddRunner`: by the runner's session (service: the SYSTEM service, an administrator's choice; interactive: the user's engine).
/// - A request about one runner (`Request::target_runner`): the engine that has it (the user's engine is asked first; a runner
///   it does not know belongs to the service).
/// - `Status` and `GetSettings`: both engines, merged (one that is not running is skipped).
/// - `SaveSettings`: split by session; the service is asked only when its part changed, so saving a user's runner never raises a
///   Windows permission prompt.
/// - Everything else (diagnostics, the container engine, machine metrics): the service.
pub struct RoutingClient {
    system: Arc<dyn ServiceClient>,
    user: Arc<dyn ServiceClient>,
}

impl RoutingClient {
    /// A router over the service's client and the user engine's.
    pub fn new(system: Arc<dyn ServiceClient>, user: Arc<dyn ServiceClient>) -> Self {
        RoutingClient { system, user }
    }

    fn by_session(&self, session: SessionMode) -> &dyn ServiceClient {
        match session {
            SessionMode::Service => &*self.system,
            SessionMode::Interactive => &*self.user,
        }
    }

    /// The engine that has the runner: the user's if it lists it, else the service.
    fn engine_of(&self, runner: &RunnerName) -> &dyn ServiceClient {
        match self.user.request(Request::Status) {
            Ok(Response::Status { runners, .. }) if runners.iter().any(|r| &r.name == runner) => &*self.user,
            _ => &*self.system,
        }
    }

    fn merged_status(&self) -> Result<Response, ClientError> {
        let system = self.system.request(Request::Status);
        let user = self.user.request(Request::Status);
        Ok(match (system, user) {
            (Ok(Response::Status { mut runners, service }), Ok(Response::Status { runners: mine, .. })) => {
                runners.extend(mine);
                Response::Status { runners, service }
            }
            (Ok(answer), _) => answer,
            (Err(_), Ok(answer @ Response::Status { .. })) => answer,
            (Err(e), _) => return Err(e),
        })
    }

    fn merged_settings(&self) -> Result<Response, ClientError> {
        let system = self.system.request(Request::GetSettings);
        let user = self.user.request(Request::GetSettings);
        Ok(match (system, user) {
            (
                Ok(Response::Settings {
                    mut config,
                    mut problems,
                    read_only,
                }),
                Ok(Response::Settings {
                    config: mine,
                    problems: more,
                    ..
                }),
            ) => {
                config.runners.extend(mine.runners);
                problems.extend(more);
                Response::Settings {
                    config,
                    problems,
                    read_only,
                }
            }
            (Ok(answer), _) => answer,
            (Err(_), Ok(answer @ Response::Settings { .. })) => answer,
            (Err(e), _) => return Err(e),
        })
    }

    /// The configuration of each engine: the runners of its session, the defaults for both.
    fn split(config: &Config) -> (Config, Config) {
        let mut system = config.clone();
        let mut user = config.clone();
        system.runners.retain(|r| r.session == SessionMode::Service);
        user.runners.retain(|r| r.session == SessionMode::Interactive);
        (system, user)
    }

    fn save_split(&self, config: &Config) -> Result<Response, ClientError> {
        let (system, user) = Self::split(config);
        // The service's part first, and only when it changed (it needs an administrator's prompt).
        let current = match self.system.request(Request::GetSettings) {
            Ok(Response::Settings { config, .. }) => Some(config),
            _ => None,
        };
        if current.as_ref() != Some(&system) {
            if let r @ Response::Error(_) = self.system.request(Request::SaveSettings { config: system })? {
                return Ok(r);
            }
        }
        self.user.request(Request::SaveSettings { config: user })
    }
}

impl ServiceClient for RoutingClient {
    fn request(&self, request: Request) -> Result<Response, ClientError> {
        match &request {
            Request::AddRunner { runner } => self.by_session(runner.session).request(request),
            Request::Status => self.merged_status(),
            Request::GetSettings => self.merged_settings(),
            Request::SaveSettings { config } => self.save_split(config),
            other => match other.target_runner() {
                Some(runner) => {
                    let engine = self.engine_of(runner);
                    engine.request(request)
                }
                None => self.system.request(request),
            },
        }
    }
}

/// The latest status of each engine, merged into the one list the window and the tray show.
#[derive(Default)]
pub struct Statuses {
    system: Mutex<Option<StatusReply>>,
    user: Mutex<Option<StatusReply>>,
    menu: Mutex<String>,
}

impl Statuses {
    fn lock(m: &Mutex<Option<StatusReply>>) -> std::sync::MutexGuard<'_, Option<StatusReply>> {
        m.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Records the service's status and returns the merged one.
    pub fn set_system(&self, status: StatusReply) -> StatusReply {
        *Self::lock(&self.system) = Some(status);
        self.merged().unwrap_or_else(unreachable_empty)
    }

    /// Records the user engine's status (`None`: it went away) and returns the merged one, if any engine has reported.
    pub fn set_user(&self, status: Option<StatusReply>) -> Option<StatusReply> {
        *Self::lock(&self.user) = status;
        self.merged()
    }

    /// Whether the tray menu must be rebuilt for `key` (what it shows changed): rebuilding closes an open menu, so only then.
    pub fn menu_changed(&self, key: &str) -> bool {
        let mut last = self.menu.lock().unwrap_or_else(|p| p.into_inner());
        if *last == key {
            false
        } else {
            *last = key.to_string();
            true
        }
    }

    /// Forgets the menu shown (a reconnect starts from nothing).
    pub fn reset_menu(&self) {
        self.menu.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }

    fn merged(&self) -> Option<StatusReply> {
        let system = Self::lock(&self.system).clone();
        let user = Self::lock(&self.user).clone();
        match (system, user) {
            (Some(mut s), Some(u)) => {
                s.runners.extend(u.runners);
                Some(s)
            }
            (Some(s), None) => Some(s),
            (None, Some(u)) => Some(u),
            (None, None) => None,
        }
    }
}

fn empty_service_info() -> pitcrew_core::protocol::ServiceInfo {
    pitcrew_core::protocol::ServiceInfo {
        version: String::new(),
        started_at: pitcrew_core::time::Timestamp(0),
        data_dir: String::new(),
        config_problems: vec![],
        read_only: false,
        machine: None,
        own_cpu_permille: 0,
        own_mem_bytes: 0,
    }
}

fn unreachable_empty() -> StatusReply {
    StatusReply {
        runners: vec![],
        service: empty_service_info(),
    }
}

/// Whether an error answer means "this engine does not know that runner" (so the other engine may).
pub fn is_unknown_runner(r: &Response) -> bool {
    matches!(r, Response::Error(e) if e.code == ErrorCode::UnknownRunner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitcrew_core::protocol::{RunnerSnapshot, ServiceError};

    /// A fake engine: its runners, its configuration, and what it was asked.
    struct Fake {
        runners: Vec<String>,
        config: Mutex<Config>,
        asked: Mutex<Vec<String>>,
        down: bool,
    }

    fn snapshot(name: &str) -> RunnerSnapshot {
        serde_json::from_value(serde_json::json!({
            "name": name, "kind": "gitea-runner", "status": "idle", "enabled": true, "pending": null,
            "session": {"kind": "notRunning"}, "runningSince": null, "nextStartAt": null, "restarts24h": 0, "problem": null,
            "currentJob": null, "currentVersion": null, "latestVersion": null, "checkedAt": null, "updateTarget": null,
            "lastUpdate": null, "updatePhase": {"phase": "idle"},
            "updatePolicy": serde_json::to_value(pitcrew_core::update::UpdatePolicy::default()).unwrap(),
            "managedBinary": true, "dir": "/r", "labels": [], "serverUrl": null, "sessionMode": "service",
            "disk": null, "nextCleanupAt": null, "usage": null, "leftovers": 0, "hold": null, "deadline": null, "updateLoop": null
        }))
        .unwrap()
    }

    impl Fake {
        fn new(runners: &[&str], down: bool) -> Arc<Fake> {
            Arc::new(Fake {
                runners: runners.iter().map(ToString::to_string).collect(),
                config: Mutex::new(Config::default()),
                asked: Mutex::default(),
                down,
            })
        }

        fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }
    }

    impl ServiceClient for Fake {
        fn request(&self, r: Request) -> Result<Response, ClientError> {
            self.asked.lock().unwrap().push(r.kind().to_string());
            if self.down {
                return Err(ClientError::Unavailable("not running".into()));
            }
            Ok(match r {
                Request::Status => Response::Status {
                    runners: self.runners.iter().map(|n| snapshot(n)).collect(),
                    service: empty_service_info(),
                },
                Request::GetSettings => Response::Settings {
                    config: self.config.lock().unwrap().clone(),
                    problems: vec![],
                    read_only: false,
                },
                Request::SaveSettings { config } => {
                    *self.config.lock().unwrap() = config;
                    Response::Ok
                }
                _ => Response::Ok,
            })
        }
    }

    fn router(system: &Arc<Fake>, user: &Arc<Fake>) -> RoutingClient {
        RoutingClient::new(system.clone(), user.clone())
    }

    fn runner(name: &str, session: &str) -> pitcrew_core::config::RunnerConfig {
        serde_json::from_value(serde_json::json!({"name": name, "kind": "gitea-runner", "dir": "/r", "session": session})).unwrap()
    }

    fn name(s: &str) -> RunnerName {
        RunnerName::parse(s).unwrap()
    }

    #[test]
    fn a_new_runner_goes_to_the_engine_of_its_session() {
        let (system, user) = (Fake::new(&[], false), Fake::new(&[], false));
        let c = router(&system, &user);
        c.request(Request::AddRunner {
            runner: runner("a", "service"),
        })
        .unwrap();
        c.request(Request::AddRunner {
            runner: runner("b", "interactive"),
        })
        .unwrap();
        assert_eq!(system.asked(), ["AddRunner"]);
        assert_eq!(user.asked(), ["AddRunner"]);
    }

    #[test]
    fn a_request_about_a_runner_goes_to_the_engine_that_has_it() {
        let (system, user) = (Fake::new(&["svc"], false), Fake::new(&["mine"], false));
        let c = router(&system, &user);
        c.request(Request::Stop { runner: name("mine") }).unwrap();
        c.request(Request::Stop { runner: name("svc") }).unwrap();
        assert_eq!(user.asked(), ["Status", "Stop", "Status"]);
        assert_eq!(system.asked(), ["Stop"]);
        // With the user's engine not running, everything goes to the service.
        let (system, user) = (Fake::new(&["svc"], false), Fake::new(&[], true));
        router(&system, &user).request(Request::Stop { runner: name("svc") }).unwrap();
        assert_eq!(system.asked(), ["Stop"]);
        // A request about no one runner goes to the service.
        router(&system, &user).request(Request::Diagnostics).unwrap();
        assert_eq!(system.asked(), ["Stop", "Diagnostics"]);
    }

    #[test]
    fn the_two_statuses_become_one_list_and_a_missing_engine_is_skipped() {
        let (system, user) = (Fake::new(&["svc"], false), Fake::new(&["mine"], false));
        let Ok(Response::Status { runners, .. }) = router(&system, &user).request(Request::Status) else {
            panic!()
        };
        assert_eq!(runners.iter().map(|r| r.name.to_string()).collect::<Vec<_>>(), ["svc", "mine"]);
        let down = Fake::new(&[], true);
        let Ok(Response::Status { runners, .. }) = router(&system, &down).request(Request::Status) else {
            panic!()
        };
        assert_eq!(runners.len(), 1);
        let Ok(Response::Status { runners, .. }) = router(&down, &user).request(Request::Status) else {
            panic!()
        };
        assert_eq!(runners.len(), 1, "the service not running: the user's runners are still shown");
        assert!(router(&down, &down).request(Request::Status).is_err());
    }

    #[test]
    fn settings_are_merged_and_saved_split_by_session_and_the_service_is_asked_only_when_its_part_changed() {
        let (system, user) = (Fake::new(&[], false), Fake::new(&[], false));
        system.config.lock().unwrap().runners.push(runner("svc", "service"));
        user.config.lock().unwrap().runners.push(runner("mine", "interactive"));
        let c = router(&system, &user);
        let Ok(Response::Settings { config, .. }) = c.request(Request::GetSettings) else {
            panic!()
        };
        assert_eq!(config.runners.len(), 2);
        system.asked.lock().unwrap().clear();
        user.asked.lock().unwrap().clear();
        // Saving the merged configuration unchanged: the service is only read, never written (no permission prompt).
        c.request(Request::SaveSettings { config: config.clone() }).unwrap();
        assert_eq!(system.asked(), ["GetSettings"]);
        assert_eq!(user.asked(), ["SaveSettings"]);
        // A change to the user's runner touches only the user's engine; a change to the service runner reaches the service.
        let mut changed = config.clone();
        changed
            .runners
            .iter_mut()
            .find(|r| r.session == SessionMode::Service)
            .unwrap()
            .labels = vec!["x".into()];
        system.asked.lock().unwrap().clear();
        c.request(Request::SaveSettings { config: changed }).unwrap();
        assert_eq!(system.asked(), ["GetSettings", "SaveSettings"]);
        assert_eq!(
            system.config.lock().unwrap().runners.len(),
            1,
            "the service keeps only its own runners"
        );
        assert_eq!(user.config.lock().unwrap().runners.len(), 1, "and so does the user's engine");
    }

    #[test]
    fn the_merged_status_shows_both_lists_and_survives_one_engine_leaving() {
        let s = Statuses::default();
        let status = |names: &[&str]| StatusReply {
            runners: names.iter().map(|n| snapshot(n)).collect(),
            service: empty_service_info(),
        };
        assert_eq!(s.set_system(status(&["svc"])).runners.len(), 1);
        assert_eq!(s.set_user(Some(status(&["mine"]))).unwrap().runners.len(), 2);
        assert_eq!(
            s.set_user(None).unwrap().runners.len(),
            1,
            "the user's engine stopped: only the service's runners"
        );
        assert_eq!(s.set_system(status(&["svc", "svc2"])).runners.len(), 2);
    }

    #[test]
    fn the_tray_menu_is_rebuilt_only_when_what_it_shows_changed() {
        let s = Statuses::default();
        assert!(s.menu_changed("a"));
        assert!(!s.menu_changed("a"));
        assert!(s.menu_changed("b"));
        s.reset_menu();
        assert!(s.menu_changed("b"));
    }

    #[test]
    fn an_unknown_runner_answer_is_recognised() {
        assert!(is_unknown_runner(&Response::Error(ServiceError::new(
            ErrorCode::UnknownRunner,
            "no"
        ))));
        assert!(!is_unknown_runner(&Response::Ok));
    }
}
