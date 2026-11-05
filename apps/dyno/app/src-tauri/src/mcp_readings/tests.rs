//! The reading tools through the real `paddock-mcp` server and a scripted agent: what the client sees is what the tests check.

use super::*;
use dyno_core::alarms::{Alarm, AlarmChange, AlarmState, ChangeKind};
use dyno_core::grouping::AppGroup;
use dyno_core::hardware::Confidence;
use dyno_core::health::Severity;
use dyno_core::history::{HistoryPoint, HistoryRange};
use dyno_core::inventory::{CpuInfo, MemoryKind, MemoryLayout, SystemInventory};
use dyno_core::lifespan::{LifeForecast, LifeOutcome, LifeSubject, LifespanOverview};
use dyno_core::metrics::{BatterySample, DiskSample, Fan, MemoryPressure, NetworkSample, PowerSource, Reading, Temperature};
use dyno_core::network::{ConnectionRow, Exposure, ListenerRow, NetworkSnapshot, Proto, SocketState};
use dyno_core::process::{Measured, UnavailableReason};
use dyno_core::protocol::{History, LiveUpdate, Overhead, Reply, Request};
use dyno_core::read_via::ReadVia;
use dyno_core::time::Timestamp;
use dyno_platform::fake::quiet_sample;
use paddock_jobs::FakeClock;
use paddock_mcp::{AuditLog, Outcome, Policy, Server, Session};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Mutex;

const READ_TOOLS: [&str; 8] = [
    "alarms",
    "cpu_memory",
    "disks",
    "history",
    "network_summary",
    "sensors",
    "system_summary",
    "top_processes",
];

/// An agent that answers from fixtures and remembers what it was asked.
struct FakeAgent {
    live: Option<LiveUpdate>,
    refuse: Option<String>,
    inventory: SystemInventory,
    lifespan: LifespanOverview,
    network: NetworkSnapshot,
    alarms: Vec<Alarm>,
    changes: Vec<AlarmChange>,
    history: History,
    asked: Mutex<Vec<Request>>,
}

impl FakeAgent {
    fn new() -> FakeAgent {
        FakeAgent {
            live: Some(live()),
            refuse: None,
            inventory: SystemInventory::new(
                "macOS 15.1".into(),
                Some("alices-macbook".into()),
                CpuInfo {
                    name: "Test CPU".into(),
                    physical_cores: 2,
                    logical_cores: 4,
                    frequency_mhz: 2400,
                },
                MemoryLayout::unified(8 << 30, MemoryKind::Lpddr5, None),
                vec![],
            ),
            lifespan: LifespanOverview {
                at: Timestamp(0),
                forecasts: vec![forecast("Test SSD", LifeSubject::Ssd, 900.0)],
            },
            network: NetworkSnapshot {
                listeners: vec![listener(80, Exposure::AllInterfaces), listener(5432, Exposure::Loopback)],
                connections: vec![
                    connection("203.0.113.9", SocketState::Established),
                    connection("198.51.100.7", SocketState::TimeWait),
                ],
                ..NetworkSnapshot::default()
            },
            alarms: vec![alarm(1, "lowDisk", "Disk C: is nearly full")],
            changes: vec![change(1, ChangeKind::Raised)],
            history: history(700),
            asked: Mutex::new(Vec::new()),
        }
    }
    fn asked(&self) -> Vec<Request> {
        self.asked.lock().unwrap().clone()
    }
}

impl AgentCalls for FakeAgent {
    fn call(&self, request: Request) -> Result<Reply, String> {
        self.asked.lock().unwrap().push(request.clone());
        if let Some(m) = &self.refuse {
            return Err(m.clone());
        }
        Ok(match request {
            Request::Inventory => Reply::Inventory(Box::new(self.inventory.clone())),
            Request::Lifespan => Reply::Lifespan(Box::new(self.lifespan.clone())),
            Request::Network { .. } => Reply::Network(Box::new(self.network.clone())),
            // The fake ignores the limit on purpose: the tool must bound what it shows even if the agent does not.
            Request::Alarms { .. } => Reply::Alarms(self.alarms.clone()),
            Request::AlarmHistory { .. } => Reply::AlarmChanges(self.changes.clone()),
            Request::History { .. } => Reply::History(Box::new(self.history.clone())),
            other => return Err(format!("the fake agent was asked something unexpected: {other:?}")),
        })
    }

    fn live(&self) -> Result<LiveUpdate, String> {
        if let Some(m) = &self.refuse {
            return Err(m.clone());
        }
        self.live
            .clone()
            .ok_or_else(|| "Dyno has not received readings from its agent yet; try again in a moment.".to_string())
    }
}

fn group(name: &str, pid: u32, cpu: f32, memory: u64) -> AppGroup {
    AppGroup {
        id: format!("/Users/alice/Secret Project/{name}"),
        name: name.into(),
        exe: Some(format!("/Users/alice/Secret Project/{name} --token=hunter2")),
        pids: vec![pid, pid + 1, pid + 2],
        cpu: Measured::Value(cpu),
        memory: Measured::Value(memory),
        disk_bps: Measured::Value(0.0),
        partial: false,
    }
}

