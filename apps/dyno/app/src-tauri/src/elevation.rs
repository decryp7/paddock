//! "Run as administrator" for this session. Starts Dyno's agent again with administrator rights after the OS prompt
//! (UAC on Windows, the password dialog on macOS, polkit on Linux), moves the link to it and shuts the normal agent down;
//! "Go back to normal rights" does the reverse. The elevated agent keeps its data in its own folder next to Dyno's, is never
//! registered to start at login and ends when Dyno quits. A cancelled prompt leaves the normal agent running.
//!
//! The steps are plain code over two traits ([`SessionLauncher`] for the OS prompt, [`SessionLink`] for the connection), so
//! the tests run them against fakes: nothing in the test suite may prompt (ER-14).

use crate::dto::ElevationStatus;
use crate::link::AgentLink;
use dyno_agent::identity;
use dyno_agent::paths::Dirs;
use dyno_platform::elevate::ElevationError;
use dyno_platform::session_elevate::{OsSessionLauncher, SessionLauncher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The folder (inside Dyno's data folder) of the elevated session.
const SESSION_FOLDER: &str = "elevated-session";
/// How long the elevated agent has to appear after the OS prompt was accepted.
const AGENT_UP_WITHIN: Duration = Duration::from_secs(30);
/// What the person sees when they cancel the prompt.
pub const DECLINED_MESSAGE: &str = "Dyno kept running with your normal rights. Nothing was changed.";

/// The connection side of a session switch.
pub trait SessionLink: Send + Sync {
    /// Stops (or allows again) starting a normal agent while the switch is under way.
    fn hold_spawn(&self, hold: bool);
    /// Whether the agent runs with administrator rights.
    fn is_elevated(&self) -> bool;
    /// Moves to the elevated agent in `session`; `cleanup` is removed when the session ends.
    fn enter_session(&self, session: Dirs, cleanup: PathBuf);
    /// Moves back to a normal agent.
    fn leave_session(&self);
}

impl SessionLink for AgentLink {
    fn hold_spawn(&self, hold: bool) {
        AgentLink::hold_spawn(self, hold);
    }
    fn is_elevated(&self) -> bool {
        AgentLink::is_elevated(self)
    }
    fn enter_session(&self, session: Dirs, cleanup: PathBuf) {
        AgentLink::enter_session(self, session, cleanup);
    }
    fn leave_session(&self) {
        AgentLink::leave_session(self);
    }
}

/// The arguments that start the agent elevated: its own folder, the user it is for and that user's Unix ids.
pub fn elevated_args(folder: &Path, user: &str, ids: Option<(u32, u32)>) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        "--elevated".to_string(),
        folder.to_string_lossy().into_owned(),
        user.to_string(),
    ];
    if let Some((uid, gid)) = ids {
        args.push(uid.to_string());
        args.push(gid.to_string());
    }
    args
}

/// The session folder inside `home`.
pub fn session_folder(home: &Dirs) -> PathBuf {
    home.root().join(SESSION_FOLDER)
}

/// Makes the session folder empty and copies the person's settings into it, so the elevated agent measures as they asked.
/// Files the elevated agent writes there belong to the person's own folder, which they can remove.
fn prepare(home: &Dirs, folder: &Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(folder);
    std::fs::create_dir_all(folder).map_err(|e| format!("Dyno could not make its folder for this session: {e}"))?;
    let settings = home.settings();
    if settings.is_file() {
        // Best effort: without it the session starts with the defaults.
        let _ = std::fs::copy(&settings, Dirs::at(folder).settings());
    }
    Ok(())
}

/// Starts and ends elevated sessions.
pub struct ElevationCtl {
    home: Dirs,
    launcher: Box<dyn SessionLauncher>,
    agent: Box<dyn Fn() -> Result<PathBuf, String> + Send + Sync>,
    up_within: Duration,
    busy: Mutex<()>,
}

impl ElevationCtl {
    /// A controller using the OS prompt and the agent program next to the app.
    pub fn os(home: Dirs) -> ElevationCtl {
        ElevationCtl::new(
            home,
            Box::new(OsSessionLauncher),
            Box::new(crate::login::agent_program),
            AGENT_UP_WITHIN,
        )
    }

    /// A controller over a launcher and an agent program lookup (tests pass fakes).
    pub fn new(
        home: Dirs,
        launcher: Box<dyn SessionLauncher>,
        agent: Box<dyn Fn() -> Result<PathBuf, String> + Send + Sync>,
        up_within: Duration,
    ) -> ElevationCtl {
        ElevationCtl {
            home,
            launcher,
            agent,
            up_within,
            busy: Mutex::new(()),
        }
    }

