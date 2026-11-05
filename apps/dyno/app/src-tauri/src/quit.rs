//! What Quit does to the agent, in words and in steps (Dyno 1.1.0): the label of Quit from the real service state, the
//! "Quit and stop measuring" sequence over the service port with a read-back, and the marker that tells the next start the
//! agent was stopped on request (not a crash) so it brings the service back.
//!
//! Pure over the [`System`] port and plain closures: the tests use the shared fakes and a temporary folder, never the real
//! launchd, systemd or the user's Dyno (ER-14).

use crate::link::ServiceState;
use paddock_service::stop::{stop, StopError, StopMechanism, StopTimings, Stopped};
use paddock_service::System;
use std::path::Path;
use std::time::Duration;

/// The id of the "Quit and stop measuring" entry in the tray menu.
pub const QUIT_STOP_ID: &str = "dyno.quit-stop";

/// The command id of the same entry in the menu bar (the UI's contribution, `paddock-manifest.json`).
pub const QUIT_STOP_COMMAND: &str = "dy.quit-stop";

/// The menu entry that stops the agent too.
pub const QUIT_STOP_LABEL: &str = "Quit and stop measuring";

/// The marker file in Dyno's data folder: the agent was stopped on request.
pub const STOP_MARKER: &str = "agent-stopped-on-request";

/// Whether the agent keeps running after a plain Quit: a service is installed and supervises it.
pub fn keeps_measuring(service: ServiceState) -> bool {
    service.installed && service.supervises
}

/// The text of Quit for the service state: it says what happens (sentence case).
pub fn quit_label(keeps: bool) -> &'static str {
    if keeps {
        "Quit Dyno (keeps measuring)"
    } else {
        "Quit Dyno"
    }
}

/// What "Quit and stop measuring" is for, in the action's help text (it never touches the administrator helper).
pub fn quit_stop_help(helper_on: bool) -> &'static str {
    if helper_on {
        "Stops the agent and closes Dyno until the next login or until you open Dyno again. Launch at login stays on. The administrator helper is separate and keeps running."
    } else {
        "Stops the agent and closes Dyno until the next login or until you open Dyno again. Launch at login stays on."
    }
}

/// What the next start does about a stopped service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resume {
    /// Nothing: the agent was not stopped on request, or a service already supervises one (never a second agent).
    Nothing,
    /// Start the service again: it was stopped on request and nothing supervises now.
    StartService,
}

/// Decides at the start: the service is started again only when the stop marker is there and the installed service
/// supervises nothing. A supervising service already runs the agent, so no second one is started.
pub fn resume_after_stop(marker_present: bool, service: ServiceState) -> Resume {
    if marker_present && service.installed && !service.supervises {
        Resume::StartService
    } else {
        Resume::Nothing
    }
}

/// Writes the marker (before anything is stopped, so a crash in between still tells the truth).
pub fn write_marker(root: &Path, now_ms: i64) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    std::fs::write(root.join(STOP_MARKER), now_ms.to_string())
}

/// Reads and removes the marker: when the agent was stopped on request, and the time of the request in ms.
pub fn take_marker(root: &Path) -> Option<i64> {
    let path = root.join(STOP_MARKER);
    let text = std::fs::read_to_string(&path).ok()?;
    if let Err(e) = std::fs::remove_file(&path) {
        log::warn!(Error = e.to_string().as_str(); "The stop marker could not be removed");
    }
    Some(text.trim().parse().unwrap_or(0))
}

/// What a finished stop did, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopReport {
    /// The service manager held a job and confirmed it gone.
    pub service: Option<Stopped>,
    /// An agent was found and asked to exit (`false`: none found).
    pub asked: bool,
}

/// How an agent that does not belong to a service is asked to exit.
pub trait DirectAgent {
    /// Asks the agent in the data folder to exit; `Ok(false)` when none was found.
    fn ask_to_exit(&mut self) -> Result<bool, String>;
    /// Whether the agent that was asked has left (its process, by identity).
    fn gone(&mut self) -> bool;
}