fn live() -> LiveUpdate {
    let mut system = quiet_sample(Timestamp(1_700_000_000_000));
    system.uptime_secs = 93_784;
    system.cpu.total = 37.46;
    system.cpu.per_core = vec![80.0, 10.04, 30.0, 29.8];
    system.memory.swap_used = 512 << 20;
    system.disks = vec![DiskSample {
        id: "/".into(),
        name: "Macintosh HD".into(),
        mount: "/".into(),
        total: 500_000_000_000,
        free: 125_000_000_000,
        removable: false,
    }];
    system.network = vec![
        NetworkSample {
            name: "en0".into(),
            rx_bps: 1000.0,
            tx_bps: 250.0,
        },
        NetworkSample {
            name: "lo0".into(),
            rx_bps: 5.0,
            tx_bps: 5.0,
        },
    ];
    system.temperatures = Reading::Available(vec![Temperature {
        label: "CPU package".into(),
        celsius: 61.5,
        critical: Some(100.0),
    }]);
    system.fans = Reading::Available(vec![Fan {
        label: "Left fan".into(),
        rpm: 1800,
    }]);
    system.battery = Reading::Available(BatterySample {
        charge: 80.0,
        source: PowerSource::Battery,
        health: 91.0,
        cycles: Some(212),
        seconds_to_empty: Some(7200),
    });
    LiveUpdate {
        system,
        pressure: MemoryPressure::Normal,
        top_cpu: vec![group("Safari", 100, 22.5, 900 << 20), group("Mail", 200, 3.0, 300 << 20)],
        top_memory: vec![group("Safari", 100, 22.5, 900 << 20), group("Mail", 200, 3.0, 300 << 20)],
        overhead: Overhead::default(),
        processes: None,
        groups: None,
        other_users: None,
        via: Default::default(),
    }
}

fn forecast(name: &str, subject: LifeSubject, mid: f32) -> LifeForecast {
    LifeForecast {
        subject,
        name: name.into(),
        outcome: LifeOutcome::Remaining {
            low_days: mid / 2.0,
            mid_days: mid,
            high_days: mid * 2.0,
        },
        confidence: Confidence::Medium,
        headline: "About 2 to 5 years of writes left".into(),
        explanation: "At the pace of the last 30 days.".into(),
        basis: vec![],
        history_days: 30.0,
        readings: 40,
        restarted_at: None,
    }
}

fn listener(port: u16, exposure: Exposure) -> ListenerRow {
    ListenerRow {
        proto: Proto::Tcp,
        ipv6: false,
        local: "10.1.2.3".into(),
        port,
        exposure,
        owner: dyno_core::network::Owner::Process {
            pid: 1,
            name: "x".into(),
            path: None,
        },
        first_seen: None,
    }
}

fn connection(remote: &str, state: SocketState) -> ConnectionRow {
    ConnectionRow {
        proto: Proto::Tcp,
        ipv6: false,
        local: "10.1.2.3".into(),
        local_port: 50000,
        remote: remote.into(),
        remote_port: 443,
        remote_is_loopback: false,
        lookup_ok: true,
        state,
        state_label: state.label().into(),
        owner: dyno_core::network::Owner::Process {
            pid: 1,
            name: "x".into(),
            path: None,
        },
        first_seen: None,
        country: None,
        flagged_by: vec![],
    }
}

fn alarm(id: u64, rule: &str, title: &str) -> Alarm {
    Alarm {
        id,
        rule: rule.into(),
        key: format!("{rule}:/Users/alice"),
        severity: Severity::Warning,
        state: AlarmState::Active,
        title: title.into(),
        detail: "Detail with /Users/alice/Documents in it".into(),
        first_seen: Timestamp(1_699_999_000_000),
        last_seen: Timestamp(1_699_999_900_000),
        repeat_count: 3,
        current_value: Some(91.5),
        metric: None,
        notes: vec![],
        acknowledged_at: None,
        cleared_at: None,
        shelved: None,
        muted: false,
        chattering: false,
        expires_at: None,
    }
}

fn change(id: u64, kind: ChangeKind) -> AlarmChange {
    AlarmChange {
        kind,
        alarm: alarm(id, "lowDisk", "Disk C: is nearly full"),
        notify: true,
        at: Timestamp(1_699_999_950_000),
    }
}

fn history(n: usize) -> History {
    History {
        range: HistoryRange::Hour,
        from: Timestamp(1_699_996_400_000),
        to: Timestamp(1_700_000_000_000),
        resolution_ms: 5_000,
        points: (0..n)
            .map(|i| HistoryPoint {
                at: Timestamp(1_699_996_400_000 + i as i64 * 5_000),
                cpu: (i % 100) as f32,
                cpu_max: (i % 100) as f32 + 1.0,
                cpu_min: 0.0,
                memory: 50.0,
                memory_max: 55.0,
                memory_min: 45.0,
                disk_read_bps: 1000.0,
                disk_write_bps: 2000.0,
                net_rx_bps: 3000.0,
                net_tx_bps: 4000.0,
                gpu: None,
                temperature: Some(60.0),
            })
            .collect(),
        events: vec![],
        top_consumers: vec![],
        disk_trends: vec![],
        boots: vec![],
        crashes: vec![],
        core_points: vec![],
    }
}

struct Rig {
    server: Server,
    audit: Arc<AuditLog>,
    agent: Arc<FakeAgent>,
}

