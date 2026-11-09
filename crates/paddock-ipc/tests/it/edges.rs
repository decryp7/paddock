//! Edge paths: a fixed-endpoint host, refused clients on a link server, a companion that is not replaced, a companion
//! that is not asked politely, one that never starts, an untrusted discovery file, waits without a limit, a callee that
//! ignores a stream cancel, and an oversized frame that names no call.

use crate::common;
use common::{GUARD, T, child_program, pair, pair_with, raw};
use paddock_ipc::frame::{FrameLimit, MIN_FRAME, ProtocolVersion};
use paddock_ipc::link::{
    ClientConfig, Discovery, Launcher, LinkClient, LinkServer, LinkState, Replace, ServerConfig,
};
use paddock_ipc::{
    CallCtx, Client, Handler, Host, IpcError, NoCalls, PeerOptions, RemoteError, StreamEvent, Token,
};
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

fn echo() -> Arc<dyn Handler> {
    Arc::new(|_: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> { Ok(body.to_vec()) })
}

#[cfg(unix)]
#[test]
fn a_host_on_a_fixed_endpoint_serves_clients_that_know_its_token() {
    let dir = tempfile::tempdir().unwrap();
    let name = dir.path().join("svc.sock").to_string_lossy().into_owned();
    let token = Token::random().unwrap();
    let mut h = Host::listen_at(
        &name,
        &paddock_ipc::endpoint::Access::service_for_interactive_users(),
        token.clone(),
        FrameLimit::default(),
        ProtocolVersion::new(1, 0),
    )
    .unwrap();
    assert_eq!(h.endpoint(), name);
    let t = std::thread::spawn(move || {
        Client::connect(
            &name,
            &token,
            FrameLimit::default(),
            ProtocolVersion::new(1, 0),
        )
    });
    let conn = h.accept(GUARD).unwrap();
    assert!(format!("{conn:?}").contains("Connection"));
    assert!(t.join().unwrap().is_ok());
}

#[test]
fn a_link_server_refuses_a_wrong_token_and_keeps_serving() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    let server = LinkServer::start(ServerConfig::new(&path, "b1"), echo()).unwrap();
    let endpoint = server.endpoint().to_string();
    assert!(
        Client::connect(
            &endpoint,
            &Token::from_bytes([1; 32]),
            FrameLimit::default(),
            ProtocolVersion::UNVERSIONED
        )
        .is_err()
    );
    // The refusal is recorded by the accept thread right after it answered: a hang guard, not a timing assertion.
    let started = Instant::now();
    while server.refused().is_empty() && started.elapsed() < GUARD {
        std::thread::yield_now();
    }
    assert!(matches!(server.refused().first(), Some(IpcError::Auth(_))));
    let d = match paddock_ipc::link::discovery::read(&path, |_| true) {
        paddock_ipc::link::Found::Live(d) => d,
        other => panic!("{other:?}"),
    };
    let conn = Client::connect(
        &endpoint,
        &d.token().unwrap(),
        FrameLimit::default(),
        ProtocolVersion::UNVERSIONED,
    )
    .unwrap();
    let peer =
        paddock_ipc::Peer::start(conn, Arc::new(NoCalls), PeerOptions::new("server")).unwrap();
    assert_eq!(peer.call(b"x".to_vec(), T).unwrap(), b"x");
    assert_eq!(server.peers().len(), 1);
}

/// A companion of `build`, returned once it printed `ready`.
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

fn retrying_with(client: &LinkClient, words: &str) -> LinkState {
    let s = client.wait_for(
        GUARD,
        |s| matches!(s, LinkState::Retrying { reason, .. } if reason.contains(words)),
    );
    s.unwrap_or_else(|| panic!("{:?}", client.state()))
}

#[test]
fn a_companion_of_another_build_is_left_alone_with_replace_never() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    let mut old = companion(&path, "1");
    let mut c = ClientConfig::new(&path, "2", "the companion");
    c.replace = Replace::Never;
    let client = LinkClient::start(c, None, None, None).unwrap();
    retrying_with(&client, "not replaced");
    assert!(old.is_running());
    drop(client);
    old.kill();
}

struct Spawner {
    discovery: std::path::PathBuf,
    build: String,
    started: Mutex<Vec<paddock_proc::Supervised>>,
}

impl Launcher for Spawner {
    fn launch(&self) -> Result<(), String> {
        let mut cmd = paddock_proc::command(child_program());
        cmd.arg("companion")
            .arg(&self.discovery)
            .arg(&self.build)
            .stdout(Stdio::null());
        self.started
            .lock()
            .unwrap()
            .push(paddock_proc::Supervised::spawn(cmd).map_err(|e| e.to_string())?);
        Ok(())
    }
}

