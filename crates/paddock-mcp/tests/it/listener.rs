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
