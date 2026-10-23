//! Installing, updating and removing the privileged helper, and telling the app what state it is in (WP-01,
//! ADR 0016). Everything here happens on the user's click: [`HelperControl::install`] is sent only from the
//! confirmation dialog, runs the OS's own permission prompt in the background and reports through
//! [`HelperControl::status`] (state `installing` until the prompt is answered).

use crate::helper_link::HelperLink;
use dyno_core::helper::TuningHost;
use dyno_core::helper_wire::{assess, prompt_text, HelperState, HelperStatus, HelperUser, Probe};
use dyno_core::protocol::{AgentError, ErrorCode};
use dyno_platform::elevate::{Elevator, OsElevator};
use dyno_platform::tuning::{SystemRunner, Tool, ToolRunner};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// What the request handlers need from the helper's lifecycle (and what tests replace).
pub trait HelperControl: Send {
    /// `Ok` when this computer can install the helper at all, else what is missing.
    fn can_install(&self) -> Result<(), String>;
    /// The state, with the next step in words.
    fn status(&mut self) -> HelperStatus;
    /// Starts the install or update (the OS prompt opens). Returns at once with the state `installing`.
    fn install(&mut self) -> Result<HelperStatus, AgentError>;
    /// Removes the helper and returns the state afterwards.
    fn uninstall(&mut self) -> Result<HelperStatus, AgentError>;
}

/// What the install needs to know about this computer.
pub trait InstallEnv: Send + Sync {
    /// Whether the helper can be installed here.
    fn supported(&self) -> Result<(), String>;
    /// The helper program shipped next to the agent.
    fn helper_exe(&self) -> Result<PathBuf, String>;
    /// The agent program (the one allowed to call the helper).
    fn agent_exe(&self) -> Result<PathBuf, String>;
    /// The user who is installing.
    fn user(&self) -> Result<HelperUser, String>;
}

/// Parses the account id out of `whoami /user /fo csv /nh` (`"DOMAIN\user","S-1-5-21-…"`).
pub fn parse_whoami_sid(text: &str) -> Option<HelperUser> {
    let sid = text.split(['"', ',']).map(str::trim).find(|t| t.starts_with("S-1-"))?;
    HelperUser::parse(&format!("sid:{sid}")).ok()
}

/// The user id out of `id -u`.
pub fn parse_id_u(text: &str) -> Option<HelperUser> {
    text.trim().parse::<u32>().ok().map(HelperUser::Uid)
}

/// The real environment.
pub struct SystemEnv {
    host: TuningHost,
}

impl SystemEnv {
    /// The environment of this computer.
    pub fn new() -> SystemEnv {
        SystemEnv {
            host: dyno_platform::elevate::this_host(),
        }
    }
}

impl Default for SystemEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl InstallEnv for SystemEnv {
    fn supported(&self) -> Result<(), String> {
        let file = |p: &str| std::path::Path::new(p).is_file();
        match self.host {
            TuningHost::Macos if !file("/usr/bin/osascript") => {
                Err("macOS did not provide osascript, so Dyno cannot ask for your password.".into())
            }
            TuningHost::Linux if !std::path::Path::new("/run/systemd/system").is_dir() => {
                Err("The helper runs as a systemd service, and this computer does not use systemd.".into())
            }
            TuningHost::Linux if !file("/usr/bin/pkexec") && !file("/bin/pkexec") => {
                Err("pkexec (polkit) is not installed, so Dyno cannot ask for your permission. Install polkit to use the helper.".into())
            }
            _ => self.helper_exe().map(|_| ()),
        }
    }

