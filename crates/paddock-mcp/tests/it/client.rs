//! The client against the real listener (ADR 0023 WP-8): `connect_loopback` is the one handshake (the bridge uses it too),
//! and a `Connection` lists and calls the server's tools. Real loopback sockets and a temp folder only (ER-14).

use crate::common::*;
use paddock_mcp::KeyStore;
use paddock_mcp::client::{
    ClientError, ConnectError, ConnectLimits, ConnectionConfig, connect_loopback,
    connect_loopback_with,
};
use paddock_mcp::listener::{Discovery, Listener, discovery_path};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// A logger that listens, so the log lines' arguments are built (they are not when no logger listens).
fn listen() {
    struct Quiet;
    impl log::Log for Quiet {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, _: &log::Record<'_>) {}
        fn flush(&self) {}
    }
    let _ = log::set_logger(&Quiet);
    log::set_max_level(log::LevelFilter::Trace);
}

fn serving(dir: &std::path::Path) -> (Arc<Listener>, Arc<KeyStore>, Rig) {
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

#[test]
fn connect_loopback_does_the_handshake_and_the_connection_lists_and_calls_tools() {
    listen();
    let dir = tempfile::tempdir().unwrap();
    let (l, keys, _r) = serving(dir.path());
    let (_, key) = keys.issue("Pitwall", 1).unwrap();
    l.reconcile(true);

    let link = connect_loopback(&discovery_path(dir.path()), &key).unwrap();
    assert!(!link.app().is_empty());
    let conn = link.into_connection(ConnectionConfig::default());
    let init = conn.initialize("Pitwall", "1.1.0").unwrap();
    assert!(!init.server_info.name.is_empty());
    let tools = conn.list_tools().unwrap();
    assert!(tools.iter().any(|t| t.name == "echo"), "{tools:?}");
    let out = conn
        .call_tool(
            "echo",
            json!({"text": "hello"}),
            Some(Duration::from_secs(10)),
        )
        .unwrap();
    assert_eq!(out.text(), "hello");
    // A tool that fails is a result with is_error, not a client error.
    let boom = conn
        .call_tool("boom", json!({}), Some(Duration::from_secs(10)))
        .unwrap();
    assert!(boom.is_error, "{boom:?}");
    // An unknown tool is the server's protocol error, with its message.
    match conn.call_tool("nope", json!({}), Some(Duration::from_secs(10))) {
        Err(ClientError::Rpc { code, .. }) => assert_eq!(code, -32602),
        other => panic!("{other:?}"),
    }
}

#[test]
fn connect_loopback_says_what_to_do_for_each_refusal() {
    listen();
    let dir = tempfile::tempdir().unwrap();
    let err = connect_loopback(&discovery_path(dir.path()), "pmk_x").unwrap_err();
    assert!(matches!(err, ConnectError::NotOpen { .. }), "{err:?}");
    assert!(err.to_string().contains("isn't open"), "{err}");

    let (l, keys, _r) = serving(dir.path());
    keys.issue("a", 1).unwrap();
    l.reconcile(true);
    let err = connect_loopback(&discovery_path(dir.path()), "pmk_wrong").unwrap_err();
    assert!(matches!(err, ConnectError::KeyRefused { .. }), "{err:?}");
    assert!(err.to_string().contains("refused the key"), "{err}");

    // A file whose process is gone is never trusted with a key.
    let port = match l.view() {
        paddock_mcp::listener::ListenerView::Listening { port, .. } => port,
        other => panic!("{other:?}"),
    };
    let stale = Discovery {
        pid: std::process::id(),
        started_us: 1,
        port,
        app: "x".into(),
        version: "1".into(),
    };
    std::fs::write(
        discovery_path(dir.path()),
        serde_json::to_string(&stale).unwrap(),
    )
    .unwrap();
    let err = connect_loopback(&discovery_path(dir.path()), "pmk_x").unwrap_err();
    assert!(matches!(err, ConnectError::NotOpen { .. }), "{err:?}");
}

/// A fake app: a discovery file naming this test process (so the client trusts it) and a loopback port; `then` runs on the one
/// connection after its key line was read. Real sockets and a temp folder only (ER-14).
fn fake_app(
    dir: &std::path::Path,
    then: impl FnOnce(std::net::TcpStream) + Send + 'static,
) -> std::thread::JoinHandle<()> {
    use std::io::BufRead;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let me = paddock_proc::ProcessId::of(std::process::id()).unwrap();
    let hint = Discovery {
        pid: me.pid(),
        started_us: me.started().as_micros(),
        port: listener.local_addr().unwrap().port(),
        app: "Demo".into(),
        version: "2.0".into(),
    };
    std::fs::write(discovery_path(dir), serde_json::to_string(&hint).unwrap()).unwrap();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        std::io::BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert!(line.contains("\"key\""), "{line}");
        then(stream);
    })
}

