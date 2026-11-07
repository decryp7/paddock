//! The app-side listener (ADR 0007 rule 1): it serves on 127.0.0.1 only while AI access is on and an unrevoked key exists,
//! writes a discovery file that is only a hint (rule 53), and the stdio bridge a host starts reaches it. Real loopback sockets
//! and a temp folder; nothing else is touched (ER-14).

use crate::common::*;
use paddock_mcp::listener::{Discovery, Listener, ListenerView, discovery_path};
use paddock_mcp::{KeyStore, bridge, client};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Cursor, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

fn rig_listener(dir: &std::path::Path) -> (Arc<Listener>, Arc<KeyStore>, Rig) {
    let r = rig();
    let keys = Arc::new(KeyStore::new());
    let l = Arc::new(Listener::new(
        r.server.clone(),
        keys.clone(),
        discovery_path(dir),
        4,
    ));
    (l, keys, r)
}

fn port_of(l: &Listener) -> u16 {
    match l.view() {
        ListenerView::Listening { port, .. } => port,
        other => panic!("not listening: {other:?}"),
    }
}

fn talk(port: u16, key: &str, lines: &[String]) -> Vec<Value> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(format!("{}\n", json!({"key": key})).as_bytes())
        .unwrap();
    for l in lines {
        s.write_all(format!("{l}\n").as_bytes()).unwrap();
    }
    let mut reader = BufReader::new(s);
    let mut out = vec![];
    let want = 1 + lines.iter().filter(|l| l.contains("\"id\"")).count();
    while out.len() < want {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        out.push(serde_json::from_str(&line).unwrap());
    }
    out
}

#[test]
fn nothing_listens_while_ai_access_is_off_or_no_key_exists() {
    let dir = tempfile::tempdir().unwrap();
    let (l, keys, _r) = rig_listener(dir.path());
    l.reconcile(false);
    assert_eq!(l.view(), ListenerView::Off);
    l.reconcile(true);
    assert_eq!(
        l.view(),
        ListenerView::NoKey,
        "on but no key: nothing to pair with"
    );
    assert!(!discovery_path(dir.path()).exists());
    keys.issue("Pitwall", 1).unwrap();
    l.reconcile(true);
    assert!(matches!(l.view(), ListenerView::Listening { .. }));
}

#[test]
fn a_client_with_a_key_lists_and_calls_tools_and_a_wrong_key_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (l, keys, _r) = rig_listener(dir.path());
    let (_, key) = keys.issue("Pitwall", 1).unwrap();
    l.reconcile(true);
    let port = port_of(&l);
    assert!(Listener::addr(&l).unwrap().ip().is_loopback());
    let replies = talk(
        port,
        &key,
        &[
            client::initialize(1, "test", "1"),
            client::initialized(),
            client::list_tools(2, None),
            client::call_tool(3, "echo", json!({"text": "hi"})),
        ],
    );
    assert_eq!(replies[0], json!({"ok": true, "client": "Pitwall"}));
    let tools = replies.iter().find(|v| v["id"] == 2).unwrap();
    assert!(tools["result"]["tools"].to_string().contains("echo"));
    let call = replies.iter().find(|v| v["id"] == 3).unwrap();
    assert_eq!(
        client::parse_call(call["result"].clone()).unwrap().text(),
        "hi"
    );

    let refused = talk(port, "pmk_wrong", &[]);
    assert_eq!(refused[0]["ok"], false);
}

#[test]
fn turning_access_off_closes_the_listener_and_removes_only_its_own_file() {
    let dir = tempfile::tempdir().unwrap();
    let (l, keys, _r) = rig_listener(dir.path());
    keys.issue("a", 1).unwrap();
    l.reconcile(true);
    let port = port_of(&l);
    let path = discovery_path(dir.path());
    let hint = Discovery::read(&path).expect("a discovery file");
    assert_eq!(hint.port, port);
    assert_eq!(hint.pid, std::process::id());
    assert!(
        !std::fs::read_to_string(&path).unwrap().contains("pmk_"),
        "no key in the file"
    );

    l.reconcile(false);
    assert_eq!(l.view(), ListenerView::Off);
    assert!(!path.exists());
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "no listener after off"
    );

    // A file another instance wrote since is not ours to remove.
    l.reconcile(true);
    let other = Discovery {
        pid: 1,
        started_us: 5,
        port: 9,
        app: "x".into(),
        version: "1".into(),
    };
    std::fs::write(&path, serde_json::to_string(&other).unwrap()).unwrap();
    l.reconcile(false);
    assert_eq!(Discovery::read(&path), Some(other));
}

