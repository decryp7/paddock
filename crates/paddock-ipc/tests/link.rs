//! The companion link: the discovery file (owner-only, stale, damaged, untrusted), the plan, reconnecting with back-off on
//! a fake clock (nothing sleeps), and a second build replacing the first with real processes.

mod common;
use common::{GUARD, T, child_program};
use paddock_ipc::frame::ProtocolVersion;
use paddock_ipc::link::{
    ClientConfig, Discovery, Found, Launcher, LinkClient, LinkServer, LinkState, Plan, Replace,
    ServerConfig, discovery, plan,
};
use paddock_ipc::{CallCtx, Handler, RemoteError, Token};
use paddock_jobs::time::{BackoffPolicy, FakeClock, GiveUp};
use std::io::BufRead;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn sample(pid: u32) -> Discovery {
    Discovery {
        endpoint: "/tmp/x/s.sock".into(),
        token: Token::from_bytes([7; 32]).to_hex(),
        pid,
        build: "1.0.0".into(),
        protocol: ProtocolVersion::new(1, 0),
        started_ms: 5,
    }
}

#[test]
fn the_discovery_file_is_private_and_removed_by_its_owner_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("agent.json");
    let d = sample(std::process::id());
    d.write(&path).unwrap();
    assert_eq!(discovery::read(&path, |_| true), Found::Live(d.clone()));
    assert!(d.token().unwrap().matches(&[7; 32]));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let other = sample(std::process::id() + 1);
    other.write(&path).unwrap();
    Discovery::remove_if_owned_by(&path, std::process::id());
    assert!(path.exists(), "another process's file stays");
    Discovery::remove_if_owned_by(&path, other.pid);
    assert!(!path.exists());
    assert!(
        Discovery {
            token: "zz".into(),
            ..d
        }
        .token()
        .is_none()
    );
}

#[test]
fn damaged_huge_dead_or_shared_files_are_not_used() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    assert_eq!(discovery::read(&path, |_| true), Found::Nothing);
    paddock_env::write_atomic(&path, b"{nope", true).unwrap();
    assert_eq!(discovery::read(&path, |_| true), Found::Nothing);
    paddock_env::write_atomic(&path, &vec![b' '; 20_000], true).unwrap();
    assert_eq!(discovery::read(&path, |_| true), Found::Nothing);
    // A file whose process is gone is stale.
    sample(42).write(&path).unwrap();
    assert_eq!(discovery::read(&path, |_| false), Found::Stale(sample(42)));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            matches!(discovery::read(&path, |_| true), Found::Untrusted(m) if m.contains("others"))
        );
    }
}

#[test]
fn the_plan_follows_the_file() {
    let v1 = ProtocolVersion::new(1, 0);
    let d = sample(1);
    assert_eq!(plan(Found::Nothing, "1.0.0", v1), Plan::Start);
    assert_eq!(
        plan(Found::Live(d.clone()), "1.0.0", v1),
        Plan::Connect(d.clone())
    );
    assert_eq!(
        plan(Found::Live(d.clone()), "1.1.0", v1),
        Plan::Replace(d.clone())
    );
    assert_eq!(
        plan(Found::Live(d.clone()), "1.0.0", ProtocolVersion::new(1, 1)),
        Plan::Replace(d.clone())
    );
    assert_eq!(
        plan(Found::Stale(d.clone()), "1.0.0", v1),
        Plan::RemoveStale(d)
    );
    assert_eq!(
        plan(Found::Untrusted("x".into()), "1.0.0", v1),
        Plan::Refuse("x".into())
    );
}

fn echo() -> Arc<dyn Handler> {
    Arc::new(|_: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> { Ok(body.to_vec()) })
}

fn server(path: &Path, build: &str) -> LinkServer {
    let mut c = ServerConfig::new(path, build);
    c.socket_dir = Some(path.parent().unwrap().to_path_buf());
    LinkServer::start(c, echo()).unwrap()
}

fn fake_client(path: &Path, clock: &FakeClock, policy: BackoffPolicy) -> LinkClient {
    let mut c = ClientConfig::new(path, "b1", "the agent");
    c.clock = Arc::new(clock.clone());
    c.backoff = policy;
    LinkClient::start(c, None, None, None).unwrap()
}

fn retrying(s: &LinkState) -> bool {
    matches!(s, LinkState::Retrying { .. })
}