    fn helper_exe(&self) -> Result<PathBuf, String> {
        let me = std::env::current_exe().map_err(|e| format!("Dyno cannot find its own program: {e}"))?;
        let name = if cfg!(windows) { "dyno-helper.exe" } else { "dyno-helper" };
        let path = me.parent().map(|d| d.join(name)).unwrap_or_default();
        // An empty file is the build's placeholder.
        if std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() > 0) {
            Ok(path)
        } else {
            Err(format!("{} is missing from this installation; reinstall Dyno.", path.display()))
        }
    }

    fn agent_exe(&self) -> Result<PathBuf, String> {
        std::env::current_exe().map_err(|e| format!("Dyno cannot find its own program: {e}"))
    }

    fn user(&self) -> Result<HelperUser, String> {
        let tool = if cfg!(windows) {
            let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
            Tool {
                program: format!("{root}\\System32\\whoami.exe"),
                args: vec!["/user".into(), "/fo".into(), "csv".into(), "/nh".into()],
                env: vec![],
            }
        } else {
            Tool {
                program: "/usr/bin/id".into(),
                args: vec!["-u".into()],
                env: vec![],
            }
        };
        let out = SystemRunner.run(&tool).map_err(|e| e.to_string())?;
        let user = if cfg!(windows) {
            parse_whoami_sid(&out.stdout)
        } else {
            parse_id_u(&out.stdout)
        };
        user.ok_or_else(|| "Dyno could not tell which account is installing the helper.".into())
    }
}

#[derive(Default)]
struct Job {
    installing: bool,
    error: Option<String>,
}

/// The helper's lifecycle over a link, an elevator and an environment.
pub struct HelperCentre {
    host: TuningHost,
    link: Arc<dyn HelperLink>,
    elevator: Arc<dyn Elevator>,
    env: Arc<dyn InstallEnv>,
    job: Arc<Mutex<Job>>,
}

impl HelperCentre {
    /// A centre for this computer over `link`.
    pub fn system(link: Arc<dyn HelperLink>) -> HelperCentre {
        HelperCentre::new(
            dyno_platform::elevate::this_host(),
            link,
            Arc::new(OsElevator),
            Arc::new(SystemEnv::new()),
        )
    }

    /// A centre that can never install (and never prompts): `reason` is what Settings says.
    pub fn unavailable(link: Arc<dyn HelperLink>, reason: &str) -> HelperCentre {
        struct Never(String);
        impl InstallEnv for Never {
            fn supported(&self) -> Result<(), String> {
                Err(self.0.clone())
            }
            fn helper_exe(&self) -> Result<PathBuf, String> {
                Err(self.0.clone())
            }
            fn agent_exe(&self) -> Result<PathBuf, String> {
                Err(self.0.clone())
            }
            fn user(&self) -> Result<HelperUser, String> {
                Err(self.0.clone())
            }
        }
        struct NoPrompt;
        impl Elevator for NoPrompt {
            fn run(&self, _: &std::path::Path, _: &[String], _: &str) -> Result<(), dyno_platform::elevate::ElevationError> {
                Err(dyno_platform::elevate::ElevationError::Unavailable(
                    "This agent does not install the helper.".into(),
                ))
            }
        }
        HelperCentre::new(
            dyno_platform::elevate::this_host(),
            link,
            Arc::new(NoPrompt),
            Arc::new(Never(reason.to_string())),
        )
    }

    /// A centre with every part given (tests).
    pub fn new(host: TuningHost, link: Arc<dyn HelperLink>, elevator: Arc<dyn Elevator>, env: Arc<dyn InstallEnv>) -> HelperCentre {
        HelperCentre {
            host,
            link,
            elevator,
            env,
            job: Arc::default(),
        }
    }

    fn assess(&self, probe: Probe) -> HelperStatus {
        let mut s = assess(self.host, self.env.supported(), probe, env!("CARGO_PKG_VERSION"));
        s.error = self.job.lock().unwrap_or_else(PoisonError::into_inner).error.clone();
        s
    }

    /// Runs the install: the OS prompt, then a check that the helper answers. Blocks until the prompt is answered.
    fn run_install(env: &dyn InstallEnv, elevator: &dyn Elevator, link: &dyn HelperLink, host: TuningHost) -> Result<(), String> {
        env.supported()?;
        let (helper, agent, user) = (env.helper_exe()?, env.agent_exe()?, env.user()?);
        let args = [
            "install".to_string(),
            "--user".into(),
            user.to_arg(),
            "--agent-exe".into(),
            agent.to_string_lossy().into_owned(),
        ];
        elevator.run(&helper, &args, prompt_text(host)).map_err(|e| e.to_string())?;
        match link.probe() {
            Probe::Answered(..) => Ok(()),
            Probe::Absent => Err(
                "The installer finished but the helper is not there. Try again; if it keeps failing, see docs/testing-manual.md.".into(),
            ),
            Probe::Silent(why) => Err(format!("The helper was installed but does not answer ({why}).")),
        }
    }

