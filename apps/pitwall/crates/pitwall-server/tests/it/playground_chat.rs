//! The Playground stream (`POST /mgmt/v1/chat`, ADR 0023 WP-1): the response starts before a cold model load and labels it, tools
//! reach the engine, tool calls and reasoning reach the client, and the time to first token is the first token's, not the end's.

use crate::common::{harness, Harness};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::time::Duration;

fn request(extra: Value) -> Value {
    let mut r = json!({
        "modelId": "tiny-q4_k_m",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.2,
        "maxTokens": 64,
    });
    for (k, v) in extra.as_object().unwrap() {
        r[k] = v.clone();
    }
    r
}

async fn open(h: &Harness, body: Value) -> pitwall_loopback::Response {
    let auth = format!("Bearer {}", h.admin);
    h.client
        .send_with(
            "POST",
            "/mgmt/v1/chat",
            Some(serde_json::to_vec(&body).unwrap()),
            &[("authorization", auth.as_str())],
            // The head must come at once: the cold load (held below) is not allowed to delay it.
            Duration::from_secs(5),
        )
        .await
        .unwrap()
}

/// The `data:` frames of a stream, parsed.
fn frames(text: &str) -> Vec<Value> {
    text.lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter_map(|d| serde_json::from_str(d).ok())
        .collect()
}

async fn all(r: pitwall_loopback::Response) -> Vec<Value> {
    frames(&String::from_utf8_lossy(&r.bytes(1 << 20).await.unwrap()))
}

#[tokio::test]
async fn the_stream_starts_and_says_loading_before_a_cold_model_load_finishes() {
    let h = harness(false).await;
    h.start_hold.store(true, Ordering::SeqCst);
    let mut r = open(&h, request(json!({}))).await;
    assert_eq!(r.status, 200);
    // The first frame arrives while the model has not started: the load is a labelled phase, not a silent wait (ER-18).
    let first = r.chunk().await.unwrap().expect("a first frame");
    let first = frames(&String::from_utf8_lossy(&first));
    assert_eq!(first[0]["kind"], "phase", "{first:?}");
    assert_eq!(first[0]["phase"]["kind"], "loading", "{first:?}");
    assert_eq!(h.starts.load(Ordering::SeqCst), 0, "the model is still loading");
    h.start_hold.store(false, Ordering::SeqCst);
    let rest = all(r).await;
    let kinds: Vec<&str> = rest.iter().map(|f| f["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"delta") && kinds.last() == Some(&"done"), "{kinds:?}");
    let generating = rest.iter().position(|f| f["kind"] == "phase" && f["phase"]["kind"] == "generating");
    let first_delta = kinds.iter().position(|k| *k == "delta");
    assert!(generating.is_some() && generating < first_delta, "{kinds:?}");
}

#[tokio::test]
async fn tools_reach_the_engine_and_tool_calls_reach_the_client() {
    let h = harness(false).await;
    h.tool_call.store(true, Ordering::SeqCst);
    let tools = json!([{"name": "cpu_memory", "description": "CPU and memory", "parameters": {"type": "object", "properties": {"top": {"type": "integer"}}}}]);
    let r = open(&h, request(json!({"tools": tools, "toolChoice": {"kind": "required"}}))).await;
    let events = all(r).await;
    let sent = h.bodies.lock().unwrap()[0].clone();
    assert_eq!(sent["tools"][0]["function"]["name"], "cpu_memory", "{sent}");
    assert_eq!(sent["tool_choice"], "required", "{sent}");

    let calls: Vec<&Value> = events.iter().filter(|f| f["kind"] == "toolCall").collect();
    assert!(!calls.is_empty(), "tool calls are not dropped: {events:?}");
    assert_eq!(calls[0]["name"], "cpu_memory");
    assert_eq!(calls[0]["id"], "call_1");
    let args: String = calls.iter().map(|c| c["arguments"].as_str().unwrap_or("")).collect();
    assert_eq!(args, "{\"top\":3}");
    let done = events.last().unwrap();
    assert_eq!(
        (done["kind"].as_str(), done["finish"].as_str()),
        (Some("done"), Some("toolCalls")),
        "{done}"
    );
}

#[tokio::test]
async fn a_tool_result_message_goes_back_to_the_engine_with_its_call() {
    let h = harness(false).await;
    let messages = json!([
        {"role": "user", "content": "how busy is it?"},
        {"role": "assistant", "content": "", "toolCalls": [{"id": "call_1", "name": "cpu_memory", "arguments": "{\"top\":3}"}]},
        {"role": "tool", "content": "cpu 12%", "toolCallId": "call_1"},
    ]);
    let tools = json!([{"name": "cpu_memory", "description": "d", "parameters": {"type": "object"}}]);
    let r = open(&h, request(json!({"messages": messages, "tools": tools}))).await;
    let events = all(r).await;
    assert_eq!(events.last().unwrap()["kind"], "done", "{events:?}");
    let sent = h.bodies.lock().unwrap()[0].clone();
    let m = sent["messages"].as_array().unwrap();
    assert_eq!(m[1]["tool_calls"][0]["id"], "call_1", "{sent}");
    assert_eq!(
        (m[2]["role"].as_str(), m[2]["tool_call_id"].as_str()),
        (Some("tool"), Some("call_1")),
        "{sent}"
    );
}

#[tokio::test]
async fn time_to_first_token_is_the_first_tokens_not_the_whole_answers() {
    let h = harness(false).await;
    // Warm the model so the cold load is not in the numbers, then make the answer's tail slow.
    all(open(&h, request(json!({}))).await).await;
    h.tail_delay_ms.store(400, Ordering::SeqCst);
    let events = all(open(&h, request(json!({}))).await).await;
    let done = events.last().unwrap();
    let ttft = done["ttftMs"].as_u64().unwrap();
    assert!(ttft < 300, "the first token came long before the end, ttft {ttft} ms: {done}");
}
