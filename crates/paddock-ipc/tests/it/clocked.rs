//! Every limit of the IPC on an injected clock: call timeouts and the cancel grace, a stream's idle wait, the accept
//! wait, each handshake (a silent client delays nobody), the connection cap and subscriptions. A `FakeClock` moves only
//! once the work is blocked on it; nothing sleeps and nothing measures time.

use crate::common;
use common::{GUARD, T, pair_with};
use paddock_ipc::endpoint::Stream;
use paddock_ipc::frame::{FrameLimit, ProtocolVersion};
use paddock_ipc::link::{Found, LinkServer, ServerConfig, Subscriptions, discovery};
use paddock_ipc::{
    CallCtx, Client, Handler, Host, IpcError, NoCalls, Peer, PeerOptions, RemoteError, StreamEvent,
    codes,
};
use paddock_jobs::time::{Clock, FakeClock};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

fn on(clock: &FakeClock, label: &str) -> PeerOptions {
    let mut o = PeerOptions::new(label);
    o.clock = Arc::new(clock.clone());
    o
}

/// A handler that holds every call until the test lets it go, and reports when it started.
fn held() -> (Arc<dyn Handler>, mpsc::Receiver<()>, mpsc::Sender<Vec<u8>>) {
    let (started_tx, started) = mpsc::channel();
    let (release, answers) = mpsc::channel::<Vec<u8>>();
    let answers = Mutex::new(answers);
    let h: Arc<dyn Handler> = Arc::new(
        move |_: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
            let _ = started_tx.send(());
            Ok(answers.lock().unwrap().recv().unwrap_or_default())
        },
    );
    (h, started, release)
}

#[test]
fn a_call_times_out_and_its_cancel_grace_runs_out_on_the_fake_clock() {
    let clock = FakeClock::new();
    let (h, started, release) = held();
    let (a, _b) = pair_with(
        Arc::new(NoCalls),
        on(&clock, "caller"),
        h,
        PeerOptions::new("callee"),
        FrameLimit::default(),
    );
    let a2 = a.clone();
    let call = std::thread::spawn(move || a2.call(b"x".to_vec(), Some(Duration::from_secs(30))));
    started.recv_timeout(GUARD).unwrap();
    assert!(clock.wait_for_started(1, GUARD));
    clock.advance(Duration::from_secs(30));
    // The cancel went out; now the grace (two seconds) runs: one second is not enough.
    assert!(clock.wait_for_started(2, GUARD));
    clock.advance(Duration::from_secs(1));
    assert_eq!(clock.waiters(), 1);
    clock.advance(Duration::from_secs(1));
    assert_eq!(call.join().unwrap(), Err(IpcError::Timeout));
    release.send(Vec::new()).unwrap();
    // A reply before the limit needs no time at all.
    let (h, started, release) = held();
    let (a, _b) = pair_with(
        Arc::new(NoCalls),
        on(&clock, "caller"),
        h,
        PeerOptions::new("callee"),
        FrameLimit::default(),
    );
    let a2 = a.clone();
    let call = std::thread::spawn(move || a2.call(b"x".to_vec(), Some(Duration::from_secs(30))));
    started.recv_timeout(GUARD).unwrap();
    release.send(b"in time".to_vec()).unwrap();
    assert_eq!(call.join().unwrap().unwrap(), b"in time");
}

#[test]
fn an_answer_within_the_grace_still_counts_on_the_fake_clock() {
    let clock = FakeClock::new();
    let (h, started, release) = held();
    let (a, _b) = pair_with(
        Arc::new(NoCalls),
        on(&clock, "caller"),
        h,
        PeerOptions::new("callee"),
        FrameLimit::default(),
    );
    let a2 = a.clone();
    let call = std::thread::spawn(move || a2.call(Vec::new(), Some(Duration::from_secs(5))));
    started.recv_timeout(GUARD).unwrap();
    assert!(clock.wait_for_started(1, GUARD));
    clock.advance(Duration::from_secs(5));
    assert!(clock.wait_for_started(2, GUARD));
    release.send(b"late".to_vec()).unwrap();
    assert_eq!(call.join().unwrap().unwrap(), b"late");
}

#[test]
fn a_stream_is_idle_until_its_wait_runs_out_on_the_fake_clock() {
    let clock = FakeClock::new();
    let (h, started, release) = held();
    let (a, _b) = pair_with(
        Arc::new(NoCalls),
        on(&clock, "caller"),
        h,
        PeerOptions::new("callee"),
        FrameLimit::default(),
    );
    let mut s = a.stream(Vec::new(), 1).unwrap();
    started.recv_timeout(GUARD).unwrap();
    let waiting = std::thread::spawn(move || {
        let first = s.next(Some(Duration::from_secs(3)));
        (first, s)
    });
    assert!(clock.wait_for_started(1, GUARD));
    clock.advance(Duration::from_secs(3));
    let (first, mut s) = waiting.join().unwrap();
    assert_eq!(first.unwrap(), StreamEvent::Idle);
    release.send(b"end".to_vec()).unwrap();
    assert_eq!(s.next(T).unwrap(), StreamEvent::Done(b"end".to_vec()));
}