    /// Installs and waits for the result (what the background thread runs).
    pub fn install_blocking(&self) -> Result<(), String> {
        let result = Self::run_install(&*self.env, &*self.elevator, &*self.link, self.host);
        let mut job = self.job.lock().unwrap_or_else(PoisonError::into_inner);
        job.installing = false;
        job.error = result.clone().err();
        result
    }
}

impl HelperControl for HelperCentre {
    fn can_install(&self) -> Result<(), String> {
        self.env.supported()
    }

    fn status(&mut self) -> HelperStatus {
        if self.job.lock().unwrap_or_else(PoisonError::into_inner).installing {
            let mut s = self.assess(Probe::Absent);
            s.state = HelperState::Installing;
            s.message = "Waiting for you to answer the system's permission prompt.".into();
            return s;
        }
        let probe = self.link.probe();
        self.assess(probe)
    }

    fn install(&mut self) -> Result<HelperStatus, AgentError> {
        self.env.supported().map_err(|m| AgentError::new(ErrorCode::Unsupported, m))?;
        {
            let mut job = self.job.lock().unwrap_or_else(PoisonError::into_inner);
            if job.installing {
                return Err(AgentError::new(
                    ErrorCode::Busy,
                    "The helper is already being installed; answer the system's prompt.",
                ));
            }
            job.installing = true;
            job.error = None;
        }
        let me = HelperCentre {
            host: self.host,
            link: self.link.clone(),
            elevator: self.elevator.clone(),
            env: self.env.clone(),
            job: self.job.clone(),
        };
        let spawned = std::thread::Builder::new().name("helper-install".into()).spawn(move || {
            let _ = me.install_blocking();
        });
        if let Err(e) = spawned {
            let mut job = self.job.lock().unwrap_or_else(PoisonError::into_inner);
            job.installing = false;
            return Err(AgentError::new(
                ErrorCode::Internal,
                format!("Dyno could not start the installer: {e}"),
            ));
        }
        Ok(self.status())
    }

