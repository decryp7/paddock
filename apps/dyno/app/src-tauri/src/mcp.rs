//! Dyno's tools for the family's MCP server (ADR 0032 decision D7, ADR 0007): eight read-only reading tools
//! ([`crate::mcp_readings`]), two read-only power tools (`power_state`, `wol_devices`)
//! and exactly two actions (`wake_device`, `keep_awake`, at most 12 hours). The actions are hidden and refused until the user
//! allows each by name in the AI access settings ([`paddock_mcp::Policy::allow_action`]); sleep, shut down, timed actions and
//! scheduled wakes are never tools. A tool asks the agent through [`AgentCalls`], the same requests the Power page sends, so the
//! agent's own validation, settings (`power.keepAwake.enabled`, `power.wol.enabled`) and logs apply to every call.

use dyno_core::power::awake::{AwakeScope, AwakeSpec, AwakeUntil, AwakeView};
use dyno_core::power::PowerView;
use dyno_core::protocol::{LiveUpdate, Reply, Request};
use dyno_core::wol::WolView;
use paddock_env::AppDirs;
use paddock_jobs::time::SystemClock;
use paddock_mcp::listener::Listener;
use paddock_mcp::{AuditLog, Emit, FileKeys, FileSettings, KeyStore, McpControl, Server, ToolDef, ToolError, ToolOutput};
use serde_json::{json, Value};
use std::sync::Arc;

/// How long a tool waits for the agent: under the tools' 10 s call limit ([`crate::mcp_readings`]), so the server never has to
/// abandon a call that is still blocked on the link.
pub const AGENT_CALL_LIMIT: std::time::Duration = std::time::Duration::from_secs(8);

/// Most MCP clients connected at once.
const MAX_CLIENTS: usize = 4;

/// The longest `keep_awake` an AI client may ask for, in minutes (12 hours).
pub const MAX_KEEP_AWAKE_MINUTES: u64 = 12 * 60;

/// The app's wiring: the agent link the shell holds (looked up at each call, so a tool works after the agent reconnects).
pub struct ShellCalls<R: tauri::Runtime>(pub tauri::AppHandle<R>);

impl<R: tauri::Runtime> AgentCalls for ShellCalls<R> {
    fn call(&self, request: Request) -> Result<Reply, String> {
        use tauri::Manager;
        match self.0.try_state::<Arc<crate::shell::ShellOf<R>>>() {
            Some(shell) => shell.call_agent(request),
            None => Err("Dyno is still starting; try again in a moment.".into()),
        }
    }

    fn live(&self) -> Result<LiveUpdate, String> {
        use tauri::Manager;
        match self.0.try_state::<Arc<crate::shell::ShellOf<R>>>() {
            Some(shell) => shell.newest_live(),
            None => Err("Dyno is still starting; try again in a moment.".into()),
        }
    }
}

/// How a tool reaches the agent. The app's wiring is its agent link; a test is a script.
pub trait AgentCalls: Send + Sync {
    /// Sends one request; the error is the agent's message, for the client to read.
    fn call(&self, request: Request) -> Result<Reply, String>;

    /// The newest live update the agent pushed (what the dashboard and the tray draw); the error says why there is none yet.
    fn live(&self) -> Result<LiveUpdate, String>;
}

fn power(calls: &dyn AgentCalls) -> Result<PowerView, ToolError> {
    match calls.call(Request::PowerState).map_err(ToolError)? {
        Reply::Power(v) => Ok(*v),
        _ => Err(ToolError("The agent answered something unexpected.".into())),
    }
}

fn wol(calls: &dyn AgentCalls) -> Result<WolView, ToolError> {
    match calls.call(Request::WolDevices).map_err(ToolError)? {
        Reply::Wol(v) => Ok(*v),
        _ => Err(ToolError("The agent answered something unexpected.".into())),
    }
}

