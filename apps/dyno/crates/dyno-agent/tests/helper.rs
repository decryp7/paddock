#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The privileged helper end to end, in process: a real agent (fake platform), a real helper daemon on a private
//! endpoint with its real authentication (the user's id, the program's path, the token), the agent's real link, and
//! fakes for the operating system and for the permission prompt. Nothing here installs anything or touches the
//! machine.

use dyno_agent::agent::lock;
use dyno_agent::cli::{ask, connect};
use dyno_agent::helper_centre::{HelperCentre, InstallEnv};
use dyno_agent::helper_link::{HelperLink, IpcHelperLink};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::helper::HelperHost;
use dyno_core::helper_wire::{HelperInfo, HelperState, HelperUser, Probe, HELPER_API};
use dyno_core::protocol::{Message, Reply, Request};
use dyno_core::time::Timestamp;
use dyno_helper::audit::AuditLog;
use dyno_helper::auth::{OsImages, Policy};
use dyno_helper::dispatch::{Dispatcher, Parts};
use dyno_helper::server::Daemon;
use dyno_ipc::{Host, Peer};
use dyno_platform::elevate::{ElevationError, Elevator};
use dyno_platform::fake::{quiet_sample, Fake};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The user the daemon serves: this process's owner (Unix) or any account (Windows, where the pipe list decides).
fn this_user() -> HelperUser {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let f = tempfile::NamedTempFile::new().unwrap();
        HelperUser::Uid(std::fs::metadata(f.path()).unwrap().uid())
    }
    #[cfg(not(unix))]
    {
        HelperUser::Sid("S-1-5-21-1-2-3-1001".into())
    }
}

/// A daemon thread on its own private endpoint over `system`, serving this very test program.
struct Helper {
    endpoint: String,
    token_file: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<Result<(), String>>>,
}

fn start_helper(dir: &Path, agent_exe: PathBuf) -> Helper {
    let host = Host::listen().unwrap();
    let (endpoint, token) = (host.name().to_string(), host.token().clone());
    let token_file = dir.join("token");
    std::fs::write(&token_file, token.to_hex()).unwrap();
    let dispatcher = Arc::new(Dispatcher::new(Parts {
        info: HelperInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            api: HELPER_API,
            host: HelperHost::Macos,
            agent_exe: agent_exe.to_string_lossy().into_owned(),
        },
        traffic: Box::new(NoTraffic),
        audit: AuditLog::new(dir.join("audit.jsonl")),
        clock: Box::new(|| 1_000),
        on_uninstall: Box::new(|| {}),
    }));
    let stop = Arc::new(AtomicBool::new(false));
    let daemon = Daemon {
        host,
        policy: Policy {
            user: this_user(),
            agent_exe,
        },
        images: Arc::new(OsImages),
        dispatcher,
        stop: stop.clone(),
    };
    Helper {
        endpoint,
        token_file,
        stop,
        thread: Some(std::thread::spawn(move || daemon.run())),
    }
}