#[test]
fn a_listener_that_answers_wrongly_or_not_at_all_gets_a_message_each() {
    use std::io::Write;
    type Then = Box<dyn FnOnce(std::net::TcpStream) + Send>;
    let cases: Vec<(&str, Then)> = vec![
        ("closed", Box::new(drop)),
        (
            "not-listener",
            Box::new(|mut s| s.write_all(b"hello\n").unwrap()),
        ),
        (
            "busy",
            Box::new(|mut s| {
                s.write_all(b"{\"ok\":false,\"code\":\"busy\",\"error\":\"full\"}\n")
                    .unwrap()
            }),
        ),
        (
            "no-reason",
            Box::new(|mut s| s.write_all(b"{\"ok\":false}\n").unwrap()),
        ),
    ];
    for (name, then) in cases {
        listen();
        let dir = tempfile::tempdir().unwrap();
        let peer = fake_app(dir.path(), then);
        let err = connect_loopback(&discovery_path(dir.path()), "pmk_x").unwrap_err();
        peer.join().unwrap();
        match (name, &err) {
            ("closed", ConnectError::ClosedEarly { app })
            | ("not-listener", ConnectError::NotListener { app }) => assert_eq!(app, "Demo"),
            ("busy", ConnectError::Refused { why, .. }) => assert_eq!(why, "full"),
            ("no-reason", ConnectError::Refused { why, .. }) => assert_eq!(why, "no reason given"),
            _ => panic!("{name}: {err:?}"),
        }
        assert!(!err.to_string().contains("refused the key"), "{err}");
    }
}

#[test]
fn a_listener_that_accepts_and_never_answers_ends_at_the_answer_limit() {
    listen();
    let dir = tempfile::tempdir().unwrap();
    let (release, hold) = std::sync::mpsc::channel::<()>();
    let peer = fake_app(dir.path(), move |_stream| {
        // Holds the connection open, silent, until the test is done.
        let _ = hold.recv();
    });
    let limits = ConnectLimits {
        answer: Duration::from_millis(50),
        ..ConnectLimits::default()
    };
    let err = connect_loopback_with(&discovery_path(dir.path()), "pmk_x", limits).unwrap_err();
    assert!(matches!(err, ConnectError::NoAnswer { .. }), "{err:?}");
    assert!(err.to_string().contains("did not answer the key"), "{err}");
    release.send(()).unwrap();
    peer.join().unwrap();
}

#[test]
fn a_live_process_whose_port_nobody_serves_reads_as_not_open() {
    listen();
    let dir = tempfile::tempdir().unwrap();
    // Port 0 is one no listener ever holds, so the connect fails on every OS without a freed port that could be reused (C39).
    let port = 0;
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
    let err = connect_loopback(&discovery_path(dir.path()), "pmk_x").unwrap_err();
    assert_eq!(err, ConnectError::NotOpen { app: "Demo".into() });
}

#[test]
fn every_connect_error_says_what_happened() {
    let app = || "Demo".to_string();
    let cases = [
        (
            ConnectError::NoAnswer {
                app: app(),
                cause: "c".into(),
            },
            "did not answer the key",
        ),
        (
            ConnectError::KeyRefused {
                app: app(),
                why: "revoked".into(),
            },
            "refused the key: revoked",
        ),
    ];
    for (e, text) in cases {
        assert!(e.to_string().contains(text), "{e}");
    }
    assert_eq!(ConnectLimits::default().connect, Duration::from_secs(5));
}

#[test]
fn the_link_reports_its_version_and_a_connection_pings() {
    listen();
    let dir = tempfile::tempdir().unwrap();
    let (l, keys, _r) = serving(dir.path());
    let (_, key) = keys.issue("Pitwall", 1).unwrap();
    l.reconcile(true);
    let link = connect_loopback(&discovery_path(dir.path()), &key).unwrap();
    assert!(!link.version().is_empty());
    let conn = link.into_connection(ConnectionConfig::default());
    conn.ping().unwrap();
    drop(conn);
    drop(l);
}

/// A long-lived child that stands for the app's process: the discovery file can name it with its true start time, and killing it
/// is the app being killed. Never the user's programs (ER-14).
fn owner_process() -> std::process::Child {
    #[cfg(unix)]
    let mut c = std::process::Command::new("sleep");
    #[cfg(unix)]
    c.arg("300");
    #[cfg(windows)]
    let mut c = std::process::Command::new("ping");
    #[cfg(windows)]
    c.args(["-n", "300", "127.0.0.1"]);
    c.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap()
}

