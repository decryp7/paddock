//! What a plugin can do through the host, against a real plugin process: tools, settings, views, MCP, contracts, host
//! files, tasks, outputs, elevated operations, and the grants that bound them.

use crate::common::{ELEVATE, READ_DOCS, Rig};

use paddock_ipc::Json;
use paddock_plugin_api::protocol::{Fail, McpBudget};
use paddock_plugin_api::tools::FailureReason;
use paddock_plugin_host::services::ElevatedRunner;
use paddock_plugin_host::{FileError, Grants, HostError, Status, Supervisor};
use paddock_plugin_package::Permission;
use paddock_plugin_test::fixture::{self, FixtureContract, FixtureReply, FixtureRequest};
use paddock_view::{ViewAction, ViewPatch};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

fn tool(s: &Supervisor, op: &str, args: Value) -> Result<Value, HostError> {
    s.invoke(fixture::TOOL, op, args)
}

fn failed(r: Result<Value, HostError>) -> Fail {
    match r {
        Err(HostError::Failed(f)) => f,
        other => panic!("expected a typed failure, got {other:?}"),
    }
}

fn invalid(r: Result<Value, HostError>) -> String {
    match r {
        Err(HostError::Invalid(m)) => m,
        other => panic!("expected an invalid request, got {other:?}"),
    }
}

#[test]
fn the_plugin_describes_itself_and_tools_check_their_arguments_before_anything_is_sent() {
    let rig = Rig::new();
    let s = rig.supervisor("", Grants::none());
    assert!(s.described().is_none());
    assert_eq!(
        tool(&s, "echo", json!({"text": "hi"})).unwrap(),
        json!({"text": "hi"})
    );
    let d = s.described().unwrap();
    assert_eq!(
        (d.id.as_str(), d.tools.len(), d.extensions.len()),
        (fixture::ID, 1, 1)
    );
    assert!(invalid(tool(&s, "echo", json!({"text": 5}))).contains("args.text"));
    assert!(invalid(s.invoke("nope", "x", json!({}))).contains("has no tool \"nope\""));
    assert!(invalid(tool(&s, "nope", json!({}))).contains("no operation \"nope\""));
    assert!(invalid(tool(&s, "root", json!({}))).contains("administrator rights"));
    // None of that was a fault.
    assert_eq!(rig.quarantine.status(fixture::ID).total, 0);
    assert_eq!(
        s.program(),
        rig.plugins.load(fixture::ID).unwrap().unwrap().program
    );
    assert_eq!(s.manifest().id.as_str(), fixture::ID);
    assert_eq!(s.id(), fixture::ID);
}

#[test]
fn settings_are_validated_by_the_host_applied_and_given_again_after_a_restart() {
    let rig = Rig::new();
    let s = rig.supervisor("", Grants::none());
    s.apply_settings(json!({"greeting": "ahoy", "shout": true}))
        .unwrap();
    assert_eq!(tool(&s, "greet", json!({})).unwrap(), json!("AHOY"));
    let wrong = s.apply_settings(json!({"greeting": 5})).unwrap_err();
    assert!(
        matches!(&wrong, HostError::Invalid(m) if m.starts_with("greeting: ")),
        "{wrong}"
    );
    // The plugin refusing the values on purpose is a remote refusal, not a fault.
    let refused = s.apply_settings(json!({"greeting": "refuse"})).unwrap_err();
    assert!(
        matches!(&refused, HostError::Remote(r) if r.message == "the fixture refuses this greeting"),
        "{refused}"
    );
    // The values it took stay in force; after a crash and a restart it is given them again.
    assert!(matches!(
        tool(&s, "crash", json!({})),
        Err(HostError::Fault { .. })
    ));
    rig.clock.advance(Duration::from_secs(1));
    assert_eq!(tool(&s, "greet", json!({})).unwrap(), json!("AHOY"));
}