#[test]
fn the_client_reconnects_with_back_off_on_a_fake_clock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    let clock = FakeClock::new();
    let policy = BackoffPolicy::retry_forever(
        Duration::from_secs(1),
        Duration::from_secs(8),
        Duration::from_secs(60),
    );
    let mut client = fake_client(&path, &clock, policy);
    // Nothing runs and there is no launcher: it waits one second on the fake clock.
    let s = client.wait_for(GUARD, retrying).unwrap();
    assert!(
        matches!(&s, LinkState::Retrying { attempt: 1, after, reason } if *after == Duration::from_secs(1) && reason.contains("not running")),
        "{s:?}"
    );
    let mut first = server(&path, "b1");
    assert!(clock.wait_for_waiters(1, GUARD));
    clock.advance(Duration::from_secs(1));
    let peer = client.wait_connected(GUARD).unwrap();
    assert_eq!(peer.call(b"hi".to_vec(), T).unwrap(), b"hi");
    assert_eq!(
        client.state(),
        LinkState::Connected {
            build: "b1".into(),
            pid: std::process::id()
        }
    );
    assert_eq!(first.peers().len(), 1);
    assert!(first.refused().is_empty());
    // The companion goes away: the next wait is longer (two seconds: the link was not healthy for a minute).
    first.stop();
    assert!(!path.exists(), "the server removed its discovery file");
    let s = client.wait_for(GUARD, retrying).unwrap();
    assert!(
        matches!(s, LinkState::Retrying { attempt: 1, after, .. } if after == Duration::from_secs(2)),
        "{s:?}"
    );
    let _second = server(&path, "b1");
    assert!(clock.wait_for_waiters(1, GUARD));
    // One second is not enough.
    clock.advance(Duration::from_secs(1));
    assert!(retrying(&client.state()));
    clock.advance(Duration::from_secs(1));
    assert!(client.wait_connected(GUARD).is_some());
    client.stop();
    assert_eq!(client.state(), LinkState::Stopped);
    assert!(client.peer().is_none());
}

#[test]
fn a_stale_discovery_file_is_ignored_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    // A companion that exited: a real process id that is gone.
    let mut gone = paddock_proc::Supervised::spawn({
        let mut c = paddock_proc::command(child_program());
        c.arg("exit");
        c
    })
    .unwrap();
    let pid = gone.id().unwrap();
    gone.wait();
    sample(pid).write(&path).unwrap();
    let clock = FakeClock::new();
    let client = fake_client(&path, &clock, BackoffPolicy::default());
    let s = client.wait_for(GUARD, retrying).unwrap();
    assert!(
        matches!(&s, LinkState::Retrying { reason, .. } if reason.contains("not running")),
        "{s:?}"
    );
    assert!(!path.exists(), "the stale file was removed");
    assert!(client.peer().is_none());
}

#[test]
fn the_back_off_gives_up_and_retry_starts_over() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    let clock = FakeClock::new();
    let policy = BackoffPolicy {
        give_up: Some(GiveUp {
            max_failures: 1,
            window: Duration::from_secs(600),
        }),
        ..BackoffPolicy::default()
    };
    let states = Arc::new(Mutex::new(Vec::new()));
    let seen = states.clone();
    let mut c = ClientConfig::new(&path, "b1", "the agent");
    c.clock = Arc::new(clock.clone());
    c.backoff = policy;
    let client = LinkClient::start(
        c,
        None,
        None,
        Some(Arc::new(move |s: &LinkState| {
            seen.lock().unwrap().push(s.clone())
        })),
    )
    .unwrap();
    client.wait_for(GUARD, retrying).unwrap();
    assert!(clock.wait_for_waiters(1, GUARD));
    clock.advance(Duration::from_secs(1));
    let s = client
        .wait_for(GUARD, |s| matches!(s, LinkState::GaveUp { .. }))
        .unwrap();
    assert!(matches!(s, LinkState::GaveUp { reason } if reason.contains("not running")));
    // The user presses Retry once the companion runs.
    let _server = server(&path, "b1");
    client.retry();
    assert!(client.wait_connected(GUARD).is_some());
    assert!(
        states
            .lock()
            .unwrap()
            .iter()
            .any(|s| matches!(s, LinkState::GaveUp { .. }))
    );
}