#[test]
fn a_companion_that_was_not_asked_is_ended() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    let mut old = companion(&path, "1");
    let launcher = Arc::new(Spawner {
        discovery: path.clone(),
        build: "2".into(),
        started: Mutex::new(Vec::new()),
    });
    let mut c = ClientConfig::new(&path, "2", "the companion");
    c.start_wait = GUARD;
    c.replace = Replace::Ask {
        ask: Arc::new(|_: &Discovery| false),
        grace: GUARD,
    };
    assert!(format!("{:?}", c.replace).contains("Ask"));
    assert!(format!("{:?}", Replace::Terminate { grace: GUARD }).contains("Terminate"));
    let client = LinkClient::start(c, None, Some(launcher.clone()), None).unwrap();
    assert!(client.wait_connected(GUARD).is_some());
    assert!(!old.is_running());
    drop(client);
    launcher.started.lock().unwrap().clear();
}

struct Nothing;

impl Launcher for Nothing {
    fn launch(&self) -> Result<(), String> {
        Ok(())
    }
}

struct Refuses;

impl Launcher for Refuses {
    fn launch(&self) -> Result<(), String> {
        Err("the program is missing".into())
    }
}

#[test]
fn a_companion_that_never_starts_or_cannot_be_started_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    let mut c = ClientConfig::new(&path, "1", "the companion");
    c.start_wait = Duration::ZERO;
    let client = LinkClient::start(c.clone(), None, Some(Arc::new(Nothing)), None).unwrap();
    retrying_with(&client, "did not start in time");
    drop(client);
    let client = LinkClient::start(c, None, Some(Arc::new(Refuses)), None).unwrap();
    retrying_with(&client, "the program is missing");
}

#[cfg(unix)]
#[test]
fn an_untrusted_discovery_file_is_not_used() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    let _server = LinkServer::start(ServerConfig::new(&path, "1"), echo()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
    let client = LinkClient::start(
        ClientConfig::new(&path, "1", "the companion"),
        None,
        None,
        None,
    )
    .unwrap();
    retrying_with(&client, "others");
}

#[test]
fn waits_without_a_limit_and_a_stream_cancel_the_callee_ignores() {
    let (a, _b) = pair(Arc::new(NoCalls), echo());
    assert_eq!(a.call(b"no limit".to_vec(), None).unwrap(), b"no limit");
    let streamer: Arc<dyn Handler> =
        Arc::new(|ctx: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
            ctx.send_item(b"one".to_vec())?;
            Ok(b"done".to_vec())
        });
    let (a, _b) = pair(Arc::new(NoCalls), streamer);
    let mut s = a.stream(Vec::new(), 1).unwrap();
    assert_eq!(s.next(None).unwrap(), StreamEvent::Item(b"one".to_vec()));
    assert_eq!(s.next(None).unwrap(), StreamEvent::Done(b"done".to_vec()));

    // A callee that ignores the cancel: the caller gives up after its grace.
    let (release, gate) = mpsc::channel::<()>();
    let gate = Mutex::new(gate);
    let (started_tx, started) = mpsc::channel();
    let deaf: Arc<dyn Handler> = Arc::new(
        move |ctx: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
            let probe = ctx.cancel_probe();
            let _ = started_tx.send(probe.is_cancelled());
            let _ = gate.lock().unwrap().recv();
            Ok(Vec::new())
        },
    );
    let mut quick = PeerOptions::new("caller");
    quick.cancel_grace = Duration::from_millis(1);
    let (a, _b) = pair_with(
        Arc::new(NoCalls),
        quick,
        deaf,
        PeerOptions::new("deaf"),
        FrameLimit::default(),
    );
    let mut s = a.stream(Vec::new(), 1).unwrap();
    assert!(!started.recv_timeout(GUARD).unwrap());
    assert_eq!(s.cancel_and_wait(), Err(IpcError::Timeout));
    release.send(()).unwrap();
}

#[test]
fn an_oversized_frame_that_names_no_call_closes_the_connection() {
    let (p, mut io) = raw(
        Arc::new(NoCalls),
        PeerOptions::new("the plugin"),
        FrameLimit::new(MIN_FRAME),
    );
    // A "Cancel" (variant 6) of 2000 bytes: nothing to fail but the connection.
    let mut bytes = 2000u32.to_le_bytes().to_vec();
    bytes.push(6);
    bytes.extend(std::iter::repeat_n(0u8, 1999));
    io.write_all(&bytes).unwrap();
    assert!(p.wait_closed(Some(GUARD)));
    assert!(matches!(p.closed(), Some(IpcError::Protocol(m)) if m.contains("exceeds")));
}