/// "Quit and stop measuring": stops the service's job (when one supervises), asks any agent still answering to exit and reads the
/// result back. `Ok` only when the OS confirms the job is gone and no agent is left.
///
/// # Errors
/// What did not work and the next step.
pub fn stop_measuring(
    sys: &mut dyn System,
    service: Option<&StopMechanism>,
    direct: &mut dyn DirectAgent,
    timings: StopTimings,
) -> Result<StopReport, String> {
    let service = match service {
        Some(m) => Some(stop(sys, m, timings).map_err(|StopError(e)| e)?),
        None => None,
    };
    let asked = direct.ask_to_exit()?;
    let mut waited = Duration::ZERO;
    while asked && !direct.gone() {
        if waited >= timings.gone_wait {
            return Err(
                "The agent was asked to exit but still answers. Quit Dyno, then end dyno-agent in Activity Monitor or Task Manager.".into(),
            );
        }
        sys.pause(timings.poll);
        waited += timings.poll;
    }
    Ok(StopReport { service, asked })
}

/// The first claim wins: a second click, or the menu and the tray together, starts one stop.
pub struct OnceFlag(std::sync::atomic::AtomicBool);

impl OnceFlag {
    /// A flag nobody has claimed.
    pub const fn new() -> OnceFlag {
        OnceFlag(std::sync::atomic::AtomicBool::new(false))
    }

    /// Whether this call is the first.
    pub fn claim(&self) -> bool {
        !self.0.swap(true, std::sync::atomic::Ordering::SeqCst)
    }
}

impl Default for OnceFlag {
    fn default() -> Self {
        OnceFlag::new()
    }
}

static STOP_BEGUN: OnceFlag = OnceFlag::new();

/// What the start did about a stop on request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartOutcome {
    /// No marker: the agent was not stopped on request.
    NoMarker,
    /// The agent had been stopped on request; nothing had to start (a service already supervises, or none is installed).
    StoppedOnRequest,
    /// The agent had been stopped on request and the service was started again (`Ok`) or could not be (`Err`).
    Resumed(Result<bool, String>),
}

/// At the start, before the link decides anything: tells the log when the agent had been stopped on request (so it is not read as
/// a crash), and starts the service again when it is installed and nothing supervises it. The marker is read once.
pub fn resume_at_start(
    root: &Path,
    probe: &dyn Fn() -> ServiceState,
    location: Result<(), String>,
    service: &mut dyn crate::login::AgentService,
) -> StartOutcome {
    let Some(at) = take_marker(root) else {
        return StartOutcome::NoMarker;
    };
    log::info!(RequestedAtMs = at; "The agent had been stopped on request (Quit and stop measuring), not by a crash");
    let state = probe();
    match resume_after_stop(true, state) {
        Resume::Nothing => {
            log::info!(Installed = state.installed, Supervises = state.supervises; "After a stop on request: no service to start again");
            StartOutcome::StoppedOnRequest
        }
        Resume::StartService => {
            // From a place the bundle cannot register from (a disk image, a translocated copy) nothing is started, and the
            // stop marker stays for the start from the right place.
            if let Err(e) = location {
                if let Err(w) = write_marker(root, at) {
                    log::warn!(Error = w.to_string().as_str(); "The stop marker could not be written back");
                }
                log::warn!(Error = e.as_str(); "After a stop on request: the service is not started again from here; the marker is kept");
                return StartOutcome::Resumed(Err(e));
            }
            let began = std::time::Instant::now();
            let result = service.resume();
            let ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
            match &result {
                Ok(true) => log::info!(Ms = ms; "After a stop on request: the service was started again (the system holds its job)"),
                Ok(false) => log::info!(Ms = ms; "After a stop on request: there is no service to start; Dyno starts an agent of its own"),
                Err(e) => {
                    log::warn!(Error = e.as_str(), Ms = ms; "After a stop on request: the agent service could not be started again; Dyno starts an agent of its own")
                }
            }
            StartOutcome::Resumed(result)
        }
    }
}