/// A discovery file for `owner` (its true start time) and a listener on a loopback port.
fn owned_listener(dir: &std::path::Path, owner: &std::process::Child) -> std::net::TcpListener {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let id = paddock_proc::ProcessId::of(owner.id()).unwrap();
    let hint = Discovery {
        pid: id.pid(),
        started_us: id.started().as_micros(),
        port: listener.local_addr().unwrap().port(),
        app: "Demo".into(),
        version: "1".into(),
    };
    std::fs::write(discovery_path(dir), serde_json::to_string(&hint).unwrap()).unwrap();
    listener
}

/// Limits far above any wait a test should reach: only the owner's end can finish the handshake early, so a client that does not
/// look at its owner takes the whole of it.
const LONG: ConnectLimits = ConnectLimits {
    connect: Duration::from_secs(30),
    answer: Duration::from_secs(30),
};

/// The owner is killed while its port holds the connection and has not answered the key. `close_socket` says whether the dying
/// process's socket is then closed (what the OS does when a process ends) or stays open (a listener a backlog still completes).
fn killed_while_the_key_is_unanswered(close_socket: bool) {
    use std::io::BufRead;
    listen();
    let dir = tempfile::tempdir().unwrap();
    let mut owner = owner_process();
    let listener = owned_listener(dir.path(), &owner);
    let (got_key, key_arrived) = std::sync::mpsc::channel();
    let (end, hold) = std::sync::mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        std::io::BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        got_key.send(()).unwrap();
        // Until the test says the owner is dead (and, for a closing socket, the connection ends then).
        let _ = hold.recv_timeout(Duration::from_secs(60));
        drop(stream);
    });
    let file = discovery_path(dir.path());
    let client = std::thread::spawn(move || connect_loopback_with(&file, "pmk_x", LONG).err());
    key_arrived.recv_timeout(Duration::from_secs(30)).unwrap();
    owner.kill().unwrap();
    owner.wait().unwrap();
    let killed = std::time::Instant::now();
    if close_socket {
        end.send(()).unwrap();
    }
    let err = client.join().unwrap();
    let waited = killed.elapsed();
    let _ = end.send(());
    peer.join().unwrap();
    assert_eq!(err, Some(ConnectError::NotOpen { app: "Demo".into() }));
    // conformance: hang-guard (the limits are 30 s; the client looks at its owner every 100 ms)
    assert!(
        waited < Duration::from_secs(10),
        "the client waited {waited:?} for an app that was gone"
    );
}

#[test]
fn an_owner_killed_while_the_key_is_unanswered_reads_as_not_open_at_once() {
    killed_while_the_key_is_unanswered(false);
}

#[test]
fn an_owner_whose_socket_closes_as_it_dies_reads_as_not_open_not_as_a_closed_connection() {
    killed_while_the_key_is_unanswered(true);
}

#[test]
fn a_stale_file_of_a_killed_owner_answers_without_touching_its_port() {
    listen();
    let dir = tempfile::tempdir().unwrap();
    let mut owner = owner_process();
    // A port that completes connections and never answers: contacting it would cost the whole answer limit.
    let listener = owned_listener(dir.path(), &owner);
    owner.kill().unwrap();
    owner.wait().unwrap();
    let started = std::time::Instant::now();
    let err = connect_loopback_with(&discovery_path(dir.path()), "pmk_x", LONG).unwrap_err();
    assert_eq!(
        err,
        ConnectError::NotOpen {
            app: "The app".into()
        }
    );
    // conformance: hang-guard (nothing is contacted: the 30 s limits would be spent otherwise)
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    drop(listener);
}

/// A killed app that its parent has not collected yet is a zombie: its pid and start time still read, but nothing runs.
#[cfg(unix)]
#[test]
fn a_killed_owner_that_is_not_collected_yet_reads_as_not_open_at_once() {
    listen();
    let dir = tempfile::tempdir().unwrap();
    let mut owner = owner_process();
    let listener = owned_listener(dir.path(), &owner);
    owner.kill().unwrap();
    // Not waited for: the child stays a zombie until `wait` below. The kill is delivered when the signal call returns, but the
    // process table changes a moment later, so look until it is a zombie (a condition, not a delay).
    let id = paddock_proc::ProcessId::of(owner.id()).unwrap();
    while !id.has_ended() {
        std::thread::yield_now();
    }
    let started = std::time::Instant::now();
    let err = connect_loopback_with(&discovery_path(dir.path()), "pmk_x", LONG).unwrap_err();
    assert_eq!(
        err,
        ConnectError::NotOpen {
            app: "The app".into()
        }
    );
    // conformance: hang-guard (nothing is contacted: the 30 s limits would be spent otherwise)
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    owner.wait().unwrap();
    drop(listener);
}
