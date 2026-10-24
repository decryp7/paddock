//! The wire contract: every message the server sends has the shape the MCP specification (2025-06-18) gives it. The schemas are
//! written out here from the specification's JSON Schema (only the parts a server fills in) and checked with the crate's own
//! validator, so a change to a shape fails here first.

use crate::common;

use common::*;
use paddock_mcp::schema::validate;
use paddock_mcp::{Policy, ResourceDef, Session, ToolDef, ToolOutput};
use serde_json::{Value, json};

fn envelope() -> Value {
    json!({"type":"object","required":["jsonrpc","id"],"properties":{"jsonrpc":{"enum":["2.0"]},"id":{"type":["string","integer","null"]}}})
}

fn ok_result(reply: &Value, result_schema: Value) {
    validate(&envelope(), reply).unwrap_or_else(|e| panic!("envelope: {e}: {reply}"));
    assert!(reply.get("error").is_none(), "{reply}");
    validate(&result_schema, &reply["result"]).unwrap_or_else(|e| panic!("{e}: {reply}"));
}

fn error_reply(reply: &Value, code: i64) {
    validate(&envelope(), reply).unwrap_or_else(|e| panic!("envelope: {e}: {reply}"));
    validate(&json!({"type":"object","required":["code","message"],"properties":{"code":{"type":"integer"},"message":{"type":"string"}}}), &reply["error"])
        .unwrap_or_else(|e| panic!("{e}: {reply}"));
    assert!(reply.get("result").is_none(), "{reply}");
    assert_eq!(reply["error"]["code"], code, "{reply}");
}

#[test]
fn initialize_result_has_the_specified_shape() {
    let r = rig();
    let s = Session::new("");
    let reply = rpc(
        &r.server,
        &s,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"Pitwall","version":"1"}}}),
    );
    ok_result(
        &reply,
        json!({"type":"object","required":["protocolVersion","capabilities","serverInfo"],"properties":{
            "protocolVersion":{"type":"string"},
            "capabilities":{"type":"object","properties":{"tools":{"type":"object","properties":{"listChanged":{"type":"boolean"}}}}},
            "serverInfo":{"type":"object","required":["name","version"],"properties":{"name":{"type":"string"},"version":{"type":"string"}}},
            "instructions":{"type":"string"}}}),
    );
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(
        reply["result"]["serverInfo"],
        json!({"name":"demo","version":"1.2.3"})
    );
    assert_eq!(
        reply["result"]["capabilities"],
        json!({"tools":{"listChanged":false}}),
        "no resources registered, so none declared"
    );
    assert_eq!(reply["result"]["instructions"], "Ask about demo data.");
}

#[test]
fn an_unknown_protocol_version_is_answered_with_one_we_speak() {
    let r = rig();
    for (asked, want) in [("2024-11-05", "2024-11-05"), ("1999-01-01", "2025-06-18")] {
        let reply = rpc(
            &r.server,
            &Session::new(""),
            json!({"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":asked}}),
        );
        assert_eq!(reply["result"]["protocolVersion"], want);
        assert_eq!(reply["id"], "a", "string ids come back as strings");
    }
}

#[test]
fn tools_list_has_the_specified_shape_and_hides_actions() {
    let r = rig();
    let s = ready(&r.server);
    let reply = rpc(
        &r.server,
        &s,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    );
    ok_result(
        &reply,
        json!({"type":"object","required":["tools"],"properties":{"tools":{"type":"array","items":{"type":"object","required":["name","inputSchema"],"properties":{
            "name":{"type":"string"},"title":{"type":"string"},"description":{"type":"string"},
            "inputSchema":{"type":"object","required":["type"],"properties":{"type":{"enum":["object"]}}},
            "annotations":{"type":"object","properties":{"readOnlyHint":{"type":"boolean"},"destructiveHint":{"type":"boolean"},"openWorldHint":{"type":"boolean"}}}}}},
            "nextCursor":{"type":"string"}}}),
    );
    let names: Vec<&str> = reply["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["boom", "echo"],
        "sorted, and the action is not offered"
    );
    assert_eq!(
        reply["result"]["tools"][1]["annotations"]["readOnlyHint"],
        true
    );
    // With the action allowed it appears, marked as not read-only and destructive.
    r.server
        .set_policy(Policy::read_only().allow_action("delete"));
    let reply = rpc(
        &r.server,
        &s,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}),
    );
    let del = reply["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "delete")
        .cloned()
        .unwrap();
    assert_eq!(
        (
            del["annotations"]["readOnlyHint"].clone(),
            del["annotations"]["destructiveHint"].clone()
        ),
        (json!(false), json!(true))
    );
}