#[test]
fn a_revoked_last_key_stops_the_listener_and_a_second_instance_does_not_clash() {
    let dir = tempfile::tempdir().unwrap();
    let dir2 = tempfile::tempdir().unwrap();
    let (a, ka, _ra) = rig_listener(dir.path());
    let (b, kb, _rb) = rig_listener(dir2.path());
    let (ida, _) = ka.issue("a", 1).unwrap();
    kb.issue("b", 1).unwrap();
    a.reconcile(true);
    b.reconcile(true);
    assert_ne!(port_of(&a), port_of(&b), "ephemeral ports never clash");
    ka.revoke(ida);
    a.reconcile(true);
    assert_eq!(a.view(), ListenerView::NoKey);
    assert!(matches!(b.view(), ListenerView::Listening { .. }));
}

#[test]
fn the_bridge_carries_a_hosts_stdio_lines_to_the_running_app() {
    let dir = tempfile::tempdir().unwrap();
    let (l, keys, _r) = rig_listener(dir.path());
    let (_, key) = keys.issue("Claude Desktop", 1).unwrap();
    l.reconcile(true);
    let input = [
        client::initialize(1, "host", "1"),
        client::initialized(),
        client::call_tool(2, "echo", json!({"text": "via stdio"})),
    ]
    .join("\n");
    let mut out = Vec::new();
    bridge::run(
        &discovery_path(dir.path()),
        &key,
        Cursor::new(input.into_bytes()),
        &mut out,
    )
    .unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("via stdio"), "{text}");
    assert!(
        !text.contains("\"ok\":true"),
        "the handshake reply stays between the bridge and the app"
    );
}

#[test]
fn the_bridge_says_so_when_the_app_is_not_open_or_the_key_is_wrong() {
    let dir = tempfile::tempdir().unwrap();
    let err = bridge::run(
        &discovery_path(dir.path()),
        "pmk_x",
        Cursor::new(vec![]),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("isn't open"), "{err}");

    let (l, keys, _r) = rig_listener(dir.path());
    keys.issue("a", 1).unwrap();
    l.reconcile(true);
    let err = bridge::run(
        &discovery_path(dir.path()),
        "pmk_wrong",
        Cursor::new(vec![]),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("key"), "{err}");

    // A stale file whose process is gone is not trusted: the key is never sent to its port.
    let stale = Discovery {
        pid: std::process::id(),
        started_us: 1,
        port: port_of(&l),
        app: "x".into(),
        version: "1".into(),
    };
    std::fs::write(
        discovery_path(dir.path()),
        serde_json::to_string(&stale).unwrap(),
    )
    .unwrap();
    let err = bridge::run(
        &discovery_path(dir.path()),
        "pmk_x",
        Cursor::new(vec![]),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("isn't open"), "{err}");
}