struct NoTraffic;
impl dyno_core::ports::ProcessTraffic for NoTraffic {
    fn counters(&self) -> Result<Vec<dyno_core::network_traffic::TrafficCounter>, dyno_core::ports::PlatformError> {
        Err(dyno_core::ports::PlatformError::Unsupported("none".into()))
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Rig {
    running: Option<Running>,
    peer: Peer<Message>,
    _helper: Option<Helper>,
    _tmp: tempfile::TempDir,
}

/// A macOS laptop. `with_helper` starts the daemon first.
fn rig(with_helper: bool) -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let fake = Fake::new();
    fake.push_sample(quiet_sample(Timestamp(0)));
    let running = Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
    std::fs::create_dir_all(tmp.path().join("helper")).unwrap();
    let helper = with_helper.then(|| start_helper(&tmp.path().join("helper"), std::env::current_exe().unwrap()));
    let link = Arc::new(match &helper {
        Some(h) => IpcHelperLink::new(h.endpoint.clone(), h.token_file.clone()),
        None => IpcHelperLink::new("/nonexistent/dyno-test.sock", tmp.path().join("no-token")),
    });
    // Never the real helper or the real prompt: a helper that can be installed here, and a prompt nobody answers.
    *lock(&running.shared.ports.helper) = Box::new(HelperCentre::new(
        HelperHost::Macos,
        link.clone(),
        Arc::new(NoPrompt),
        Arc::new(Env),
    ));
    let peer = connect(&dirs).unwrap().0;
    let rig = Rig {
        running: Some(running),
        peer,
        _helper: helper,
        _tmp: tmp,
    };
    for _ in 0..100 {
        if ask(&rig.peer, Request::Health).is_ok() {
            return rig;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("the agent never took a first measurement");
}

impl Drop for Rig {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop();
        }
    }
}

/// A permission prompt that nobody answers.
struct NoPrompt;
impl Elevator for NoPrompt {
    fn run(&self, _: &Path, _: &[String], _: &str) -> Result<(), ElevationError> {
        Err(ElevationError::Declined)
    }
}

/// An environment where everything the installer needs is present.
struct Env;
impl InstallEnv for Env {
    fn supported(&self) -> Result<(), String> {
        Ok(())
    }
    fn helper_exe(&self) -> Result<PathBuf, String> {
        Ok(PathBuf::from("/app/dyno-helper"))
    }
    fn agent_exe(&self) -> Result<PathBuf, String> {
        std::env::current_exe().map_err(|e| e.to_string())
    }
    fn user(&self) -> Result<HelperUser, String> {
        Ok(this_user())
    }
}

/// A permission prompt that starts the daemon when the user "approves" (or refuses).
struct Prompt {
    approve: bool,
    dir: PathBuf,
    started: Mutex<Option<Helper>>,
    link_target: Arc<Mutex<Option<(String, PathBuf)>>>,
}
impl Elevator for Prompt {
    fn run(&self, _: &Path, args: &[String], _: &str) -> Result<(), ElevationError> {
        if !self.approve {
            return Err(ElevationError::Declined);
        }
        assert_eq!(args[0], "install");
        let h = start_helper(&self.dir, std::env::current_exe().unwrap());
        *self.link_target.lock().unwrap() = Some((h.endpoint.clone(), h.token_file.clone()));
        *self.started.lock().unwrap() = Some(h);
        Ok(())
    }
}

/// A link that follows the endpoint the prompt created.
struct FollowingLink(Arc<Mutex<Option<(String, PathBuf)>>>);
impl FollowingLink {
    fn current(&self) -> IpcHelperLink {
        match self.0.lock().unwrap().clone() {
            Some((e, t)) => IpcHelperLink::new(e, t),
            None => IpcHelperLink::new("/nonexistent/dyno-test.sock", "/nonexistent/dyno-test-token"),
        }
    }
}
impl HelperLink for FollowingLink {
    fn probe(&self) -> Probe {
        self.current().probe()
    }
    fn traffic(&self) -> Result<Vec<dyno_core::network_traffic::TrafficCounter>, dyno_core::ports::PlatformError> {
        self.current().traffic()
    }
    fn uninstall(&self) -> Result<(), dyno_core::ports::PlatformError> {
        self.current().uninstall()
    }
}

fn wait_for(rig: &Rig, done: impl Fn(&dyno_core::helper_wire::HelperStatus) -> bool) -> dyno_core::helper_wire::HelperStatus {
    for _ in 0..100 {
        let Reply::Helper(s) = ask(&rig.peer, Request::HelperStatus).unwrap() else {
            panic!()
        };
        if done(&s) {
            return *s;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the helper never reached the expected state");
}

#[test]
fn the_install_click_prompts_once_and_then_the_helper_answers_but_declining_changes_nothing() {
    for approve in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let rig = rig(false);
        let target = Arc::new(Mutex::new(None));
        let prompt = Arc::new(Prompt {
            approve,
            dir: tmp.path().to_path_buf(),
            started: Mutex::new(None),
            link_target: target.clone(),
        });
        *lock(&rig.running.as_ref().unwrap().shared.ports.helper) = Box::new(HelperCentre::new(
            HelperHost::Macos,
            Arc::new(FollowingLink(target)),
            prompt.clone(),
            Arc::new(Env),
        ));
        // Looking never prompts.
        assert_eq!(wait_for(&rig, |_| true).state, HelperState::NotInstalled);
        assert!(prompt.started.lock().unwrap().is_none());
        let Reply::Helper(s) = ask(&rig.peer, Request::InstallHelper).unwrap() else {
            panic!()
        };
        assert!(matches!(
            s.state,
            HelperState::Installing | HelperState::NotInstalled | HelperState::Outdated | HelperState::Ready
        ));
        let done = wait_for(&rig, |s| s.state != HelperState::Installing);
        if approve {
            assert_eq!(done.state, HelperState::Ready, "{done:?}");
            assert_eq!(done.version.as_deref(), Some(env!("CARGO_PKG_VERSION")));
        } else {
            assert_eq!(done.state, HelperState::NotInstalled);
            assert!(done.error.unwrap().contains("without the helper"));
            assert!(prompt.started.lock().unwrap().is_none());
        }
    }
}