fn rig_with(agent: FakeAgent, policy: Policy) -> Rig {
    let agent = Arc::new(agent);
    let audit = Arc::new(AuditLog::new(50));
    let server = Server::new(
        "Dyno",
        "1.1.0",
        Arc::new(FakeClock::at_wall_ms(1_700_000_000_000)),
        audit.clone(),
        Arc::new(paddock_log::Redactor::new()),
    );
    for t in crate::mcp::tools(agent.clone()) {
        server.add_tool(t).unwrap();
    }
    server.set_policy(policy);
    Rig { server, audit, agent }
}

fn rig() -> Rig {
    rig_with(FakeAgent::new(), Policy::read_only())
}

fn rpc(s: &Server, session: &Session, v: Value) -> Value {
    serde_json::from_str(&s.handle(session, &v.to_string()).expect("an answer")).unwrap()
}

fn ready(s: &Server) -> Session {
    let session = Session::new("tester");
    rpc(
        s,
        &session,
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}),
    );
    assert!(s
        .handle(&session, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .is_none());
    session
}

fn call(r: &Rig, session: &Session, name: &str, args: Value) -> Value {
    rpc(
        &r.server,
        session,
        json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":name,"arguments":args}}),
    )
}

fn list(r: &Rig, session: &Session) -> Vec<Value> {
    rpc(
        &r.server,
        session,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
    )["result"]["tools"]
        .as_array()
        .unwrap()
        .clone()
}

fn is_error(v: &Value) -> bool {
    v["error"].is_object() || v["result"]["isError"] == json!(true)
}

/// The tool's answer as the client reads it: the structured data, which must equal what the text says.
fn answer(r: &Rig, name: &str, args: Value) -> Value {
    let session = ready(&r.server);
    let v = call(r, &session, name, args);
    assert!(!is_error(&v), "{name}: {v}");
    let text: Value = serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).expect("the text is JSON");
    assert_eq!(text, v["result"]["structuredContent"], "{name}: text and structured data agree");
    text
}

fn error_text(r: &Rig, name: &str, args: Value) -> String {
    let session = ready(&r.server);
    let v = call(r, &session, name, args);
    assert!(is_error(&v), "{name} should fail: {v}");
    v["result"]["content"][0]["text"]
        .as_str()
        .or(v["error"]["message"].as_str())
        .unwrap_or_default()
        .to_string()
}

#[test]
fn tools_list_shows_exactly_the_allowed_tools() {
    let r = rig();
    let session = ready(&r.server);
    let names: BTreeSet<String> = list(&r, &session).iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    let mut want: BTreeSet<String> = READ_TOOLS.iter().map(|s| s.to_string()).collect();
    want.extend(["power_state".to_string(), "wol_devices".to_string()]);
    assert_eq!(names, want, "the reads, and no action until the user allows it by name");

    // The user switched some off in Settings > AI access: only the others exist.
    let only: BTreeSet<String> = ["sensors".to_string(), "disks".to_string()].into();
    let narrowed = rig_with(
        FakeAgent::new(),
        Policy {
            tools: Some(only.clone()),
            ..Policy::read_only()
        },
    );
    let session = ready(&narrowed.server);
    let names: BTreeSet<String> = list(&narrowed, &session)
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, only);
    assert!(
        is_error(&call(&narrowed, &session, "top_processes", json!({}))),
        "a switched-off tool is refused, not just hidden"
    );
    assert!(narrowed.agent.asked().is_empty());

    // Off by default: nothing is served.
    let off = rig_with(FakeAgent::new(), Policy::default());
    let session = Session::new("tester");
    rpc(
        &off.server,
        &session,
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}),
    );
    let _ = off
        .server
        .handle(&session, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    assert!(rpc(
        &off.server,
        &session,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}})
    )["error"]
        .is_object());
}

#[test]
fn every_reading_tool_is_read_only_with_a_strict_schema_and_a_plain_description() {
    let r = rig();
    let session = ready(&r.server);
    let tools = list(&r, &session);
    for name in READ_TOOLS {
        let t = tools
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} is missing"));
        assert_eq!(t["inputSchema"]["type"], "object", "{name}");
        assert_eq!(t["inputSchema"]["additionalProperties"], json!(false), "{name}");
        let d = t["description"].as_str().unwrap();
        assert!(d.len() > 60 && d.len() < 900, "{name}: {d}");
        assert!(t["title"].as_str().is_some_and(|s| !s.is_empty()), "{name}");
        // Read-only tools: the host may call them freely.
        assert_ne!(t["annotations"]["readOnlyHint"], json!(false), "{name}");
    }
    // Units and limits are written in the descriptions a model reads.
    let d = |n: &str| {
        tools.iter().find(|t| t["name"] == n).unwrap()["description"]
            .as_str()
            .unwrap()
            .to_lowercase()
    };
    assert!(d("top_processes").contains("percent") && d("top_processes").contains("bytes") && d("top_processes").contains("50"));
    assert!(d("history").contains("percent") && d("history").contains("bytes per second"));
    assert!(d("sensors").contains("celsius"));
}

