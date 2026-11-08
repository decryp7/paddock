//! The client side, for hosts such as Pitwall: request builders and typed results over the same types the server uses, the
//! one loopback handshake ([`connect_loopback`], which [`crate::bridge::run`] uses too) and a [`Connection`] that lists and
//! calls tools with ids, timeouts and cancel (ADR 0023 WP-8). A host treats every tool result as untrusted data (ADR 0007
//! rule 8).

mod connect;
mod connection;

pub(crate) use connect::AGAIN;
pub use connect::{
    ConnectError, ConnectLimits, LoopbackLink, SharedStream, connect_loopback,
    connect_loopback_with,
};
pub use connection::{
    Canceller, ClientError, Connection, ConnectionConfig, MAX_LIST_PAGES, PendingCall,
};

use crate::jsonrpc::{Id, notification_line, request_line};
use crate::types::{CallToolResult, InitializeResult, ListToolsResult, PROTOCOL_VERSIONS};
use serde_json::{Value, json};

/// The `initialize` request.
pub fn initialize(id: i64, client_name: &str, client_version: &str) -> String {
    request_line(
        &Id::Num(id),
        "initialize",
        json!({"protocolVersion": PROTOCOL_VERSIONS[0], "capabilities": {}, "clientInfo": {"name": client_name, "version": client_version}}),
    )
}

/// The notification that follows a successful `initialize`.
pub fn initialized() -> String {
    notification_line("notifications/initialized", None)
}

/// `tools/list`, optionally continuing from a cursor.
pub fn list_tools(id: i64, cursor: Option<&str>) -> String {
    request_line(
        &Id::Num(id),
        "tools/list",
        cursor.map_or_else(|| json!({}), |c| json!({"cursor": c})),
    )
}

/// `tools/call`.
pub fn call_tool(id: i64, name: &str, arguments: Value) -> String {
    request_line(
        &Id::Num(id),
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    )
}

/// `ping`.
pub fn ping(id: i64) -> String {
    request_line(&Id::Num(id), "ping", json!({}))
}

/// Reads the result of `initialize`.
pub fn parse_initialize(result: Value) -> Result<InitializeResult, String> {
    serde_json::from_value(result).map_err(|e| format!("not an initialize result: {e}"))
}

/// Reads the result of `tools/list`.
pub fn parse_tools(result: Value) -> Result<ListToolsResult, String> {
    serde_json::from_value(result).map_err(|e| format!("not a tools/list result: {e}"))
}

/// Reads the result of `tools/call`.
pub fn parse_call(result: Value) -> Result<CallToolResult, String> {
    serde_json::from_value(result).map_err(|e| format!("not a tools/call result: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonrpc::{Incoming, parse};

    #[test]
    fn builders_make_well_formed_requests() {
        for (line, method) in [
            (initialize(1, "Pitwall", "1.0"), "initialize"),
            (list_tools(2, None), "tools/list"),
            (list_tools(3, Some("p3")), "tools/list"),
            (call_tool(4, "search", json!({"q": "x"})), "tools/call"),
            (ping(5), "ping"),
        ] {
            assert!(
                matches!(parse(&line).unwrap(), Incoming::Request { method: m, .. } if m == method),
                "{line}"
            );
        }
        assert!(matches!(
            parse(&initialized()).unwrap(),
            Incoming::Notification { .. }
        ));
    }

    #[test]
    fn results_parse_into_the_shared_types_or_say_what_is_wrong() {
        assert!(parse_tools(json!({"tools": []})).unwrap().tools.is_empty());
        assert_eq!(
            parse_call(json!({"content": [{"type": "text", "text": "hi"}]}))
                .unwrap()
                .text(),
            "hi"
        );
        assert_eq!(parse_initialize(json!({"protocolVersion": "2025-06-18", "capabilities": {}, "serverInfo": {"name": "a", "version": "1"}})).unwrap().server_info.name, "a");
        assert!(parse_tools(json!(3)).unwrap_err().contains("tools/list"));
        assert!(parse_call(json!({})).unwrap_err().contains("tools/call"));
        assert!(
            parse_initialize(json!({}))
                .unwrap_err()
                .contains("initialize")
        );
    }
}