#[test]
fn views_render_validated_documents_and_actions_must_be_declared() {
    let rig = Rig::new();
    let s = rig.supervisor("", Grants::none());
    let doc = s
        .render_view(fixture::VIEW, json!({"title": "Mine"}))
        .unwrap();
    assert_eq!((doc.title.as_str(), doc.etag), ("Mine", 1));
    let action = |action: &str| ViewAction {
        view: fixture::VIEW.into(),
        action: action.into(),
        args: Json(json!({})),
    };
    assert!(
        matches!(s.view_action(&action(fixture::ACTION)).unwrap(), ViewPatch::Doc(d) if d.etag == 2)
    );
    assert!(matches!(
        s.view_action(&action(fixture::REPLACE)).unwrap(),
        ViewPatch::Replace { etag: 3, .. }
    ));
    // The manifest does not list this action: nothing is sent.
    let e = s.view_action(&action("delete-everything")).unwrap_err();
    assert!(
        matches!(&e, HostError::Invalid(m) if m.contains("does not declare the action")),
        "{e}"
    );
    assert!(matches!(
        s.view_action(&action("")),
        Err(HostError::Invalid(_))
    ));
    assert!(matches!(
        s.render_view("nope", json!({})),
        Err(HostError::Invalid(_))
    ));
    // The plugin refuses to send a document over the limits; that is its typed failure.
    let broken = s
        .render_view(fixture::VIEW, json!({"broken": true}))
        .unwrap_err();
    assert!(
        matches!(&broken, HostError::Failed(f) if f.message.contains("the view is not valid")),
        "{broken}"
    );
    assert_eq!(rig.quarantine.status(fixture::ID).total, 0);
}

#[test]
fn mcp_tools_need_the_permission_asked_and_granted_and_stay_within_their_budget() {
    let rig = Rig::new();
    let budget = McpBudget {
        max_bytes: 1000,
        max_ms: 5000,
    };
    let none = rig.supervisor("", Grants::none());
    let denied = none
        .mcp_tool(fixture::MCP_TOOL, json!({"text": "hi"}), budget)
        .unwrap_err();
    assert!(
        matches!(&denied, HostError::Invalid(m) if m.contains("may not offer tools over MCP")),
        "{denied}"
    );

    let rig = Rig::new();
    let s = rig.supervisor("", Grants::none().allow(Permission::McpExpose));
    assert_eq!(
        s.mcp_tool(fixture::MCP_TOOL, json!({"text": "hi"}), budget)
            .unwrap(),
        json!({"text": "hi"})
    );
    assert!(matches!(
        s.mcp_tool(fixture::MCP_TOOL, json!({"text": 5}), budget),
        Err(HostError::Invalid(_))
    ));
    assert!(matches!(
        s.mcp_tool("nope", json!({}), budget),
        Err(HostError::Invalid(_))
    ));
    let over = s
        .mcp_tool(fixture::MCP_TOOL, json!({"text": "x".repeat(2000)}), budget)
        .unwrap_err();
    assert!(
        matches!(&over, HostError::Failed(f) if f.message.contains("the budget is 1000")),
        "{over}"
    );
}

#[test]
fn a_contract_is_called_through_a_guarded_proxy() {
    let rig = Rig::new();
    let s = rig.supervisor("", Grants::none());
    let r = s.remote::<FixtureContract>().unwrap();
    assert_eq!(r.plugin(), fixture::ID);
    assert_eq!(
        r.call(&FixtureRequest::Add(2, 3)).unwrap(),
        FixtureReply::Sum(5)
    );
    assert_eq!(
        r.clone()
            .call_within(&FixtureRequest::Twice("ab".into()), Duration::from_secs(5))
            .unwrap(),
        FixtureReply::Text("abab".into())
    );
    // A request that does not fit a frame is not sent, and the plugin is not blamed.
    let huge = r
        .call(&FixtureRequest::Echo("x".repeat(600_000)))
        .unwrap_err();
    assert!(
        matches!(&huge, HostError::Invalid(m) if m.contains("the request is")),
        "{huge}"
    );
    assert_eq!(rig.quarantine.status(fixture::ID).total, 0);
    // A reply this app cannot decode is a counted fault: this app's idea of the contract has another reply type.
    struct Liar;
    impl paddock_plugin_api::Contract for Liar {
        const NS: &'static str = "paddock.fixture.v1";
        const MINOR: u16 = 1;
        type Request = FixtureRequest;
        type Reply = (String, Vec<String>);
    }
    let liar = s.remote::<Liar>().unwrap();
    let e = liar.call(&FixtureRequest::Add(1, 1)).unwrap_err();
    assert!(
        matches!(&e, HostError::Fault { kind: paddock_plugin_host::FaultKind::Garbled, message } if message.contains("cannot read")),
        "{e}"
    );
    assert_eq!(rig.quarantine.status(fixture::ID).total, 1);
    assert_eq!(s.status(), Status::Running);
}