#[test]
fn system_summary_gives_os_uptime_cpu_memory_and_swap_without_the_host_name() {
    let r = rig();
    let v = answer(&r, "system_summary", json!({}));
    assert_eq!(v["os"], "macOS 15.1");
    assert_eq!(v["uptimeSeconds"], 93_784);
    assert_eq!(v["cpu"]["percent"], 37.5);
    assert_eq!(v["cpu"]["logicalCores"], 4);
    assert_eq!(v["memory"]["usedPercent"], 50.0);
    assert_eq!(v["memory"]["pressure"], "normal");
    assert_eq!(v["swap"]["usedBytes"], 512u64 << 20);
    assert_eq!(v["swap"]["totalBytes"], 2u64 << 30);
    assert!(!v.to_string().contains("alices-macbook"), "the host name stays out");
}

#[test]
fn system_summary_still_answers_when_only_the_inventory_fails() {
    // The OS name comes from the inventory; if the agent cannot give it the rest still answers and the field says why.
    let broken = {
        struct NoInventory(FakeAgent);
        impl AgentCalls for NoInventory {
            fn call(&self, request: Request) -> Result<Reply, String> {
                match request {
                    Request::Inventory => Err("The inventory is still being read.".into()),
                    other => self.0.call(other),
                }
            }
            fn live(&self) -> Result<LiveUpdate, String> {
                self.0.live()
            }
        }
        NoInventory(FakeAgent::new())
    };
    let server = Server::new(
        "Dyno",
        "1.1.0",
        Arc::new(FakeClock::at_wall_ms(0)),
        Arc::new(AuditLog::new(5)),
        Arc::new(paddock_log::Redactor::new()),
    );
    for t in crate::mcp::tools(Arc::new(broken)) {
        server.add_tool(t).unwrap();
    }
    server.set_policy(Policy::read_only());
    let session = ready(&server);
    let v = rpc(
        &server,
        &session,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"system_summary","arguments":{}}}),
    );
    assert!(!is_error(&v), "{v}");
    let out: Value = serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(out["os"]["available"], json!(false));
    assert!(out["os"]["reason"].as_str().unwrap().contains("still being read"));
    assert_eq!(out["cpu"]["percent"], 37.5);
}

#[test]
fn cpu_memory_gives_current_readings_and_per_core_only_when_asked() {
    let r = rig();
    let v = answer(&r, "cpu_memory", json!({}));
    assert_eq!(v["cpu"]["percent"], 37.5);
    assert_eq!(v["cpu"]["frequencyMhz"], 2400);
    assert!(v["cpu"].get("perCorePercent").is_none());
    assert_eq!(v["memory"]["totalBytes"], 8u64 << 30);
    assert_eq!(v["memory"]["usedBytes"], 4u64 << 30);
    assert_eq!(v["memory"]["availableBytes"], 4u64 << 30);
    assert_eq!(v["memory"]["usedPercent"], 50.0);
    assert_eq!(
        v["memory"]["commit"]["available"],
        json!(false),
        "a reading the OS does not give says so"
    );
    assert_eq!(v["memory"]["commit"]["reason"], "Fake");
    let v = answer(&r, "cpu_memory", json!({"per_core": true}));
    assert_eq!(v["cpu"]["perCorePercent"], json!([80.0, 10.0, 30.0, 29.8]));
}

#[test]
fn disks_lists_volumes_with_space_and_health_where_known() {
    let r = rig();
    let v = answer(&r, "disks", json!({}));
    let vol = &v["volumes"][0];
    assert_eq!(vol["name"], "Macintosh HD");
    assert_eq!(vol["mount"], "/");
    assert_eq!(vol["totalBytes"], 500_000_000_000u64);
    assert_eq!(vol["freeBytes"], 125_000_000_000u64);
    assert_eq!(vol["usedBytes"], 375_000_000_000u64);
    assert_eq!(vol["usedPercent"], 75.0);
    assert_eq!(v["health"][0]["name"], "Test SSD");
    assert_eq!(v["health"][0]["summary"], "About 2 to 5 years of writes left");
    assert_eq!(v["health"][0]["confidence"], "medium");
    assert!(r.agent.asked().contains(&Request::Lifespan));
}

#[test]
fn disk_health_that_needs_the_helper_says_so_instead_of_staying_silent() {
    let mut agent = FakeAgent::new();
    agent.lifespan.forecasts = vec![dyno_core::lifespan::unavailable(
        LifeSubject::Ssd,
        "Test SSD",
        "Turn on Administrator readings in Settings so the helper can read this.",
    )];
    let r = rig_with(agent, Policy::read_only());
    let v = answer(&r, "disks", json!({}));
    assert_eq!(v["health"][0]["available"], json!(false));
    assert!(v["health"][0]["reason"].as_str().unwrap().contains("Administrator readings"));
    // No forecasts at all: a reason, not an empty list; the administrator hint only when the helper state says so.
    let mut agent = FakeAgent::new();
    agent.lifespan.forecasts = vec![forecast("Battery", LifeSubject::Battery, 400.0)];
    let r = rig_with(agent, Policy::read_only());
    let v = answer(&r, "disks", json!({}));
    assert_eq!(v["health"]["available"], json!(false));
    assert!(!v["health"]["reason"].as_str().unwrap().contains("Administrator readings"), "{v}");
    let mut agent = FakeAgent::new();
    agent.lifespan.forecasts = vec![forecast("Battery", LifeSubject::Battery, 400.0)];
    agent.live.as_mut().unwrap().via.processes = ReadVia::needs_admin("Needs the helper");
    let v = answer(&rig_with(agent, Policy::read_only()), "disks", json!({}));
    assert!(v["health"]["reason"].as_str().unwrap().contains("Administrator readings"), "{v}");
}