#[test]
fn a_client_never_replaces_its_own_process_or_with_replace_never() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    let _old = server(&path, "old");
    let clock = FakeClock::new();
    let client = fake_client(&path, &clock, BackoffPolicy::default());
    let s = client.wait_for(GUARD, retrying).unwrap();
    assert!(
        matches!(&s, LinkState::Retrying { reason, .. } if reason.contains("this process")),
        "{s:?}"
    );
    let mut c = ClientConfig::new(&path, "new", "the agent");
    c.clock = Arc::new(clock.clone());
    c.replace = Replace::Never;
    assert!(format!("{:?}", c.replace).contains("Never"));
    let other = LinkClient::start(c, None, None, None).unwrap();
    let s = other.wait_for(GUARD, retrying).unwrap();
    assert!(
        matches!(&s, LinkState::Retrying { reason, .. } if reason.contains("this process") || reason.contains("not replaced")),
        "{s:?}"
    );
}

/// Starts companions of `build` as children of the test and keeps them, so none is left behind.
struct Companions {
    program: std::path::PathBuf,
    discovery: std::path::PathBuf,
    build: String,
    started: Mutex<Vec<paddock_proc::Supervised>>,
}

impl Launcher for Companions {
    fn launch(&self) -> Result<(), String> {
        let mut cmd = paddock_proc::command(&self.program);
        cmd.arg("companion")
            .arg(&self.discovery)
            .arg(&self.build)
            .stdout(Stdio::null());
        let child = paddock_proc::Supervised::spawn(cmd).map_err(|e| e.to_string())?;
        self.started.lock().unwrap().push(child);
        Ok(())
    }
}

/// A companion of `build`, returned once it printed `ready` (its discovery file is written).
fn companion(discovery: &Path, build: &str) -> paddock_proc::Supervised {
    let mut cmd = paddock_proc::command(child_program());
    cmd.arg("companion")
        .arg(discovery)
        .arg(build)
        .stdout(Stdio::piped());
    let mut child = paddock_proc::Supervised::spawn(cmd).unwrap();
    let mut line = String::new();
    std::io::BufReader::new(child.take_stdout().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "ready");
    child
}

#[test]
fn a_second_build_replaces_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    let mut old = companion(&path, "1");
    let old_pid = old.id().unwrap();
    let launcher = Arc::new(Companions {
        program: child_program(),
        discovery: path.clone(),
        build: "2".into(),
        started: Mutex::new(Vec::new()),
    });
    let mut c = ClientConfig::new(&path, "2", "the companion");
    c.replace = Replace::Terminate { grace: GUARD };
    c.start_wait = GUARD;
    let client = LinkClient::start(c, None, Some(launcher.clone()), None).unwrap();
    let peer = client.wait_connected(GUARD).unwrap();
    let LinkState::Connected { build, pid } = client.state() else {
        panic!("{:?}", client.state())
    };
    assert_eq!(build, "2");
    assert_ne!(pid, old_pid);
    assert_eq!(
        peer.call(b"pid".to_vec(), T).unwrap(),
        pid.to_string().into_bytes()
    );
    assert!(!old.is_running(), "the first build was made to exit");
    assert_eq!(launcher.started.lock().unwrap().len(), 1);
    // The client stops first (it would start another companion), then the companions end with the test.
    drop(client);
    launcher.started.lock().unwrap().clear();
}

#[test]
fn a_companion_asked_through_the_apps_protocol_makes_way() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    let mut old = companion(&path, "1");
    let launcher = Arc::new(Companions {
        program: child_program(),
        discovery: path.clone(),
        build: "2".into(),
        started: Mutex::new(Vec::new()),
    });
    let mut c = ClientConfig::new(&path, "2", "the companion");
    c.start_wait = GUARD;
    let asked = Arc::new(Mutex::new(0));
    let a2 = asked.clone();
    c.replace = Replace::Ask {
        ask: Arc::new(move |d: &Discovery| {
            *a2.lock().unwrap() += 1;
            let token = d.token().unwrap();
            let conn =
                paddock_ipc::Client::connect(&d.endpoint, &token, Default::default(), d.protocol)
                    .unwrap();
            let p = paddock_ipc::Peer::start(
                conn,
                Arc::new(paddock_ipc::NoCalls),
                paddock_ipc::PeerOptions::new("old"),
            )
            .unwrap();
            let _ = p.call(b"stop".to_vec(), T);
            true
        }),
        grace: GUARD,
    };
    let client = LinkClient::start(c, None, Some(launcher.clone()), None).unwrap();
    assert!(client.wait_connected(GUARD).is_some());
    assert_eq!(*asked.lock().unwrap(), 1);
    assert_eq!(
        old.wait().and_then(|s| s.code()),
        Some(0),
        "it exited by itself"
    );
    drop(client);
    launcher.started.lock().unwrap().clear();
}
