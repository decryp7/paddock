//! Peers over a real private endpoint, both sides in this process.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use dyno_ipc::frame::FrameLimit;
use dyno_ipc::wire::codes;
use dyno_ipc::{Client, Connection, Host, IpcError, Peer, PeerOptions, RemoteError};
use std::io::{Read, Write};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

const LONG: Duration = Duration::from_secs(10);

fn connect_with(limit: FrameLimit) -> (Connection, Connection) {
    let mut host = Host::listen_with(limit).unwrap();
    let (name, token) = (host.name().to_string(), host.token().clone());
    let client = std::thread::spawn(move || Client::connect_with(&name, &token, limit, dyno_ipc::PROTOCOL_VERSION).unwrap());
    let server = host.accept(LONG).unwrap();
    (server, client.join().unwrap())
}

fn connect() -> (Connection, Connection) {
    connect_with(FrameLimit::default())
}

/// A handler: "boom" panics, "fail" fails, anything else is echoed upper-cased.
fn handler(body: String) -> Result<String, RemoteError> {
    match body.as_str() {
        "boom" => panic!("boom"),
        "fail" => Err(RemoteError::new("nope", "it failed")),
        other => Ok(other.to_uppercase()),
    }
}

/// A handler whose "hold" call tells the test it started (`entered`) and then waits until the test releases it, so no
/// test needs a sleep to have a call in flight; every other body goes to [`handler`].
struct Hold {
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
}

fn holding() -> (Hold, impl Fn(String) -> Result<String, RemoteError> + Send + Sync + 'static) {
    let (entered_tx, entered) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let serve = move |body: String| {
        if body == "hold" {
            let _ = entered_tx.send(());
            // Bounded, so a test that fails early never leaves the thread behind for long.
            let _ = release_rx.lock().unwrap().recv_timeout(LONG);
            return Ok("held".into());
        }
        handler(body)
    };
    (Hold { entered, release }, serve)
}

#[test]
fn calls_go_both_ways_with_replies_failures_and_panics() {
    let (s, c) = connect();
    let host = Peer::start(s, PeerOptions::new().serve(handler)).unwrap();
    let child = Peer::start(c, PeerOptions::new().serve(handler)).unwrap();
    assert_eq!(host.call("hi".to_string(), LONG).unwrap(), "HI");
    assert_eq!(child.call("yo".to_string(), LONG).unwrap(), "YO");
    assert_eq!(
        host.call("fail".to_string(), LONG),
        Err(IpcError::Remote(RemoteError::new("nope", "it failed")))
    );
    match host.call("boom".to_string(), LONG) {
        Err(IpcError::Remote(e)) => assert_eq!(e.code, codes::PANIC),
        other => panic!("{other:?}"),
    }
    // The connection survives a panicking handler.
    assert_eq!(host.call("again".to_string(), LONG).unwrap(), "AGAIN");
    assert!(!host.is_closed() && host.closed_reason().is_none());
}

#[test]
fn many_calls_at_once_are_matched_to_their_callers() {
    let (s, c) = connect();
    let host = std::sync::Arc::new(Peer::start(s, PeerOptions::new()).unwrap());
    let _child = Peer::start(c, PeerOptions::new().serve(handler).workers(3)).unwrap();
    let threads: Vec<_> = (0..16)
        .map(|i| {
            let host = host.clone();
            std::thread::spawn(move || (i, host.call(format!("m{i}"), LONG)))
        })
        .collect();
    for t in threads {
        let (i, r) = t.join().unwrap();
        assert_eq!(r.unwrap(), format!("M{i}"));
    }
}

#[test]
fn a_call_times_out_and_the_connection_keeps_working() {
    let (s, c) = connect();
    let (hold, serve) = holding();
    let host = Peer::start(s, PeerOptions::new()).unwrap();
    let _child = Peer::start(c, PeerOptions::new().serve(serve)).unwrap();
    assert_eq!(
        host.call("hold".to_string(), Duration::from_millis(50)),
        Err(IpcError::Timeout(Duration::from_millis(50)))
    );
    // The handler was reached and is still holding: the caller gave up without waiting for it.
    hold.entered.recv_timeout(LONG).unwrap();
    hold.release.send(()).unwrap();
    // The late reply is dropped; the next call gets its own answer.
    assert_eq!(host.call("ok".to_string(), LONG).unwrap(), "OK");
}

#[test]
fn the_peer_closing_mid_call_is_closed() {
    let (s, c) = connect();
    let (hold, serve) = holding();
    let host = Peer::start(s, PeerOptions::new()).unwrap();
    let child = Peer::start(c, PeerOptions::new().serve(serve)).unwrap();
    let closer = std::thread::spawn(move || {
        // Close only once the call is inside the handler.
        hold.entered.recv_timeout(LONG).unwrap();
        drop(child);
        hold.release.send(()).unwrap();
    });
    assert_eq!(host.call("hold".to_string(), LONG), Err(IpcError::Closed));
    closer.join().unwrap();
    assert!(host.wait_closed(Some(LONG)));
    assert_eq!(host.call("late".to_string(), LONG), Err(IpcError::Closed));
    assert_eq!(host.push("late".to_string()), Err(IpcError::Closed));
}