#[test]
fn top_processes_ranks_apps_by_cpu_or_memory_with_name_pid_and_numbers_only() {
    let r = rig();
    let v = answer(&r, "top_processes", json!({}));
    assert_eq!(v["sortedBy"], "cpu");
    let rows = v["processes"].as_array().unwrap();
    assert_eq!(
        rows[0],
        json!({"name": "Safari", "pid": 100, "processCount": 3, "cpuPercent": 22.5, "memoryBytes": 900u64 << 20})
    );
    assert_eq!(rows[1]["name"], "Mail");
    let v = answer(&r, "top_processes", json!({"sort": "memory", "limit": 1}));
    assert_eq!(v["processes"].as_array().unwrap().len(), 1);
    assert_eq!(v["sortedBy"], "memory");
    assert_eq!(v["shown"], 1);
}

#[test]
fn process_output_has_no_command_line_user_name_or_path() {
    let r = rig();
    for args in [json!({}), json!({"sort": "memory", "limit": 50})] {
        let text = answer(&r, "top_processes", args).to_string();
        for secret in ["hunter2", "--token", "/Users", "alice", "Secret Project"] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
    }
    let row = &answer(&r, "top_processes", json!({}))["processes"][0];
    let keys: BTreeSet<&str> = row.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, BTreeSet::from(["cpuPercent", "memoryBytes", "name", "pid", "processCount"]));
}

#[test]
fn a_process_value_that_was_not_read_gets_a_reason_not_a_zero() {
    let mut agent = FakeAgent::new();
    let live = agent.live.as_mut().unwrap();
    live.top_cpu[0].cpu = Measured::unavailable(UnavailableReason::NeedsAdmin);
    live.top_cpu[0].memory = Measured::unavailable(UnavailableReason::NotSupported);
    let r = rig_with(agent, Policy::read_only());
    let row = &answer(&r, "top_processes", json!({}))["processes"];
    let safari = row.as_array().unwrap().iter().find(|p| p["name"] == "Safari").unwrap();
    assert_eq!(safari["cpuPercent"], Value::Null);
    assert!(safari["cpuUnavailable"].as_str().unwrap().contains("administrator"));
    assert!(safari["memoryUnavailable"].as_str().unwrap().contains("Not on this"));
}

#[test]
fn top_processes_uses_the_full_list_when_the_task_manager_has_it_and_says_how_many_it_could_rank() {
    let mut agent = FakeAgent::new();
    let live = agent.live.as_mut().unwrap();
    live.groups = Some(
        (0..5000u32)
            .map(|i| group(&format!("App{i}"), i * 10, (i % 97) as f32, u64::from(i) << 20))
            .collect(),
    );
    let r = rig_with(agent, Policy::read_only());
    let v = answer(&r, "top_processes", json!({"limit": 50}));
    assert_eq!(v["processes"].as_array().unwrap().len(), 50);
    assert_eq!(v["ranked"], 5000);
    // Without it, only the agent's short top lists exist, and the answer says so.
    let v = answer(&rig(), "top_processes", json!({"limit": 50}));
    assert_eq!(v["processes"].as_array().unwrap().len(), 2);
    assert!(v["note"].as_str().unwrap().contains("short list"));
}

#[test]
fn network_summary_has_throughput_and_counts_but_no_addresses() {
    let r = rig();
    let v = answer(&r, "network_summary", json!({}));
    assert_eq!(
        v["interfaces"][0],
        json!({"name": "en0", "receiveBytesPerSecond": 1000.0, "sendBytesPerSecond": 250.0})
    );
    assert_eq!(v["totals"]["receiveBytesPerSecond"], 1005.0);
    assert_eq!(v["connections"]["listening"], 2);
    assert_eq!(v["connections"]["listeningOnAllInterfaces"], 1);
    assert_eq!(v["connections"]["total"], 2);
    assert_eq!(v["connections"]["established"], 1);
    let text = v.to_string();
    for addr in ["203.0.113.9", "198.51.100.7", "10.1.2.3"] {
        assert!(!text.contains(addr), "{addr} leaked");
    }
}

#[test]
fn alarms_lists_active_alarms_and_recent_history_without_details_or_paths() {
    let r = rig();
    let v = answer(&r, "alarms", json!({}));
    assert_eq!(v["activeCount"], 1);
    let a = &v["active"][0];
    assert_eq!(a["rule"], "lowDisk");
    assert_eq!(a["severity"], "warning");
    assert_eq!(a["state"], "active");
    assert_eq!(a["title"], "Disk C: is nearly full");
    assert_eq!(a["repeatCount"], 3);
    assert_eq!(a["currentValue"], 91.5);
    assert_eq!(v["recentChanges"][0]["change"], "raised");
    assert!(!v.to_string().contains("alice"), "alarm detail and keys carry paths and stay out");
    // No alarms is a real answer, not an error.
    let mut agent = FakeAgent::new();
    agent.alarms.clear();
    agent.changes.clear();
    let v = answer(&rig_with(agent, Policy::read_only()), "alarms", json!({}));
    assert_eq!(v["activeCount"], 0);
    assert_eq!(v["active"], json!([]));
}

