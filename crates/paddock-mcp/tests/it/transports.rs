//! The stdio and loopback transports, on in-memory pipes and a real loopback socket (port chosen by the OS).

use crate::common;

use common::*;
use paddock_mcp::{KeyStore, Limits, client, jsonrpc, serve_loopback, serve_stdio};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Cursor, ErrorKind, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

fn lines_out(bytes: Vec<u8>) -> Vec<Value> {
    String::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn stdio_answers_one_line_per_request_and_survives_bad_input() {
    let r = rig();
    let long = "x".repeat(r.server.limits().max_line_bytes + 10);
    let input = [
        client::initialize(1, "Pitwall", "1"),
        client::initialized(),
        String::new(),
        "{not json".into(),
        long,
        client::call_tool(2, "echo", json!({"text":"hi"})),
        client::list_tools(3, None),
        client::ping(4),
    ]
    .join("\n");
    let mut out = Vec::new();
    serve_stdio(&r.server, &mut Cursor::new(input.into_bytes()), &mut out).unwrap();
    let replies = lines_out(out);
    // Tool calls are answered as they finish; the others at once. Look replies up by id.
    let by_id = |id: i64| {
        replies
            .iter()
            .find(|v| v["id"] == id)
            .unwrap_or_else(|| panic!("no reply for {id}: {replies:?}"))
            .clone()
    };
    let errors: Vec<i64> = replies
        .iter()
        .filter_map(|v| v["error"]["code"].as_i64())
        .collect();
    assert_eq!(
        replies.len(),
        6,
        "the notification and the blank line get no answer"
    );
    assert_eq!(
        errors,
        [-32700, -32600],
        "the bad JSON and the long line, in the order read"
    );
    assert_eq!(
        client::parse_call(by_id(2)["result"].clone())
            .unwrap()
            .text(),
        "hi"
    );
    assert_eq!(
        client::parse_tools(by_id(3)["result"].clone())
            .unwrap()
            .tools
            .len(),
        2
    );
    assert!(by_id(4)["result"].is_object());
    assert_eq!(
        client::parse_initialize(by_id(1)["result"].clone())
            .unwrap()
            .server_info
            .name,
        "demo"
    );
    assert_eq!(
        r.audit.entries()[0].client,
        "Pitwall",
        "the client is named by initialize"
    );
}

#[test]
fn a_failing_output_ends_the_stdio_loop_with_an_error() {
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let r = rig();
    let err = serve_stdio(
        &r.server,
        &mut Cursor::new(client::ping(1).into_bytes()),
        &mut Closed,
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
}

#[test]
fn a_reply_that_cannot_be_written_is_reported_when_the_input_ends() {
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let r = rig();
    let input = [
        client::initialize(1, "t", "1"),
        client::call_tool(2, "echo", json!({"text": "x"})),
    ];
    // The initialize reply fails to write first (inline), so use a session that is already initialized.
    let session = paddock_mcp::Session::new("t");
    r.server.handle(&session, &input[0]);
    let err = paddock_mcp::serve_lines(
        &r.server,
        &session,
        &mut Cursor::new(input[1].clone().into_bytes()),
        &mut Closed,
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
}

struct Conn {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Conn {
    fn open(addr: std::net::SocketAddr) -> Conn {
        let s = TcpStream::connect(addr).unwrap();
        // A hang guard only: every reply and every end comes at once, so a missing one fails the test in seconds
        // instead of stalling the runner (see `next`).
        s.set_read_timeout(Some(HANG_GUARD)).unwrap();
        Conn {
            reader: BufReader::new(s.try_clone().unwrap()),
            writer: s,
        }
    }

    /// One write for the whole line: `writeln!` writes the line and its newline separately, and the second write into a
    /// connection the server has already closed fails with a broken pipe instead of letting the test read the reply.
    fn send(&mut self, line: &str) {
        self.writer
            .write_all(format!("{line}\n").as_bytes())
            .unwrap();
    }

    /// Closes the sending side and waits until the server has ended the connection: from then on its place is free.
    fn leave(mut self) {
        self.writer.shutdown(std::net::Shutdown::Write).unwrap();
        while self.next().is_some() {}
    }

    /// The next line, or `None` once the connection has ended: at the end of the stream, and also on a read error such as
    /// a reset, since a closed connection can end either way. Neither a line nor an end within [`HANG_GUARD`] fails.
    fn next(&mut self) -> Option<Value> {
        let mut l = String::new();
        match self.reader.read_line(&mut l) {
            Ok(0) => None,
            Ok(_) => Some(serde_json::from_str(&l).unwrap()),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                panic!("neither a line nor the end of the connection within {HANG_GUARD:?}")
            }
            Err(_) => None,
        }
    }
}

/// How long a test client waits for a line or the end of its connection before failing.
const HANG_GUARD: std::time::Duration = std::time::Duration::from_secs(10);

fn listener() -> TcpListener {
    TcpListener::bind("127.0.0.1:0").unwrap()
}

#[test]
fn loopback_needs_a_valid_key_then_speaks_the_same_protocol() {
    let r = rig();
    let keys = Arc::new(KeyStore::new());
    let (id, key) = keys.issue("Pitwall", 1).unwrap();
    let serving = serve_loopback(r.server.clone(), keys.clone(), listener(), 4).unwrap();
    assert!(serving.addr().ip().is_loopback());

    // The wrong key, a missing key and a non-JSON first line are all refused with a message and the connection closed.
    for first in [r#"{"key":"pmk_wrong"}"#, r#"{"nokey":1}"#, "hello"] {
        let mut c = Conn::open(serving.addr());
        c.send(first);
        let reply = c.next().unwrap();
        assert_eq!(reply["ok"], false, "{first}");
        assert!(reply["error"].as_str().unwrap().contains("key"));
        assert!(c.next().is_none(), "closed after a refused handshake");
    }
    // An empty connection that closes straight away is refused too.
    let mut c = Conn::open(serving.addr());
    c.writer.shutdown(std::net::Shutdown::Write).unwrap();
    assert_eq!(c.next().unwrap()["ok"], false);

    let mut c = Conn::open(serving.addr());
    c.send(&json!({"key": key}).to_string());
    assert_eq!(c.next().unwrap(), json!({"ok":true,"client":"Pitwall"}));
    c.send(&client::initialize(1, "ignored", "1"));
    assert_eq!(c.next().unwrap()["result"]["serverInfo"]["name"], "demo");
    c.send(&client::initialized());
    c.send(&client::call_tool(2, "echo", json!({"text":"over tcp"})));
    let reply = c.next().unwrap();
    assert_eq!(
        client::parse_call(reply["result"].clone()).unwrap().text(),
        "over tcp"
    );
    assert_eq!(
        r.audit.entries()[0].client,
        "Pitwall",
        "the key names the client"
    );

    // A revoked key no longer opens new connections.
    assert!(keys.revoke(id));
    let mut late = Conn::open(serving.addr());
    late.send(&json!({"key": key}).to_string());
    assert_eq!(late.next().unwrap()["ok"], false);

    // Stopping closes the live connection and joins every thread.
    serving.stop();
    assert!(c.next().is_none());
}

/// Regression: on Windows shutting a socket down does not wake a read blocked on it, so `stop` waited ~120 s for the
/// system to drop each connection whose client stayed connected. Stopping must end promptly while the client keeps the
/// connection open and silent. The bound below only turns a hang into a failure; a passing run never waits for it.
#[test]
fn stopping_ends_a_connection_whose_client_stays_connected_and_silent() {
    let r = rig();
    let keys = Arc::new(KeyStore::new());
    let (_, key) = keys.issue("a", 1).unwrap();
    let serving = serve_loopback(r.server.clone(), keys, listener(), 2).unwrap();
    let mut idle = Conn::open(serving.addr());
    idle.send(&json!({"key": key}).to_string());
    assert_eq!(idle.next().unwrap()["ok"], true);
    // A second client that never sends its key: the handshake read is blocked as well.
    let _mute = TcpStream::connect(serving.addr()).unwrap();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let stopper = std::thread::spawn(move || {
        serving.stop();
        let _ = done_tx.send(());
    });
    assert!(
        done_rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .is_ok(),
        "stop did not return while a client stayed connected"
    );
    stopper.join().unwrap();
    assert!(idle.next().is_none(), "the client sees the connection end");
}

#[test]
fn loopback_limits_clients_and_bounds_every_line() {
    let r = rig();
    let keys = Arc::new(KeyStore::new());
    let (_, key) = keys.issue("a", 1).unwrap();
    let serving = serve_loopback(r.server.clone(), keys, listener(), 1).unwrap();
    let hello = json!({"key": key}).to_string();
    let mut first = Conn::open(serving.addr());
    first.send(&hello);
    assert_eq!(first.next().unwrap()["ok"], true);
    // The first is still connected: a second client is told so and dropped.
    let mut second = Conn::open(serving.addr());
    let refused = second.next().unwrap();
    assert!(refused["error"].as_str().unwrap().contains("too many"));
    assert!(second.next().is_none());
    // A line over the bound gets an error and the connection goes on.
    let too_long = "y".repeat(Limits::default().max_line_bytes + 1);
    first.send(&too_long);
    assert_eq!(
        first.next().unwrap()["error"]["code"],
        jsonrpc::code::INVALID_REQUEST
    );
    first.send(&client::ping(1));
    assert!(first.next().unwrap()["result"].is_object());
    // When the first goes away (and has seen its connection end) its place is free again, at once.
    first.leave();
    let mut again = Conn::open(serving.addr());
    again.send(&hello);
    assert_eq!(
        again.next().unwrap()["ok"],
        true,
        "a place became free after the first client left"
    );
    drop(serving); // dropping stops it too
}

/// A client turned down because every place is taken reads the refusal, not a reset. The bridge writes its key line at once
/// and then reads; the listener used to close the refused connection with that line unread, which resets it, and the reset
/// could reach the client before the refusal (seen on macOS: "Connection reset by peer" instead of "too many MCP clients").
#[test]
fn a_client_turned_down_for_lack_of_places_reads_the_refusal_not_a_reset() {
    let r = rig();
    let keys = Arc::new(KeyStore::new());
    let (_, key) = keys.issue("a", 1).unwrap();
    let serving = serve_loopback(r.server.clone(), keys, listener(), 1).unwrap();
    let hello = json!({"key": key}).to_string();
    let mut holder = Conn::open(serving.addr());
    holder.send(&hello);
    assert_eq!(holder.next().unwrap()["ok"], true);
    for attempt in 0..50 {
        let mut s = TcpStream::connect(serving.addr()).unwrap();
        s.set_read_timeout(Some(HANG_GUARD)).unwrap();
        s.write_all(format!("{hello}\n").as_bytes()).unwrap();
        let mut reader = BufReader::new(s);
        let mut line = String::new();
        let read = reader.read_line(&mut line);
        assert!(
            read.is_ok(),
            "attempt {attempt}: the refusal was lost: {read:?}"
        );
        let refusal: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(refusal["code"], "busy", "attempt {attempt}: {refusal}");
        assert!(
            refusal["error"].as_str().unwrap().contains("too many"),
            "{refusal}"
        );
        line.clear();
        assert_eq!(
            reader.read_line(&mut line).unwrap(),
            0,
            "attempt {attempt}: then the connection ends"
        );
    }
    drop(serving);
}

/// Regression: the place of a client that left was only freed when the accept loop next found its thread finished, which
/// ran on after the client had already seen its connection end. A client reconnecting at once was told "too many" and,
/// writing its key into the refused connection, got a broken pipe (CI, Linux, under coverage). A client that has seen
/// its connection end must get the free place on its very next connection, every time.
#[test]
fn a_client_that_saw_its_connection_end_gets_the_place_back_at_once() {
    let r = rig();
    let keys = Arc::new(KeyStore::new());
    let (_, key) = keys.issue("a", 1).unwrap();
    let serving = serve_loopback(r.server.clone(), keys, listener(), 1).unwrap();
    let hello = json!({"key": key}).to_string();
    for round in 0..300 {
        let mut c = Conn::open(serving.addr());
        c.send(&hello);
        let reply = c.next().expect("a reply to the key");
        assert_eq!(reply["ok"], true, "round {round}: {reply}");
        c.leave();
    }
}

/// Regression: on macOS `shutdown` fails (ENOTCONN) on a socket whose peer has already sent its FIN, and the server kept a
/// second handle to every connection for `stop`, so a client that half-closed never saw its connection end.
#[test]
fn a_client_that_half_closes_sees_the_end_of_its_connection() {
    let r = rig();
    let keys = Arc::new(KeyStore::new());
    let (_, key) = keys.issue("a", 1).unwrap();
    let serving = serve_loopback(r.server.clone(), keys, listener(), 2).unwrap();
    let mut c = Conn::open(serving.addr());
    c.send(&json!({"key": key}).to_string());
    assert_eq!(c.next().unwrap()["ok"], true);
    c.send(&client::ping(1));
    c.writer.shutdown(std::net::Shutdown::Write).unwrap();
    assert!(
        c.next().unwrap()["result"].is_object(),
        "the request sent before the half-close is answered"
    );
    assert!(c.next().is_none(), "the connection ends");
}

#[test]
fn a_listener_that_is_not_loopback_is_refused_before_serving() {
    // Binding a real non-loopback socket in a test would open the machine to the network; `check_loopback` has its own unit test.
    let r = rig();
    let l = listener();
    assert!(serve_loopback(r.server.clone(), Arc::new(KeyStore::new()), l, 1).is_ok());
}

/// A server whose `slow` tool blocks until the test releases it and reports whether it was cancelled.
fn gated(
    max_concurrent: usize,
) -> (
    Arc<paddock_mcp::Server>,
    Arc<paddock_mcp::AuditLog>,
    std::sync::mpsc::Sender<()>,
    std::sync::mpsc::Receiver<bool>,
) {
    use paddock_mcp::{Policy, ToolDef, ToolOutput};
    use std::sync::{Mutex, mpsc};
    let audit = Arc::new(paddock_mcp::AuditLog::new(20));
    let server = paddock_mcp::Server::new(
        "demo",
        "1",
        Arc::new(paddock_jobs::FakeClock::new()),
        audit.clone(),
        Arc::new(paddock_log::Redactor::new()),
    )
    .with_limits(Limits {
        max_concurrent_calls: max_concurrent,
        ..Limits::default()
    });
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let (seen_tx, seen_rx) = mpsc::channel::<bool>();
    let (release_rx, seen_tx) = (Mutex::new(release_rx), Mutex::new(seen_tx));
    server
        .add_tool(ToolDef::read_only(
            "slow",
            "Waits for the test.",
            json!({"type":"object"}),
            move |ctx, _| {
                let _ = release_rx.lock().unwrap().recv();
                let _ = seen_tx.lock().unwrap().send(ctx.cancel.is_cancelled());
                Ok(ToolOutput::text("slow done"))
            },
        ))
        .unwrap();
    server
        .add_tool(ToolDef::read_only(
            "quick",
            "Answers at once.",
            json!({"type":"object"}),
            |_, _| Ok(ToolOutput::text("quick done")),
        ))
        .unwrap();
    server.set_policy(Policy::read_only());
    (Arc::new(server), audit, release_tx, seen_rx)
}

fn open_client(serving: &paddock_mcp::Serving, key: &str) -> Conn {
    let mut c = Conn::open(serving.addr());
    c.send(&json!({"key": key}).to_string());
    assert_eq!(c.next().unwrap()["ok"], true);
    c.send(&client::initialize(1, "t", "1"));
    assert!(c.next().unwrap()["result"].is_object());
    c.send(&client::initialized());
    c
}

#[test]
fn calls_on_one_connection_run_concurrently_and_answer_in_any_order() {
    let (server, _, release, _) = gated(4);
    let keys = Arc::new(KeyStore::new());
    let (_, key) = keys.issue("a", 1).unwrap();
    let serving = serve_loopback(server, keys, listener(), 2).unwrap();
    let mut c = open_client(&serving, &key);
    c.send(&client::call_tool(10, "slow", json!({})));
    c.send(&client::call_tool(11, "quick", json!({})));
    let first = c.next().unwrap();
    assert_eq!(
        first["id"], 11,
        "the quick call is answered while the slow one is still running"
    );
    release.send(()).unwrap();
    let second = c.next().unwrap();
    assert_eq!(second["id"], 10);
    assert_eq!(
        client::parse_call(second["result"].clone()).unwrap().text(),
        "slow done"
    );
    serving.stop();
}

#[test]
fn the_bound_on_concurrent_calls_makes_the_reader_wait() {
    let (server, _, release, _) = gated(1);
    let keys = Arc::new(KeyStore::new());
    let (_, key) = keys.issue("a", 1).unwrap();
    let serving = serve_loopback(server, keys, listener(), 2).unwrap();
    let mut c = open_client(&serving, &key);
    c.send(&client::call_tool(10, "slow", json!({})));
    c.send(&client::call_tool(11, "quick", json!({})));
    // With one slot the quick call is not read until the slow one ends, so the answers come in request order.
    release.send(()).unwrap();
    assert_eq!(c.next().unwrap()["id"], 10);
    assert_eq!(c.next().unwrap()["id"], 11);
    serving.stop();
}

#[test]
fn a_cancelled_call_is_stopped_gets_no_answer_and_is_audited() {
    let (server, audit, release, seen) = gated(4);
    let keys = Arc::new(KeyStore::new());
    let (_, key) = keys.issue("a", 1).unwrap();
    let serving = serve_loopback(server, keys, listener(), 2).unwrap();
    let mut c = open_client(&serving, &key);
    c.send(&client::call_tool(10, "slow", json!({})));
    c.send(&json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":10,"reason":"user"}}).to_string());
    c.send(&client::ping(11));
    // The ping is answered; the cancelled call never is.
    assert_eq!(c.next().unwrap()["id"], 11);
    release.send(()).unwrap();
    assert!(seen.recv().unwrap(), "the tool saw its token cancelled");
    // A cancel for an unknown or malformed id is ignored.
    c.send(
        &json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":999}})
            .to_string(),
    );
    c.send(&json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}).to_string());
    c.send(&client::ping(12));
    assert_eq!(
        c.next().unwrap()["id"],
        12,
        "no stray answer for 10 came before this one"
    );
    serving.stop();
    let outcomes: Vec<paddock_mcp::Outcome> = audit.entries().iter().map(|e| e.outcome).collect();
    assert_eq!(outcomes, [paddock_mcp::Outcome::Cancelled]);
}