/// A discovery file that cannot be written fails the start with a message that says what to fix; nothing keeps listening
/// (the port it had opened is closed) and a later reconcile with a writable place starts normally.
#[test]
fn an_unwritable_discovery_file_is_reported_and_leaves_nothing_listening() {
    let dir = tempfile::tempdir().unwrap();
    // The file's parent is a regular file, so the hint can never be created there.
    let blocker = dir.path().join("not-a-folder");
    std::fs::write(&blocker, b"x").unwrap();
    let r = rig();
    let keys = Arc::new(KeyStore::new());
    keys.issue("Pitwall", 1).unwrap();
    let l = Listener::new(r.server.clone(), keys.clone(), blocker.join("mcp.json"), 4);
    l.reconcile(true);
    match l.view() {
        ListenerView::Failed { message } => {
            assert!(message.contains("Could not write"), "{message}");
            assert!(message.contains("writable"), "{message}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    assert!(l.addr().is_none(), "a failed start keeps no listener");
    // Turning access off clears the failure.
    l.reconcile(false);
    assert_eq!(l.view(), ListenerView::Off);
}

/// What the fake app does with a bridge's connection after the key line.
#[derive(Clone, Copy)]
enum PeerGoes {
    /// Closes without answering the key (the app went away mid-handshake).
    BeforeTheKeyAnswer,
    /// Answers the key, reads every request the host sent and closes without answering any (the field case: `dyno mcp`
    /// printed nothing and exited 0 while the agent restarted).
    AfterReadingTheRequests,
    /// Answers the key line with this refusal line and closes.
    Refusing(&'static str),
    /// Answers the key, waits until the host's first request has arrived and resets the connection: the bridge's read
    /// fails instead of ending.
    #[cfg(unix)]
    ResettingAfterTheFirstRequest,
    /// Answers the key, then reads until the host's side ends and answers only the requests with these ids, as the server
    /// does with a call the host cancelled (it never answers it).
    AnsweringOnly(&'static [i64]),
}

/// Makes closing `s` reset the connection (SO_LINGER 0).
#[cfg(unix)]
fn reset_on_close(s: &TcpStream) {
    use std::os::fd::AsRawFd;
    let linger = libc::linger {
        l_onoff: 1,
        l_linger: 0,
    };
    // SAFETY: a valid socket and a correctly sized option value.
    let r = unsafe {
        libc::setsockopt(
            s.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_LINGER,
            (&raw const linger).cast::<libc::c_void>(),
            libc::socklen_t::try_from(std::mem::size_of::<libc::linger>()).unwrap(),
        )
    };
    assert_eq!(r, 0, "SO_LINGER");
}

/// A fake app: a discovery file naming this test process (so the bridge trusts it) and a loopback port whose one connection
/// ends as `goes` says. Real sockets and a temp folder only (ER-14).
fn peer_that_goes(
    dir: &std::path::Path,
    goes: PeerGoes,
    requests: usize,
) -> std::thread::JoinHandle<()> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let me = paddock_proc::ProcessId::of(std::process::id()).unwrap();
    let hint = Discovery {
        pid: me.pid(),
        started_us: me.started().as_micros(),
        port: listener.local_addr().unwrap().port(),
        app: "Demo".into(),
        version: "1".into(),
    };
    std::fs::write(discovery_path(dir), serde_json::to_string(&hint).unwrap()).unwrap();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(
            line.contains("\"key\""),
            "the bridge sends its key first: {line}"
        );
        if let PeerGoes::Refusing(answer) = goes {
            (&stream)
                .write_all(format!("{answer}\n").as_bytes())
                .unwrap();
        }
        #[cfg(unix)]
        if let PeerGoes::ResettingAfterTheFirstRequest = goes {
            (&stream)
                .write_all(b"{\"ok\":true,\"client\":\"host\"}\n")
                .unwrap();
            // Blocks until the request is there (a condition, not a time), then closes with a reset (SO_LINGER 0), as an
            // app that crashes does.
            stream.peek(&mut [0u8; 1]).unwrap();
            reset_on_close(&stream);
        }
        if let PeerGoes::AfterReadingTheRequests = goes {
            (&stream)
                .write_all(b"{\"ok\":true,\"client\":\"host\"}\n")
                .unwrap();
            let mut seen = 0;
            while seen < requests {
                line.clear();
                if reader.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                if line.contains("\"id\"") {
                    seen += 1;
                }
            }
        }
        if let PeerGoes::AnsweringOnly(ids) = goes {
            (&stream)
                .write_all(b"{\"ok\":true,\"client\":\"host\"}\n")
                .unwrap();
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                let v: Value = serde_json::from_str(&line).unwrap();
                if let Some(id) = v["id"].as_i64().filter(|id| ids.contains(id)) {
                    (&stream)
                        .write_all(
                            format!("{}\n", json!({"jsonrpc": "2.0", "id": id, "result": {}}))
                                .as_bytes(),
                        )
                        .unwrap();
                }
            }
        }
        // Dropping both handles closes the connection with nothing left unread.
    })
}

fn four_lines() -> String {
    [
        client::initialize(1, "host", "1"),
        client::initialized(),
        client::list_tools(2, None),
        client::call_tool(3, "system_summary", json!({})),
    ]
    .join("\n")
        + "\n"
}

/// The field case of 2026-11-07 (Linux VM, RC 2e66322a4): the app's side of the connection ended after the handshake without
/// an answer, and `dyno mcp` printed nothing and exited 0. The bridge must answer every request it carried with a JSON-RPC
/// error and end with an error that says what happened.
#[test]
fn the_bridge_answers_every_request_when_the_app_ends_the_connection_before_answering() {
    let dir = tempfile::tempdir().unwrap();
    let peer = peer_that_goes(dir.path(), PeerGoes::AfterReadingTheRequests, 3);
    let mut out = Vec::new();
    let result = bridge::run(
        &discovery_path(dir.path()),
        "pmk_x",
        Cursor::new(four_lines().into_bytes()),
        &mut out,
    );
    peer.join().unwrap();
    let text = String::from_utf8(out).unwrap();
    let replies: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let mut ids: Vec<i64> = replies.iter().filter_map(|r| r["id"].as_i64()).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        [1, 2, 3],
        "every request gets an answer, got: {text:?}"
    );
    for r in &replies {
        assert_eq!(r["jsonrpc"], "2.0", "{r}");
        let message = r["error"]["message"].as_str().unwrap_or_default();
        assert!(message.contains("ended the connection"), "{r}");
    }
    let err = result
        .expect_err("an app that ends the connection unasked is not a clean end (exit 1, not 0)");
    assert!(
        err.to_string().contains("Demo ended the connection"),
        "{err}"
    );
}

