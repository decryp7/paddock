#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The privileged helper end to end, in process: a real agent (fake platform), a real helper daemon on a private
//! endpoint with its real authentication (the user's id, the program's path, the token), the agent's real link, and
//! fakes for the operating system and for the permission prompt. Nothing here installs anything or touches the
//! machine: the "system" is a fake tuning system both sides share.

use dyno_agent::agent::lock;
use dyno_agent::cli::{ask, connect};
use dyno_agent::helper_centre::{HelperCentre, InstallEnv};
use dyno_agent::helper_link::{ElevatedTuning, HelperLink, IpcHelperLink};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::activity::UndoState;
use dyno_core::helper::{HelperAction, SleepSource, TuningHost};
use dyno_core::helper_wire::{AuditVerdict, HelperInfo, HelperState, HelperUser, Probe, HELPER_API};
use dyno_core::metrics::{BatterySample, PowerSource, Reading};
use dyno_core::protocol::{Message, Reply, Request};
use dyno_core::time::Timestamp;
use dyno_helper::audit::AuditLog;
use dyno_helper::auth::{OsImages, Policy};
use dyno_helper::dispatch::{Dispatcher, Parts};
use dyno_helper::server::Daemon;
use dyno_ipc::{Host, Peer};
use dyno_platform::elevate::{ElevationError, Elevator};
use dyno_platform::fake::{quiet_sample, Fake};
use dyno_platform::tuning::FakeTuning;
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

