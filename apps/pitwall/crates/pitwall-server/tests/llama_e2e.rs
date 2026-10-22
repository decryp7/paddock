//! A real end-to-end run: installs the pinned CPU llama.cpp build, downloads a tiny model and asks the OpenAI chat
//! endpoint once. Off by default and in CI: it downloads files from the allow-listed hosts (GitHub releases and Hugging
//! Face), which the user asked for by setting `PITWALL_LLAMA_E2E=1`. Everything lives in a temp folder that is removed
//! when the test ends; no engine process outlives it.
mod common;

use serde_json::json;
use std::time::{Duration, Instant};

const REPO: &str = "ggml-org/tiny-llamas";
const FILE: &str = "stories260K.gguf";

fn enabled() -> bool {
    std::env::var("PITWALL_LLAMA_E2E").is_ok_and(|v| v == "1")
}

/// Polls `downloads` until the named job ends; returns its final state or panics after `limit`.
async fn wait_download(h: &common::Harness, name_part: &str, limit: Duration) {
    let start = Instant::now();
    loop {
        let d = h.rpc("downloads", json!({})).await.json();
        if let Some(j) = d.as_array().unwrap().iter().find(|j| j["name"].as_str().is_some_and(|n| n.contains(name_part))) {
            match j["state"].as_str().unwrap() {
                "done" => return,
                "failed" | "cancelled" => panic!("{name_part} did not finish: {j}"),
                _ => {}
            }
        }
        assert!(start.elapsed() < limit, "timed out waiting for {name_part}");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[tokio::test]
async fn the_real_engine_answers_one_chat_request() {
    if !enabled() {
        eprintln!("skipped: set PITWALL_LLAMA_E2E=1 to download llama.cpp and a tiny model and run a real request");
        return;
    }
    let h = common::real_harness(tempfile::tempdir().unwrap()).await;

    // The pinned CPU build where there is one (Apple Silicon ships only Metal): checksummed and self-tested by the installer.
    let engines = h.rpc("engines", json!({})).await.json();
    let llama = engines.as_array().unwrap().iter().find(|e| e["kind"] == "llama.cpp").expect("llama.cpp is listed");
    let catalog = llama["catalog"].as_array().unwrap();
    let build = catalog
        .iter()
        .find(|b| b["id"] == "cpu")
        .or_else(|| catalog.iter().find(|b| b["suggested"] == true))
        .expect("the catalog has a build for this platform");
    let cpu = build["id"].as_str().unwrap().to_string();
    let r = h.rpc("install_build", json!({"buildId": cpu})).await;
    assert_eq!(r.status, 200, "{}", r.body);
    wait_download(&h, &cpu, Duration::from_secs(600)).await;

    let r = h.rpc("download_model", json!({"repo": REPO, "file": FILE, "projector": null})).await;
    assert_eq!(r.status, 200, "{}", r.body);
    wait_download(&h, FILE, Duration::from_secs(600)).await;

    let model = h.rpc("local_models", json!({})).await.json()[0]["id"].as_str().unwrap().to_string();
    let key = h.make_key("Real Engine Test").await;
    let body = json!({"model": model, "messages": [{"role": "user", "content": "Once upon a time"}], "max_tokens": 8, "stream": false});
    let reply = h.call("POST", "/v1/chat/completions", Some(&key), Some(body), &[]).await;
    // Loading a real engine takes a while; the harness waits up to its call timeout, so retry once on a timeout-ish 5xx.
    assert_eq!(reply.status, 200, "{}", reply.body);
    let v = reply.json();
    assert!(v["usage"]["completion_tokens"].as_u64().unwrap() > 0, "{v}");
    assert_eq!(reply.header("x-pitwall-engine"), Some("llama.cpp"));
    h.st.serving.release_all();
}