#[test]
fn host_files_are_read_by_the_plugin_only_when_the_app_opened_them() {
    let rig = Rig::new();
    let docs = rig.dir.path().join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(docs.join("a.txt"), "granted content").unwrap();
    std::fs::write(rig.dir.path().join("private.txt"), "private").unwrap();
    let m = fixture::manifest("2.0", "paddock-plugin-fixture", READ_DOCS);
    let grants = Grants::none()
        .allow(m.permissions[1].permission.clone())
        .folder("docs", &docs);
    let s = rig.supervisor(READ_DOCS, grants);
    // A file inside the granted folder can be opened for it; one outside cannot.
    let ok = s
        .files()
        .open_granted(s.manifest(), &s.grants(), &docs.join("a.txt"))
        .unwrap();
    assert_eq!(
        tool(&s, "read", json!({"id": ok.id, "len": ok.len})).unwrap(),
        json!("granted content")
    );
    let outside = s.files().open_granted(
        s.manifest(),
        &s.grants(),
        &rig.dir.path().join("private.txt"),
    );
    assert!(matches!(outside, Err(FileError::Denied(_))));
    // A file the user chose is opened without a grant.
    let chosen = s
        .files()
        .open_chosen(&rig.dir.path().join("private.txt"))
        .unwrap();
    assert_eq!(
        tool(&s, "read", json!({"id": chosen.id, "len": chosen.len})).unwrap(),
        json!("private")
    );
    // The plugin cannot read an id the app never opened: it is denied, and that is no fault.
    let denied = failed(tool(&s, "read", json!({"id": 4242, "len": 5})));
    assert_eq!(denied.reason, Some(FailureReason::Denied));
    assert_eq!(rig.quarantine.status(fixture::ID).total, 0);
    // A closed file is gone.
    s.files().close(chosen.id);
    assert_eq!(
        failed(tool(&s, "read", json!({"id": chosen.id, "len": 7}))).reason,
        Some(FailureReason::Denied)
    );
}

#[test]
fn path_arguments_must_lie_inside_the_folders_the_user_granted() {
    let rig = Rig::new();
    let (docs, secret) = (rig.dir.path().join("docs"), rig.dir.path().join("secret"));
    for d in [&docs, &secret] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(docs.join("a.txt"), "in docs").unwrap();
    std::fs::write(secret.join("s.txt"), "secret").unwrap();
    let m = fixture::manifest("2.0", "paddock-plugin-fixture", READ_DOCS);
    let grants = Grants::none()
        .allow(m.permissions[1].permission.clone())
        .folder("docs", &docs);
    let s = rig.supervisor(READ_DOCS, grants);
    let path = |p: &std::path::Path| json!({"path": p.to_str().unwrap()});
    assert_eq!(
        tool(&s, "open", path(&docs.join("a.txt"))).unwrap(),
        json!("in docs")
    );
    let outside = invalid(tool(&s, "open", path(&secret.join("s.txt"))));
    assert!(
        outside.contains("not inside a folder you allowed this plugin to read"),
        "{outside}"
    );
    assert!(invalid(tool(&s, "open", json!({"path": "relative/x"}))).contains("absolute"));
    assert!(invalid(tool(&s, "open", path(&docs.join("missing.txt")))).contains("cannot be used"));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&secret, docs.join("escape")).unwrap();
        assert!(
            invalid(tool(&s, "open", path(&docs.join("escape").join("s.txt"))))
                .contains("not inside")
        );
    }
    // Changing the grants takes effect at once.
    s.set_grants(Grants::none());
    assert!(invalid(tool(&s, "open", path(&docs.join("a.txt")))).contains("not inside"));
    assert_eq!(s.grants(), Grants::none());
    assert_eq!(s.sandbox(), paddock_plugin_host::SandboxSpec::locked());
}

#[test]
fn background_tasks_are_listed_reported_stopped_and_ended_with_the_process() {
    let rig = Rig::new();
    let s = rig.supervisor("", Grants::none());
    assert_eq!(tool(&s, "task", json!({})).unwrap(), json!("started"));
    let t = s.tasks();
    assert_eq!(
        (
            t[0].tool.as_str(),
            t[0].task.id.as_str(),
            t[0].progress,
            t[0].note.as_str()
        ),
        (fixture::TOOL, "job-1", Some(0.5), "halfway")
    );
    s.stop_task("job-1").unwrap();
    assert!(s.tasks().is_empty());
    let none = s.stop_task("job-1").unwrap_err();
    assert!(
        matches!(&none, HostError::Failed(f) if f.reason == Some(FailureReason::Missing)),
        "{none}"
    );
    // A task still running when the process ends is ended with it.
    tool(&s, "task", json!({})).unwrap();
    s.stop();
    assert!(s.tasks().is_empty());
    let seen = rig.seen.0.lock().unwrap().clone();
    assert_eq!(seen.iter().filter(|l| l.starts_with("started")).count(), 2);
    assert_eq!(seen.iter().filter(|l| l.starts_with("finished")).count(), 2);
    assert!(
        seen.iter()
            .any(|l| l.contains("report net.example.fixture job-1 Some(0.5) halfway"))
    );
}

