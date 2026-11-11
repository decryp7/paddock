//! The Playground stream (`POST /mgmt/v1/chat`, ADR 0023 WP-1): the response starts before a cold model load and labels it, tools
//! reach the engine, tool calls and reasoning reach the client, and the time to first token is the first token's, not the end's.

use crate::common;
use common::{harness, Harness};
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
    h.start_hold.close();
    let mut r = open(&h, request(json!({}))).await;
    assert_eq!(r.status, 200);
    // The first frame arrives while the model has not started: the load is a labelled phase, not a silent wait (ER-18).
    let first = r.chunk().await.unwrap().expect("a first frame");
    let first = frames(&String::from_utf8_lossy(&first));
    assert_eq!(first[0]["kind"], "phase", "{first:?}");
    assert_eq!(first[0]["phase"]["kind"], "loading", "{first:?}");
    assert_eq!(h.starts.load(Ordering::SeqCst), 0, "the model is still loading");
    h.start_hold.release();
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

/// WP-5: an image reaches the model check. The test model has no vision projector, so the stream ends in an error that says so and
/// what to do; before, the server dropped the images and answered the text alone, as if they had been read. A bad image is a
/// `badRequest` in words, never a silent text answer.
#[tokio::test]
async fn images_go_to_the_vision_check_and_a_bad_one_is_refused() {
    let h = harness(false).await;
    let image = json!({"mime": "Png", "data": "iVBORw0KGgo="});
    let messages = json!([{"role": "user", "content": "what is this?", "images": [image]}]);
    let events = all(open(&h, request(json!({"messages": messages}))).await).await;
    let last = events.last().unwrap();
    assert_eq!(last["kind"], "error", "{events:?}");
    let words = last["error"]["message"].as_str().unwrap();
    assert!(words.contains("cannot read images") && words.contains("vision model"), "{words}");
    assert!(h.bodies.lock().unwrap().is_empty(), "no request reached the engine");

    let bad = json!([{"role": "user", "content": "x", "images": [{"mime": "Png", "data": "***"}]}]);
    let events = all(open(&h, request(json!({"messages": bad}))).await).await;
    let last = events.last().unwrap();
    assert_eq!(
        (last["kind"].as_str(), last["error"]["code"].as_str()),
        (Some("error"), Some("badRequest")),
        "{events:?}"
    );
    assert!(last["error"]["message"].as_str().unwrap().contains("not valid base64"), "{last}");
}

#[tokio::test]
async fn time_to_first_token_is_the_first_tokens_not_the_whole_answers() {
    let h = harness(false).await;
    // Warm the model so the cold load is not in the numbers, then make the answer's tail slow.
    all(open(&h, request(json!({}))).await).await;
    // Time is moved by hand: the playground reads the server's clock, so nothing here waits for it.
    let now = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(pitwall_server::paths::now_ms()));
    let clock = now.clone();
    h.st.serving.set_clock(std::sync::Arc::new(move || clock.load(Ordering::SeqCst)));
    h.tail_hold.close();
    let mut r = open(&h, request(json!({}))).await;
    // The first token reaches the client while the rest of the answer is still held back by the engine.
    let mut seen = vec![];
    loop {
        let chunk = r.chunk().await.unwrap().expect("the stream ends only after a first token");
        seen.extend(frames(&String::from_utf8_lossy(&chunk)));
        if seen.iter().any(|f| f["kind"] == "delta") {
            break;
        }
    }
    assert!(!seen.iter().any(|f| f["kind"] == "done"), "the end has not been sent yet: {seen:?}");
    // The answer's tail takes ten seconds more.
    now.fetch_add(10_000, Ordering::SeqCst);
    h.tail_hold.release();
    seen.extend(frames(&String::from_utf8_lossy(&r.bytes(1 << 20).await.unwrap())));
    let done = seen.last().unwrap();
    assert_eq!(done["kind"], "done", "{seen:?}");
    let ttft = done["ttftMs"].as_u64().unwrap();
    assert!(ttft < 10_000, "the first token's time ({ttft} ms) is not the end's: {done}");
}