#[test]
fn a_hello_that_lies_about_its_pid_or_breaks_the_frame_rules_is_refused() {
    use paddock_ipc::endpoint::Stream;
    use paddock_ipc::frame::{self, Frame};
    let mut h = Host::listen(FrameLimit::new(MIN_FRAME), ProtocolVersion::UNVERSIONED).unwrap();
    let token = *h.token().as_bytes();
    let send = |h: &Host, bytes: Vec<u8>| {
        let name = h.endpoint().to_string();
        std::thread::spawn(move || {
            let mut s = Stream::connect(&name, GUARD).unwrap();
            let _ = s.write_all(&bytes);
            let mut rest = Vec::new();
            let _ = std::io::Read::read_to_end(&mut s, &mut rest);
        })
    };
    // The right token from a process that claims another pid.
    let liar = Frame::Hello {
        version: frame::VERSION,
        protocol: ProtocolVersion::UNVERSIONED,
        token,
        pid: std::process::id() + 1,
    };
    let t = send(&h, frame::encode(&liar, FrameLimit::default()).unwrap());
    let e = h.accept(GUARD).unwrap_err();
    if cfg!(any(windows, target_os = "linux", target_os = "macos")) {
        assert!(
            matches!(&e, IpcError::Auth(m) if m.contains("claims")),
            "{e:?}"
        );
    }
    t.join().unwrap();
    // A hello over the limit, and a length beyond any limit.
    let mut big = 2000u32.to_le_bytes().to_vec();
    big.extend(std::iter::repeat_n(0u8, 2000));
    let t = send(&h, big);
    assert!(matches!(h.accept(GUARD), Err(IpcError::Protocol(_))));
    t.join().unwrap();
    let t = send(&h, (200u32 << 20).to_le_bytes().to_vec());
    assert!(matches!(h.accept(GUARD), Err(IpcError::Protocol(_))));
    t.join().unwrap();
}

#[test]
fn a_client_sees_a_host_that_breaks_off_or_answers_too_much() {
    use paddock_ipc::endpoint::Listener;
    use paddock_ipc::frame::{self, Frame};
    let mut l = Listener::bind().unwrap();
    for answer in [vec![9u8, 0, 0, 0, 1], {
        let mut b = 2000u32.to_le_bytes().to_vec();
        b.extend(std::iter::repeat_n(1u8, 2000));
        b
    }] {
        let name = l.name().to_string();
        let t = std::thread::spawn(move || {
            Client::connect(
                &name,
                &Token::from_bytes([1; 32]),
                FrameLimit::new(MIN_FRAME),
                ProtocolVersion::UNVERSIONED,
            )
        });
        let (mut s, _) = l.accept(GUARD).unwrap();
        let _hello: Frame = frame::read_frame(&mut s, FrameLimit::default()).unwrap();
        s.write_all(&answer).unwrap();
        drop(s);
        assert!(matches!(
            t.join().unwrap(),
            Err(IpcError::Closed(_) | IpcError::Protocol(_))
        ));
    }
    // Removing a discovery file that is not there is fine.
    paddock_ipc::link::Discovery::remove_if_owned_by(Path::new("/no/such/agent.json"), 1);
}

#[cfg(unix)]
#[test]
fn a_service_serves_its_link_on_a_fixed_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("service.json");
    let socket = dir.path().join("svc.sock").to_string_lossy().into_owned();
    let mut config = ServerConfig::new(&path, "1");
    config.fixed = Some((
        socket.clone(),
        paddock_ipc::endpoint::Access::service_for_interactive_users(),
    ));
    let server = LinkServer::start(config, echo()).unwrap();
    assert_eq!(server.endpoint(), socket);
    let client = LinkClient::start(
        ClientConfig::new(&path, "1", "the service"),
        None,
        None,
        None,
    )
    .unwrap();
    let peer = client.wait_connected(GUARD).unwrap();
    assert_eq!(peer.call(b"x".to_vec(), T).unwrap(), b"x");
}

/// Runs the test child's `mode` (an `out-of-files` mode) and returns what the host and the client saw.
#[cfg(unix)]
fn out_of_files(mode: &str) -> (String, String, String) {
    let out = paddock_proc::command(child_program())
        .arg(mode)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let line = |prefix: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(prefix))
            .unwrap_or_default()
            .to_string()
    };
    (line("host: "), line("client: "), text)
}

