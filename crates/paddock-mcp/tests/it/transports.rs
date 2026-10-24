//! The stdio and loopback transports, on in-memory pipes and a real loopback socket (port chosen by the OS).

use crate::common;

use common::*;
use paddock_mcp::{KeyStore, Limits, client, jsonrpc, serve_loopback, serve_stdio};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Cursor, Write};
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
    let codes: Vec<Option<i64>> = replies
        .iter()
        .map(|v| v["error"]["code"].as_i64())
        .collect();
    assert_eq!(
        codes,
        [None, Some(-32700), Some(-32600), None, None, None],
        "the notification and the blank line get no answer"
    );
    let call = client::parse_call(replies[3]["result"].clone()).unwrap();
    assert_eq!(call.text(), "hi");
    assert_eq!(
        client::parse_tools(replies[4]["result"].clone())
            .unwrap()
            .tools
            .len(),
        2
    );
    assert_eq!(
        client::parse_initialize(replies[0]["result"].clone())
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

struct Conn {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Conn {
    fn open(addr: std::net::SocketAddr) -> Conn {
        let s = TcpStream::connect(addr).unwrap();
        Conn {
            reader: BufReader::new(s.try_clone().unwrap()),
            writer: s,
        }
    }

    fn send(&mut self, line: &str) {
        writeln!(self.writer, "{line}").unwrap();
    }

    /// The next line, or `None` at end of stream.
    fn next(&mut self) -> Option<Value> {
        let mut l = String::new();
        (self.reader.read_line(&mut l).unwrap_or(0) > 0).then(|| serde_json::from_str(&l).unwrap())
    }
}

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
    // When the first goes away its place is free again.
    drop(first);
    let mut again = None;
    for _ in 0..10_000 {
        let mut c = Conn::open(serving.addr());
        c.send(&hello);
        match c.next() {
            Some(v) if v["ok"] == true => {
                again = Some(c);
                break;
            }
            _ => std::thread::yield_now(),
        }
    }
    assert!(
        again.is_some(),
        "a place became free after the first client left"
    );
    drop(serving); // dropping stops it too
}

#[test]
fn a_listener_that_is_not_loopback_is_refused_before_serving() {
    // Binding a real non-loopback socket in a test would open the machine to the network; `check_loopback` has its own unit test.
    let r = rig();
    let l = listener();
    assert!(serve_loopback(r.server.clone(), Arc::new(KeyStore::new()), l, 1).is_ok());
}
