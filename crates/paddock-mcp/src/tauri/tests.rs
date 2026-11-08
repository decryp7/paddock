//! The adapter on Tauri's mock runtime: no window reaches the desktop, nothing sleeps.

use super::*;
use crate::control::{ErrorCode, MemorySettings};
use crate::{AuditLog, KeyStore, MemoryKeys, Policy, Server, ToolDef, ToolOutput};
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use paddock_jobs::FakeClock;
use paddock_log::Redactor;
use serde_json::{Value, json};
use std::sync::mpsc;

type App = ::tauri::App<MockRuntime>;

fn invoke(w: &WebviewWindow<MockRuntime>, cmd: &str, body: Value) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .unwrap(),
        body: InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(w, request).map(|b| b.deserialize::<Value>().unwrap())
}

fn app() -> (App, WebviewWindow<MockRuntime>, Arc<McpControl>) {
    let app = mock_builder()
        .invoke_handler(invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
        .build()
        .unwrap();
    let control = install(app.handle(), |emit| {
        let log = Arc::new(AuditLog::new(50));
        let sink = McpControl::announcing_sink(log.clone(), emit.clone());
        let server = Arc::new(Server::new(
            "Demo",
            "1",
            Arc::new(FakeClock::at_wall_ms(5)),
            sink,
            Arc::new(Redactor::new()),
        ));
        server
            .add_tool(ToolDef::read_only(
                "look",
                "Looks.",
                json!({"type":"object"}),
                |_, _| Ok(ToolOutput::text("seen")),
            ))
            .unwrap();
        server
            .add_tool(ToolDef::action(
                "change",
                "Changes.",
                json!({"type":"object"}),
                |_, _| Ok(ToolOutput::text("done")),
            ))
            .unwrap();
        Ok::<_, std::convert::Infallible>(Arc::new(McpControl::new(
            server,
            log,
            Arc::new(KeyStore::new()),
            Arc::new(MemorySettings::default()),
            Arc::new(MemoryKeys::default()),
            emit,
            Arc::new(|| 1_700_000_000_000),
        )))
    })
    .unwrap();
    (app, window, control)
}

#[test]
fn the_commands_show_save_issue_and_revoke() {
    let (_app, w, _control) = app();
    let v = invoke(&w, "mcp_get", json!({})).unwrap();
    assert_eq!(v["settings"]["enabled"], false);
    assert_eq!(v["tools"].as_array().unwrap().len(), 2);
    assert_eq!(
        v["tools"][0]["exposed"], false,
        "off until the user turns it on"
    );
    let saved = invoke(&w, "mcp_save", json!({"settings": {"enabled": true, "resources": true, "disabledTools": [], "allowedActions": ["change"]}})).unwrap();
    let exposed: Vec<(String, bool)> = saved["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["name"].as_str().unwrap().into(),
                t["exposed"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        exposed,
        [("change".to_string(), true), ("look".to_string(), true)]
    );
    assert_eq!(invoke(&w, "mcp_get", json!({})).unwrap(), saved);
    let issued = invoke(&w, "mcp_issue_key", json!({"client": "Pitwall"})).unwrap();
    assert!(issued["key"].as_str().unwrap().starts_with("pmk_"));
    let listed = invoke(&w, "mcp_get", json!({})).unwrap();
    assert_eq!(listed["keys"][0]["client"], "Pitwall");
    assert!(
        !listed.to_string().contains(issued["key"].as_str().unwrap()),
        "the key is never listed"
    );
    let revoked = invoke(&w, "mcp_revoke_key", json!({"id": issued["id"]})).unwrap();
    assert_eq!(revoked["keys"][0]["revoked"], true);
}

#[test]
fn a_refused_command_carries_the_code_and_the_message() {
    let (_app, w, _control) = app();
    let e = invoke(
        &w,
        "mcp_save",
        json!({"settings": {"disabledTools": ["nope"]}}),
    )
    .unwrap_err();
    assert_eq!(e["code"], json!(ErrorCode::Invalid));
    assert!(e["message"].as_str().unwrap().contains("nope"));
    let e = invoke(&w, "mcp_issue_key", json!({"client": "  "})).unwrap_err();
    assert_eq!(e["retryable"], false);
}

#[test]
fn audit_changes_arrive_on_the_event_and_the_log_can_be_read_and_cleared() {
    let (app, w, control) = app();
    let (tx, rx) = mpsc::channel();
    app.listen(EVENT, move |e| {
        let _ = tx.send(serde_json::from_str::<Value>(e.payload()).unwrap());
    });
    invoke(&w, "mcp_save", json!({"settings": {"enabled": true}})).unwrap();
    // A call through the server shows up in the audit log and as an event.
    let session = crate::Session::new("Pitwall");
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    control.server_for_tests().handle(&session, init);
    control.server_for_tests().handle(
        &session,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"look"}}"#,
    );
    let event = rx.recv().unwrap();
    assert_eq!(event, json!({"kind": "audit", "count": 1}));
    let rows = invoke(&w, "mcp_audit", json!({})).unwrap();
    assert_eq!(
        (
            rows[0]["tool"].as_str(),
            rows[0]["client"].as_str(),
            rows[0]["outcome"].as_str()
        ),
        (Some("look"), Some("Pitwall"), Some("ok"))
    );
    invoke(&w, "mcp_audit_clear", json!({})).unwrap();
    assert_eq!(rx.recv().unwrap(), json!({"kind": "audit", "count": 0}));
    assert_eq!(invoke(&w, "mcp_audit", json!({})).unwrap(), json!([]));
}

#[test]
fn install_twice_returns_the_managed_control_and_the_table_matches_the_handler() {
    let (app, _w, first) = app();
    let again = install(app.handle(), |_| Err::<Arc<McpControl>, ()>(())).unwrap();
    assert!(Arc::ptr_eq(&first, &again));
    for c in [
        "mcp_get",
        "mcp_save",
        "mcp_issue_key",
        "mcp_revoke_key",
        "mcp_audit",
        "mcp_audit_clear",
    ] {
        assert!(crate::COMMANDS.spec(c).is_some(), "{c}");
    }
    assert_eq!(failed().code, ErrorCode::Failed);
    let _ = Policy::default();
}