/// A host that has no file descriptor left for a client's handshake (CI run 7763: the agent of a test that leaked
/// connections) answers that client with a `busy` refusal that says why, instead of closing the connection without a word
/// ("the host closed the connection during the handshake", which reads like a crash). Its own process: it lowers the limit.
#[cfg(unix)]
#[test]
fn a_host_out_of_file_descriptors_tells_the_client_why_it_cannot_take_the_connection() {
    let (host, client, text) = out_of_files("out-of-files");
    assert!(
        host.starts_with("Err("),
        "the host must not welcome a client it cannot serve: {text}"
    );
    assert!(
        client.contains("Fail") && client.contains("busy") && client.contains("open files"),
        "the client must be told why (a busy refusal naming the cause), not see the stream end: {text}"
    );
}

/// With no descriptor left even for the accept, the client is accepted on the listener's reserve one and told why (Linux
/// keeps it queued otherwise: it waited its whole handshake limit while the host failed the same accept every 50 ms, measured
/// in the CI image). macOS drops a client whose accept failed before the host sees it, so there it ends without a word.
#[cfg(unix)]
#[test]
fn a_host_with_no_descriptor_left_to_accept_tells_the_waiting_client_why() {
    let (host, client, text) = out_of_files("out-of-files-at-accept");
    assert!(
        host.contains("Too many open files") || host.contains("os error"),
        "{text}"
    );
    if cfg!(target_os = "macos") {
        assert!(
            client.contains("Fail") || client.contains("Closed"),
            "{text}"
        );
    } else {
        assert!(
            client.contains("Fail") && client.contains("busy") && client.contains("open files"),
            "the waiting client must be told why: {text}"
        );
    }
}

/// A `LinkServer` whose accept thread finds no descriptor left tells the waiting client why (Linux), records the refusal and
/// keeps serving; it pauses after a failed accept instead of failing the same accept in a tight loop. The client runs here,
/// in a process with its own descriptor table (in the child it raced the accept thread for the last slot).
#[cfg(unix)]
#[test]
fn a_link_server_with_no_descriptor_left_to_accept_tells_the_waiting_client_why() {
    use paddock_ipc::endpoint::Stream;
    use paddock_ipc::frame;
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = paddock_proc::command(child_program());
    cmd.arg("link-out-of-files")
        .arg(dir.path().join("link.json"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = paddock_proc::Supervised::spawn(cmd).unwrap();
    let mut stdin = child.take_stdin().unwrap();
    let mut out = std::io::BufReader::new(child.take_stdout().unwrap());
    let mut ready = String::new();
    out.read_line(&mut ready).unwrap();
    let endpoint = ready
        .trim()
        .strip_prefix("ready ")
        .unwrap_or_else(|| panic!("{ready:?}"))
        .to_string();
    let mut client = Stream::connect(&endpoint, GUARD).unwrap();
    client.set_read_timeout(Some(GUARD)).unwrap();
    let answer = frame::read_frame(&mut client, FrameLimit::default());
    writeln!(stdin, "done").unwrap();
    let mut rest = String::new();
    std::io::Read::read_to_string(&mut out, &mut rest).unwrap();
    assert!(child.wait().is_some_and(|s| s.success()), "{rest}");
    assert!(
        rest.contains("refused: ") && rest.contains("Too many open files"),
        "the server records why: {rest}"
    );
    let client = format!("{answer:?}");
    if cfg!(target_os = "macos") {
        assert!(
            client.contains("Fail") || client.contains("Closed"),
            "{client}"
        );
    } else {
        assert!(
            client.contains("Fail") && client.contains("busy") && client.contains("open files"),
            "the waiting client must be told why: {client}"
        );
    }
}

/// With no descriptor left and nobody connecting, Linux fails the accept itself (it takes a descriptor before it looks for a
/// client): the listener must keep its reserve for a client that is really waiting, or the freed descriptor goes to whoever
/// opens a file next and the next client waits in the queue (16 of 40 runs of the link test in review). The client that
/// comes later is told `busy` (Linux).
#[cfg(unix)]
#[test]
fn an_accept_with_nobody_waiting_keeps_the_reserve_descriptor() {
    let (_, client, text) = out_of_files("out-of-files-idle");
    assert!(
        text.contains("free after the idle accept: false"),
        "the reserve was given up with no client waiting: {text}"
    );
    if cfg!(target_os = "macos") {
        assert!(
            client.contains("Fail") || client.contains("Closed"),
            "{text}"
        );
    } else {
        assert!(client.contains("Fail") && client.contains("busy"), "{text}");
    }
}