#[test]
fn the_accept_wait_and_a_silent_handshake_run_out_on_the_fake_clock() {
    let clock = FakeClock::new();
    let mut h = Host::listen(FrameLimit::default(), ProtocolVersion::UNVERSIONED)
        .unwrap()
        .with_clock(Arc::new(clock.clone()), Duration::from_secs(5));
    // Nobody connects: the accept ends when the clock passes its limit.
    let (looping_tx, looping) = mpsc::channel();
    let c = clock.clone();
    let waiter = std::thread::spawn(move || {
        let r = h.accept_while(None, Duration::from_secs(60), &mut || {
            let _ = looping_tx.send(());
            Ok(())
        });
        (r, h)
    });
    looping.recv_timeout(GUARD).unwrap();
    c.advance(Duration::from_secs(60));
    let (r, mut h) = waiter.join().unwrap();
    assert_eq!(r.unwrap_err(), IpcError::Timeout);
    // A client that connects and says nothing: its handshake ends after five seconds on the clock.
    let name = h.endpoint().to_string();
    let _silent = Stream::connect(&name, GUARD).unwrap();
    let accept = std::thread::spawn(move || h.accept(GUARD));
    assert!(clock.wait_for_started(1, GUARD));
    clock.advance(Duration::from_secs(5));
    assert_eq!(accept.join().unwrap().unwrap_err(), IpcError::Timeout);
}

fn echo() -> Arc<dyn Handler> {
    Arc::new(|_: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> { Ok(body.to_vec()) })
}

fn live(path: &std::path::Path) -> paddock_ipc::link::Discovery {
    match discovery::read(path, |_| true) {
        Found::Live(d) => d,
        other => panic!("{other:?}"),
    }
}

fn wait_until(f: impl Fn() -> bool) {
    // A hang guard around a state the server reaches right after an event the test caused.
    let start = std::time::Instant::now();
    while !f() {
        assert!(start.elapsed() < GUARD, "the server never got there");
        std::thread::yield_now();
    }
}

#[test]
fn a_silent_client_delays_nobody_and_its_handshake_runs_out() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    let clock = FakeClock::new();
    let mut config = ServerConfig::new(&path, "1");
    config.clock = Arc::new(clock.clone());
    config.handshake_timeout = Duration::from_secs(5);
    let server = LinkServer::start(config, echo()).unwrap();
    let d = live(&path);
    let _silent = Stream::connect(&d.endpoint, GUARD).unwrap();
    // The silent client's watchdog waits on the clock; a real client gets through meanwhile.
    assert!(clock.wait_for_started(1, GUARD));
    let conn = Client::connect(
        &d.endpoint,
        &d.token().unwrap(),
        FrameLimit::default(),
        ProtocolVersion::UNVERSIONED,
    )
    .unwrap();
    let peer = Peer::start(conn, Arc::new(NoCalls), PeerOptions::new("server")).unwrap();
    assert_eq!(peer.call(b"ok".to_vec(), T).unwrap(), b"ok");
    clock.advance(Duration::from_secs(5));
    wait_until(|| server.refused().contains(&IpcError::Timeout));
}

#[test]
fn connections_beyond_the_cap_are_refused_as_busy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    let mut config = ServerConfig::new(&path, "1");
    config.max_connections = 1;
    let server = LinkServer::start(config, echo()).unwrap();
    let d = live(&path);
    let connect = || {
        Client::connect(
            &d.endpoint,
            &d.token().unwrap(),
            FrameLimit::default(),
            ProtocolVersion::UNVERSIONED,
        )
    };
    let first = Peer::start(
        connect().unwrap(),
        Arc::new(NoCalls),
        PeerOptions::new("server"),
    )
    .unwrap();
    assert_eq!(first.call(b"1".to_vec(), T).unwrap(), b"1");
    wait_until(|| server.peers().len() == 1);
    assert!(connect().is_err());
    wait_until(|| {
        server
            .refused()
            .iter()
            .any(|e| matches!(e, IpcError::Remote(r) if r.code == codes::BUSY))
    });
    // When the first leaves, there is room again.
    first.close();
    wait_until(|| server.peers().is_empty());
    assert!(connect().is_ok());
}