/// The agent in Dyno's data folder, asked through its own connection (what `dyno-agent stop` does). Which process it is comes from
/// the discovery file's pid and start time ([`paddock_proc::ProcessId`], rule 53): a pid that was handed on is not the agent, and an
/// agent that is alive but does not answer is an error, never "gone".
pub struct LiveAgent {
    dirs: dyno_agent::paths::Dirs,
    target: Option<paddock_proc::ProcessId>,
    /// How long a held lock with no usable discovery file is waited on (an agent that is already exiting) before it is an error.
    wait: Duration,
}

impl LiveAgent {
    /// The agent of `dirs`, not yet asked.
    pub fn new(dirs: dyno_agent::paths::Dirs) -> LiveAgent {
        LiveAgent {
            dirs,
            target: None,
            wait: StopTimings::default().gone_wait,
        }
    }

    /// No usable discovery file (missing, unreadable, or naming a process that is gone): what the lock says. An agent that is
    /// already exiting has removed its file and may hold the lock a little longer, so a held lock is waited on for `wait` first.
    fn without_a_named_agent(&self, file_present: bool) -> Result<bool, String> {
        use paddock_proc::HeldState;
        let began = std::time::Instant::now();
        loop {
            match dyno_agent::instance::held_state(&self.dirs) {
                HeldState::Free => {
                    log::info!(FilePresent = file_present; "No agent found: no process is named by the discovery file and the data folder's lock is free");
                    return Ok(false);
                }
                HeldState::Unknown => {
                    return Err("The agent's lock in Dyno's data folder could not be looked at, so Dyno cannot tell whether an agent runs. Check that the folder is readable, then end dyno-agent in Activity Monitor or Task Manager if it runs.".into());
                }
                HeldState::Held if began.elapsed() >= self.wait => {
                    return Err("An agent holds this data folder, but the discovery file that names it is missing or unreadable, so Dyno cannot ask it to exit. End dyno-agent in Activity Monitor or Task Manager.".into());
                }
                HeldState::Held => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    }

    /// The same with another wait for a lock that is held while no discovery file names the agent.
    #[must_use]
    pub fn with_wait(mut self, wait: Duration) -> LiveAgent {
        self.wait = wait;
        self
    }
}

impl DirectAgent for LiveAgent {
    fn ask_to_exit(&mut self) -> Result<bool, String> {
        // The discovery file is only a hint (rule 53): the lock the agent holds says whether one runs here at all.
        let file = dyno_agent::discovery::Discovery::read(&self.dirs.discovery());
        let id = file.as_ref().and_then(|f| paddock_proc::ProcessId::of_recorded(f.pid, f.started));
        let Some(id) = id else {
            return self.without_a_named_agent(file.is_some());
        };
        let (peer, _) = dyno_agent::cli::connect(&self.dirs).map_err(|e| {
            format!("The agent ({id}) is running but does not answer: {e} End dyno-agent in Activity Monitor or Task Manager.")
        })?;
        self.target = Some(id);
        log::info!(Pid = id.pid(); "Asking the agent to exit (Quit and stop measuring)");
        // The agent may end the connection as it exits, so a missing reply is not a failure: the process itself is the read-back.
        if let Err(e) = dyno_agent::cli::ask(&peer, dyno_core::protocol::Request::Shutdown) {
            log::info!(Error = e.as_str(); "No reply to the shutdown request; reading the process instead");
        }
        Ok(true)
    }

    fn gone(&mut self) -> bool {
        match self.target {
            Some(id) => !id.is_alive(),
            None => !dyno_agent::instance::is_held(&self.dirs),
        }
    }
}

/// "Quit and stop measuring", off the UI thread (ER-17): the link stops first (nothing may start another agent), then the service
/// job and any agent still answering are stopped and read back, then the app quits. A stop that cannot be confirmed says so
/// with the next step in a message and the app still quits (as a plain Quit would).
pub fn quit_and_stop<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    if !STOP_BEGUN.claim() {
        log::info!("Quit and stop measuring: already under way");
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("dyno-quit-stop".into()).spawn(move || {
        let began = std::time::Instant::now();
        let Some(link) = app.try_state::<std::sync::Arc<crate::link::AgentLink>>().map(|l| l.inner().clone()) else {
            paddock_window::tauri::quit(&app);
            return;
        };
        let root = link.dirs.root().to_path_buf();
        let mechanism = crate::login::platform_service().stop_mechanism();
        log::info!(Service = format!("{mechanism:?}").as_str(); "Quit and stop measuring: begin");
        if let Err(e) = write_marker(&root, crate::link::ports::now_ms()) {
            log::warn!(Error = e.to_string().as_str(); "The stop marker could not be written");
        }
        link.quit();
        let mut sys = paddock_service::RealSystem::new().with_timeout(Duration::from_secs(10));
        let result = stop_measuring(
            &mut sys,
            mechanism.as_ref(),
            &mut LiveAgent::new(link.dirs.clone()),
            StopTimings::default(),
        );
        let ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
        match result {
            Ok(report) => {
                let outcome = if report.service.is_some() || report.asked {
                    "Quit and stop measuring: the system confirms the agent is gone"
                } else {
                    "Quit and stop measuring: no agent was found"
                };
                log::info!(Ms = ms, Service = format!("{:?}", report.service).as_str(), Asked = report.asked; "{outcome}");
            }
            Err(e) => {
                log::error!(Ms = ms, Error = e.as_str(); "Quit and stop measuring: the agent could not be confirmed gone");
                // The agent runs on, so this was not a stop on request.
                if let Err(rm) = std::fs::remove_file(root.join(STOP_MARKER)) {
                    log::info!(Error = rm.to_string().as_str(); "The stop marker was not there to remove");
                }
                use tauri_plugin_dialog::DialogExt;
                app.dialog()
                    .message(format!("Dyno could not confirm that the agent stopped. {e}"))
                    .title("Quit and stop measuring")
                    .kind(tauri_plugin_dialog::MessageDialogKind::Warning)
                    .blocking_show();
            }
        }
        paddock_window::tauri::quit(&app);
    });
    if let Err(e) = spawned {
        log::error!(Error = e.to_string().as_str(); "Quit and stop measuring: the worker could not start");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_service::port::Output;
    use paddock_service::testing::FakeSystem;

    const SUPERVISED: ServiceState = ServiceState {
        installed: true,
        supervises: true,
    };
    const STOPPED_SERVICE: ServiceState = ServiceState {
        installed: true,
        supervises: false,
    };
    const NONE: ServiceState = ServiceState {
        installed: false,
        supervises: false,
    };
    const FAST: StopTimings = StopTimings {
        gone_wait: Duration::from_secs(2),
        poll: Duration::from_millis(500),
    };

    #[test]
    fn quit_says_it_keeps_measuring_only_where_a_service_supervises_the_agent() {
        assert_eq!(quit_label(keeps_measuring(SUPERVISED)), "Quit Dyno (keeps measuring)");
        assert_eq!(
            quit_label(keeps_measuring(STOPPED_SERVICE)),
            "Quit Dyno",
            "installed but nothing supervises: Quit ends it"
        );
        assert_eq!(quit_label(keeps_measuring(NONE)), "Quit Dyno");
        assert_eq!(QUIT_STOP_LABEL, "Quit and stop measuring");
        assert!(quit_stop_help(true).contains("administrator helper is separate"));
        assert!(!quit_stop_help(false).contains("helper"));
    }

    #[test]
    fn a_stop_on_request_starts_the_service_again_at_the_next_start_but_never_a_second_agent() {
        assert_eq!(resume_after_stop(true, STOPPED_SERVICE), Resume::StartService);
        assert_eq!(
            resume_after_stop(false, STOPPED_SERVICE),
            Resume::Nothing,
            "no marker: the user's own stop is not undone"
        );
        assert_eq!(
            resume_after_stop(true, SUPERVISED),
            Resume::Nothing,
            "a supervising service already runs the agent"
        );
        assert_eq!(
            resume_after_stop(true, NONE),
            Resume::Nothing,
            "no service: the app starts the agent itself"
        );
    }

    #[test]
    fn the_marker_is_read_once_and_removed() {
        let dir = std::env::temp_dir().join(format!("dyno-quit-marker-{}", std::process::id()));
        assert_eq!(take_marker(&dir), None);
        write_marker(&dir, 1234).unwrap();
        assert_eq!(take_marker(&dir), Some(1234));
        assert_eq!(take_marker(&dir), None, "read once");
        std::fs::write(dir.join(STOP_MARKER), "junk").unwrap();
        assert_eq!(take_marker(&dir), Some(0), "a damaged marker still counts as a stop on request");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    struct Direct {
        there: bool,
        leaves: bool,
        asked: u32,
        polls: u32,
    }

    impl DirectAgent for Direct {
        fn ask_to_exit(&mut self) -> Result<bool, String> {
            self.asked += 1;
            Ok(self.there)
        }
        fn gone(&mut self) -> bool {
            self.polls += 1;
            self.leaves
        }
    }

    fn launchd_that(drops: bool) -> (FakeSystem, StopMechanism) {
        let sys = FakeSystem::new();
        let up = std::sync::Arc::new(std::sync::Mutex::new(true));
        let (a, b) = (up.clone(), up);
        sys.on("/bin/launchctl print", move |_, _| {
            Ok(if *a.lock().unwrap() {
                Output::ok("state = running\n")
            } else {
                Output::code(113)
            })
        });
        sys.on("/bin/launchctl bootout", move |_, _| {
            *b.lock().unwrap() = !drops;
            Ok(Output::code(0))
        });
        (
            sys,
            StopMechanism::Launchd {
                target: "gui/501/net.decryptology.dyno.agent".into(),
            },
        )
    }

    #[test]
    fn macos_stops_the_job_through_launchd_and_waits_for_the_agent_to_leave() {
        let (sys, m) = launchd_that(true);
        let mut direct = Direct {
            there: true,
            leaves: true,
            asked: 0,
            polls: 0,
        };
        let r = stop_measuring(&mut sys.clone(), Some(&m), &mut direct, FAST).unwrap();
        assert_eq!(
            r,
            StopReport {
                service: Some(Stopped::Stopped),
                asked: true
            }
        );
        assert!(sys
            .commands()
            .contains(&"/bin/launchctl bootout gui/501/net.decryptology.dyno.agent".to_string()));
    }

    #[test]
    fn linux_stops_the_user_unit_and_never_disables_it() {
        let sys = FakeSystem::new();
        let up = std::sync::Arc::new(std::sync::Mutex::new(true));
        let (a, b) = (up.clone(), up);
        sys.on("systemctl --user is-active", move |_, _| {
            Ok(Output::ok(if *a.lock().unwrap() { "active\n" } else { "inactive\n" }))
        });
        sys.on("systemctl --user stop", move |_, _| {
            *b.lock().unwrap() = false;
            Ok(Output::code(0))
        });
        let m = StopMechanism::SystemdUser {
            unit: "dyno-agent.service".into(),
        };
        let mut direct = Direct {
            there: false,
            leaves: true,
            asked: 0,
            polls: 0,
        };
        let r = stop_measuring(&mut sys.clone(), Some(&m), &mut direct, FAST).unwrap();
        assert_eq!(r.service, Some(Stopped::Stopped));
        assert!(sys.commands().iter().all(|c| !c.contains("disable")));
    }

    #[test]
    fn windows_has_no_service_job_so_the_agent_itself_is_asked_and_read_back() {
        let mut direct = Direct {
            there: true,
            leaves: true,
            asked: 0,
            polls: 0,
        };
        let r = stop_measuring(&mut FakeSystem::new(), None, &mut direct, FAST).unwrap();
        assert_eq!(
            r,
            StopReport {
                service: None,
                asked: true
            }
        );
        assert_eq!((direct.asked, direct.polls), (1, 1));
    }

    #[test]
    fn a_failed_read_back_is_an_error_with_the_next_step_not_a_success() {
        // the service job stays after bootout
        let (sys, m) = launchd_that(false);
        let e = stop_measuring(
            &mut sys.clone(),
            Some(&m),
            &mut Direct {
                there: false,
                leaves: true,
                asked: 0,
                polls: 0,
            },
            FAST,
        )
        .unwrap_err();
        assert!(e.contains("still reports the agent") && e.contains("launchctl bootout"), "{e}");
        // the agent answers after it was asked to exit
        let e = stop_measuring(
            &mut FakeSystem::new(),
            None,
            &mut Direct {
                there: true,
                leaves: false,
                asked: 0,
                polls: 0,
            },
            FAST,
        )
        .unwrap_err();
        assert!(e.contains("still answers") && e.contains("Activity Monitor"), "{e}");
    }

    #[test]
    fn nothing_running_is_a_quiet_success_and_an_unreachable_agent_is_reported() {
        let r = stop_measuring(
            &mut FakeSystem::new(),
            None,
            &mut Direct {
                there: false,
                leaves: false,
                asked: 0,
                polls: 0,
            },
            FAST,
        )
        .unwrap();
        assert_eq!(
            r,
            StopReport {
                service: None,
                asked: false
            }
        );
        struct Broken;
        impl DirectAgent for Broken {
            fn ask_to_exit(&mut self) -> Result<bool, String> {
                Err("The agent did not answer.".into())
            }
            fn gone(&mut self) -> bool {
                true
            }
        }
        assert!(stop_measuring(&mut FakeSystem::new(), None, &mut Broken, FAST).is_err());
    }

    /// A fake service that records what the start asks of it.
    #[derive(Default)]
    struct Svc {
        resumed: u32,
        fail: bool,
    }

    impl paddock_service::login::LoginService for Svc {
        fn install(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn uninstall(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    impl crate::login::AgentService for Svc {
        fn installed(&mut self) -> bool {
            true
        }
        fn repair(&mut self, _: crate::login::AnswerFn) -> Result<bool, String> {
            Ok(false)
        }
        fn resume(&mut self) -> Result<bool, String> {
            self.resumed += 1;
            if self.fail {
                Err("launchd refused".into())
            } else {
                Ok(true)
            }
        }
    }

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("dyno-quit-{name}-{}", std::process::id()))
    }

    #[test]
    fn opening_dyno_after_a_stop_on_request_starts_the_service_again_once() {
        let dir = temp("resume");
        write_marker(&dir, 5).unwrap();
        let mut svc = Svc::default();
        assert_eq!(
            resume_at_start(&dir, &|| STOPPED_SERVICE, Ok(()), &mut svc),
            StartOutcome::Resumed(Ok(true))
        );
        assert_eq!(svc.resumed, 1);
        assert_eq!(
            resume_at_start(&dir, &|| STOPPED_SERVICE, Ok(()), &mut svc),
            StartOutcome::NoMarker,
            "the marker is read once"
        );
        assert_eq!(svc.resumed, 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_refused_location_starts_nothing_and_keeps_the_stop_marker_for_the_right_place() {
        let dir = temp("location");
        write_marker(&dir, 5).unwrap();
        let mut svc = Svc::default();
        let from_a_disk_image = || Err("move Dyno to Applications".to_string());
        assert_eq!(
            resume_at_start(&dir, &|| STOPPED_SERVICE, from_a_disk_image(), &mut svc),
            StartOutcome::Resumed(Err("move Dyno to Applications".into()))
        );
        assert_eq!(svc.resumed, 0);
        assert_eq!(
            resume_at_start(&dir, &|| STOPPED_SERVICE, Ok(()), &mut svc),
            StartOutcome::Resumed(Ok(true)),
            "the next start from a good place still resumes"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_service_that_cannot_start_is_reported_and_the_app_starts_its_own_agent_as_before() {
        let dir = temp("fail");
        write_marker(&dir, 5).unwrap();
        let mut svc = Svc {
            fail: true,
            ..Svc::default()
        };
        assert_eq!(
            resume_at_start(&dir, &|| STOPPED_SERVICE, Ok(()), &mut svc),
            StartOutcome::Resumed(Err("launchd refused".into()))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_second_agent_is_started_beside_a_supervising_service_or_without_a_marker() {
        let dir = temp("nosecond");
        write_marker(&dir, 5).unwrap();
        let mut svc = Svc::default();
        assert_eq!(
            resume_at_start(&dir, &|| SUPERVISED, Ok(()), &mut svc),
            StartOutcome::StoppedOnRequest
        );
        assert_eq!(resume_at_start(&dir, &|| STOPPED_SERVICE, Ok(()), &mut svc), StartOutcome::NoMarker);
        write_marker(&dir, 6).unwrap();
        assert_eq!(resume_at_start(&dir, &|| NONE, Ok(()), &mut svc), StartOutcome::StoppedOnRequest);
        assert_eq!(svc.resumed, 0, "the service was never started in these three");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_live_agent_is_nothing_to_stop_when_no_discovery_file_exists() {
        let dir = temp("live");
        let mut live = LiveAgent::new(dyno_agent::paths::Dirs::at(&dir));
        assert_eq!(live.ask_to_exit(), Ok(false));
        assert!(live.gone());
    }

    fn write_discovery(dirs: &dyno_agent::paths::Dirs, pid: u32, started: i64) {
        let d = dyno_agent::discovery::Discovery {
            endpoint: "dyno-test-none".into(),
            token: "00".repeat(32),
            pid,
            started,
            version: "0".into(),
            protocol: 0,
        };
        std::fs::create_dir_all(dirs.root()).unwrap();
        d.write(&dirs.discovery()).unwrap();
    }

    #[test]
    fn a_live_agent_that_does_not_answer_is_an_error_not_gone_and_a_stale_file_is_nothing_to_stop() {
        let dir = temp("alive");
        let dirs = dyno_agent::paths::Dirs::at(&dir);
        // this very process, written now (so it started before): alive, but nothing listens on the endpoint
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        write_discovery(&dirs, std::process::id(), now);
        let mut live = LiveAgent::new(dirs.clone());
        let e = live.ask_to_exit().unwrap_err();
        assert!(e.contains("is running but does not answer"), "{e}");
        // a file written long before its holder started (a handed-on pid) is stale
        write_discovery(&dirs, std::process::id(), 1_000);
        assert_eq!(LiveAgent::new(dirs).ask_to_exit(), Ok(false));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_the_first_stop_begins() {
        let flag = OnceFlag::default();
        assert!(flag.claim());
        assert!(!flag.claim());
    }

    /// A process of its own that runs until it is ended (a stand-in for the agent).
    // A test stand-in for the agent, ended by the test itself: the app's bounded, console-less spawn helper is for real children.
    #[allow(clippy::disallowed_methods)]
    fn long_process() -> std::process::Child {
        #[cfg(unix)]
        let child = std::process::Command::new("sleep").arg("60").spawn();
        #[cfg(windows)]
        let child = std::process::Command::new("ping").args(["-n", "60", "127.0.0.1"]).spawn();
        child.expect("a test process")
    }

    #[test]
    fn gone_is_the_process_by_identity_not_the_discovery_file() {
        let dir = temp("gone");
        let dirs = dyno_agent::paths::Dirs::at(&dir);
        let mut child = long_process();
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        // the file stays in place for the whole test, as a crashed or slow agent leaves it
        write_discovery(&dirs, child.id(), now);
        let mut live = LiveAgent::new(dirs.clone());
        live.target = paddock_proc::ProcessId::of(child.id());
        assert!(live.target.is_some());
        assert!(!live.gone(), "the process runs, whatever the file says");
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(dirs.discovery().exists());
        assert!(live.gone(), "the process ended although the file is still there");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_held_lock_with_no_usable_discovery_file_is_an_error_and_a_free_lock_is_none_found() {
        let dir = temp("hidden");
        let dirs = dyno_agent::paths::Dirs::at(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            LiveAgent::new(dirs.clone()).with_wait(Duration::from_millis(100)).ask_to_exit(),
            Ok(false),
            "nothing holds the lock: none found"
        );
        let _agent = dyno_agent::instance::InstanceLock::acquire(&dirs).expect("the test takes the lock like an agent");
        let e = LiveAgent::new(dirs.clone())
            .with_wait(Duration::from_millis(100))
            .ask_to_exit()
            .unwrap_err();
        assert!(e.contains("holds this data folder") && e.contains("Activity Monitor"), "{e}");
        std::fs::write(dirs.discovery(), "not json").unwrap();
        assert!(
            LiveAgent::new(dirs.clone())
                .with_wait(Duration::from_millis(100))
                .ask_to_exit()
                .is_err(),
            "an unreadable file is no reason to call it gone"
        );
        let mut live = LiveAgent::new(dirs);
        assert!(!live.gone(), "without a target the held lock says it runs");
        drop(_agent);
        assert!(live.gone());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The menu bar's entry is wired: the manifest the UI is checked against names its command and the menu, and the Rust side
    /// registers an action for exactly that id.
    #[test]
    fn quit_and_stop_is_in_the_manifest_and_the_shell_knows_its_id() {
        let manifest: serde_json::Value = serde_json::from_str(crate::MANIFEST).unwrap();
        let has = |list: &str, key: &str| manifest[list].as_array().unwrap().iter().any(|e| e[key] == QUIT_STOP_COMMAND);
        assert!(has("commands", "id"), "the command is contributed");
        assert!(has("menus", "command"), "the command has a menu entry");
        let lib = include_str!("lib.rs");
        assert!(
            lib.contains(".menu_action_without_window(quit::QUIT_STOP_COMMAND"),
            "the shell runs it with no window open"
        );
        assert!(lib.contains("id == quit::QUIT_STOP_ID"), "the tray menu runs it");
    }

    #[test]
    fn an_agent_that_is_already_exiting_is_waited_for_before_the_hidden_agent_error() {
        let dir = temp("exiting");
        let dirs = dyno_agent::paths::Dirs::at(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // the agent removed its discovery file and still holds the lock for a moment, longer than the lock check's own 400 ms
        let held = dyno_agent::instance::InstanceLock::acquire(&dirs).unwrap();
        let ender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(900));
            drop(held);
        });
        let mut live = LiveAgent::new(dirs.clone()).with_wait(Duration::from_secs(10));
        assert_eq!(live.ask_to_exit(), Ok(false), "it left while waited for: none found");
        ender.join().unwrap();
        // one that stays is the error
        let _stays = dyno_agent::instance::InstanceLock::acquire(&dirs).unwrap();
        let e = LiveAgent::new(dirs.clone())
            .with_wait(Duration::from_millis(300))
            .ask_to_exit()
            .unwrap_err();
        assert!(e.contains("holds this data folder"), "{e}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_lock_that_cannot_be_looked_at_is_not_taken_for_free() {
        let dir = temp("unknownlock");
        std::fs::create_dir_all(&dir).unwrap();
        let blocker = dir.join("data");
        std::fs::write(&blocker, "a file where the data folder should be").unwrap();
        let e = LiveAgent::new(dyno_agent::paths::Dirs::at(&blocker))
            .with_wait(Duration::from_millis(100))
            .ask_to_exit()
            .unwrap_err();
        assert!(e.contains("could not be looked at"), "{e}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