/// The keep-awake state in a sentence.
pub fn describe_awake(a: &AwakeView) -> String {
    match a {
        AwakeView::Off => "Dyno is not keeping the computer awake.".into(),
        AwakeView::Holding { ends: Some(_), .. } => "Dyno is keeping the computer awake until a set time.".into(),
        AwakeView::Holding { ends: None, .. } => "Dyno is keeping the computer awake until it is stopped.".into(),
        AwakeView::Waiting { why, .. } => format!("Keep awake is paused: {why}."),
        AwakeView::Ended { why, .. } => format!("Keep awake ended: {why}."),
    }
}

/// The saved computers, one per line; names are what the user chose.
pub fn describe_devices(v: &WolView) -> String {
    if !v.enabled {
        return "Wake-on-LAN is turned off in Dyno's settings.".into();
    }
    if v.devices.is_empty() {
        return "No computers are saved for Wake-on-LAN.".into();
    }
    v.devices
        .iter()
        .map(|d| match &d.last_wake {
            Some(w) => format!("{} (last wake: {})", d.name, w.summary),
            None => format!("{} (not woken yet)", d.name),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The device a client named: by its exact name (without case), or by the id the list shows when the name is a number.
fn find_device(v: &WolView, wanted: &str) -> Result<u64, ToolError> {
    let wanted = wanted.trim();
    let by_name: Vec<_> = v.devices.iter().filter(|d| d.name.eq_ignore_ascii_case(wanted)).collect();
    match by_name.as_slice() {
        [one] => Ok(one.id),
        [] => Err(ToolError(format!(
            "No saved computer is called \"{}\". Call wol_devices to see the names.",
            wanted.chars().take(64).collect::<String>()
        ))),
        _ => Err(ToolError(
            "More than one saved computer has that name; ask the user to rename one.".into(),
        )),
    }
}

/// Makes or checks the folder that holds the AI access files (`mcp-keys.json`, `mcp.json`, the listener's discovery file) through the
/// shared private-folder type (ADR 0092, W7): owner-only from creation, and an existing folder others can change, another account's
/// folder or a folder under a parent others can change is refused as it is (never tightened). The config folder is not the data
/// folder: portable mode (data beside the executable) puts it outside the user's profile, and `DYNO_DATA_DIR` does not move it.
///
/// # Errors
/// The refusal; its text names the folder and the next step, its `reason()` names no path (log that).
pub fn prepare_config_folder(config: &std::path::Path) -> Result<(), paddock_env::private::PrivateError> {
    dyno_agent::paths::open_private(config, "the AI access folder").map(|_| ())
}

/// The key list and settings stores of a config folder that was refused: nothing is read from or written to the folder. The list
/// is empty with the refusal as its problem (no key works, and the AI access page shows why), the settings are the defaults (off),
/// and a save fails with the same text.
struct RefusedFolder(String);

impl paddock_mcp::KeyListStore for RefusedFolder {
    fn load(&self) -> paddock_mcp::KeysLoad {
        paddock_mcp::KeysLoad {
            entries: vec![],
            problem: Some(self.0.clone()),
        }
    }
    fn save(&self, _: &[paddock_mcp::KeyEntry]) -> Result<(), String> {
        Err(self.0.clone())
    }
}

impl paddock_mcp::SettingsStore for RefusedFolder {
    fn load(&self) -> paddock_mcp::McpSettings {
        paddock_mcp::McpSettings::default()
    }
    fn save(&self, _: &paddock_mcp::McpSettings) -> Result<(), String> {
        Err(self.0.clone())
    }
}

/// What AI access works over: the stores, and whether the loopback listener may run.
struct Access {
    settings: Arc<dyn paddock_mcp::SettingsStore>,
    keys: Arc<dyn paddock_mcp::KeyListStore>,
    listener_allowed: bool,
}

/// Fails closed (ADR 0092, W7): a config folder that is refused means AI access starts off, no key list or settings file is read,
/// and no listener is built (it would write its discovery file into that folder). The refusal text becomes `keys_problem`. Logs
/// the reason only, never the path (ER-27); the text with the path and the next step goes to the user.
fn access_over(config: &std::path::Path) -> Access {
    match prepare_config_folder(config) {
        Ok(()) => Access {
            settings: Arc::new(FileSettings::new(config.join("mcp.json"))),
            keys: Arc::new(FileKeys::new(config.join("mcp-keys.json"))),
            listener_allowed: true,
        },
        Err(e) => {
            log::error!(Why = e.reason(); "AI access is off: its folder is not safe, so no key is read and nothing listens; next step: make the folder yours and closed to others, or move it");
            let refused = Arc::new(RefusedFolder(format!("AI access is off: {e}")));
            Access {
                settings: refused.clone(),
                keys: refused,
                listener_allowed: false,
            }
        }
    }
}

/// Builds the AI access control the shell manages (`Builder::mcp`): the server with Dyno's tools (off until the user switches it on in
/// Settings > AI access, every action off until allowed by name), the settings in the config folder, the client keys in the
/// owner-only `mcp-keys.json` beside them (a hash per key, never the key, and never the OS credential store: opening a Keychain item of another build blocks the app before its window) and the audit log.
pub fn control(dirs: &AppDirs, app_name: &str, version: &str, emit: Emit, calls: Arc<dyn AgentCalls>) -> Arc<McpControl> {
    let log = Arc::new(AuditLog::new(500));
    let sink = McpControl::announcing_sink(log.clone(), emit.clone());
    let server = Arc::new(
        Server::new(app_name, version, Arc::new(SystemClock), sink, Arc::new(paddock_log::Redactor::new())).with_instructions(
            "Ask about this computer's current readings (system summary, CPU and memory, disks, top programs, network, alarms, history, sensors), its power state and its Wake-on-LAN computers. Readings say why when one is not available. Waking a computer and keeping this one awake work only if the user allowed them.",
        ),
    );
    for tool in tools(calls) {
        // The definitions are fixed above; a refusal would be a programming error, shown in the log rather than panicking.
        if let Err(e) = server.add_tool(tool) {
            log::error!("a Dyno MCP tool was refused: {e}");
        }
    }
    let access = access_over(dirs.config());
    let keys = Arc::new(KeyStore::new());
    let listener = Arc::new(Listener::new(
        server.clone(),
        keys.clone(),
        paddock_mcp::listener::discovery_path(dirs.config()),
        MAX_CLIENTS,
    ));
    let control = Arc::new(McpControl::new(
        server,
        log,
        keys,
        access.settings,
        access.keys,
        emit,
        Arc::new(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        }),
    ));
    // Serves on 127.0.0.1 while AI access is on and a key exists (ADR 0007, ADR 0032 amendment 2026-11-05); this process holds the
    // tools (they ask the agent through the shell), so a client works while the window is closed and the app stays in the tray.
    if access.listener_allowed {
        control.attach_listener(listener);
    }
    control
}

/// Every tool Dyno offers over `calls`: the power and Wake-on-LAN tools here and the reading tools of [`crate::mcp_readings`].
pub fn tools(calls: Arc<dyn AgentCalls>) -> Vec<ToolDef> {
    let mut all = power_tools(calls.clone());
    all.extend(crate::mcp_readings::tools(calls));
    all
}

/// The power state, Wake-on-LAN list and the two actions.
fn power_tools(calls: Arc<dyn AgentCalls>) -> Vec<ToolDef> {
    let (c1, c2, c3, c4) = (calls.clone(), calls.clone(), calls.clone(), calls);
    vec![
        ToolDef::read_only("power_state", "Whether Dyno is keeping the computer awake, and why a hold ended.", json!({"type": "object", "additionalProperties": false}), move |_, _| {
            Ok(ToolOutput::text(describe_awake(&power(&*c1)?.awake)))
        })
        .title("Power state"),
        ToolDef::read_only("wol_devices", "The computers saved for Wake-on-LAN and how their last wake ended.", json!({"type": "object", "additionalProperties": false}), move |_, _| {
            Ok(ToolOutput::text(describe_devices(&wol(&*c2)?)))
        })
        .title("Wake-on-LAN computers"),
        ToolDef::action(
            "wake_device",
            "Sends a Wake-on-LAN wake to one saved computer. The computer may wake; nothing else changes.",
            json!({"type": "object", "required": ["device"], "properties": {"device": {"type": "string", "minLength": 1, "maxLength": 64, "description": "The computer's name, as wol_devices lists it."}}, "additionalProperties": false}),
            move |_, args: &Value| {
                let id = find_device(&wol(&*c3)?, args["device"].as_str().unwrap_or(""))?;
                c3.call(Request::WolWake { id, test: false }).map_err(ToolError)?;
                Ok(ToolOutput::text("The wake was started. Use wol_devices later to see how it ended."))
            },
        )
        .title("Wake a computer"),
        ToolDef::action(
            "keep_awake",
            "Keeps this computer awake for a number of minutes (at most 720). It replaces a hold Dyno already has.",
            json!({"type": "object", "required": ["minutes"], "properties": {
                "minutes": {"type": "integer", "minimum": 1, "maximum": MAX_KEEP_AWAKE_MINUTES},
                "display": {"type": "boolean", "description": "Keep the display on too."}
            }, "additionalProperties": false}),
            move |_, args: &Value| {
                let minutes = args["minutes"].as_u64().filter(|m| (1..=MAX_KEEP_AWAKE_MINUTES).contains(m)).ok_or_else(|| ToolError("minutes is 1 to 720.".into()))?;
                let scope = if args["display"].as_bool().unwrap_or(false) { AwakeScope::SystemAndDisplay } else { AwakeScope::System };
                let spec = AwakeSpec { scope, until: AwakeUntil::For { minutes: u32::try_from(minutes).unwrap_or(1) } };
                match c4.call(Request::PowerKeepAwake { spec }).map_err(ToolError)? {
                    Reply::Power(v) => Ok(ToolOutput::text(describe_awake(&v.awake))),
                    _ => Err(ToolError("The agent answered something unexpected.".into())),
                }
            },
        )
        .title("Keep the computer awake"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::power::{Capability, PowerCaps};
    use dyno_core::time::Timestamp;
    use dyno_core::wol::{LastWake, Mac, WolDevice};
    use paddock_jobs::FakeClock;
    use paddock_mcp::{AuditLog, Policy, Server, Session};
    use std::sync::Mutex;

    /// Answers like the agent and remembers what it was asked.
    struct Script {
        asked: Mutex<Vec<Request>>,
        devices: Vec<WolDevice>,
        refuse: Option<String>,
    }

    impl Script {
        fn new(devices: Vec<WolDevice>) -> Arc<Script> {
            Arc::new(Script {
                asked: Mutex::new(Vec::new()),
                devices,
                refuse: None,
            })
        }
        fn asked(&self) -> Vec<Request> {
            self.asked.lock().unwrap().clone()
        }
    }

    impl AgentCalls for Script {
        fn live(&self) -> Result<LiveUpdate, String> {
            Err("The script has no live readings.".into())
        }

        fn call(&self, request: Request) -> Result<Reply, String> {
            self.asked.lock().unwrap().push(request.clone());
            if let Some(m) = &self.refuse {
                return Err(m.clone());
            }
            Ok(match request {
                Request::PowerState => Reply::Power(Box::new(PowerView {
                    awake: AwakeView::Off,
                    caps: PowerCaps {
                        keep_awake: Capability::Known,
                        sleep: Capability::Known,
                        hibernate: Capability::Known,
                        shut_down: Capability::Known,
                    },
                    ..PowerView::default()
                })),
                Request::PowerKeepAwake { spec } => Reply::Power(Box::new(PowerView {
                    awake: AwakeView::Holding {
                        spec,
                        since: Timestamp(0),
                        ends: Some(Timestamp(1)),
                        os_ref: "x".into(),
                        busy: None,
                    },
                    caps: PowerCaps {
                        keep_awake: Capability::Known,
                        sleep: Capability::Known,
                        hibernate: Capability::Known,
                        shut_down: Capability::Known,
                    },
                    ..PowerView::default()
                })),
                Request::WolDevices => Reply::Wol(Box::new(WolView {
                    enabled: true,
                    devices: self.devices.clone(),
                    can_send: Capability::Known,
                })),
                _ => Reply::Power(Box::new(PowerView {
                    awake: AwakeView::Off,
                    caps: PowerCaps {
                        keep_awake: Capability::Known,
                        sleep: Capability::Known,
                        hibernate: Capability::Known,
                        shut_down: Capability::Known,
                    },
                    ..PowerView::default()
                })),
            })
        }
    }

    fn dev(id: u64, name: &str, last: Option<&str>) -> WolDevice {
        WolDevice {
            id,
            name: name.into(),
            mac: Mac([2, 0, 0x5e, 0x10, 0, id as u8]),
            last_wake: last.map(|s| LastWake {
                at: Timestamp(0),
                ok: true,
                tested: true,
                summary: s.into(),
            }),
            ..WolDevice::default()
        }
    }

    fn server(calls: Arc<dyn AgentCalls>, policy: Policy) -> Server {
        let s = Server::new(
            "Dyno",
            "1.1.0",
            Arc::new(FakeClock::at_wall_ms(1_700_000_000_000)),
            Arc::new(AuditLog::new(10)),
            Arc::new(paddock_log::Redactor::new()),
        );
        for t in tools(calls) {
            s.add_tool(t).unwrap();
        }
        s.set_policy(policy);
        s
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

    fn call(s: &Server, session: &Session, name: &str, args: Value) -> Value {
        rpc(
            s,
            session,
            json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":name,"arguments":args}}),
        )
    }

    fn names(s: &Server, session: &Session) -> Vec<String> {
        let r = rpc(s, session, json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}));
        r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }

    /// The names of every read-only tool (the two power reads and the reading tools) plus `actions`, in the order `tools/list` gives.
    fn reads_and(actions: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = [
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
        ]
        .iter()
        .chain(actions)
        .map(|s| s.to_string())
        .collect();
        v.sort();
        v
    }

    /// Whether the server refuses to serve at all (its master switch is off).
    fn refuses(s: &Server) -> bool {
        let session = Session::new("tester");
        rpc(
            s,
            &session,
            json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}),
        );
        let _ = s.handle(&session, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        rpc(s, &session, json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))["error"].is_object()
    }

    #[test]
    fn only_the_reads_exist_until_the_user_allows_an_action_by_name() {
        let s = server(Script::new(vec![]), Policy::read_only());
        let session = ready(&s);
        assert_eq!(names(&s, &session), reads_and(&[]));
        let refused = call(&s, &session, "keep_awake", json!({"minutes": 30}));
        assert!(
            refused["error"].is_object() || refused["result"]["isError"] == json!(true),
            "{refused}"
        );
        let s = server(Script::new(vec![]), Policy::read_only().allow_action("wake_device"));
        let session = ready(&s);
        assert_eq!(names(&s, &session), reads_and(&["wake_device"]));
    }

    #[test]
    fn off_by_default() {
        let s = server(Script::new(vec![]), Policy::default());
        assert!(refuses(&s), "a server nobody switched on serves nothing");
        let s = server(Script::new(vec![]), Policy::read_only());
        assert!(!refuses(&s));
    }

    #[test]
    fn the_reads_say_what_the_agent_says() {
        let calls = Script::new(vec![
            dev(1, "Media PC", Some("Awake after 23 s (answered ping).")),
            dev(2, "NAS", None),
        ]);
        let s = server(calls, Policy::read_only());
        let session = ready(&s);
        let r = call(&s, &session, "power_state", json!({}));
        assert_eq!(r["result"]["content"][0]["text"], "Dyno is not keeping the computer awake.");
        let r = call(&s, &session, "wol_devices", json!({}));
        assert_eq!(
            r["result"]["content"][0]["text"],
            "Media PC (last wake: Awake after 23 s (answered ping).)\nNAS (not woken yet)"
        );
        // No hardware address or IP ever reaches the client.
        assert!(!r.to_string().contains("02:00:5e"));
    }

    #[test]
    fn wake_device_finds_the_computer_by_name_and_asks_for_a_plain_wake() {
        let calls = Script::new(vec![dev(1, "Media PC", None), dev(2, "NAS", None)]);
        let s = server(calls.clone(), Policy::read_only().allow_action("wake_device"));
        let session = ready(&s);
        let r = call(&s, &session, "wake_device", json!({"device": " nas "}));
        assert!(
            r["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("The wake was started."),
            "{r}"
        );
        assert!(calls.asked().contains(&Request::WolWake { id: 2, test: false }));
        let r = call(&s, &session, "wake_device", json!({"device": "Printer"}));
        assert_eq!(r["result"]["isError"], json!(true));
        assert!(r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("No saved computer is called \"Printer\""));
        assert_eq!(calls.asked().iter().filter(|q| matches!(q, Request::WolWake { .. })).count(), 1);
    }

    #[test]
    fn two_computers_with_one_name_are_not_guessed() {
        let calls = Script::new(vec![dev(1, "PC", None), dev(2, "pc", None)]);
        let s = server(calls.clone(), Policy::read_only().allow_action("wake_device"));
        let session = ready(&s);
        let r = call(&s, &session, "wake_device", json!({"device": "PC"}));
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("More than one"));
        assert!(!calls.asked().iter().any(|q| matches!(q, Request::WolWake { .. })));
    }

    #[test]
    fn keep_awake_is_bounded_to_twelve_hours_and_asks_the_agent_for_a_timed_hold() {
        let calls = Script::new(vec![]);
        let s = server(calls.clone(), Policy::read_only().allow_action("keep_awake"));
        let session = ready(&s);
        let r = call(&s, &session, "keep_awake", json!({"minutes": 90, "display": true}));
        assert_eq!(
            r["result"]["content"][0]["text"],
            "Dyno is keeping the computer awake until a set time."
        );
        assert!(calls.asked().contains(&Request::PowerKeepAwake {
            spec: AwakeSpec {
                scope: AwakeScope::SystemAndDisplay,
                until: AwakeUntil::For { minutes: 90 }
            }
        }));
        for bad in [
            json!({"minutes": 721}),
            json!({"minutes": 0}),
            json!({"minutes": -5}),
            json!({}),
            json!({"minutes": "60"}),
            json!({"minutes": 30, "x": 1}),
        ] {
            let r = call(&s, &session, "keep_awake", bad.clone());
            assert!(r["error"].is_object() || r["result"]["isError"] == json!(true), "{bad}: {r}");
        }
        assert_eq!(
            calls.asked().iter().filter(|q| matches!(q, Request::PowerKeepAwake { .. })).count(),
            1,
            "bad arguments never reach the agent"
        );
    }

    #[test]
    fn the_agents_refusal_is_the_clients_message() {
        let calls = Arc::new(Script {
            asked: Mutex::new(vec![]),
            devices: vec![],
            refuse: Some("Keep awake is turned off in Settings.".into()),
        });
        let s = server(calls, Policy::read_only().allow_action("keep_awake"));
        let session = ready(&s);
        let r = call(&s, &session, "keep_awake", json!({"minutes": 30}));
        assert_eq!(r["result"]["isError"], json!(true));
        assert!(r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("turned off in Settings"));
    }

    #[test]
    fn descriptions_cover_every_state() {
        assert!(describe_awake(&AwakeView::Waiting {
            spec: AwakeSpec {
                scope: AwakeScope::System,
                until: AwakeUntil::Indefinite
            },
            why: "asleep".into(),
            since: Timestamp(0),
            ends: None
        })
        .contains("paused: asleep"));
        assert!(describe_awake(&AwakeView::Ended {
            why: "Dyno restarted".into(),
            at: Timestamp(0)
        })
        .contains("Dyno restarted"));
        assert!(describe_awake(&AwakeView::Holding {
            spec: AwakeSpec {
                scope: AwakeScope::System,
                until: AwakeUntil::Indefinite
            },
            since: Timestamp(0),
            ends: None,
            os_ref: "x".into(),
            busy: None
        })
        .contains("until it is stopped"));
        let off = WolView {
            enabled: false,
            devices: vec![],
            can_send: Capability::Known,
        };
        assert!(describe_devices(&off).contains("turned off"));
        assert!(describe_devices(&WolView { enabled: true, ..off }).contains("No computers"));
    }
    #[cfg(unix)]
    #[test]
    fn the_config_folder_is_made_owner_only_and_an_open_one_is_refused_untouched() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let made = tmp.path().join("config");
        prepare_config_folder(&made).unwrap();
        assert_eq!(std::fs::metadata(&made).unwrap().permissions().mode() & 0o777, 0o700);
        let open = tmp.path().join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        let e = prepare_config_folder(&open).unwrap_err().to_string();
        assert!(e.contains("chmod go-w"), "{e}");
        assert_eq!(std::fs::metadata(&open).unwrap().permissions().mode() & 0o777, 0o777);
        // A folder under a parent others can change is refused too.
        let under = open.join("config");
        assert!(prepare_config_folder(&under).is_err());
    }
    #[cfg(windows)]
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a Windows-only test runs icacls on its own temporary folder to set and read its access list"
    )]
    fn windows_the_ai_access_folder_is_refused_when_everyone_can_write_to_it() {
        let tmp = tempfile::tempdir().unwrap();
        let open = tmp.path().join("open");
        std::fs::create_dir(&open).unwrap();
        let out = std::process::Command::new("icacls")
            .arg(&open)
            .args(["/grant", "*S-1-1-0:(OI)(CI)F"])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(prepare_config_folder(&open).is_err());
        prepare_config_folder(&tmp.path().join("own").join("config")).unwrap();
    }
    /// W7 fails closed: a refused config folder means no key list or settings file is read or written, AI access is off, the refusal
    /// is the keys problem, and no listener is allowed (it would write its discovery file into that folder).
    #[cfg(unix)]
    #[test]
    fn a_refused_config_folder_loads_no_keys_starts_nothing_and_writes_nothing() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let open = tmp.path().join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        // A key list planted by another account: it must not be read.
        std::fs::write(open.join("mcp-keys.json"), br#"{"version":1,"keys":[{"id":"planted"}]}"#).unwrap();
        let before: Vec<_> = std::fs::read_dir(&open).unwrap().flatten().map(|e| e.file_name()).collect();
        let access = access_over(&open);
        assert!(!access.listener_allowed, "no listener from an unsafe folder");
        let loaded = access.keys.load();
        assert!(loaded.entries.is_empty(), "no planted key works");
        let problem = loaded.problem.expect("the refusal is the keys problem");
        assert!(problem.contains("AI access is off") && problem.contains("chmod go-w"), "{problem}");
        assert!(!access.settings.load().enabled, "AI access starts off");
        assert!(access.settings.save(&Default::default()).is_err());
        assert!(access.keys.save(&[]).is_err());
        let after: Vec<_> = std::fs::read_dir(&open).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(before, after, "nothing was written to the folder");
        assert_eq!(
            std::fs::metadata(&open).unwrap().permissions().mode() & 0o777,
            0o777,
            "never tightened"
        );
        // A safe folder still gets the real stores and the listener.
        let own = access_over(&tmp.path().join("config"));
        assert!(own.listener_allowed && own.keys.load().problem.is_none());
    }
}