#[test]
fn closing_one_side_ends_the_other() {
    let (s, c) = connect();
    let host = Peer::start(s, PeerOptions::<String>::new()).unwrap();
    let child = Peer::start(c, PeerOptions::<String>::new()).unwrap();
    assert!(!child.wait_closed(Some(Duration::from_millis(20))));
    host.close();
    host.close();
    assert!(child.wait_closed(Some(LONG)));
    assert_eq!(child.closed_reason(), Some(IpcError::Closed));
}

#[test]
fn pushes_arrive_and_a_side_without_a_handler_refuses_calls() {
    let (s, c) = connect();
    let (tx, rx) = mpsc::channel();
    let host = Peer::start(s, PeerOptions::new().on_push(move |b: String| tx.send(b).unwrap())).unwrap();
    let child = Peer::start(c, PeerOptions::<String>::new()).unwrap();
    child.push("alert".to_string()).unwrap();
    assert_eq!(rx.recv_timeout(LONG).unwrap(), "alert");
    assert_eq!(host.dropped_pushes(), 0);
    // Pushes to a side that does not listen are ignored.
    host.push("ignored".to_string()).unwrap();
    match child.call("x".to_string(), LONG) {
        Err(IpcError::Remote(e)) => assert_eq!(e.code, codes::UNSUPPORTED),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_slow_push_listener_drops_pushes_instead_of_blocking() {
    let (s, c) = connect();
    let (release, release_rx) = mpsc::channel::<()>();
    let release_rx = Mutex::new(release_rx);
    let host = Peer::start(
        s,
        PeerOptions::new().queue(1).on_push(move |_: String| {
            // The listener is stuck until the test lets it go (bounded).
            let _ = release_rx.lock().unwrap().recv_timeout(LONG);
        }),
    )
    .unwrap();
    let child = Peer::start(c, PeerOptions::<String>::new().serve(handler)).unwrap();
    for _ in 0..10 {
        child.push("p".to_string()).unwrap();
    }
    // The host's reader handles frames in the order they were sent: when this call (which the host, without a handler,
    // refuses) has its answer, all ten pushes before it were queued or dropped. No waiting for a time.
    assert!(matches!(child.call("after".to_string(), LONG), Err(IpcError::Remote(e)) if e.code == codes::UNSUPPORTED));
    assert!(host.dropped_pushes() > 0);
    drop(release);
}

#[test]
fn an_oversized_call_fails_locally_and_an_oversized_reply_becomes_a_failure() {
    let (s, c) = connect_with(FrameLimit::new(64));
    let host = Peer::start(s, PeerOptions::new()).unwrap();
    let _child = Peer::start(c, PeerOptions::new().serve(|b: String| Ok(b.repeat(100)))).unwrap();
    assert!(matches!(host.call("x".repeat(100), LONG), Err(IpcError::Protocol(m)) if m.contains("exceeds")));
    match host.call("abc".to_string(), LONG) {
        Err(IpcError::Remote(e)) => assert_eq!(e.code, codes::TOO_LARGE),
        other => panic!("{other:?}"),
    }
    assert_eq!(host.call(String::new(), LONG).unwrap(), "");
}

#[test]
fn garbage_after_the_handshake_is_a_protocol_error_for_waiting_callers() {
    let (s, mut c) = connect();
    let host = Peer::start(s, PeerOptions::<String>::new()).unwrap();
    let caller = std::thread::spawn(move || host.call("x".to_string(), LONG));
    // Read the call off the wire: it is registered and waiting before the garbage follows.
    let mut len = [0u8; 4];
    c.stream_mut().read_exact(&mut len).unwrap();
    let mut call = vec![0u8; u32::from_le_bytes(len) as usize];
    c.stream_mut().read_exact(&mut call).unwrap();
    // A length of 1 byte, then a byte that is no frame variant.
    c.stream_mut().write_all(&[1, 0, 0, 0, 0xee]).unwrap();
    assert!(matches!(caller.join().unwrap(), Err(IpcError::Protocol(_))));
}

#[test]
fn a_handshake_frame_after_the_handshake_ends_the_connection() {
    let (s, mut c) = connect();
    let host = Peer::start(s, PeerOptions::<String>::new()).unwrap();
    let welcome = dyno_ipc::frame::encode(&dyno_ipc::Frame::<String>::Welcome { version: 1 }, FrameLimit::default()).unwrap();
    c.stream_mut().write_all(&welcome).unwrap();
    assert!(host.wait_closed(Some(LONG)));
    assert!(matches!(host.closed_reason(), Some(IpcError::Protocol(_))));
}
