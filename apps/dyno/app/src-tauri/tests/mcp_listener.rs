//! Dyno's AI access listener end to end: the real `mcp::control` (as the shell builds it) with a scripted agent, a real loopback
//! socket, a real key, and the `dyno mcp` bridge's library call. Settings live in a temp folder and the keys in memory (ER-14).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use dyno_app_lib::mcp::{self, AgentCalls};
use dyno_core::protocol::{LiveUpdate, Reply, Request};
use paddock_mcp::listener::{discovery_path, ListenerView};
use paddock_mcp::{bridge, client, McpControl, McpSettings};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Cursor, Write};
use std::net::TcpStream;
use std::sync::Arc;

struct Script;

impl AgentCalls for Script {
    fn call(&self, request: Request) -> Result<Reply, String> {
        Err(format!("The script does not answer {request:?}."))
    }

    fn live(&self) -> Result<LiveUpdate, String> {
        let mut system = dyno_platform::fake::quiet_sample(dyno_core::time::Timestamp(1_700_000_000_000));
        system.cpu.total = 12.0;
        Ok(LiveUpdate {
            system,
            pressure: dyno_core::metrics::MemoryPressure::Normal,
            top_cpu: vec![],
            top_memory: vec![],
            overhead: dyno_core::protocol::Overhead::default(),
            processes: None,
            groups: None,
            other_users: None,
            via: Default::default(),
        })
    }
}

fn control(dir: &std::path::Path) -> (Arc<McpControl>, paddock_env::AppDirs) {
    let id = paddock_env::AppId::new(dyno_app_lib::APP_ID, dyno_app_lib::APP_NAME).unwrap();
    let dirs = paddock_env::AppDirs::resolve(
        &id,
        paddock_env::Os::current(),
        &paddock_env::MapVars::new().with(&id.home_var(), dir.to_str().unwrap()),
        None,
        true,
    )
    .unwrap();
    (
        mcp::control(&dirs, "Dyno", "1.1.0", Arc::new(|_| {}), Arc::new(Script)),
        dirs,
    )
}

fn port(c: &McpControl) -> u16 {
    match c.get().listener {
        ListenerView::Listening { port, .. } => port,
        other => panic!("not listening: {other:?}"),
    }
}

fn ask(port: u16, key: &str, lines: &[String]) -> Vec<Value> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    writeln!(s, "{}", json!({"key": key})).unwrap();
    for l in lines {
        writeln!(s, "{l}").unwrap();
    }
    let want = 1 + lines.iter().filter(|l| l.contains("\"id\"")).count();
    let mut reader = BufReader::new(s);
    let mut out = vec![];
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
fn dyno_serves_mcp_on_loopback_only_while_access_is_on_and_a_key_exists() {
    let dir = tempfile::tempdir().unwrap();
    let (control, dirs) = control(dir.path());
    assert_eq!(control.get().listener, ListenerView::Off);

    control
        .save(McpSettings {
            enabled: true,
            ..McpSettings::default()
        })
        .unwrap();
    assert_eq!(control.get().listener, ListenerView::NoKey, "no key, nobody could pair");

    let key = control.issue_key("Pitwall").unwrap().key;
    let p = port(&control);
    let replies = ask(
        p,
        &key,
        &[
            client::initialize(1, "test", "1"),
            client::initialized(),
            client::list_tools(2, None),
            client::call_tool(3, "system_summary", json!({})),
        ],
    );
    assert_eq!(replies[0]["client"], "Pitwall");
    let tools = client::parse_tools(replies.iter().find(|v| v["id"] == 2).unwrap()["result"].clone()).unwrap();
    assert_eq!(tools.tools.len(), 10, "the reads; actions stay hidden");
    let summary = client::parse_call(replies.iter().find(|v| v["id"] == 3).unwrap()["result"].clone()).unwrap();
    let summary: Value = serde_json::from_str(summary.text()).unwrap();
    assert_eq!(summary["cpu"]["percent"], 12.0);
    assert_eq!(ask(p, "pmk_wrong", &[])[0]["ok"], false, "a wrong key is refused");

    // The stdio bridge a host starts reaches the same app.
    let input = [
        client::initialize(1, "host", "1"),
        client::initialized(),
        client::call_tool(2, "system_summary", json!({})),
    ]
    .join("\n");
    let mut out = Vec::new();
    bridge::run(&discovery_path(dirs.config()), &key, Cursor::new(input.into_bytes()), &mut out).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("percent"));

    control
        .save(McpSettings {
            enabled: false,
            ..McpSettings::default()
        })
        .unwrap();
    assert_eq!(control.get().listener, ListenerView::Off);
    assert!(TcpStream::connect(("127.0.0.1", p)).is_err(), "switched off means no listener");
    assert!(!discovery_path(dirs.config()).exists());
}

#[test]
fn a_saved_on_setting_starts_the_listener_when_the_app_starts() {
    let dir = tempfile::tempdir().unwrap();
    let (first, _) = control(dir.path());
    first
        .save(McpSettings {
            enabled: true,
            ..McpSettings::default()
        })
        .unwrap();
    let key = first.issue_key("a").unwrap().key;
    drop(first);
    // The keys are in the owner-only file beside the settings, so a fresh control (the next start) listens at once and the key works.
    let (second, _) = control(dir.path());
    let p = port(&second);
    let replies = ask(p, &key, &[client::initialize(1, "test", "1")]);
    assert_eq!(replies[0]["client"], "a");
}