#[test]
fn alarms_asks_the_agent_for_open_alarms_with_the_limits_given() {
    let r = rig();
    // The agent is asked for every open alarm (up to its own cap) so the count is true; the rows shown are the first five.
    let v = answer(&r, "alarms", json!({"active_limit": 5, "history_limit": 7}));
    assert_eq!(v["active"].as_array().unwrap().len(), 1);
    let asked = r.agent.asked();
    assert!(
        asked
            .iter()
            .any(|q| matches!(q, Request::Alarms { filter } if filter.limit == 500 && filter.states.is_empty())),
        "{asked:?}"
    );
    assert!(
        asked
            .iter()
            .any(|q| matches!(q, Request::AlarmHistory { alarm_id: None, limit: 7 })),
        "{asked:?}"
    );
    let r = rig();
    let v = answer(&r, "alarms", json!({"history_limit": 0}));
    assert!(
        !r.agent.asked().iter().any(|q| matches!(q, Request::AlarmHistory { .. })),
        "history_limit 0 asks nothing"
    );
    assert_eq!(v["recentChanges"], json!([]));
}

#[test]
fn history_returns_one_metric_over_a_range_with_a_bounded_number_of_points() {
    let r = rig();
    let v = answer(&r, "history", json!({"metric": "cpu", "range": "hour", "points": 10}));
    assert_eq!(v["metric"], "cpu");
    assert_eq!(v["unit"], "percent");
    assert_eq!(v["range"], "hour");
    assert_eq!(v["resolutionMs"].as_i64().unwrap() % 5_000, 0);
    let pts = v["points"].as_array().unwrap();
    assert!(pts.len() <= 10 && pts.len() >= 9, "{}", pts.len());
    assert!(pts[0]["atMs"].as_i64().unwrap() >= 1_699_996_400_000);
    assert!(pts[0]["value"].is_number() && pts[0]["max"].is_number());
    assert!(r.agent.asked().contains(&Request::History {
        range: HistoryRange::Hour,
        end: None
    }));
    // Defaults: the last hour of memory in at most 60 points.
    let v = answer(&r, "history", json!({"metric": "memory"}));
    assert_eq!(v["range"], "hour");
    assert!(v["points"].as_array().unwrap().len() <= 60);
    let v = answer(&r, "history", json!({"metric": "network_receive", "range": "day"}));
    assert_eq!(v["unit"], "bytes per second");
    assert_eq!(v["points"][0]["value"], 3000.0);
}

#[test]
fn history_of_a_metric_nobody_measured_or_an_empty_range_says_why() {
    let r = rig();
    let t = error_text(&r, "history", json!({"metric": "gpu"}));
    assert!(t.contains("GPU") && t.contains("readings"), "{t}");
    let mut agent = FakeAgent::new();
    agent.history = history(0);
    let t = error_text(&rig_with(agent, Policy::read_only()), "history", json!({"metric": "cpu"}));
    assert!(t.contains("no history"), "{t}");
}

#[test]
fn sensors_lists_temperatures_fans_and_battery() {
    let r = rig();
    let v = answer(&r, "sensors", json!({}));
    assert_eq!(
        v["temperatures"]["readings"][0],
        json!({"label": "CPU package", "celsius": 61.5, "criticalCelsius": 100.0})
    );
    assert_eq!(v["fans"]["readings"][0], json!({"label": "Left fan", "rpm": 1800}));
    assert_eq!(v["battery"]["chargePercent"], 80.0);
    assert_eq!(v["battery"]["pluggedIn"], json!(false));
    assert_eq!(v["battery"]["healthPercent"], 91.0);
}

#[test]
fn a_sensor_that_needs_the_helper_or_is_missing_gets_a_reason() {
    let mut agent = FakeAgent::new();
    let live = agent.live.as_mut().unwrap();
    live.system.temperatures = Reading::unavailable("Needs a vendor driver");
    live.via.temperatures = ReadVia::needs_admin("Temperatures need the helper");
    live.system.battery = Reading::unavailable("No battery");
    let r = rig_with(agent, Policy::read_only());
    let v = answer(&r, "sensors", json!({}));
    assert_eq!(v["temperatures"]["available"], json!(false));
    let why = v["temperatures"]["reason"].as_str().unwrap();
    assert!(
        why.contains("Needs a vendor driver") && why.contains("Temperatures need the helper"),
        "{why}"
    );
    assert_eq!(v["battery"]["reason"], "No battery");
    // Nothing at all to read: an error with the reasons, never an empty success.
    let mut agent = FakeAgent::new();
    let live = agent.live.as_mut().unwrap();
    live.system.temperatures = Reading::unavailable("Not on this OS");
    live.system.fans = Reading::unavailable("Not on this OS");
    live.system.battery = Reading::unavailable("No battery");
    live.system.gpu = Reading::unavailable("Not on this OS");
    let t = error_text(&rig_with(agent, Policy::read_only()), "sensors", json!({}));
    assert!(t.contains("Not on this OS") && t.contains("No battery"), "{t}");
}