#[test]
fn subscribers_receive_their_topics_until_they_leave() {
    let subs = Subscriptions::new();
    let s2 = subs.clone();
    let handler: Arc<dyn Handler> = Arc::new(
        move |ctx: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> {
            let topic = std::str::from_utf8(body).unwrap_or_default();
            match topic.strip_prefix('-') {
                Some(t) => s2.unsubscribe(ctx.peer(), t),
                None => s2.subscribe(ctx.peer(), topic),
            }
            Ok(Vec::new())
        },
    );
    let (tx, rx) = mpsc::channel();
    let listener = PeerOptions::new("client").on_push(move |b| {
        let _ = tx.send(b);
    });
    let (client, server_side) = pair_with(
        Arc::new(NoCalls),
        listener,
        handler,
        PeerOptions::new("server"),
        FrameLimit::default(),
    );
    client.call(b"jobs".to_vec(), T).unwrap();
    assert_eq!(
        subs.topics_of(&server_side).into_iter().collect::<Vec<_>>(),
        ["jobs"]
    );
    assert_eq!(subs.publish("jobs", b"event"), 1);
    assert_eq!(subs.publish("health", b"other"), 0);
    assert_eq!(rx.recv_timeout(GUARD).unwrap(), b"event");
    client.call(b"-jobs".to_vec(), T).unwrap();
    assert_eq!(subs.publish("jobs", b"gone"), 0);
    client.call(b"jobs".to_vec(), T).unwrap();
    client.close();
    assert!(server_side.wait_closed(Some(GUARD)));
    assert_eq!(
        subs.publish("jobs", b"closed"),
        0,
        "a closed connection is forgotten"
    );
    subs.unsubscribe(&server_side, "never");
    assert_ne!(client.id(), server_side.id());
    let _ = FakeClock::new().monotonic();
}

#[test]
fn a_connection_that_closes_during_the_grace_or_a_cancel_wait_ends_those_waits() {
    let clock = FakeClock::new();
    let (h, started, release) = held();
    let (a, b) = pair_with(
        Arc::new(NoCalls),
        on(&clock, "caller"),
        h,
        PeerOptions::new("callee"),
        FrameLimit::default(),
    );
    let a2 = a.clone();
    let call = std::thread::spawn(move || a2.call(Vec::new(), Some(Duration::from_secs(1))));
    started.recv_timeout(GUARD).unwrap();
    assert!(clock.wait_for_started(1, GUARD));
    clock.advance(Duration::from_secs(1));
    assert!(clock.wait_for_started(2, GUARD));
    b.close();
    assert!(matches!(call.join().unwrap(), Err(IpcError::Closed(_))));
    release.send(Vec::new()).unwrap();

    let clock = FakeClock::new();
    let (h, started, release) = held();
    let (a, b) = pair_with(
        Arc::new(NoCalls),
        on(&clock, "caller"),
        h,
        PeerOptions::new("callee"),
        FrameLimit::default(),
    );
    let mut s = a.stream(Vec::new(), 1).unwrap();
    started.recv_timeout(GUARD).unwrap();
    let waiting = std::thread::spawn(move || s.cancel_and_wait());
    assert!(clock.wait_for_started(1, GUARD));
    b.close();
    assert!(matches!(waiting.join().unwrap(), Err(IpcError::Closed(_))));
    release.send(Vec::new()).unwrap();
}

#[test]
fn errors_convert_and_configurations_describe_themselves() {
    use paddock_ipc::frame::FrameError;
    assert_eq!(
        IpcError::from(FrameError::TooLarge { len: 2, max: 1 }),
        IpcError::TooLarge { len: 2, max: 1 }
    );
    assert!(matches!(
        IpcError::from(FrameError::Closed),
        IpcError::Protocol(_)
    ));
    assert_eq!(
        IpcError::TooLarge { len: 2, max: 1 }.to_remote().code,
        codes::TOO_LARGE
    );
    assert_eq!(IpcError::Timeout.to_remote().code, codes::INTERNAL);
    let h = Host::listen(FrameLimit::default(), ProtocolVersion::UNVERSIONED).unwrap();
    assert!(format!("{:?}", h.authenticator()).contains("Authenticator"));
    assert!(format!("{:?}", ServerConfig::new("/x/agent.json", "1")).contains("ServerConfig"));
}

#[test]
fn the_refusal_list_keeps_the_latest_64() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    let server = LinkServer::start(ServerConfig::new(&path, "1"), echo()).unwrap();
    let d = live(&path);
    for _ in 0..66 {
        let _ = Client::connect(
            &d.endpoint,
            &paddock_ipc::Token::from_bytes([3; 32]),
            FrameLimit::default(),
            ProtocolVersion::UNVERSIONED,
        );
    }
    wait_until(|| server.refused().len() == 64);
    assert!(
        server
            .refused()
            .iter()
            .all(|e| matches!(e, IpcError::Auth(_)))
    );
}
