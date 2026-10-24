#![allow(dead_code)]

use paddock_jobs::FakeClock;
use paddock_log::Redactor;
use paddock_mcp::{AuditLog, Policy, Server, Session, ToolDef, ToolOutput};
use serde_json::{Value, json};
use std::sync::Arc;

pub struct Rig {
    pub server: Arc<Server>,
    pub audit: Arc<AuditLog>,
}

/// A server with two read-only tools (`echo`, `boom`) and an action (`delete`), switched on read-only.
pub fn rig() -> Rig {
    let audit = Arc::new(AuditLog::new(100));
    let server = Server::new(
        "demo",
        "1.2.3",
        Arc::new(FakeClock::at_wall_ms(1_700_000_000_000)),
        audit.clone(),
        Arc::new(Redactor::new()),
    )
    .with_instructions("Ask about demo data.");
    server
        .add_tool(ToolDef::read_only(
            "echo",
            "Returns its text.",
            json!({"type":"object","required":["text"],"properties":{"text":{"type":"string","maxLength":50}},"additionalProperties":false}),
            |_, a| Ok(ToolOutput { text: a["text"].as_str().unwrap_or("").to_string(), structured: Some(json!({"echo": a["text"]})), next_cursor: None }),
        ))
        .unwrap();
    server
        .add_tool(ToolDef::read_only(
            "boom",
            "Panics.",
            json!({"type":"object"}),
            |_, _| panic!("kaboom"),
        ))
        .unwrap();
    server
        .add_tool(ToolDef::action(
            "delete",
            "Deletes things.",
            json!({"type":"object"}),
            |_, _| Ok(ToolOutput::text("deleted")),
        ))
        .unwrap();
    server.set_policy(Policy::read_only());
    Rig {
        server: Arc::new(server),
        audit,
    }
}

/// Sends one request through a session and parses the reply.
pub fn rpc(server: &Server, session: &mut Session, request: Value) -> Value {
    let line = server
        .handle(session, &request.to_string())
        .expect("a request gets an answer");
    assert!(!line.contains('\n'), "one line per message");
    serde_json::from_str(&line).unwrap()
}

/// A session that has completed the handshake.
pub fn ready(server: &Server) -> Session {
    let mut s = Session::new("tester");
    let r = rpc(
        server,
        &mut s,
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}),
    );
    assert!(r["result"].is_object(), "{r}");
    assert!(
        server
            .handle(
                &mut s,
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
            )
            .is_none()
    );
    s
}

pub fn call(server: &Server, s: &mut Session, id: i64, name: &str, args: Value) -> Value {
    rpc(
        server,
        s,
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}}),
    )
}
