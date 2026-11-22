//! The guard against servers that break on purpose (`paddock_testkit::net::broken_server`): each test is also a positive control,
//! because the healthy server in the same test shows that the check can tell a working store from a broken one.

use super::*;
use paddock_testkit::net::{Mode, broken_server};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Short limits: a stall is 300 ms and the breaker stays open 600 ms.
fn short() -> Limits {
    Limits {
        stall: Duration::from_millis(300),
        cooldown: Duration::from_millis(600),
        connect: Duration::from_millis(500),
        idle_exit: Duration::from_secs(60),
    }
}

/// A guard in front of `upstream`: its port, its log lines and the thread that serves.
fn start(
    upstream: String,
    limits: Limits,
) -> (u16, Arc<Mutex<Vec<String>>>, std::thread::JoinHandle<()>) {
    let guard = Guard::bind(upstream, limits).expect("a loopback port");
    let port = guard.port();
    let lines = Arc::new(Mutex::new(Vec::new()));
    let sink = lines.clone();
    let log: Log = Arc::new(move |l: &str| {
        if let Ok(mut v) = sink.lock() {
            v.push(l.to_string());
        }
    });
    (port, lines, std::thread::spawn(move || guard.serve(log)))
}

fn logged(lines: &Arc<Mutex<Vec<String>>>, what: &str) -> bool {
    lines
        .lock()
        .is_ok_and(|v| v.iter().any(|l| l.contains(what)))
}

/// What came back from one request, and how long it took.
struct Reply {
    body: Vec<u8>,
    took: Duration,
    /// The guard closed the connection (or reset it) rather than the wait running out.
    closed: bool,
}