#[test]
fn bad_arguments_are_refused_before_anything_reaches_the_agent() {
    let r = rig();
    let session = ready(&r.server);
    for (tool, args) in [
        ("top_processes", json!({"limit": 0})),
        ("top_processes", json!({"limit": 51})),
        ("top_processes", json!({"limit": "10"})),
        ("top_processes", json!({"limit": 2.5})),
        ("top_processes", json!({"sort": "disk"})),
        ("top_processes", json!({"user": "alice"})),
        ("history", json!({})),
        ("history", json!({"metric": "fans"})),
        ("history", json!({"metric": "cpu", "range": "year"})),
        ("history", json!({"metric": "cpu", "points": 1})),
        ("history", json!({"metric": "cpu", "points": 121})),
        ("history", json!({"metric": "cpu", "end": 5})),
        ("alarms", json!({"active_limit": 0})),
        ("alarms", json!({"active_limit": 51})),
        ("alarms", json!({"history_limit": -1})),
        ("alarms", json!({"history_limit": 51})),
        ("cpu_memory", json!({"per_core": "yes"})),
        ("system_summary", json!({"verbose": true})),
        ("disks", json!({"path": "/"})),
        ("network_summary", json!({"address": "10.0.0.1"})),
        ("sensors", json!({"x": 1})),
    ] {
        let v = call(&r, &session, tool, args.clone());
        assert!(is_error(&v), "{tool} {args} should be refused: {v}");
    }
    assert!(
        r.agent.asked().is_empty(),
        "bad arguments never reach the agent: {:?}",
        r.agent.asked()
    );
}