/// The app went away between the key line and its answer (it restarted mid-handshake): a clear error, never "refused the key".
#[test]
fn the_bridge_says_the_app_went_away_when_the_handshake_has_no_answer() {
    let dir = tempfile::tempdir().unwrap();
    let peer = peer_that_goes(dir.path(), PeerGoes::BeforeTheKeyAnswer, 0);
    let err = bridge::run(
        &discovery_path(dir.path()),
        "pmk_x",
        Cursor::new(four_lines().into_bytes()),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    peer.join().unwrap();
    assert!(
        err.contains("closed the connection before answering the key"),
        "{err}"
    );
    assert!(!err.contains("refused the key"), "{err}");
}

/// Field case C: the app could not read the key line (a signal interrupted the read) and turned the connection down, and the
/// bridge said "Dyno refused the key", which sends users to issue a new key for nothing. Only a key refusal may say so.
#[test]
fn the_bridge_says_refused_the_key_only_when_the_key_was_refused() {
    let turned_down = |answer: &'static str| {
        let dir = tempfile::tempdir().unwrap();
        let peer = peer_that_goes(dir.path(), PeerGoes::Refusing(answer), 0);
        let err = bridge::run(
            &discovery_path(dir.path()),
            "pmk_x",
            Cursor::new(vec![]),
            &mut Vec::new(),
        )
        .unwrap_err()
        .to_string();
        peer.join().unwrap();
        err
    };
    // What the listener now sends when the key line could not be read.
    let err = turned_down(
        r#"{"ok":false,"code":"handshake","error":"the app could not read the key line; try again"}"#,
    );
    assert!(
        !err.contains("refused the key"),
        "a key that was never read is not a refused key: {err}"
    );
    assert!(
        err.contains("Demo turned the connection down: the app could not read the key line"),
        "{err}"
    );
    // What RC 2e66322a4 sent in the field (no code): not reported as a key refusal either.
    let err = turned_down(r#"{"ok":false,"error":"send {\"key\": \"...\"} as the first line"}"#);
    assert!(!err.contains("refused the key"), "{err}");

    // The real listener: a wrong key is still reported as refused.
    let dir = tempfile::tempdir().unwrap();
    let r = rig();
    let keys = Arc::new(KeyStore::new());
    keys.issue("Claude Desktop", 1).unwrap();
    let l = Listener::new(
        r.server.clone(),
        keys.clone(),
        discovery_path(dir.path()),
        1,
    );
    l.reconcile(true);
    let err = bridge::run(
        &discovery_path(dir.path()),
        "pmk_wrong",
        Cursor::new(vec![]),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("demo refused the key: the key is missing, wrong or revoked"),
        "{err}"
    );
}

/// A host that has gone (its output fails) ends the bridge without an error: nobody is left to read one.
#[test]
fn the_bridge_ends_quietly_when_the_host_stops_reading() {
    struct Gone;
    impl Write for Gone {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let (l, keys, _r) = rig_listener(dir.path());
    let (_, key) = keys.issue("host", 1).unwrap();
    l.reconcile(true);
    let input = client::initialize(1, "host", "1") + "\n";
    bridge::run(
        &discovery_path(dir.path()),
        &key,
        Cursor::new(input.into_bytes()),
        &mut Gone,
    )
    .unwrap();
}

#[cfg(unix)]
/// The app's side failed (reset) rather than ended: every request the bridge carried is still answered. macOS reports the
/// reset to the read, so the error names it; Linux may report a reset that arrives during the read as the end of input, so
/// there only the answers are checked.
#[test]
fn the_bridge_answers_with_errors_when_the_app_resets_the_connection() {
    let dir = tempfile::tempdir().unwrap();
    let peer = peer_that_goes(dir.path(), PeerGoes::ResettingAfterTheFirstRequest, 0);
    let mut out = Vec::new();
    let err = bridge::run(
        &discovery_path(dir.path()),
        "pmk_x",
        Cursor::new(four_lines().into_bytes()),
        &mut out,
    )
    .unwrap_err()
    .to_string();
    peer.join().unwrap();
    assert!(err.contains("Demo ended the connection"), "{err}");
    #[cfg(target_os = "macos")]
    assert!(
        err.contains("Demo ended the connection ("),
        "the cause is named: {err}"
    );
    let text = String::from_utf8(out).unwrap();
    assert!(
        !text.is_empty(),
        "at least the request that was there gets an answer"
    );
    for l in text.lines() {
        let r: Value = serde_json::from_str(l).unwrap();
        assert!(
            r["error"]["message"]
                .as_str()
                .unwrap()
                .contains("ended the connection before answering"),
            "{r}"
        );
    }
}

/// A host that has gone when the bridge has errors to write for unanswered requests: the bridge still ends with its error.
#[test]
fn the_bridge_ends_with_its_error_when_the_host_is_gone_too() {
    struct Gone;
    impl Write for Gone {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let peer = peer_that_goes(dir.path(), PeerGoes::AfterReadingTheRequests, 3);
    let err = bridge::run(
        &discovery_path(dir.path()),
        "pmk_x",
        Cursor::new(four_lines().into_bytes()),
        &mut Gone,
    )
    .unwrap_err();
    peer.join().unwrap();
    assert!(err.to_string().contains("3 request(s) unanswered"), "{err}");
}

/// A discovery file whose process is alive but whose port has nobody listening (the listener just closed): "isn't open".
#[test]
fn the_bridge_says_the_app_is_not_open_when_its_port_refuses_the_connection() {
    struct Quiet;
    impl log::Log for Quiet {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, _: &log::Record<'_>) {}
        fn flush(&self) {}
    }
    // The bridge's log line is built only when a logger listens.
    let _ = log::set_logger(&Quiet);
    log::set_max_level(log::LevelFilter::Warn);
    let dir = tempfile::tempdir().unwrap();
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let me = paddock_proc::ProcessId::of(std::process::id()).unwrap();
    let hint = Discovery {
        pid: me.pid(),
        started_us: me.started().as_micros(),
        port,
        app: "Demo".into(),
        version: "1".into(),
    };
    std::fs::write(
        discovery_path(dir.path()),
        serde_json::to_string(&hint).unwrap(),
    )
    .unwrap();
    let err = bridge::run(
        &discovery_path(dir.path()),
        "pmk_x",
        Cursor::new(vec![]),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("Demo isn't open"), "{err}");
}

/// A call the host cancelled is never answered (MCP; `Server::handle`), so it is not left waiting for an answer: the session
/// ends cleanly instead of with an error for that id (review of 31f2bb60b, P1).
#[test]
fn a_call_the_host_cancelled_is_not_reported_unanswered() {
    let dir = tempfile::tempdir().unwrap();
    let peer = peer_that_goes(dir.path(), PeerGoes::AnsweringOnly(&[1]), 0);
    let input = [
        client::initialize(1, "host", "1"),
        client::call_tool(2, "system_summary", json!({})),
        json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 2, "reason": "user"}}).to_string(),
    ]
    .join("\n")
        + "\n";
    let mut out = Vec::new();
    let result = bridge::run(
        &discovery_path(dir.path()),
        "pmk_x",
        Cursor::new(input.into_bytes()),
        &mut out,
    );
    peer.join().unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(
        result.is_ok(),
        "a cancelled call is not an unanswered one: {result:?}, output {text:?}"
    );
    assert_eq!(
        text.lines().count(),
        1,
        "only the initialize answer: {text:?}"
    );
}