#[test]
fn tools_list_pages_with_cursors() {
    let r = rig();
    for i in 0..5 {
        r.server
            .add_tool(ToolDef::read_only(
                &format!("t{i}"),
                "d",
                json!({"type":"object"}),
                |_, _| Ok(ToolOutput::default()),
            ))
            .unwrap();
    }
    let server = std::sync::Arc::try_unwrap(r.server)
        .ok()
        .unwrap()
        .with_limits(paddock_mcp::Limits {
            page_size: 3,
            ..Default::default()
        });
    let s = ready(&server);
    let (mut seen, mut cursor) = (vec![], None::<String>);
    for id in 10.. {
        let params = cursor
            .as_ref()
            .map_or_else(|| json!({}), |c| json!({"cursor": c}));
        let reply = rpc(
            &server,
            &s,
            json!({"jsonrpc":"2.0","id":id,"method":"tools/list","params":params}),
        );
        seen.extend(
            reply["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_string()),
        );
        match reply["result"]["nextCursor"].as_str() {
            Some(c) => cursor = Some(c.to_string()),
            None => break,
        }
    }
    assert_eq!(seen.len(), 7, "{seen:?}");
    let bad = rpc(
        &server,
        &s,
        json!({"jsonrpc":"2.0","id":99,"method":"tools/list","params":{"cursor":"nonsense"}}),
    );
    error_reply(&bad, -32602);
    let bad = rpc(
        &server,
        &s,
        json!({"jsonrpc":"2.0","id":99,"method":"tools/list","params":{"cursor":5}}),
    );
    error_reply(&bad, -32602);
}

fn call_result_schema() -> Value {
    json!({"type":"object","required":["content"],"properties":{
        "content":{"type":"array","items":{"type":"object","required":["type","text"],"properties":{"type":{"enum":["text"]},"text":{"type":"string"}}}},
        "structuredContent":{"type":"object"},"isError":{"type":"boolean"}}})
}

#[test]
fn tools_call_results_have_the_specified_shape() {
    let r = rig();
    let s = ready(&r.server);
    let ok = call(&r.server, &s, 1, "echo", json!({"text":"hello"}));
    ok_result(&ok, call_result_schema());
    assert_eq!(
        ok["result"]["content"],
        json!([{"type":"text","text":"hello"}])
    );
    assert_eq!(ok["result"]["structuredContent"], json!({"echo":"hello"}));
    assert_eq!(ok["result"]["isError"], false);
    // A wrong argument is a tool error the model can read and fix, not a protocol error.
    let bad = call(&r.server, &s, 2, "echo", json!({"text": 5}));
    ok_result(&bad, call_result_schema());
    assert_eq!(bad["result"]["isError"], true);
    assert!(
        bad["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("arguments.text must be string")
    );
    let missing = call(&r.server, &s, 3, "echo", json!({}));
    assert!(
        missing["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("needs \"text\"")
    );
}

#[test]
fn protocol_errors_use_the_standard_codes() {
    let r = rig();
    let s = ready(&r.server);
    error_reply(&call(&r.server, &s, 1, "nope", json!({})), -32602);
    error_reply(&call(&r.server, &s, 2, "delete", json!({})), -32602); // an action that is not allowed does not exist
    error_reply(
        &rpc(
            &r.server,
            &s,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{}}),
        ),
        -32602,
    );
    error_reply(
        &rpc(
            &r.server,
            &s,
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"echo","arguments":[1]}}),
        ),
        -32602,
    );
    error_reply(
        &rpc(
            &r.server,
            &s,
            json!({"jsonrpc":"2.0","id":5,"method":"prompts/list"}),
        ),
        -32601,
    );
    error_reply(
        &rpc(
            &r.server,
            &s,
            json!({"jsonrpc":"2.0","id":6,"method":"resources/read","params":{}}),
        ),
        -32602,
    );
    for (line, code) in [
        ("{broken", -32700),
        (
            "[{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}]",
            -32600,
        ),
        (
            "{\"jsonrpc\":\"2.0\",\"id\":null,\"method\":\"ping\"}",
            -32600,
        ),
    ] {
        let reply: Value = serde_json::from_str(&r.server.handle(&s, line).unwrap()).unwrap();
        error_reply(&reply, code);
        assert_eq!(reply["id"], Value::Null, "{line}");
    }
}