#[test]
fn when_the_agent_is_down_or_silent_every_tool_says_so() {
    for (name, args) in [
        ("system_summary", json!({})),
        ("cpu_memory", json!({})),
        ("disks", json!({})),
        ("top_processes", json!({})),
        ("network_summary", json!({})),
        ("alarms", json!({})),
        ("history", json!({"metric": "cpu"})),
        ("sensors", json!({})),
    ] {
        let mut agent = FakeAgent::new();
        agent.refuse = Some("Dyno is not connected to its agent; try again in a moment.".into());
        let r = rig_with(agent, Policy::read_only());
        let t = error_text(&r, name, args.clone());
        assert!(t.contains("not connected to its agent"), "{name}: {t}");

        let mut agent = FakeAgent::new();
        agent.live = None;
        let r = rig_with(agent, Policy::read_only());
        let session = ready(&r.server);
        let v = call(&r, &session, name, args);
        // Tools that only need the agent's requests (history, alarms) still answer; the live ones say nothing has arrived yet.
        if !matches!(name, "history" | "alarms") {
            assert!(is_error(&v), "{name}: {v}");
            assert!(
                v["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("has not received readings"),
                "{name}: {v}"
            );
        }
    }
}

#[test]
fn output_is_bounded_in_rows_and_bytes_even_when_the_agent_sends_too_much() {
    let mut agent = FakeAgent::new();
    agent.alarms = (0..300).map(|i| alarm(i, "lowDisk", &"t".repeat(1000))).collect();
    agent.changes = (0..300)
        .map(|i| {
            let mut c = change(i, ChangeKind::Repeated);
            c.alarm.title = "t".repeat(1000);
            c
        })
        .collect();
    agent.network.connections = (0..5000).map(|_| connection("203.0.113.9", SocketState::Established)).collect();
    agent.live.as_mut().unwrap().system.disks = (0..500)
        .map(|i| DiskSample {
            id: i.to_string(),
            name: "d".repeat(500),
            mount: format!("/m{i}"),
            total: 10,
            free: 5,
            removable: false,
        })
        .collect();
    agent.live.as_mut().unwrap().system.temperatures = Reading::Available(
        (0..1000)
            .map(|i| Temperature {
                label: format!("sensor {i} {}", "x".repeat(300)),
                celsius: 40.0,
                critical: None,
            })
            .collect(),
    );
    agent.live.as_mut().unwrap().system.cpu.per_core = vec![1.0; 100_000];
    agent.history = history(50_000);
    let r = rig_with(agent, Policy::read_only());
    let session = ready(&r.server);
    for (name, args) in [
        ("alarms", json!({"active_limit": 50, "history_limit": 50})),
        ("disks", json!({})),
        ("network_summary", json!({})),
        ("sensors", json!({})),
        ("cpu_memory", json!({"per_core": true})),
        ("history", json!({"metric": "cpu", "range": "month", "points": 120})),
        ("top_processes", json!({"limit": 50})),
    ] {
        let v = call(&r, &session, name, args);
        assert!(!is_error(&v), "{name}: {v}");
        let text = v["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.len() <= 32 * 1024, "{name}: {} bytes", text.len());
        assert!(
            v["result"]["_meta"]["paddock/truncated"].is_null(),
            "{name} should fit without being cut: {} bytes",
            text.len()
        );
    }
    let v = answer(&r, "alarms", json!({"active_limit": 50, "history_limit": 50}));
    assert_eq!(v["active"].as_array().unwrap().len(), 50);
    assert_eq!(v["activeCount"], 300, "the count says how many exist");
    assert_eq!(v["recentChanges"].as_array().unwrap().len(), 50);
    assert!(v["active"][0]["title"].as_str().unwrap().chars().count() <= 120);
    assert!(
        answer(&r, "history", json!({"metric": "cpu", "points": 120}))["points"]
            .as_array()
            .unwrap()
            .len()
            <= 120
    );
}

#[test]
fn the_tools_only_read() {
    let r = rig();
    let session = ready(&r.server);
    for (name, args) in [
        ("system_summary", json!({})),
        ("cpu_memory", json!({"per_core": true})),
        ("disks", json!({})),
        ("top_processes", json!({})),
        ("network_summary", json!({})),
        ("alarms", json!({})),
        ("history", json!({"metric": "memory"})),
        ("sensors", json!({})),
    ] {
        assert!(!is_error(&call(&r, &session, name, args)), "{name}");
    }
    for q in r.agent.asked() {
        assert!(
            matches!(
                q,
                Request::Inventory
                    | Request::Lifespan
                    | Request::Network { .. }
                    | Request::Alarms { .. }
                    | Request::AlarmHistory { .. }
                    | Request::History { .. }
            ),
            "a reading tool asked {q:?}"
        );
    }
}

#[test]
fn every_call_is_audited_with_tool_arguments_size_and_outcome_but_never_the_output() {
    let r = rig();
    let session = ready(&r.server);
    call(&r, &session, "top_processes", json!({"limit": 3}));
    call(&r, &session, "history", json!({"metric": "cpu", "points": 500}));
    let entries = r.audit.entries();
    let ok = entries.iter().find(|e| e.tool == "top_processes").expect("audited");
    assert_eq!(ok.outcome, Outcome::Ok);
    assert_eq!(ok.client, "tester");
    assert_eq!(ok.arguments, json!({"limit": 3}));
    assert!(ok.result_bytes > 0);
    let bad = entries.iter().find(|e| e.tool == "history").expect("a refused call is audited too");
    assert_ne!(bad.outcome, Outcome::Ok);
    let dump = serde_json::to_string(&entries).unwrap();
    assert!(
        !dump.contains("Safari") && !dump.contains("Macintosh"),
        "the audit never holds output: {dump}"
    );
}

#[test]
fn disks_never_carry_a_user_name_from_a_mount_point_or_a_label() {
    let mut agent = FakeAgent::new();
    agent.live.as_mut().unwrap().system.disks = vec![
        DiskSample {
            id: "a".into(),
            name: "/media/alice/KINGSTON".into(),
            mount: "/media/alice/KINGSTON".into(),
            total: 10,
            free: 5,
            removable: true,
        },
        DiskSample {
            id: "b".into(),
            name: "Home".into(),
            mount: "/home/alice/data".into(),
            total: 10,
            free: 5,
            removable: false,
        },
        DiskSample {
            id: "c".into(),
            name: "System".into(),
            mount: "C:\\".into(),
            total: 10,
            free: 5,
            removable: false,
        },
        DiskSample {
            id: "d".into(),
            name: "C:\\Users\\alice\\vol".into(),
            mount: "/".into(),
            total: 10,
            free: 5,
            removable: false,
        },
    ];
    let v = answer(&rig_with(agent, Policy::read_only()), "disks", json!({}));
    assert!(!v.to_string().to_lowercase().contains("alice"), "{v}");
    assert!(v["volumes"][0].get("mount").is_none() && v["volumes"][1].get("mount").is_none());
    assert_eq!(v["volumes"][2]["mount"], "C:\\");
    assert_eq!(v["volumes"][0]["name"], "/media/<user>/KINGSTON");
}

#[test]
fn no_volumes_or_sensor_readings_say_why_instead_of_an_empty_list() {
    let mut agent = FakeAgent::new();
    agent.live.as_mut().unwrap().system.disks = vec![];
    let v = answer(&rig_with(agent, Policy::read_only()), "disks", json!({}));
    assert_eq!(v["volumes"]["available"], json!(false));
    let mut agent = FakeAgent::new();
    let live = agent.live.as_mut().unwrap();
    live.system.temperatures = Reading::Available(vec![]);
    live.system.fans = Reading::Available(vec![]);
    let v = answer(&rig_with(agent, Policy::read_only()), "sensors", json!({}));
    assert_eq!(v["temperatures"]["available"], json!(false));
    assert_eq!(v["fans"]["available"], json!(false));
}

#[test]
fn alarm_titles_carry_no_port_number() {
    let mut agent = FakeAgent::new();
    agent.alarms = vec![alarm(1, "network.listener", "Editor started listening on port 8080")];
    let v = answer(&rig_with(agent, Policy::read_only()), "alarms", json!({}));
    assert_eq!(v["active"][0]["title"], "Editor started listening");
}

#[test]
fn an_answer_over_the_byte_bound_is_cut_to_valid_json_that_says_so() {
    let rows = |n: usize| {
        (0..n)
            .map(|i| json!({"label": format!("row {i} {}", "x".repeat(200)), "n": i}))
            .collect::<Vec<_>>()
    };
    let mut v = json!({"a": {"readings": rows(200)}, "b": rows(80), "takenAtMs": 5});
    super::fit(&mut v);
    let text = v.to_string();
    assert!(text.len() < super::MAX_BYTES, "{}", text.len());
    assert!(serde_json::from_str::<Value>(&text).is_ok());
    assert!(v["truncated"].is_string(), "the answer says it was cut");
    assert_eq!(v["takenAtMs"], 5);
    assert!(!v["a"]["readings"].as_array().unwrap().is_empty() && !v["b"].as_array().unwrap().is_empty());
    let mut small = json!({"a": [1, 2, 3]});
    super::fit(&mut small);
    assert!(small.get("truncated").is_none(), "a small answer is untouched");
}