    /// Restarts the agent with administrator rights. A cancelled prompt is `Ok` with a calm message and the normal agent
    /// keeps running; a failure is an `Err` with what to do.
    pub fn run_as_administrator(&self, link: &dyn SessionLink) -> Result<ElevationStatus, String> {
        let _one_at_a_time = self.busy.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if link.is_elevated() {
            return Ok(ElevationStatus {
                elevated: true,
                message: None,
            });
        }
        let program = (self.agent)()?;
        let folder = session_folder(&self.home);
        prepare(&self.home, &folder)?;
        let session = Dirs::at(&folder);
        link.hold_spawn(true);
        let args = elevated_args(&folder, &identity::this_user_key(), identity::this_ids());
        let up = || dyno_agent::discovery::Discovery::read(&session.discovery()).is_some();
        let started = self.launcher.start(
            &program,
            &args,
            "Dyno needs your permission to read every process on this computer.",
            &up,
        );
        let outcome = match started {
            Ok(()) => wait_until(&up, self.up_within)
                .then_some(())
                .ok_or_else(|| "Dyno started as administrator but it did not answer in time.".to_string()),
            Err(ElevationError::Declined) => {
                link.hold_spawn(false);
                let _ = std::fs::remove_dir_all(&folder);
                return Ok(ElevationStatus {
                    elevated: false,
                    message: Some(DECLINED_MESSAGE.into()),
                });
            }
            Err(e) => Err(e.to_string()),
        };
        match outcome {
            Ok(()) => {
                link.enter_session(session, folder);
                Ok(ElevationStatus {
                    elevated: true,
                    message: None,
                })
            }
            Err(message) => {
                link.hold_spawn(false);
                let _ = std::fs::remove_dir_all(&folder);
                Err(message)
            }
        }
    }

    /// Goes back to normal rights: the elevated agent exits and a normal one starts.
    pub fn run_unelevated(&self, link: &dyn SessionLink) -> Result<ElevationStatus, String> {
        let _one_at_a_time = self.busy.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if link.is_elevated() {
            link.hold_spawn(true);
            link.leave_session();
        }
        Ok(ElevationStatus {
            elevated: false,
            message: None,
        })
    }
}

