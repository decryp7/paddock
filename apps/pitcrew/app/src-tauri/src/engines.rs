//! Two engines on Windows (ADR 0004, second amendment): the SYSTEM **service** manages the runners whose folders only
//! administrators can change (`session = service`), and the **per-user engine**, the same engine started in the user's own
//! session, manages the runners in folders the user can change (`session = interactive`). The app talks to both and the window sees
//! one list. This module is the pure part: where a request goes ([`RoutingClient`]) and how the two statuses become one
//! ([`Statuses`]). Elsewhere there is one engine and none of this is used.

use crate::dto::StatusReply;
use crate::service_client::{ClientError, ServiceClient};
use pitcrew_core::config::{Config, SessionMode};
use pitcrew_core::names::RunnerName;
use pitcrew_core::protocol::{ErrorCode, Request, Response, ServiceError};
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

/// Who has a runner, by the status each engine reported.
enum Owner<'a> {
    /// This engine listed it.
    Engine(&'a dyn ServiceClient),
    /// Neither engine listed it, and both answered.
    Nobody,
    /// Neither listed it and one did not answer: it may be there.
    NotKnown,
}

impl RoutingClient {
    /// A router over the service's client and the user engine's.
    pub fn new(system: Arc<dyn ServiceClient>, user: Arc<dyn ServiceClient>) -> Self {
        RoutingClient { system, user }
    }

    fn by_session(&self, session: SessionMode) -> (&dyn ServiceClient, &dyn ServiceClient) {
        match session {
            SessionMode::Service => (&*self.system, &*self.user),
            SessionMode::Interactive => (&*self.user, &*self.system),
        }
    }

    /// Whether the engine's status lists the runner: `Some(true/false)` when it answered, `None` when it did not.
    fn lists(engine: &dyn ServiceClient, runner: &RunnerName) -> Option<bool> {
        match engine.request(Request::Status) {
            Ok(Response::Status { runners, .. }) => Some(runners.iter().any(|r| &r.name == runner)),
            _ => None,
        }
    }

    /// The engine whose status listed the runner. A request is **never** sent to the service just because the user's engine did not
    /// answer: a runner that nobody listed gets an answer that says so, not a guess.
    fn owner_of(&self, runner: &RunnerName) -> Owner<'_> {
        let user = Self::lists(&*self.user, runner);
        if user == Some(true) {
            return Owner::Engine(&*self.user);
        }
        let system = Self::lists(&*self.system, runner);
        if system == Some(true) {
            return Owner::Engine(&*self.system);
        }
        if user.is_none() || system.is_none() {
            Owner::NotKnown
        } else {
            Owner::Nobody
        }
    }

    fn add_runner(&self, request: Request) -> Result<Response, ClientError> {
        let Request::AddRunner { runner } = &request else {
            return self.system.request(request);
        };
        let (target, other) = self.by_session(runner.session);
        // Names are unique across both engines: the same name in the other one would make every later request ambiguous.
        if Self::lists(other, &runner.name) == Some(true) {
            return Ok(Response::Error(ServiceError::new(
                ErrorCode::BadRequest,
                format!(
                    "a runner named {} already exists in the other engine (one for service runners, one for your own); runner names are unique across both",
                    runner.name
                ),
            )));
        }
        target.request(request)
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

    /// Whether the service's own part of the configuration already is what is wanted: its service runners and the defaults, and
    /// none of the user's kind of runner left in it (an older Pitcrew kept those there; saving moves them to the user's engine).
    /// Nothing else decides, so saving a user's own runner never raises a permission prompt.
    fn service_part_is_current(current: &Config, wanted: &Config) -> bool {
        let service_runners = |c: &Config| {
            c.runners
                .iter()
                .filter(|r| r.session == SessionMode::Service)
                .cloned()
                .collect::<Vec<_>>()
        };
        current.runners.iter().all(|r| r.session == SessionMode::Service)
            && service_runners(current) == service_runners(wanted)
            && current.defaults == wanted.defaults
    }

    fn save_split(&self, config: &Config) -> Result<Response, ClientError> {
        let (system, user) = Self::split(config);
        let mut service_saved = false;
        let current = match self.system.request(Request::GetSettings) {
            Ok(Response::Settings { config, .. }) => Some(config),
            _ => None,
        };
        if !current.as_ref().is_some_and(|c| Self::service_part_is_current(c, &system)) {
            if let r @ Response::Error(_) = self.system.request(Request::SaveSettings { config: system })? {
                return Ok(r); // nothing was applied
            }
            service_saved = true;
        }
        let answer = self.user.request(Request::SaveSettings { config: user });
        let failed = match &answer {
            Ok(Response::Error(e)) => Some(e.message.clone()),
            Err(e) => Some(e.to_string()),
            Ok(_) => None,
        };
        match (service_saved, failed) {
            // The service's part is already saved and cannot be taken back: say so, so nobody believes nothing changed.
            (true, Some(why)) => Ok(Response::Error(ServiceError::new(
                ErrorCode::Internal,
                format!(
                    "the service's part of the settings was saved, but the part for your own runners was not ({why}); the window shows what each engine has now, save again to finish"
                ),
            ))),
            _ => answer,
        }
    }
}