#[test]
fn output_files_land_in_the_folder_the_app_chose() {
    let rig = Rig::new();
    let s = rig.supervisor("", Grants::none());
    let saved = tool(&s, "save", json!({"name": "capture.bin", "bytes": 300_000})).unwrap();
    assert_eq!(saved, json!({"name": "capture.bin", "bytes": 300_000}));
    let path = rig
        .dir
        .path()
        .join("outputs")
        .join(fixture::ID)
        .join("capture.bin");
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 300_000);
    // It is never replaced, and a bad name is refused.
    assert!(
        failed(tool(&s, "save", json!({"name": "capture.bin", "bytes": 1})))
            .message
            .contains("already exists")
    );
    assert!(
        failed(tool(&s, "save", json!({"name": "../x", "bytes": 1})))
            .message
            .contains("invalid output file name")
    );
    assert_eq!(
        std::fs::read_dir(rig.dir.path().join("outputs").join(fixture::ID))
            .unwrap()
            .count(),
        1
    );
}

struct Lift;
impl ElevatedRunner for Lift {
    fn run(
        &self,
        plugin: &str,
        tool: &str,
        op: &str,
        _args: Value,
        timeout: Duration,
    ) -> Result<Value, Fail> {
        Ok(json!({"plugin": plugin, "tool": tool, "op": op, "timeout": timeout.as_secs()}))
    }
}

#[test]
fn elevated_operations_run_only_by_a_name_the_user_allowed() {
    let m = fixture::manifest("2.0", "paddock-plugin-fixture", ELEVATE);
    let elevate = m.permissions[1].permission.clone();
    // Not granted: denied, with the reason the app can show.
    let rig = Rig::with(|c| c.elevated = Some(Arc::new(Lift)));
    let s = rig.supervisor(ELEVATE, Grants::none());
    let denied = failed(tool(&s, "lift", json!({"op": "collect"})));
    assert_eq!(denied.reason, Some(FailureReason::Denied));
    assert!(
        denied
            .message
            .contains("may not run \"collect\" with administrator rights")
    );
    // Granted for "collect" only.
    s.set_grants(Grants::none().allow(elevate.clone()));
    let ok = tool(&s, "lift", json!({"op": "collect"})).unwrap();
    assert_eq!(
        ok,
        json!({"plugin": fixture::ID, "tool": fixture::TOOL, "op": "collect", "timeout": 5})
    );
    assert_eq!(
        failed(tool(&s, "lift", json!({"op": "format-disk"}))).reason,
        Some(FailureReason::Denied)
    );
    // The app cannot elevate at all.
    let rig = Rig::new();
    let bare = rig.supervisor(ELEVATE, Grants::none().allow(elevate));
    assert_eq!(
        failed(tool(&bare, "lift", json!({"op": "collect"}))).reason,
        Some(FailureReason::Missing)
    );
}

#[test]
fn switching_off_stops_the_plugin_and_switching_on_lets_it_start_again() {
    let rig = Rig::new();
    let s = rig.supervisor("", Grants::none());
    tool(&s, "echo", json!({"text": "a"})).unwrap();
    s.set_enabled(false);
    assert_eq!(s.status(), Status::Disabled);
    let e = tool(&s, "echo", json!({"text": "a"})).unwrap_err();
    assert!(
        matches!(&e, HostError::Unavailable(m) if m.contains("switched off")),
        "{e}"
    );
    s.set_enabled(true);
    assert_eq!(s.status(), Status::Stopped);
    assert_eq!(
        tool(&s, "echo", json!({"text": "b"})).unwrap(),
        json!({"text": "b"})
    );
    // A quarantined plugin stays quarantined when it is switched on again; only release lifts it.
    s.set_enabled(false);
    for _ in 0..3 {
        rig.quarantine
            .record(fixture::ID, paddock_guard::Failure::Panic, "x");
    }
    s.set_enabled(true);
    assert!(matches!(s.status(), Status::Quarantined { .. }));
    s.release();
    assert_eq!(s.status(), Status::Stopped);
    // Enabling something that is already on changes nothing.
    s.set_enabled(true);
    assert_eq!(s.status(), Status::Stopped);
}