fn start_helper(dir: &Path, system: FakeTuning, agent_exe: PathBuf) -> Helper {
    let host = Host::listen().unwrap();
    let (endpoint, token) = (host.name().to_string(), host.token().clone());
    let token_file = dir.join("token");
    std::fs::write(&token_file, token.to_hex()).unwrap();
    let dispatcher = Arc::new(Dispatcher::new(Parts {
        info: HelperInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            api: HELPER_API,
            host: system_host(&system),
            agent_exe: agent_exe.to_string_lossy().into_owned(),
        },
        tuning: Box::new(system),
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

fn system_host(t: &FakeTuning) -> TuningHost {
    use dyno_core::ports::TuningSystem;
    t.clone().host()
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

fn battery() -> Reading<BatterySample> {
    Reading::Available(BatterySample {
        charge: 80.0,
        source: PowerSource::Battery,
        health: 95.0,
        cycles: None,
        seconds_to_empty: None,
    })
}

struct Rig {
    running: Option<Running>,
    peer: Peer<Message>,
    system: FakeTuning,
    _helper: Option<Helper>,
    link: Arc<IpcHelperLink>,
    _tmp: tempfile::TempDir,
}

fn sleep(minutes: u32) -> HelperAction {
    HelperAction::SetSleepMinutes {
        source: SleepSource::Battery,
        minutes,
    }
}

/// A macOS laptop whose sleep timer needs the helper. `with_helper` starts the daemon first.
fn rig(with_helper: bool) -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let fake = Fake::new();
    let mut sample = quiet_sample(Timestamp(0));
    sample.battery = battery();
    fake.push_sample(sample);
    let running = Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
    let system = FakeTuning::new(TuningHost::Macos);
    system.set_value(&sleep(0), "0");
    std::fs::create_dir_all(tmp.path().join("helper")).unwrap();
    let helper = with_helper.then(|| start_helper(&tmp.path().join("helper"), system.clone(), std::env::current_exe().unwrap()));
    let link = Arc::new(match &helper {
        Some(h) => IpcHelperLink::new(h.endpoint.clone(), h.token_file.clone()),
        None => IpcHelperLink::new("/nonexistent/dyno-test.sock", tmp.path().join("no-token")),
    });
    // The agent's own tuning sees the same fake system the helper changes; it never changes it itself.
    *lock(&running.shared.ports.tuning) = Box::new(ElevatedTuning::new(system.clone(), link.clone()));
    // Never the real helper or the real prompt: a helper that can be installed here, and a prompt nobody answers.
    *lock(&running.shared.ports.helper) = Box::new(HelperCentre::new(
        TuningHost::Macos,
        link.clone(),
        Arc::new(NoPrompt),
        Arc::new(Env),
    ));
    let peer = connect(&dirs).unwrap().0;
    let rig = Rig {
        running: Some(running),
        peer,
        system,
        _helper: helper,
        link,
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

fn applied(rig: &Rig) -> Vec<HelperAction> {
    rig.system.applied()
}

#[test]
fn an_administrator_item_is_applied_by_the_helper_logged_and_undone_through_it() {
    let rig = rig(true);
    let Reply::Tuning(list) = ask(&rig.peer, Request::Tuning).unwrap() else {
        panic!()
    };
    let item = list.iter().find(|s| s.id == "sleepOnBattery").unwrap();
    assert!(item.needs_admin);
    let Reply::Activity(entry) = ask(
        &rig.peer,
        Request::ApplyTuning {
            id: "sleepOnBattery".into(),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(applied(&rig), [sleep(15)], "the change reached the system through the helper");
    // The helper's own audit log saw it, with the value before.
    let Probe::Answered(_, recent) = rig.link.probe() else {
        panic!("the helper should answer")
    };
    assert_eq!(recent[0].verdict, AuditVerdict::Applied);
    assert_eq!((recent[0].before.as_deref(), recent[0].after.as_deref()), (Some("0"), Some("15")));
    // And the activity log can undo it, again through the helper.
    let Reply::Activity(undone) = ask(&rig.peer, Request::Undo { activity_id: entry.id }).unwrap() else {
        panic!()
    };
    assert_eq!(undone.undo, UndoState::Undone);
    assert_eq!(applied(&rig), [sleep(15), sleep(0)]);
}

#[test]
fn without_a_helper_the_click_gets_a_clear_next_step_and_nothing_changes() {
    let rig = rig(false);
    let err = ask(
        &rig.peer,
        Request::ApplyTuning {
            id: "sleepOnBattery".into(),
        },
    )
    .unwrap_err();
    assert!(err.contains("Settings > Helper"), "{err}");
    assert!(applied(&rig).is_empty());
    assert!(!matches!(rig.link.probe(), Probe::Answered(..)));
}

#[test]
fn a_helper_that_serves_another_program_refuses_this_agent_and_the_item_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let system = FakeTuning::new(TuningHost::Macos);
    let helper = start_helper(tmp.path(), system.clone(), PathBuf::from("/opt/not-dyno/dyno-agent"));
    let link = IpcHelperLink::new(helper.endpoint.clone(), helper.token_file.clone());
    let err = dyno_core::ports::TuningSystem::apply(&mut ElevatedTuning::new(system.clone(), Arc::new(link)), &sleep(5)).unwrap_err();
    assert!(err.to_string().contains("did not answer"), "{err}");
    assert!(system.applied().is_empty());
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
    system: FakeTuning,
    started: Mutex<Option<Helper>>,
    link_target: Arc<Mutex<Option<(String, PathBuf)>>>,
}
impl Elevator for Prompt {
    fn run(&self, _: &Path, args: &[String], _: &str) -> Result<(), ElevationError> {
        if !self.approve {
            return Err(ElevationError::Declined);
        }
        assert_eq!(args[0], "install");
        let h = start_helper(&self.dir, self.system.clone(), std::env::current_exe().unwrap());
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
    fn apply(&self, a: &HelperAction) -> Result<(), dyno_core::ports::PlatformError> {
        self.current().apply(a)
    }
    fn read(&self, a: &HelperAction) -> Result<String, dyno_core::ports::PlatformError> {
        self.current().read(a)
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
            system: rig.system.clone(),
            started: Mutex::new(None),
            link_target: target.clone(),
        });
        *lock(&rig.running.as_ref().unwrap().shared.ports.helper) = Box::new(HelperCentre::new(
            TuningHost::Macos,
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