impl ServiceClient for RoutingClient {
    fn request(&self, request: Request) -> Result<Response, ClientError> {
        match &request {
            Request::AddRunner { .. } => self.add_runner(request),
            Request::Status => self.merged_status(),
            Request::GetSettings => self.merged_settings(),
            Request::SaveSettings { config } => self.save_split(config),
            other => match other.target_runner() {
                Some(runner) => match self.owner_of(runner) {
                    Owner::Engine(engine) => engine.request(request),
                    Owner::Nobody => Ok(Response::Error(ServiceError::new(
                        ErrorCode::UnknownRunner,
                        format!("no runner is named {runner}"),
                    ))),
                    Owner::NotKnown => Err(ClientError::Unavailable(format!(
                        "the engine that has {runner} did not answer, so the request was not sent anywhere else; try again when it is running"
                    ))),
                },
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
    /// What is wrong with the per-user engine (it could not be started, its pipe is not the user's), shown with the service's
    /// configuration problems in the window.
    user_problem: Mutex<Option<String>>,
}

impl Statuses {
    fn lock<T>(m: &Mutex<Option<T>>) -> std::sync::MutexGuard<'_, Option<T>> {
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

    /// Records what is wrong with the per-user engine (`None`: nothing) and returns the merged status to show, if any engine has
    /// reported.
    pub fn set_user_problem(&self, problem: Option<String>) -> Option<StatusReply> {
        *Self::lock(&self.user_problem) = problem;
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
        let problem = Self::lock(&self.user_problem).clone();
        let merged = match (system, user) {
            (Some(mut s), Some(u)) => {
                s.runners.extend(u.runners);
                Some(s)
            }
            (Some(s), None) => Some(s),
            (None, Some(u)) => Some(u),
            (None, None) => None,
        };
        merged.map(|mut m| {
            if let Some(p) = problem {
                m.service.config_problems.push(format!("Your own runners' engine: {p}"));
            }
            m
        })
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
    fn a_new_runner_goes_to_the_engine_of_its_session_and_never_when_the_other_engine_has_the_name() {
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
        // Each add asks the other engine for its names first, then adds: the service saw its own add (first) and was then asked
        // for its names (second add, a user's runner); the user's engine was asked first, then added.
        assert_eq!(
            system.asked(),
            ["AddRunner", "Status"],
            "the service: its own add, then asked for its names"
        );
        assert_eq!(
            user.asked(),
            ["Status", "AddRunner"],
            "the user's engine: asked for its names, then its own add"
        );
        // The same name in the other engine: refused, and nothing is added anywhere.
        let (system, user) = (Fake::new(&["same"], false), Fake::new(&[], false));
        let answer = router(&system, &user)
            .request(Request::AddRunner {
                runner: runner("same", "interactive"),
            })
            .unwrap();
        assert!(
            matches!(&answer, Response::Error(e) if e.message.contains("already exists in the other engine")),
            "{answer:?}"
        );
        assert_eq!(
            system.asked(),
            ["Status"],
            "the engine that might have the name was asked, and that is all"
        );
        assert!(user.asked().is_empty(), "{:?}", user.asked());
    }

    #[test]
    fn a_request_about_a_runner_goes_to_the_engine_that_listed_it_and_never_to_a_guess() {
        let (system, user) = (Fake::new(&["svc"], false), Fake::new(&["mine"], false));
        let c = router(&system, &user);
        c.request(Request::Stop { runner: name("mine") }).unwrap();
        c.request(Request::Stop { runner: name("svc") }).unwrap();
        assert_eq!(user.asked(), ["Status", "Stop", "Status"]);
        assert_eq!(system.asked(), ["Status", "Stop"]);
        // A runner nobody has, both engines answering: an answer that says so, and nothing is sent to either.
        let answer = c.request(Request::Stop { runner: name("nobody") }).unwrap();
        assert!(
            matches!(&answer, Response::Error(e) if e.code == ErrorCode::UnknownRunner),
            "{answer:?}"
        );
        assert_eq!(
            system.asked().iter().chain(user.asked().iter()).filter(|k| *k == "Stop").count(),
            2,
            "no third Stop was sent"
        );
        // The user's engine not answering must not send a user's runner's request to the service.
        let (system, user) = (Fake::new(&["svc"], false), Fake::new(&[], true));
        let c = router(&system, &user);
        let e = c.request(Request::Stop { runner: name("mine") }).unwrap_err();
        assert!(e.to_string().contains("did not answer"), "{e}");
        assert!(!system.asked().contains(&"Stop".to_string()), "{:?}", system.asked());
        // ...but a runner the service lists is the service's, whatever the user's engine does.
        c.request(Request::Stop { runner: name("svc") }).unwrap();
        assert!(system.asked().contains(&"Stop".to_string()));
        // A request about no one runner goes to the service.
        c.request(Request::Diagnostics).unwrap();
        assert!(system.asked().contains(&"Diagnostics".to_string()));
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
        // A change to the user's own runner touches only the user's engine.
        let mut mine = config.clone();
        mine.runners
            .iter_mut()
            .find(|r| r.session == SessionMode::Interactive)
            .unwrap()
            .labels = vec!["x".into()];
        system.asked.lock().unwrap().clear();
        c.request(Request::SaveSettings { config: mine }).unwrap();
        assert_eq!(system.asked(), ["GetSettings"], "a user's own save never prompts");
        // A change to the service runner reaches the service.
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
    fn a_users_runner_left_in_the_services_config_by_an_older_pitcrew_is_moved_out_by_the_next_save() {
        let (system, user) = (Fake::new(&[], false), Fake::new(&[], false));
        system.config.lock().unwrap().runners.push(runner("svc", "service"));
        system.config.lock().unwrap().runners.push(runner("old", "interactive"));
        let c = router(&system, &user);
        let Ok(Response::Settings { config, .. }) = c.request(Request::GetSettings) else {
            panic!()
        };
        system.asked.lock().unwrap().clear();
        c.request(Request::SaveSettings { config }).unwrap();
        assert_eq!(
            system.asked(),
            ["GetSettings", "SaveSettings"],
            "the stray makes the service part stale"
        );
        assert!(system
            .config
            .lock()
            .unwrap()
            .runners
            .iter()
            .all(|r| r.session == SessionMode::Service));
    }

    #[test]
    fn a_save_that_reached_the_service_but_not_the_users_engine_says_so() {
        let (system, user) = (Fake::new(&[], false), Fake::new(&[], true));
        let c = router(&system, &user);
        let mut config = Config::default();
        config.runners.push(runner("svc", "service"));
        config.runners.push(runner("mine", "interactive"));
        let answer = c.request(Request::SaveSettings { config }).unwrap();
        let Response::Error(e) = answer else { panic!("{answer:?}") };
        assert!(
            e.message.contains("service's part of the settings was saved") && e.message.contains("not running"),
            "{}",
            e.message
        );
        assert_eq!(
            system.config.lock().unwrap().runners.len(),
            1,
            "the service's part is applied, as the message says"
        );
        // When the service refuses, nothing is applied and the user's engine is not touched.
        let refusing: Arc<dyn ServiceClient> = Arc::new(Refusing);
        let user = Fake::new(&[], false);
        let c = RoutingClient::new(refusing, user.clone());
        let mut config = Config::default();
        config.runners.push(runner("svc", "service"));
        config.runners.push(runner("mine", "interactive"));
        let answer = c.request(Request::SaveSettings { config }).unwrap();
        assert!(
            matches!(&answer, Response::Error(e) if e.code == ErrorCode::PermissionDenied),
            "{answer:?}"
        );
        assert!(!user.asked().contains(&"SaveSettings".to_string()), "{:?}", user.asked());
    }

    /// An engine that refuses every change.
    struct Refusing;

    impl ServiceClient for Refusing {
        fn request(&self, r: Request) -> Result<Response, ClientError> {
            Ok(match r {
                Request::GetSettings => Response::Settings {
                    config: Config::default(),
                    problems: vec![],
                    read_only: false,
                },
                _ => Response::Error(ServiceError::new(ErrorCode::PermissionDenied, "declined")),
            })
        }
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
    fn what_is_wrong_with_the_user_engine_is_shown_with_the_services_problems() {
        let s = Statuses::default();
        assert!(
            s.set_user_problem(Some("the pipe is not yours".into())).is_none(),
            "nothing to attach it to yet"
        );
        let merged = s.set_system(StatusReply {
            runners: vec![],
            service: empty_service_info(),
        });
        assert_eq!(merged.service.config_problems, ["Your own runners' engine: the pipe is not yours"]);
        assert!(s.set_user_problem(None).unwrap().service.config_problems.is_empty());
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