    fn uninstall(&mut self) -> Result<HelperStatus, AgentError> {
        self.link.uninstall()?;
        // The helper removes itself a moment after answering; wait until it is gone.
        for _ in 0..15 {
            if !matches!(self.link.probe(), Probe::Answered(..)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        self.job.lock().unwrap_or_else(PoisonError::into_inner).error = None;
        Ok(self.status())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::helper::{HelperAction, TuningHost};
    use dyno_core::helper_wire::{HelperInfo, HELPER_API};
    use dyno_core::network_traffic::TrafficCounter;
    use dyno_core::ports::PlatformError;
    use dyno_platform::elevate::ElevationError;

    struct Env {
        supported: Result<(), String>,
    }
    impl InstallEnv for Env {
        fn supported(&self) -> Result<(), String> {
            self.supported.clone()
        }
        fn helper_exe(&self) -> Result<PathBuf, String> {
            Ok(PathBuf::from("/app/dyno-helper"))
        }
        fn agent_exe(&self) -> Result<PathBuf, String> {
            Ok(PathBuf::from("/app/dyno-agent"))
        }
        fn user(&self) -> Result<HelperUser, String> {
            Ok(HelperUser::Uid(501))
        }
    }

    /// A link whose helper "appears" once the elevator ran.
    struct Link {
        installed: Arc<Mutex<bool>>,
        removed_calls: Mutex<u32>,
        version: String,
    }
    impl HelperLink for Link {
        fn probe(&self) -> Probe {
            if *self.installed.lock().unwrap() {
                Probe::Answered(
                    HelperInfo {
                        version: self.version.clone(),
                        api: HELPER_API,
                        host: TuningHost::Macos,
                        agent_exe: "/app/dyno-agent".into(),
                    },
                    vec![],
                )
            } else {
                Probe::Absent
            }
        }
        fn apply(&self, _: &HelperAction) -> Result<(), PlatformError> {
            Ok(())
        }
        fn read(&self, _: &HelperAction) -> Result<String, PlatformError> {
            Ok(String::new())
        }
        fn traffic(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
            Ok(vec![])
        }
        fn uninstall(&self) -> Result<(), PlatformError> {
            *self.removed_calls.lock().unwrap() += 1;
            *self.installed.lock().unwrap() = false;
            Ok(())
        }
    }

    struct Elev {
        installed: Arc<Mutex<bool>>,
        answer: Result<(), ElevationError>,
        ran: Mutex<Vec<(PathBuf, Vec<String>)>>,
    }
    impl Elevator for Elev {
        fn run(&self, program: &std::path::Path, args: &[String], _: &str) -> Result<(), ElevationError> {
            self.ran.lock().unwrap().push((program.to_path_buf(), args.to_vec()));
            if self.answer.is_ok() {
                *self.installed.lock().unwrap() = true;
            }
            self.answer.clone()
        }
    }

    fn centre(answer: Result<(), ElevationError>, supported: Result<(), String>, version: &str) -> (HelperCentre, Arc<Elev>, Arc<Link>) {
        let installed = Arc::new(Mutex::new(false));
        let link = Arc::new(Link {
            installed: installed.clone(),
            removed_calls: Mutex::new(0),
            version: version.into(),
        });
        let elev = Arc::new(Elev {
            installed,
            answer,
            ran: Mutex::new(vec![]),
        });
        (
            HelperCentre::new(TuningHost::Macos, link.clone(), elev.clone(), Arc::new(Env { supported })),
            elev,
            link,
        )
    }

    fn wait_until_done(c: &mut HelperCentre) -> HelperStatus {
        for _ in 0..100 {
            let s = c.status();
            if s.state != HelperState::Installing {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the install never finished");
    }

    #[test]
    fn nothing_is_installed_until_the_user_clicks_and_then_the_prompt_runs_the_installer_with_typed_arguments() {
        let (mut c, elev, _) = centre(Ok(()), Ok(()), env!("CARGO_PKG_VERSION"));
        assert_eq!(c.status().state, HelperState::NotInstalled);
        assert!(elev.ran.lock().unwrap().is_empty(), "looking at the state never prompts");
        c.install().unwrap();
        let s = wait_until_done(&mut c);
        assert_eq!(s.state, HelperState::Ready);
        assert_eq!(s.error, None);
        let ran = elev.ran.lock().unwrap();
        assert_eq!(ran.len(), 1);
        assert_eq!(ran[0].0, PathBuf::from("/app/dyno-helper"));
        assert_eq!(ran[0].1, ["install", "--user", "uid:501", "--agent-exe", "/app/dyno-agent"]);
    }

    #[test]
    fn declining_the_prompt_leaves_dyno_working_and_says_so() {
        let (mut c, _, _) = centre(Err(ElevationError::Declined), Ok(()), "1");
        c.install().unwrap();
        let s = wait_until_done(&mut c);
        assert_eq!(s.state, HelperState::NotInstalled);
        assert!(s.error.as_deref().unwrap().contains("without the helper"));
        // Asking again clears the old error.
        c.install().unwrap();
        wait_until_done(&mut c);
    }

    #[test]
    fn a_failed_installer_and_a_helper_that_does_not_answer_are_errors_with_a_reason() {
        let (mut c, _, _) = centre(
            Err(ElevationError::Failed(Some(1), "The helper installer did not finish: boom".into())),
            Ok(()),
            "1",
        );
        c.install().unwrap();
        assert!(wait_until_done(&mut c).error.unwrap().contains("boom"));
        // The installer reports success but nothing answers.
        struct Nothing;
        impl Elevator for Nothing {
            fn run(&self, _: &std::path::Path, _: &[String], _: &str) -> Result<(), ElevationError> {
                Ok(())
            }
        }
        let link = Arc::new(Link {
            installed: Arc::new(Mutex::new(false)),
            removed_calls: Mutex::new(0),
            version: "1".into(),
        });
        let mut c = HelperCentre::new(TuningHost::Macos, link, Arc::new(Nothing), Arc::new(Env { supported: Ok(()) }));
        c.install().unwrap();
        assert!(wait_until_done(&mut c).error.unwrap().contains("not there"));
    }

    #[test]
    fn a_second_click_while_the_prompt_is_open_is_busy() {
        struct Slow(Mutex<std::sync::mpsc::Receiver<()>>);
        impl Elevator for Slow {
            fn run(&self, _: &std::path::Path, _: &[String], _: &str) -> Result<(), ElevationError> {
                let _ = self.0.lock().unwrap().recv_timeout(Duration::from_secs(5));
                Err(ElevationError::Declined)
            }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let link = Arc::new(Link {
            installed: Arc::new(Mutex::new(false)),
            removed_calls: Mutex::new(0),
            version: "1".into(),
        });
        let mut c = HelperCentre::new(
            TuningHost::Macos,
            link,
            Arc::new(Slow(Mutex::new(rx))),
            Arc::new(Env { supported: Ok(()) }),
        );
        let first = c.install().unwrap();
        assert_eq!(first.state, HelperState::Installing);
        assert!(first.message.contains("permission prompt"));
        assert_eq!(c.install().unwrap_err().code, ErrorCode::Busy);
        tx.send(()).unwrap();
        wait_until_done(&mut c);
    }

    #[test]
    fn a_computer_that_cannot_install_it_says_what_is_missing_and_never_prompts() {
        let (mut c, elev, _) = centre(Ok(()), Err("pkexec (polkit) is not installed.".into()), "1");
        assert_eq!(c.status().state, HelperState::Unsupported);
        assert_eq!(c.install().unwrap_err().code, ErrorCode::Unsupported);
        assert!(c.can_install().is_err());
        assert!(elev.ran.lock().unwrap().is_empty());
    }

    #[test]
    fn a_helper_of_another_version_is_outdated_and_uninstalling_waits_until_it_is_gone() {
        let (mut c, _, link) = centre(Ok(()), Ok(()), "0.0.1");
        c.install().unwrap();
        let s = wait_until_done(&mut c);
        assert_eq!(s.state, HelperState::Outdated);
        assert!(s.message.contains("Update the helper"));
        let after = c.uninstall().unwrap();
        assert_eq!(after.state, HelperState::NotInstalled);
        assert_eq!(*link.removed_calls.lock().unwrap(), 1);
    }

    #[test]
    fn account_ids_are_read_from_the_tools_that_print_them() {
        assert_eq!(parse_id_u("501\n"), Some(HelperUser::Uid(501)));
        assert_eq!(parse_id_u("root"), None);
        let csv = "\"DESKTOP-1\\me\",\"S-1-5-21-111-222-333-1001\"\r\n";
        assert_eq!(parse_whoami_sid(csv), Some(HelperUser::Sid("S-1-5-21-111-222-333-1001".into())));
        assert_eq!(parse_whoami_sid("ERROR: nope"), None);
        assert_eq!(parse_whoami_sid("\"a\",\"S-1-5-21-1-\""), None);
    }

    #[test]
    fn an_unavailable_centre_never_installs_or_prompts() {
        let link = Arc::new(Link {
            installed: Arc::new(Mutex::new(false)),
            removed_calls: Mutex::new(0),
            version: "1".into(),
        });
        let mut c = HelperCentre::unavailable(link, "Own folder.");
        assert_eq!(c.status().state, HelperState::Unsupported);
        assert_eq!(c.status().message, "Own folder.");
        assert_eq!(c.install().unwrap_err().code, ErrorCode::Unsupported);
        assert_eq!(c.can_install(), Err("Own folder.".to_string()));
    }

    #[test]
    fn the_real_environment_finds_this_programs_neighbour_or_says_it_is_missing() {
        let env = SystemEnv::new();
        // In a test the helper is not next to the test program: the answer names the file to reinstall.
        let e = env.helper_exe().unwrap_err();
        assert!(e.contains("dyno-helper") && e.contains("reinstall"), "{e}");
        assert!(env.agent_exe().unwrap().is_absolute());
        #[cfg(unix)]
        assert!(matches!(env.user(), Ok(HelperUser::Uid(_))));
        let _ = env.supported();
    }
}