/// Requests whose id the server cannot accept (1.5, true, an object) are answered by the server with id null; the bridge
/// must not wait for them as well and answer them a second time with -32603 (review of 31f2bb60b, P2).
#[test]
fn requests_with_ids_the_server_rejects_are_answered_once() {
    let dir = tempfile::tempdir().unwrap();
    let (l, keys, _r) = rig_listener(dir.path());
    let (_, key) = keys.issue("host", 1).unwrap();
    l.reconcile(true);
    let input = [
        client::initialize(1, "host", "1"),
        json!({"jsonrpc": "2.0", "id": 1.5, "method": "ping"}).to_string(),
        json!({"jsonrpc": "2.0", "id": true, "method": "ping"}).to_string(),
        json!({"jsonrpc": "2.0", "id": {"a": 1}, "method": "ping"}).to_string(),
    ]
    .join("\n")
        + "\n";
    let mut out = Vec::new();
    let result = bridge::run(
        &discovery_path(dir.path()),
        &key,
        Cursor::new(input.into_bytes()),
        &mut out,
    );
    let text = String::from_utf8(out).unwrap();
    assert!(result.is_ok(), "{result:?}, output {text:?}");
    assert_eq!(text.lines().count(), 4, "one answer each: {text:?}");
    assert!(!text.contains("-32603"), "{text}");
}