#[test]
fn requests_need_initialize_first_but_ping_does_not() {
    let r = rig();
    let s = Session::new("c");
    error_reply(
        &rpc(
            &r.server,
            &s,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        ),
        -32600,
    );
    ok_result(
        &rpc(
            &r.server,
            &s,
            json!({"jsonrpc":"2.0","id":2,"method":"ping"}),
        ),
        json!({"type":"object"}),
    );
}

#[test]
fn notifications_and_stray_responses_get_no_answer() {
    let r = rig();
    let s = ready(&r.server);
    for line in [
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
        r#"{"jsonrpc":"2.0","method":"unknown/notification"}"#,
        r#"{"jsonrpc":"2.0","id":5,"result":{}}"#,
    ] {
        assert!(r.server.handle(&s, line).is_none(), "{line}");
    }
}

#[test]
fn resources_have_the_specified_shapes() {
    let r = rig();
    r.server
        .add_resource(
            ResourceDef::new("demo://summary", "Summary", || Ok("all good".into()))
                .description("The summary")
                .mime_type("text/plain"),
        )
        .unwrap();
    r.server
        .add_resource(ResourceDef::new("demo://broken", "Broken", || {
            Err("not ready".into())
        }))
        .unwrap();
    let s = ready(&r.server);
    let list = rpc(
        &r.server,
        &s,
        json!({"jsonrpc":"2.0","id":1,"method":"resources/list"}),
    );
    ok_result(
        &list,
        json!({"type":"object","required":["resources"],"properties":{"resources":{"type":"array","items":{"type":"object","required":["uri","name"],"properties":{
        "uri":{"type":"string"},"name":{"type":"string"},"description":{"type":"string"},"mimeType":{"type":"string"}}}}}}),
    );
    assert_eq!(list["result"]["resources"].as_array().unwrap().len(), 2);
    let read = rpc(
        &r.server,
        &s,
        json!({"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"demo://summary"}}),
    );
    ok_result(
        &read,
        json!({"type":"object","required":["contents"],"properties":{"contents":{"type":"array","items":{"type":"object","required":["uri"],"properties":{
        "uri":{"type":"string"},"mimeType":{"type":"string"},"text":{"type":"string"}}}}}}),
    );
    assert_eq!(
        read["result"]["contents"][0],
        json!({"uri":"demo://summary","mimeType":"text/plain","text":"all good"})
    );
    error_reply(
        &rpc(
            &r.server,
            &s,
            json!({"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"demo://missing"}}),
        ),
        -32002,
    );
    error_reply(
        &rpc(
            &r.server,
            &s,
            json!({"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"demo://broken"}}),
        ),
        -32603,
    );
    ok_result(
        &rpc(
            &r.server,
            &s,
            json!({"jsonrpc":"2.0","id":5,"method":"resources/templates/list"}),
        ),
        json!({"type":"object","required":["resourceTemplates"]}),
    );
    // Resources can be switched off on their own.
    r.server.set_policy(Policy {
        resources: false,
        ..Policy::read_only()
    });
    let list = rpc(
        &r.server,
        &s,
        json!({"jsonrpc":"2.0","id":6,"method":"resources/list"}),
    );
    assert_eq!(list["result"]["resources"], json!([]));
    error_reply(
        &rpc(
            &r.server,
            &s,
            json!({"jsonrpc":"2.0","id":7,"method":"resources/read","params":{"uri":"demo://summary"}}),
        ),
        -32002,
    );
    let init = rpc(
        &r.server,
        &Session::new(""),
        json!({"jsonrpc":"2.0","id":8,"method":"initialize","params":{}}),
    );
    assert_eq!(
        init["result"]["capabilities"]["resources"],
        json!({"subscribe":false,"listChanged":false})
    );
}
