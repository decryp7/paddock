//! The conformance checks themselves: each one must fail for a plugin that breaks its rule.

use paddock_ipc::{CallCtx, FrameLimit, Handler, Json, RemoteError, decode_request, encode_reply};
use paddock_plugin_api::protocol::{
    Described, ExtensionInfo, McpToolInfo, PlatformRequest, ViewInfo,
};
use paddock_plugin_api::tools::{OpSpec, Schema, ToolInfo};
use paddock_plugin_api::{Fail, PLATFORM_PROTOCOL};
use paddock_plugin_test::FakeHost;
use paddock_plugin_test::conformance::{self, Finding};
use paddock_plugin_test::fixture::FixturePlugin;
use paddock_settings::{FieldKind, SectionOwner, SettingField, SettingsSection};
use paddock_view::{Node, SCHEMA, ViewAction, ViewDoc, ViewPatch};
use serde_json::{Value, json};
use std::sync::Arc;

/// A plugin that answers everything and refuses nothing.
struct Lenient;

impl Handler for Lenient {
    fn call(&self, _: &CallCtx, body: &[u8]) -> Result<Vec<u8>, RemoteError> {
        let described = Described {
            id: String::new(),
            name: " ".into(),
            version: String::new(),
            protocol: paddock_ipc::ProtocolVersion::new(1, 0),
            memory_limit: None,
            tools: vec![
                ToolInfo::new("bad tool", "T"),
                ToolInfo::new("t", "T").op(OpSpec::new("o", "O").args(Schema::object())),
            ],
            settings: Some(SettingsSection::new("s", "S", SectionOwner::App).field(
                SettingField::new(
                    "k",
                    "K",
                    FieldKind::Int {
                        min: 5,
                        max: 1,
                        step: 1,
                        unit: None,
                    },
                    3,
                ),
            )),
            views: vec![ViewInfo {
                id: "v".into(),
                title: "V".into(),
                writes: false,
            }],
            mcp_tools: vec![McpToolInfo {
                name: "m".into(),
                description: String::new(),
                args: Schema::Any,
            }],
            extensions: vec![
                ExtensionInfo {
                    ns: String::new(),
                    minor: 0,
                },
                ExtensionInfo {
                    ns: "x.v1".into(),
                    minor: 0,
                },
            ],
        };
        match decode_request::<PlatformRequest>(body)? {
            PlatformRequest::Describe => {
                encode_reply(&Json(serde_json::to_value(&described).unwrap()))
            }
            // It takes anything but its own defaults.
            PlatformRequest::ApplySettings { json } if json["k"] == json!(3) => {
                Err(RemoteError::new("invalid", "no"))
            }
            PlatformRequest::ApplySettings { .. } => encode_reply(&()),
            PlatformRequest::Stop { .. } => encode_reply(&Ok::<(), Fail>(())),
            PlatformRequest::RenderView { .. } => {
                let doc = ViewDoc {
                    schema: SCHEMA + 98,
                    title: "bad".into(),
                    root: Node::text("x"),
                    etag: 0,
                };
                encode_reply(&Ok::<Json, Fail>(Json(serde_json::to_value(doc).unwrap())))
            }
            PlatformRequest::Extension { .. } => Ok(Vec::new()),
            PlatformRequest::Invoke { .. }
            | PlatformRequest::ViewAction { .. }
            | PlatformRequest::McpTool { .. } => encode_reply(&Ok::<Json, Fail>(Json(Value::Null))),
        }
    }
}

fn checks(findings: &[Finding]) -> Vec<&'static str> {
    let mut c: Vec<&'static str> = findings.iter().map(|f| f.check).collect();
    c.dedup();
    c
}

#[test]
fn a_plugin_that_refuses_nothing_fails_every_check() {
    let host = FakeHost::start_handler(Arc::new(Lenient), FrameLimit::default(), None).unwrap();
    let findings = conformance::run(&host);
    let text: String = findings
        .iter()
        .map(|f| format!("[{}] {}\n", f.check, f.problem))
        .collect();
    for expected in [
        "it was built for protocol 1.0",
        "id, name and version are required",
        "settings:",
        "an unknown tool was not refused",
        "an unknown view was not refused",
        "an unknown MCP tool was not refused",
        "an unknown contract was not refused",
        "stopping an unknown task was not a typed failure",
        "was not refused with 'update it'",
        "a garbled body for",
        "returned an invalid document",
        "answered over a budget of 0 bytes",
        "refused its own default settings",
        "settings of the wrong type",
        "a contract has no namespace",
    ] {
        assert!(text.contains(expected), "missing: {expected}\n{text}");
    }
    assert_eq!(
        checks(&findings),
        [
            "describe",
            "unknown requests",
            "newer contract minor",
            "malformed input",
            "views",
            "limits"
        ],
        "{text}"
    );
}

#[test]
fn a_plugin_that_does_not_even_describe_itself_stops_at_the_first_finding() {
    struct Mute;
    impl Handler for Mute {
        fn call(&self, _: &CallCtx, _: &[u8]) -> Result<Vec<u8>, RemoteError> {
            Err(RemoteError::new("internal", "nothing works"))
        }
    }
    let host = FakeHost::start_handler(Arc::new(Mute), FrameLimit::default(), None).unwrap();
    let findings = conformance::run(&host);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].check, "describe");
    assert!(findings[0].problem.contains("nothing works"));
}

