//! JSON-RPC lines and cursors come from other processes: no input makes the parsers panic (conformance check C36).
use paddock_mcp::client::{parse_call, parse_initialize, parse_tools};
use paddock_mcp::jsonrpc::{parse, parse_response};
use paddock_mcp::page::decode;
use paddock_testkit::assert_never_panics;

#[test]
fn rpc_lines_cursors_and_results_survive_hostile_input() {
    let request = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
    let response = br#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"t","inputSchema":{"type":"object"}}]}}"#;
    assert_never_panics(&[request, response, b"MTI="], |b| {
        let text = String::from_utf8_lossy(b);
        let _ = parse(&text);
        let _ = parse_response(&text);
        let _ = decode(&text);
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(b) {
            let _ = parse_initialize(value.clone());
            let _ = parse_tools(value.clone());
            let _ = parse_call(value);
        }
    });
}
