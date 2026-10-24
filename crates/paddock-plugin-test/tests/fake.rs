//! The platform through the fake host: a plugin in this process, a real connection, the fixture's every contribution.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_ipc::{FrameLimit, IpcError, RemoteError, codes};
use paddock_plugin_api::protocol::{McpBudget, SavedOutput};
use paddock_plugin_api::serve::{CallEnv, ELEVATED_ROLE, PluginHandler};
use paddock_plugin_api::tools::{FailureReason, Host, NoHost};
use paddock_plugin_api::{Contract, Contributions, Fail, Plugin, RegisterError};
use paddock_plugin_test::fixture::{self, FixtureContract, FixturePlugin, FixtureReply, FixtureRequest};
use paddock_plugin_test::{CallFailure, FakeHost, conformance};
use paddock_view::{ViewAction, ViewPatch};
use serde_json::{Value, json};
use std::sync::Arc;

fn host() -> FakeHost {
    FakeHost::start(Arc::new(FixturePlugin::new())).unwrap()
}

fn tool(h: &FakeHost, op: &str, args: Value) -> Result<Value, CallFailure> {
    h.invoke(fixture::TOOL, op, args)
}

fn remote(r: &Result<impl std::fmt::Debug, CallFailure>) -> &RemoteError {
    match r {
        Err(CallFailure::Remote(e)) => e,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn fail(r: &Result<impl std::fmt::Debug, CallFailure>) -> &Fail {
    match r {
        Err(CallFailure::Fail(f)) => f,
        other => panic!("expected a typed failure, got {other:?}"),
    }
}

#[test]
fn the_fixture_conforms() {
    conformance::assert_conforms(Arc::new(FixturePlugin::new()));
}

#[test]
fn what_the_fixture_describes_is_golden() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/described.json");
    paddock_plugin_test::golden::assert_described(Arc::new(FixturePlugin::new()), &path);
}

#[test]
fn a_view_document_is_golden() {
    let h = host();
    let doc = h.render_view(fixture::VIEW, json!({})).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/view.json");
    paddock_plugin_test::golden::assert_view(&doc, &path);
}

#[test]
fn tools_run_and_report_typed_failures_without_harming_the_plugin() {
    let h = host();
    assert_eq!(tool(&h, "echo", json!({"text": "hi"})).unwrap(), json!({"text": "hi"}));
    let failed = tool(&h, "fail", json!({}));
    assert_eq!((fail(&failed).reason, fail(&failed).message.as_str()), (None, "the fixture failed on purpose"));
    let denied = tool(&h, "deny", json!({}));
    assert_eq!(fail(&denied).reason, Some(FailureReason::Denied));
    // Arguments that do not fit the schema, an unknown tool and an unknown operation are refused.
    assert_eq!(remote(&tool(&h, "echo", json!({"text": 5}))).code, codes::INVALID);
    assert_eq!(remote(&h.invoke("nope", "x", json!({}))).message, "no tool \"nope\"");
    assert_eq!(remote(&tool(&h, "nope", json!({}))).message, "tool \"fixture.tool\" has no operation \"nope\"");
    // An operation of the elevated process does not run in the normal one.
    assert!(remote(&tool(&h, "root", json!({}))).message.contains("runs in the plugin's elevated process"));
    // A panic is contained: the call fails with `panic` and the plugin answers the next one.
    assert_eq!(remote(&tool(&h, "panic", json!({}))).code, codes::PANIC);
    assert!(!h.is_closed());
    assert_eq!(tool(&h, "echo", json!({"text": "again"})).unwrap(), json!({"text": "again"}));
}

#[test]
fn settings_are_validated_applied_and_refused_with_a_reason() {
    let h = host();
    assert_eq!(tool(&h, "greet", json!({})).unwrap(), json!("hello"));
    h.apply_settings(json!({"greeting": "ahoy", "shout": true})).unwrap();
    assert_eq!(tool(&h, "greet", json!({})).unwrap(), json!("AHOY"));
    let wrong = h.apply_settings(json!({"greeting": 5}));
    assert_eq!(remote(&wrong).code, codes::INVALID);
    assert!(remote(&wrong).message.starts_with("greeting: "));
    let refused = h.apply_settings(json!({"greeting": "refuse"}));
    assert_eq!(remote(&refused).message, "the fixture refuses this greeting");
    // Unknown keys are dropped, missing ones take their default.
    h.apply_settings(json!({"unknown": 1})).unwrap();
    assert_eq!(tool(&h, "greet", json!({})).unwrap(), json!("hello"));
}

#[test]
fn host_files_are_read_through_the_connection_and_unknown_ids_are_denied() {
    let h = host();
    let file = h.add_file("0123456789".repeat(150_000));
    let content = tool(&h, "read", json!({"id": file.id, "len": file.len})).unwrap();
    assert_eq!(content.as_str().unwrap().len(), 1_500_000);
    let denied = tool(&h, "read", json!({"id": 999, "len": 4}));
    assert_eq!(fail(&denied).reason, Some(FailureReason::Denied));
    // The contract can read too.
    let small = h.add_file(b"abc".to_vec());
    assert_eq!(h.extension::<FixtureContract>(1, &FixtureRequest::Read(small)).unwrap(), FixtureReply::Bytes(b"abc".to_vec()));
    let bad = h.extension::<FixtureContract>(1, &FixtureRequest::Read(paddock_plugin_api::host_file::HostFileRef { id: 77, len: 3 }));
    assert_eq!(remote(&bad).code, codes::INTERNAL);
}

#[test]
fn tasks_are_announced_reported_and_stopped_by_id() {
    let h = host();
    assert_eq!(tool(&h, "task", json!({})).unwrap(), json!("started"));
    assert_eq!(h.tasks_started()[0].0, fixture::TOOL);
    assert_eq!(h.tasks_started()[0].1.id, "job-1");
    assert_eq!(h.reports(), [("job-1".to_string(), Some(0.5), "halfway".to_string())]);
    h.stop_task("job-1").unwrap();
    assert_eq!(h.tasks_finished(), ["job-1"]);
    // Once it ended there is nothing to stop.
    let again = h.stop_task("job-1");
    assert!(matches!(&again, Err(CallFailure::Fail(f)) if f.reason == Some(FailureReason::Missing)));
    assert!(matches!(h.stop_task("other"), Err(CallFailure::Fail(_))));
}

#[test]
fn output_files_are_streamed_to_the_host_and_names_are_checked() {
    let h = host();
    let saved = tool(&h, "save", json!({"name": "capture.bin", "bytes": 600_000})).unwrap();
    assert_eq!(saved, json!({"name": "capture.bin", "bytes": 600_000}));
    assert_eq!(h.output("capture.bin").unwrap().len(), 600_000);
    assert_eq!(h.saved_output("capture.bin"), Some(SavedOutput { name: "capture.bin".into(), bytes: 600_000 }));
    assert!(fail(&tool(&h, "save", json!({"name": "../x", "bytes": 1}))).message.contains("invalid output file name"));
    assert!(h.output("../x").is_none());
}

#[test]
fn elevated_operations_go_through_the_host_by_name() {
    let h = host();
    let none = tool(&h, "lift", json!({"op": "collect"}));
    assert!(fail(&none).message.contains("no elevated operations"));
    h.on_elevated(|tool, op, _| if op == "collect" { Ok(json!({"tool": tool})) } else { Err(Fail::new("no")) });
    assert_eq!(tool(&h, "lift", json!({"op": "collect"})).unwrap(), json!({"tool": fixture::TOOL}));
    assert_eq!(fail(&tool(&h, "lift", json!({"op": "other"}))).message, "no");
    let asked: Vec<String> = h.elevated_calls().into_iter().map(|(_, op)| op).collect();
    assert_eq!(asked, ["collect", "collect", "other"]);
}

#[test]
fn views_render_validated_documents_and_answer_actions() {
    let h = host();
    let doc = h.render_view(fixture::VIEW, json!({"title": "Mine"})).unwrap();
    assert_eq!((doc.title.as_str(), doc.etag), ("Mine", 1));
    paddock_view::validate(&doc).unwrap();
    // A document over the limits never leaves the plugin.
    let broken = h.render_view(fixture::VIEW, json!({"broken": true}));
    assert!(fail(&broken).message.contains("the view is not valid"));
    let patch = h.view_action(&ViewAction { view: fixture::VIEW.into(), action: fixture::ACTION.into(), args: paddock_ipc::Json(json!({})) }).unwrap();
    assert!(matches!(patch, ViewPatch::Doc(d) if d.etag == 2));
    let unknown_action = h.view_action(&ViewAction { view: fixture::VIEW.into(), action: "nope".into(), args: paddock_ipc::Json(json!({})) });
    assert!(fail(&unknown_action).message.contains("no action nope"));
    let bad_action = h.view_action(&ViewAction { view: fixture::VIEW.into(), action: String::new(), args: paddock_ipc::Json(json!({})) });
    assert_eq!(remote(&bad_action).code, codes::INVALID);
    assert_eq!(remote(&h.render_view("nope", json!({}))).message, "no view \"nope\"");
    let missing = h.view_action(&ViewAction { view: "nope".into(), action: "a".into(), args: paddock_ipc::Json(json!({})) });
    assert_eq!(remote(&missing).message, "no view \"nope\"");
}

#[test]
fn mcp_tools_answer_within_their_budget() {
    let h = host();
    let budget = McpBudget { max_bytes: 1000, max_ms: 1000 };
    assert_eq!(h.mcp_tool(fixture::MCP_TOOL, json!({"text": "hi"}), budget).unwrap(), json!({"text": "hi"}));
    let over = h.mcp_tool(fixture::MCP_TOOL, json!({"text": "x".repeat(2000)}), budget);
    assert!(fail(&over).message.contains("the budget is 1000"));
    assert_eq!(remote(&h.mcp_tool(fixture::MCP_TOOL, json!({"text": 5}), budget)).code, codes::INVALID);
    assert_eq!(remote(&h.mcp_tool("nope", json!({}), budget)).message, "no MCP tool \"nope\"");
}

#[test]
fn a_contract_is_routed_by_namespace_and_refuses_a_newer_minor() {
    let h = host();
    assert_eq!(h.extension::<FixtureContract>(1, &FixtureRequest::Add(2, 3)).unwrap(), FixtureReply::Sum(5));
    assert_eq!(h.extension::<FixtureContract>(0, &FixtureRequest::Echo("e".into())).unwrap(), FixtureReply::Text("e".into()));
    assert_eq!(h.extension::<FixtureContract>(1, &FixtureRequest::Twice("ab".into())).unwrap(), FixtureReply::Text("abab".into()));
    // The platform never decoded the body: the plugin's own typed failure and panic arrive as they are.
    let failed = h.extension::<FixtureContract>(1, &FixtureRequest::Fail);
    assert_eq!((remote(&failed).code.as_str(), remote(&failed).message.as_str()), ("fixture.failed", "the fixture's contract failed on purpose"));
    assert_eq!(remote(&h.extension::<FixtureContract>(1, &FixtureRequest::Panic)).code, codes::PANIC);
    // A newer minor than the plugin was built for: it must be updated, and the text says so.
    let newer = h.extension::<FixtureContract>(2, &FixtureRequest::Add(1, 1));
    assert_eq!(remote(&newer).code, codes::VERSION);
    assert_eq!(
        remote(&newer).message,
        "plugin net.example.fixture was built for an older contract (\"paddock.fixture.v1\" version 1, asked for 2); update it"
    );
    assert_eq!(h.extension::<FixtureContract>(1, &FixtureRequest::Big(10)).unwrap(), FixtureReply::Bytes(vec![1; 10]));
}

#[test]
fn one_frame_over_the_limit_fails_one_call_and_the_plugin_lives() {
    let h = FakeHost::start_with(Arc::new(FixturePlugin::new()), FrameLimit::new(4096), None).unwrap();
    let big = tool(&h, "big", json!({"n": 100_000}));
    assert_eq!(remote(&big).code, codes::TOO_LARGE);
    assert_eq!(tool(&h, "echo", json!({"text": "small"})).unwrap(), json!({"text": "small"}));
    // A request over the limit is not even sent.
    let huge = tool(&h, "echo", json!({"text": "x".repeat(1000), "pad": "y".repeat(10_000)}));
    assert!(matches!(huge, Err(CallFailure::Ipc(IpcError::TooLarge { .. }))));
    assert!(!h.is_closed());
}

struct Broken;
impl Plugin for Broken {
    fn id(&self) -> &str {
        "net.example.broken"
    }
    fn name(&self) -> &str {
        "Broken"
    }
    fn version(&self) -> &str {
        "0.0.1"
    }
    fn register(&self, _: &mut Contributions) -> Result<(), RegisterError> {
        Err("it cannot register".into())
    }
}

struct Bare;
impl Plugin for Bare {
    fn id(&self) -> &str {
        "net.example.bare"
    }
    fn name(&self) -> &str {
        "Bare"
    }
    fn version(&self) -> &str {
        "0.0.1"
    }
    fn register(&self, _: &mut Contributions) -> Result<(), RegisterError> {
        Ok(())
    }
    fn memory_limit(&self) -> Option<u64> {
        Some(256 << 20)
    }
}

#[test]
fn a_plugin_that_cannot_register_answers_every_request_with_why() {
    let h = FakeHost::start(Arc::new(Broken)).unwrap();
    let d = h.describe();
    assert_eq!((remote(&d).code.as_str(), remote(&d).message.as_str()), (codes::INTERNAL, "it cannot register"));
    assert_eq!(remote(&tool(&h, "x", json!({}))).message, "it cannot register");
    assert!(!conformance::run(&h).is_empty());
}

#[test]
fn a_plugin_without_contributions_has_no_settings_and_describes_its_memory() {
    let h = FakeHost::start(Arc::new(Bare)).unwrap();
    assert_eq!(h.describe().unwrap().memory_limit, Some(256 << 20));
    assert_eq!(remote(&h.apply_settings(json!({}))).message, "this plugin has no settings");
    assert!(conformance::run(&h).is_empty());
}

struct Env(Arc<dyn Host>);
impl CallEnv for Env {
    fn host(&self) -> Arc<dyn Host> {
        self.0.clone()
    }
    fn is_cancelled(&self) -> bool {
        false
    }
    fn cancel_probe(&self) -> Arc<dyn Fn() -> bool + Send + Sync> {
        Arc::new(|| false)
    }
}

#[test]
fn the_elevated_role_runs_only_elevated_operations() {
    use paddock_plugin_api::PlatformRequest as R;
    let handler = PluginHandler::new(Arc::new(FixturePlugin::new()), true);
    assert_eq!(ELEVATED_ROLE, "elevated");
    let env = Env(Arc::new(NoHost));
    let invoke = |op: &str| handler.dispatch(&env, R::Invoke { tool: fixture::TOOL.into(), op: op.into(), args: paddock_ipc::Json(json!({})) });
    let ok: Result<paddock_ipc::Json, Fail> = paddock_ipc::wire::decode(&invoke("root").unwrap()).unwrap();
    assert_eq!(ok.unwrap().into_inner(), json!("root"));
    assert!(invoke("echo").unwrap_err().message.contains("does not run in the elevated process"));
    let settings = handler.dispatch(&env, R::ApplySettings { json: paddock_ipc::Json(json!({})) });
    assert_eq!(settings.unwrap_err().message, "the elevated process only runs elevated tool operations");
    assert!(handler.dispatch(&env, R::Describe).is_ok());
    assert_eq!(handler.plugin().id(), fixture::ID);
    // Stopping a task nobody started is a typed failure.
    let stop: Result<(), Fail> = paddock_ipc::wire::decode(&handler.dispatch(&env, R::Stop { task: "t".into() }).unwrap()).unwrap();
    assert!(stop.unwrap_err().message.contains("no running task"));
}

#[test]
fn cancellation_stops_a_long_host_file_read() {
    use paddock_plugin_api::PlatformRequest as R;
    struct Cancelled;
    impl CallEnv for Cancelled {
        fn host(&self) -> Arc<dyn Host> {
            Arc::new(NoHost)
        }
        fn is_cancelled(&self) -> bool {
            true
        }
        fn cancel_probe(&self) -> Arc<dyn Fn() -> bool + Send + Sync> {
            Arc::new(|| true)
        }
    }
    let handler = PluginHandler::new(Arc::new(FixturePlugin::new()), false);
    let body = paddock_ipc::wire::encode(&FixtureRequest::Read(paddock_plugin_api::host_file::HostFileRef { id: 1, len: 10 })).unwrap();
    let e = handler.dispatch(&Cancelled, R::Extension { ns: FixtureContract::NS.into(), minor: 1, body }).unwrap_err();
    // The contract maps its error; a tool reading with a cancelled call is cancelled for the app.
    assert_eq!(e.code, codes::INTERNAL);
    let read = handler.dispatch(&Cancelled, R::Invoke { tool: fixture::TOOL.into(), op: "read".into(), args: paddock_ipc::Json(json!({"id": 1, "len": 10})) });
    assert_eq!(read.unwrap_err().code, codes::CANCELLED);
}