#[test]
fn a_panic_in_an_operation_is_contained_so_the_plugin_conforms() {
    use paddock_plugin_api::prelude::*;
    struct Any;
    impl Tool for Any {
        fn id(&self) -> &str {
            "any"
        }
        fn describe(&self) -> ToolInfo {
            ToolInfo::new("any", "Any").op(OpSpec::new("go", "Go").args(Schema::Any))
        }
        fn invoke(&self, _: &str, _: Value, _: &ToolCtx) -> Result<Value, ToolError> {
            panic!("an operation that takes anything and breaks")
        }
    }
    struct Fragile;
    impl Plugin for Fragile {
        fn id(&self) -> &str {
            "net.example.fragile"
        }
        fn name(&self) -> &str {
            "Fragile"
        }
        fn version(&self) -> &str {
            "1.0.0"
        }
        fn register(&self, c: &mut Contributions) -> Result<(), RegisterError> {
            c.add_tool(Any);
            Ok(())
        }
    }
    conformance::assert_conforms(Arc::new(Fragile));
}

#[test]
fn assert_conforms_names_every_finding_when_it_panics() {
    let result = std::panic::catch_unwind(|| {
        struct Broken;
        impl paddock_plugin_api::Plugin for Broken {
            fn id(&self) -> &str {
                "net.example.broken"
            }
            fn name(&self) -> &str {
                "Broken"
            }
            fn version(&self) -> &str {
                "1"
            }
            fn register(
                &self,
                _: &mut paddock_plugin_api::Contributions,
            ) -> Result<(), paddock_plugin_api::RegisterError> {
                Err("no".into())
            }
        }
        conformance::assert_conforms(Arc::new(Broken));
    });
    let message = *result.unwrap_err().downcast::<String>().unwrap();
    assert!(
        message.contains("conformance check(s)") && message.contains("[describe]"),
        "{message}"
    );
    let _ = (PLATFORM_PROTOCOL, json!(null), FixturePlugin::new());
}

/// A plugin that describes itself well and then drops the connection at the first request it is given.
struct Quitter;

impl Handler for Quitter {
    fn call(&self, ctx: &CallCtx, body: &[u8]) -> Result<Vec<u8>, RemoteError> {
        match decode_request::<PlatformRequest>(body)? {
            PlatformRequest::Describe => {
                let described = Described {
                    id: "net.example.quitter".into(),
                    name: "Quitter".into(),
                    version: "1.0.0".into(),
                    protocol: PLATFORM_PROTOCOL,
                    memory_limit: None,
                    tools: vec![
                        ToolInfo::new("t", "T").op(OpSpec::new("o", "O").args(Schema::Any)),
                    ],
                    settings: None,
                    views: vec![ViewInfo {
                        id: "v".into(),
                        title: "V".into(),
                        writes: false,
                    }],
                    mcp_tools: Vec::new(),
                    extensions: Vec::new(),
                };
                encode_reply(&Json(serde_json::to_value(&described).unwrap()))
            }
            _ => {
                ctx.peer().close();
                Err(RemoteError::new("internal", "gone"))
            }
        }
    }
}

#[test]
fn a_plugin_that_drops_the_connection_fails_the_checks_that_need_it_alive() {
    let host = FakeHost::start_handler(Arc::new(Quitter), FrameLimit::default(), None).unwrap();
    let findings = conformance::run(&host);
    let text: String = findings
        .iter()
        .map(|f| format!("[{}] {}\n", f.check, f.problem))
        .collect();
    for expected in [
        "malformed arguments broke the connection",
        "the plugin stopped answering after malformed input",
        "did not render",
        "the plugin did not survive the checks",
    ] {
        assert!(text.contains(expected), "missing: {expected}\n{text}");
    }
}

#[test]
fn a_view_that_cannot_render_right_now_may_say_so_with_a_typed_failure() {
    use paddock_plugin_api::prelude::*;
    struct Waiting;
    impl View for Waiting {
        fn info(&self) -> ViewInfo {
            ViewInfo {
                id: "waiting".into(),
                title: "Waiting".into(),
                writes: false,
            }
        }
        fn render(&self, _: &ViewCtx) -> Result<ViewDoc, ToolError> {
            Err(ToolError::failed("nothing to show yet"))
        }
        fn action(&self, _: &ViewCtx, _: &ViewAction) -> Result<ViewPatch, ToolError> {
            Err(ToolError::Cancelled)
        }
    }
    struct Patient;
    impl Plugin for Patient {
        fn id(&self) -> &str {
            "net.example.patient"
        }
        fn name(&self) -> &str {
            "Patient"
        }
        fn version(&self) -> &str {
            "1.0.0"
        }
        fn register(&self, c: &mut Contributions) -> Result<(), RegisterError> {
            c.add_view(Waiting);
            Ok(())
        }
    }
    conformance::assert_conforms(Arc::new(Patient));
    let host = FakeHost::start(Arc::new(Patient)).unwrap();
    let action = ViewAction {
        view: "waiting".into(),
        action: "a".into(),
        args: Json(json!({})),
    };
    assert!(host.view_action(&action).is_err());
}