/// Sends `request` through the guard and reads until the connection ends, `want` bytes arrived or `limit` passes.
fn ask(port: u16, request: &[u8], want: usize, limit: Duration) -> Reply {
    let started = Instant::now();
    let mut c = TcpStream::connect(("127.0.0.1", port)).expect("the guard listens");
    c.set_read_timeout(Some(limit)).expect("a read timeout");
    let _ = c.write_all(request);
    let mut body = Vec::new();
    let mut buf = [0u8; 256];
    let closed = loop {
        match c.read(&mut buf) {
            Ok(0) => break true,
            Ok(n) => {
                body.extend_from_slice(&buf[..n]);
                if body.len() >= want {
                    break false;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break false;
            }
            Err(_) => break true,
        }
    };
    Reply {
        body,
        took: started.elapsed(),
        closed,
    }
}

const REQUEST: &[u8] = b"GET /bucket/key HTTP/1.1\r\n\r\n";

fn end(port: u16, g: std::thread::JoinHandle<()>) {
    assert!(stop(port), "the stop request is delivered");
    g.join().expect("the guard ends");
}

#[test]
fn a_store_that_answers_is_forwarded_unchanged() {
    let store = broken_server(Mode::Answer(b"hello".to_vec()));
    let (port, _log, g) = start(store.addr().to_string(), short());
    let r = ask(port, REQUEST, 10_000, Duration::from_millis(250));
    let text = String::from_utf8_lossy(&r.body).to_string();
    assert!(
        text.starts_with("HTTP/1.1 200 OK") && text.ends_with("hello"),
        "{text}"
    );
    end(port, g);
}

/// The user's case: a store that accepts and never answers. The request ends after about one stall, and the next one at once.
#[test]
fn a_store_that_never_answers_is_cut_after_one_stall_and_then_refused_at_once() {
    let store = broken_server(Mode::BlackHole);
    let (port, log, g) = start(store.addr().to_string(), short());
    let r = ask(port, REQUEST, 1, Duration::from_secs(10));
    assert!(
        r.closed && r.body.is_empty(),
        "the request fails: closed={} body={:?}",
        r.closed,
        r.body
    );
    assert!(
        r.took >= Duration::from_millis(250),
        "not before the stall: {:?}",
        r.took
    );
    assert!(r.took < Duration::from_secs(5), "bounded: {:?}", r.took);
    assert!(
        logged(&log, "did not answer within"),
        "{:?}",
        log.lock().map(|v| v.clone())
    );
    let before = store.accepted();
    let r = ask(
        port,
        b"GET /bucket/other HTTP/1.1\r\n\r\n",
        1,
        Duration::from_secs(10),
    );
    assert!(r.closed && r.body.is_empty(), "refused");
    assert!(
        r.took < Duration::from_millis(250),
        "a refusal is immediate: {:?}",
        r.took
    );
    assert_eq!(
        store.accepted(),
        before,
        "the store was not asked again during the cooldown"
    );
    end(port, g);
}

#[test]
fn after_the_cooldown_the_store_is_tried_again() {
    let store = broken_server(Mode::BlackHole);
    let (port, _log, g) = start(store.addr().to_string(), short());
    let _ = ask(port, REQUEST, 1, Duration::from_secs(10));
    // Each try is refused at once while the breaker is open; the first one that reaches the store is the end of the cooldown.
    let before = store.accepted();
    let mut last = None;
    assert!(
        paddock_testkit::wait::retry_within(
            Duration::from_secs(10),
            Duration::from_millis(50),
            || {
                let r = ask(port, REQUEST, 1, Duration::from_secs(10));
                let reached = store.accepted() > before;
                last = Some(r);
                reached
            }
        ),
        "the store got a new connection after the cooldown"
    );
    let r = last.expect("one try ran");
    assert!(
        r.closed && r.took >= Duration::from_millis(250),
        "and it was cut after a stall again: {:?}",
        r.took
    );
    end(port, g);
}

/// A store that drips one byte at a time keeps the stream alive: bytes keep passing, so nothing is cut.
#[test]
fn a_slow_store_that_keeps_sending_is_not_cut() {
    let store = broken_server(Mode::Drip {
        gap: Duration::from_millis(100),
        bytes: 8,
    });
    let (port, log, g) = start(store.addr().to_string(), short());
    let header = "HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\n".len();
    let r = ask(port, REQUEST, header + 8, Duration::from_secs(5));
    assert_eq!(r.body.len(), header + 8, "every byte came through");
    assert!(
        r.took > Duration::from_millis(600),
        "it was slower than the stall allows without bytes: {:?}",
        r.took
    );
    assert!(!logged(&log, "did not answer"), "nothing was cut");
    end(port, g);
}

/// A cut in the middle of an answer reaches the client as the store's own failure; the guard does not hide it or trip.
#[test]
fn a_cut_in_the_middle_of_an_answer_reaches_the_client() {
    let store = broken_server(Mode::CutMidResponse { sent: 4 });
    let (port, log, g) = start(store.addr().to_string(), short());
    let r = ask(port, REQUEST, 10_000, Duration::from_secs(5));
    assert!(r.closed, "the connection ends");
    assert!(
        r.body.len() < "HTTP/1.1 200 OK\r\nContent-Length: 1004\r\n\r\n".len() + 1004,
        "the answer is short"
    );
    assert!(
        !logged(&log, "did not answer"),
        "an answer that started is not a hang"
    );
    end(port, g);
}

#[test]
fn a_store_that_does_not_accept_a_connection_opens_the_breaker() {
    let store = broken_server(Mode::Refuse);
    let (port, log, g) = start(store.addr().to_string(), short());
    let r = ask(port, REQUEST, 1, Duration::from_secs(10));
    assert!(r.closed && r.body.is_empty());
    assert!(
        logged(&log, "did not accept a connection"),
        "{:?}",
        log.lock().map(|v| v.clone())
    );
    end(port, g);
}

/// A quiet keep-alive connection is closed after a stall, but the store is fine: no breaker, the next request is forwarded.
#[test]
fn a_quiet_connection_is_closed_without_opening_the_breaker() {
    let store = broken_server(Mode::Answer(b"hello".to_vec()));
    let (port, log, g) = start(store.addr().to_string(), short());
    let mut c = TcpStream::connect(("127.0.0.1", port)).expect("connects");
    c.set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a read timeout");
    c.write_all(REQUEST).expect("sends");
    let mut b = [0u8; 256];
    // The store writes the header and the body separately, so they may arrive in two reads: read until the body is complete.
    let mut got = Vec::new();
    while !got.ends_with(b"hello") {
        let n = c.read(&mut b).expect("answered");
        assert!(
            n > 0,
            "the connection ended before the answer was complete: {:?}",
            String::from_utf8_lossy(&got)
        );
        got.extend_from_slice(&b[..n]);
    }
    assert_eq!(c.read(&mut b).unwrap_or(0), 0, "closed after the stall");
    assert!(
        logged(&log, "was quiet"),
        "{:?}",
        log.lock().map(|v| v.clone())
    );
    let r = ask(port, REQUEST, 10_000, Duration::from_millis(250));
    assert!(
        String::from_utf8_lossy(&r.body).ends_with("hello"),
        "the breaker stayed closed"
    );
    end(port, g);
}

#[test]
fn the_stop_request_and_the_idle_limit_end_the_guard() {
    let (port, _log, g) = start("127.0.0.1:9".to_string(), short());
    assert!(stop(port));
    g.join().expect("ends on the stop request");
    let limits = Limits {
        idle_exit: Duration::from_millis(200),
        ..short()
    };
    let (_, log, g) = start("127.0.0.1:9".to_string(), limits);
    g.join().expect("ends when idle");
    assert!(logged(&log, "the guard ends"));
}

#[test]
fn a_stop_request_with_no_guard_is_not_delivered_and_not_an_error() {
    let closed = broken_server(Mode::Refuse);
    assert!(!stop(closed.port()));
}

#[test]
fn a_connection_that_never_speaks_is_dropped_and_the_errors_say_what_failed() {
    let (port, _log, g) = start("127.0.0.1:9".to_string(), short());
    let started = Instant::now();
    let mut c = TcpStream::connect(("127.0.0.1", port)).expect("connects");
    c.set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a read timeout");
    let mut b = [0u8; 8];
    assert_eq!(
        c.read(&mut b).unwrap_or(0),
        0,
        "the guard drops a connection that sends nothing"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    end(port, g);
    // The Display of the typed errors says what to do.
    let e = StallGuardError::Bind(std::io::Error::other("busy"));
    assert!(e.to_string().contains("cannot listen") && e.to_string().contains("busy"));
    let e = StallGuardError::Listener(std::io::Error::other("odd"));
    assert!(e.to_string().contains("listener"));
}
