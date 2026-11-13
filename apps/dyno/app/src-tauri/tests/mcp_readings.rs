//! Dyno's MCP tools through the real `paddock-mcp` server over its stdio transport (the loopback transport has its own tests in
//! `paddock-mcp`; it is the same lines after a key handshake), with a client's handshake (`initialize`,
//! `tools/list`, `tools/call`), and through the AI access settings the user sees. The agent is a script and the settings live in
//! a temp folder; nothing outside it is touched (ER-14).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use dyno_app_lib::mcp::{self, AgentCalls};
use dyno_core::grouping::AppGroup;
use dyno_core::process::Measured;
use dyno_core::protocol::{LiveUpdate, Overhead, Reply, Request};
use dyno_core::time::Timestamp;
use dyno_platform::fake::quiet_sample;
use paddock_jobs::FakeClock;
use paddock_mcp::{client, serve_stdio, AuditLog, McpSettings, Policy, Server};
use serde_json::{json, Value};
use std::io::Cursor;
use std::sync::Arc;

const READS: [&str; 10] = [
    "alarms",
    "cpu_memory",
    "disks",
    "history",
    "network_summary",
    "power_state",
    "sensors",
    "system_summary",
    "top_processes",
    "wol_devices",
];

/// An agent that has live readings and answers nothing else.
struct Script;

impl AgentCalls for Script {
    fn call(&self, request: Request) -> Result<Reply, String> {
        Err(format!("The script does not answer {request:?}."))
    }

    fn live(&self) -> Result<LiveUpdate, String> {
        let mut system = quiet_sample(Timestamp(1_700_000_000_000));
        system.cpu.total = 12.0;
        let group = AppGroup {
            id: "/Users/alice/bin/Editor".into(),
            name: "Editor".into(),
            exe: Some("/Users/alice/bin/Editor --open /Users/alice/secret.txt".into()),
            pids: vec![321, 322],
            cpu: Measured::Value(8.5),
            memory: Measured::Value(200 << 20),
            disk_bps: Measured::Value(0.0),
            partial: false,
        };
        Ok(LiveUpdate {
            system,
            pressure: dyno_core::metrics::MemoryPressure::Normal,
            top_cpu: vec![group.clone()],
            top_memory: vec![group],
            overhead: Overhead::default(),
            processes: None,
            groups: None,
            other_users: None,
            via: Default::default(),
        })
    }
}

fn server() -> (Arc<Server>, Arc<AuditLog>) {
    let audit = Arc::new(AuditLog::new(20));
    let server = Arc::new(Server::new(
        "Dyno",
        "1.1.0",
        Arc::new(FakeClock::at_wall_ms(1_700_000_000_000)),
        audit.clone(),
        Arc::new(paddock_log::Redactor::new()),
    ));
    for t in mcp::tools(Arc::new(Script)) {
        server.add_tool(t).unwrap();
    }
    server.set_policy(Policy::read_only());
    (server, audit)
}

fn text_json(result: &Value) -> Value {
    serde_json::from_str(client::parse_call(result.clone()).unwrap().text()).unwrap()
}

#[test]
fn a_client_over_stdio_lists_the_tools_and_reads_dyno() {
    let (server, audit) = server();
    let input = [
        client::initialize(1, "Pitwall", "1"),
        client::initialized(),
        client::list_tools(2, None),
        client::call_tool(3, "system_summary", json!({})),
        client::call_tool(4, "top_processes", json!({"limit": 5})),
        client::call_tool(5, "top_processes", json!({"limit": 500})),
        client::call_tool(6, "keep_awake", json!({"minutes": 5})),
        client::call_tool(7, "disks", json!({})),
    ]
    .join("\n");
    let mut out = Vec::new();
    serve_stdio(&server, &mut Cursor::new(input.into_bytes()), &mut out).unwrap();
    let replies: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let by_id = |id: i64| {
        replies
            .iter()
            .find(|v| v["id"] == id)
            .unwrap_or_else(|| panic!("no reply for {id}: {replies:?}"))
            .clone()
    };

    assert_eq!(
        client::parse_initialize(by_id(1)["result"].clone()).unwrap().server_info.name,
        "Dyno"
    );
    let mut names: Vec<String> = client::parse_tools(by_id(2)["result"].clone())
        .unwrap()
        .tools
        .into_iter()
        .map(|t| t.name)
        .collect();
    names.sort();
    assert_eq!(
        names, READS,
        "the reads only: no action is offered until the user allows it by name"
    );

    let summary = text_json(&by_id(3)["result"]);
    assert_eq!(summary["cpu"]["percent"], 12.0);
    let procs = text_json(&by_id(4)["result"]);
    assert_eq!(procs["processes"][0]["name"], "Editor");
    assert_eq!(procs["processes"][0]["pid"], 321);
    assert!(
        !procs.to_string().contains("alice"),
        "no path, user or command line reaches the client"
    );

    let too_many = client::parse_call(by_id(5)["result"].clone()).unwrap();
    assert!(too_many.is_error, "limit 500 is over the maximum");
    assert!(
        by_id(6)["error"].is_object() || by_id(6)["result"]["isError"] == json!(true),
        "an action is refused"
    );
    // The agent cannot answer the drive-health question: the volumes still come, and the health says why it is missing.
    let disks = text_json(&by_id(7)["result"]);
    assert_eq!(disks["health"]["available"], json!(false));
    assert!(disks["health"]["reason"].as_str().unwrap().contains("does not answer"));

    let entries = audit.entries();
    assert_eq!(
        entries.iter().filter(|e| e.client == "Pitwall").count(),
        entries.len(),
        "the handshake named the client"
    );
    assert!(entries
        .iter()
        .any(|e| e.tool == "top_processes" && e.arguments == json!({"limit": 5})));
}

/// Settings > AI access: every tool is listed with its description and its own switch, off by default; the choice is saved in the
/// app's folder and changes what a client can list.
#[test]
fn the_ai_access_settings_list_every_tool_with_its_own_switch() {
    let dir = paddock_testkit::private_tempdir().unwrap();
    let id = paddock_env::AppId::new(dyno_app_lib::APP_ID, dyno_app_lib::APP_NAME).unwrap();
    let dirs = paddock_env::AppDirs::resolve(
        &id,
        paddock_env::Os::current(),
        &paddock_env::MapVars::new().with(&id.home_var(), dir.path().to_str().unwrap()),
        None,
        true,
    )
    .unwrap();
    let control = mcp::control(&dirs, "Dyno", "1.1.0", Arc::new(|_| {}), Arc::new(Script));
    let view = control.get();
    assert!(!view.settings.enabled, "off by default");
    let mut names: Vec<&str> = view.tools.iter().filter(|t| !t.action).map(|t| t.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, READS);
    for t in &view.tools {
        assert!(t.description.len() > 20, "{} has a plain description", t.name);
        assert!(t.title.is_some(), "{} has a title", t.name);
        assert!(!t.exposed, "nothing is exposed while MCP is off");
    }
    let on = control
        .save(McpSettings {
            enabled: true,
            disabled_tools: vec!["top_processes".into()],
            ..McpSettings::default()
        })
        .unwrap();
    let exposed: Vec<&str> = on.tools.iter().filter(|t| t.exposed).map(|t| t.name.as_str()).collect();
    assert!(exposed.contains(&"sensors") && !exposed.contains(&"top_processes"), "{exposed:?}");
    assert!(
        !exposed.contains(&"keep_awake") && !exposed.contains(&"wake_device"),
        "an action needs its own permission"
    );
    assert!(dirs.config().join("mcp.json").is_file(), "saved in the app's folder");
}