fn wait_until(condition: &dyn Fn() -> bool, within: Duration) -> bool {
    let end = Instant::now() + within;
    loop {
        if condition() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    type Started = Arc<Mutex<Vec<(PathBuf, Vec<String>)>>>;

    /// Pretends to be the OS prompt: records what it was asked to start and answers as told.
    struct FakeLauncher {
        answer: Mutex<Option<Result<(), ElevationError>>>,
        started: Started,
        writes_discovery: bool,
    }

    impl SessionLauncher for FakeLauncher {
        fn start(&self, program: &Path, args: &[String], _prompt: &str, _ready: &dyn Fn() -> bool) -> Result<(), ElevationError> {
            self.started.lock().unwrap().push((program.to_path_buf(), args.to_vec()));
            let answer = self.answer.lock().unwrap().take().unwrap_or(Ok(()));
            if answer.is_ok() && self.writes_discovery {
                dyno_agent::discovery::Discovery {
                    endpoint: "e".into(),
                    token: "00".into(),
                    pid: 1,
                    started: 0,
                    version: "0".into(),
                    protocol: 1,
                    elevated_for: Some(args[3].clone()),
                }
                .write(&Dirs::at(&args[2]).discovery())
                .unwrap();
            }
            answer
        }
    }

    #[derive(Default)]
    struct FakeLink {
        elevated: AtomicBool,
        held: AtomicBool,
        calls: Mutex<Vec<String>>,
    }

    impl SessionLink for FakeLink {
        fn hold_spawn(&self, hold: bool) {
            self.held.store(hold, Ordering::SeqCst);
            self.calls.lock().unwrap().push(format!("hold {hold}"));
        }
        fn is_elevated(&self) -> bool {
            self.elevated.load(Ordering::SeqCst)
        }
        fn enter_session(&self, session: Dirs, cleanup: PathBuf) {
            assert_eq!(session.root(), cleanup);
            self.elevated.store(true, Ordering::SeqCst);
            self.held.store(false, Ordering::SeqCst);
            self.calls.lock().unwrap().push("enter".into());
        }
        fn leave_session(&self) {
            self.elevated.store(false, Ordering::SeqCst);
            self.held.store(false, Ordering::SeqCst);
            self.calls.lock().unwrap().push("leave".into());
        }
    }

    fn ctl(answer: Result<(), ElevationError>, writes_discovery: bool) -> (ElevationCtl, Started, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let started = Arc::new(Mutex::new(Vec::new()));
        let launcher = FakeLauncher {
            answer: Mutex::new(Some(answer)),
            started: started.clone(),
            writes_discovery,
        };
        let ctl = ElevationCtl::new(
            Dirs::at(tmp.path().join("Dyno")),
            Box::new(launcher),
            Box::new(|| Ok(PathBuf::from("/app/dyno-agent"))),
            Duration::from_millis(200),
        );
        (ctl, started, tmp)
    }

    #[test]
    fn an_accepted_prompt_starts_the_agent_in_its_own_folder_and_moves_the_link() {
        let (ctl, started, _tmp) = ctl(Ok(()), true);
        std::fs::create_dir_all(ctl.home.root()).unwrap();
        std::fs::write(ctl.home.settings(), b"{\"x\":1}").unwrap();
        let link = FakeLink::default();
        let status = ctl.run_as_administrator(&link).unwrap();
        assert_eq!(
            status,
            ElevationStatus {
                elevated: true,
                message: None
            }
        );
        assert_eq!(*link.calls.lock().unwrap(), ["hold true", "enter"]);
        let started = started.lock().unwrap();
        assert_eq!(started.len(), 1);
        let (program, args) = &started[0];
        assert_eq!(program, Path::new("/app/dyno-agent"));
        assert_eq!(&args[..2], ["run", "--elevated"]);
        assert_eq!(
            Path::new(&args[2]),
            session_folder(&ctl.home),
            "its data stays in Dyno's own folder"
        );
        assert_eq!(args[3], identity::this_user_key());
        assert_eq!(
            std::fs::read(Dirs::at(&args[2]).settings()).unwrap(),
            b"{\"x\":1}",
            "the person's settings carry over"
        );
        // Asking again while elevated does not prompt again.
        assert!(ctl.run_as_administrator(&link).unwrap().elevated);
        assert_eq!(started.len(), 1);
    }

    #[test]
    fn a_cancelled_prompt_leaves_the_normal_agent_running_with_a_calm_message() {
        let (ctl, _started, _tmp) = ctl(Err(ElevationError::Declined), false);
        let link = FakeLink::default();
        let status = ctl.run_as_administrator(&link).unwrap();
        assert!(!status.elevated);
        assert_eq!(status.message.as_deref(), Some(DECLINED_MESSAGE));
        assert!(
            !link.is_elevated() && !link.held.load(Ordering::SeqCst),
            "spawning is allowed again"
        );
        assert!(!session_folder(&ctl.home).exists(), "nothing is left behind");
        assert_eq!(*link.calls.lock().unwrap(), ["hold true", "hold false"]);
    }

    #[test]
    fn failures_and_an_agent_that_never_appears_are_errors_that_restore_the_normal_agent() {
        let (ctl, _s, _tmp) = ctl(Err(ElevationError::Unavailable("no polkit".into())), false);
        let link = FakeLink::default();
        assert_eq!(ctl.run_as_administrator(&link).unwrap_err(), "no polkit");
        assert!(!link.held.load(Ordering::SeqCst));
        let (ctl, _s, _tmp) = ctl_never_up();
        let link = FakeLink::default();
        assert!(ctl.run_as_administrator(&link).unwrap_err().contains("did not answer"));
        assert!(!link.is_elevated() && !link.held.load(Ordering::SeqCst));
        assert!(!session_folder(&ctl.home).exists());
    }

    fn ctl_never_up() -> (ElevationCtl, Started, tempfile::TempDir) {
        ctl(Ok(()), false)
    }

    #[test]
    fn going_back_to_normal_rights_ends_the_session_and_does_nothing_when_not_elevated() {
        let (ctl, _s, _tmp) = ctl(Ok(()), true);
        let link = FakeLink::default();
        assert!(!ctl.run_unelevated(&link).unwrap().elevated);
        assert!(link.calls.lock().unwrap().is_empty(), "nothing to undo");
        ctl.run_as_administrator(&link).unwrap();
        assert!(!ctl.run_unelevated(&link).unwrap().elevated);
        assert_eq!(*link.calls.lock().unwrap(), ["hold true", "enter", "hold true", "leave"]);
        assert!(!link.is_elevated());
    }

    #[test]
    fn the_elevated_arguments_carry_the_folder_the_user_and_the_unix_id() {
        let a = elevated_args(Path::new("/d/elevated-session"), "uid:501", Some((501, 20)));
        assert_eq!(a, ["run", "--elevated", "/d/elevated-session", "uid:501", "501", "20"]);
        assert_eq!(elevated_args(Path::new("C:\\d"), "user:alex", None).len(), 4);
    }

    #[test]
    fn a_missing_agent_program_is_reported_before_anything_is_asked() {
        let tmp = tempfile::tempdir().unwrap();
        let ctl = ElevationCtl::new(
            Dirs::at(tmp.path()),
            Box::new(FakeLauncher {
                answer: Mutex::new(None),
                started: Arc::default(),
                writes_discovery: false,
            }),
            Box::new(|| Err("The Dyno agent is missing.".into())),
            Duration::from_millis(10),
        );
        let link = FakeLink::default();
        assert_eq!(ctl.run_as_administrator(&link).unwrap_err(), "The Dyno agent is missing.");
        assert!(link.calls.lock().unwrap().is_empty());
    }
}
