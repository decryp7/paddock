//! End-to-end API tests over a fake llama-server: auth, provenance, the three API families, permissions, pairing,
//! load on first use, release and refusal paths.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::harness;
use serde_json::json;
use std::sync::atomic::Ordering;

fn chat(model: &str, stream: bool) -> serde_json::Value {
    json!({"model": model, "stream": stream, "messages": [{"role": "user", "content": "hi"}]})
}

#[tokio::test]
async fn keys_are_required_and_the_answer_carries_provenance() {
    let h = harness(false).await;
    let r = h
        .call("POST", "/v1/chat/completions", None, Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(r.status, 401);
    assert_eq!(r.json()["error"]["type"], "authentication_error");
    let key = h.make_key("Test App").await;
    let r = h.call("GET", "/v1/models", Some(&key), None, &[]).await;
    assert_eq!(r.json()["data"][0]["id"], "tiny-q4_k_m");

    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.json()["choices"][0]["message"]["content"], "Hello world");
    assert_eq!(r.json()["usage"]["completion_tokens"], 2);
    assert_eq!(r.header("x-pitwall-model"), Some("tiny-q4_k_m"));
    assert_eq!(r.header("x-pitwall-engine"), Some("llama.cpp"));
    assert_eq!(r.header("x-pitwall-chosen-by"), Some("explicit"));
    assert!(r.header("x-pitwall-cold-start-ms").is_some(), "the first request loads the model");
    assert_eq!(h.starts.load(Ordering::SeqCst), 1);

    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert!(r.header("x-pitwall-cold-start-ms").is_none(), "the second finds it hot");
    assert_eq!(h.starts.load(Ordering::SeqCst), 1);
    assert_eq!(h.st.serving.loaded().len(), 1);

    // The request log has metadata and no prompt text.
    let reqs = h.rpc("requests", json!({"limit": 10})).await.json();
    assert_eq!(reqs[0]["endpoint"], "/v1/chat/completions");
    assert_eq!(reqs[0]["hasContent"], false);
    let usage = h.rpc("apps", json!({})).await.json();
    assert_eq!(usage[0]["today"]["requests"], 2);
}

#[tokio::test]
async fn streaming_in_all_three_families() {
    let h = harness(false).await;
    let key = h.make_key("Streamer").await;
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", true)), &[])
        .await;
    assert!(
        r.body.contains("\"content\":\"Hello\"") && r.body.trim_end().ends_with("data: [DONE]"),
        "{}",
        r.body
    );

    let body = json!({"model": "tiny-q4_k_m", "max_tokens": 16, "stream": true, "messages": [{"role": "user", "content": "hi"}]});
    let r = h.call("POST", "/v1/messages", Some(&key), Some(body), &[]).await;
    assert!(
        r.body.contains("event: message_start") && r.body.contains("event: message_stop") && r.body.contains("Hello"),
        "{}",
        r.body
    );

    let r = h.call("POST", "/api/chat", Some(&key), Some(chat("tiny-q4_k_m", true)), &[]).await;
    let lines: Vec<serde_json::Value> = r.body.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines[0]["message"]["content"], "Hello");
    assert_eq!(lines.last().unwrap()["done"], true);

    let r = h
        .call(
            "POST",
            "/v1/messages",
            Some(&key),
            Some(json!({"model": "tiny-q4_k_m", "max_tokens": 16, "messages": [{"role": "user", "content": "hi"}]})),
            &[],
        )
        .await;
    assert_eq!(r.json()["content"][0]["text"], "Hello world");
    let r = h.call("POST", "/api/chat", Some(&key), Some(chat("tiny-q4_k_m", false)), &[]).await;
    assert_eq!(r.json()["message"]["content"], "Hello world");
    let r = h
        .call(
            "POST",
            "/api/generate",
            Some(&key),
            Some(json!({"model": "tiny-q4_k_m", "prompt": "hi", "stream": false})),
            &[],
        )
        .await;
    assert_eq!(r.json()["response"], "Hello world");
    let r = h.call("GET", "/api/tags", Some(&key), None, &[]).await;
    assert_eq!(r.json()["models"][0]["name"], "tiny-q4_k_m");
    let r = h.call("GET", "/api/ps", Some(&key), None, &[]).await;
    assert_eq!(r.json()["models"].as_array().unwrap().len(), 1);
    let r = h
        .call(
            "POST",
            "/v1/responses",
            Some(&key),
            Some(json!({"model": "tiny-q4_k_m", "input": "hi"})),
            &[],
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
}

#[tokio::test]
async fn model_choice_errors_and_auto_selection() {
    let h = harness(false).await;
    let key = h.make_key("Chooser").await;
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("nope", false)), &[])
        .await;
    assert_eq!(r.status, 404);
    assert!(
        r.json()["error"]["message"].as_str().unwrap().contains("tiny-q4_k_m"),
        "lists what is installed"
    );
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("auto:chat,fast", false)), &[])
        .await;
    assert_eq!(r.header("x-pitwall-chosen-by"), Some("capabilities"));
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("auto:telepathy", false)), &[])
        .await;
    assert_eq!(r.status, 400);
    let r = h
        .call("POST", "/x/v1/select", Some(&key), Some(json!({"model": "auto:vision"})), &[])
        .await;
    assert_eq!(r.status, 404, "no vision model installed: {}", r.body);
    let r = h
        .call(
            "POST",
            "/v1/chat/completions",
            Some(&key),
            Some(json!({"messages": [{"role": "user", "content": "hi"}]})),
            &[],
        )
        .await;
    assert_eq!(r.header("x-pitwall-chosen-by"), Some("default"));
    let r = h.call("POST", "/v1/chat/completions", Some(&key), None, &[]).await;
    assert_eq!(r.status, 400);
    let r = h.call("POST", "/v1/audio/transcriptions", Some(&key), None, &[]).await;
    assert_eq!(r.status, 501);
}

#[tokio::test]
async fn permissions_limits_and_priorities() {
    let h = harness(false).await;
    let r = h.rpc("create_app", json!({"name": "Limited"})).await;
    let id = r.json()["id"].as_str().unwrap().to_string();
    let key = h.rpc("create_key", json!({"appId": id})).await.json()["secret"]
        .as_str()
        .unwrap()
        .to_string();
    let perms = json!({"models": ["other-model"], "maxPriority": "background", "tools": false, "extendedApis": false, "limits": {"requestsPerMin": 1}});
    assert_eq!(
        h.rpc("set_permissions", json!({"appId": id, "permissions": perms})).await.status,
        200
    );
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(r.status, 403, "{}", r.body);
    let perms =
        json!({"models": null, "maxPriority": "background", "tools": false, "extendedApis": false, "limits": {"requestsPerMin": 1}});
    h.rpc("set_permissions", json!({"appId": id, "permissions": perms})).await;
    let ok = h
        .call(
            "POST",
            "/v1/chat/completions",
            Some(&key),
            Some(chat("tiny-q4_k_m", false)),
            &[("x-pitwall-priority", "interactive")],
        )
        .await;
    assert_eq!(ok.status, 200, "interactive is capped to background, not refused");
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(r.status, 429);
    assert_eq!(r.header("retry-after"), Some("5"));
    let r = h
        .call("POST", "/x/v1/select", Some(&key), Some(json!({"model": "auto"})), &[])
        .await;
    assert_eq!(r.status, 403, "extended APIs are off for this app");
    let r = h
        .call(
            "POST",
            "/v1/chat/completions",
            Some(&key),
            Some(chat("tiny-q4_k_m", false)),
            &[("x-pitwall-priority", "urgent")],
        )
        .await;
    assert_eq!(r.status, 400);
    // The management API is the admin's alone.
    assert_eq!(h.call("POST", "/mgmt/v1/rpc/apps", Some(&key), None, &[]).await.status, 403);
    assert_eq!(h.call("POST", "/mgmt/v1/rpc/apps", None, None, &[]).await.status, 401);
    // Revoking the key ends access.
    let key_id = h.rpc("apps", json!({})).await.json()[0]["keys"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(h.rpc("revoke_key", json!({"appId": id, "keyId": key_id})).await.status, 200);
    assert_eq!(h.call("GET", "/v1/models", Some(&key), None, &[]).await.status, 401);
}

#[tokio::test]
async fn browsers_and_dns_rebinding_are_refused() {
    let h = harness(false).await;
    let key = h.make_key("Local").await;
    let r = h
        .call("GET", "/v1/models", Some(&key), None, &[("origin", "https://evil.example")])
        .await;
    assert_eq!(r.status, 403);
    let r = h.call("GET", "/v1/models", Some(&key), None, &[("host", "evil.example")]).await;
    assert_eq!(r.status, 403);
    let r = h
        .call("GET", "/v1/models", Some(&key), None, &[("origin", "http://localhost:4200")])
        .await;
    assert_eq!(r.status, 200);
    let r = h.call("GET", "/x/v1/health", None, None, &[("host", "evil.example")]).await;
    assert_eq!(r.status, 403);
    let r = h.call("GET", "/x/v1/health", None, None, &[]).await;
    assert_eq!(r.json()["apiVersion"], 1);
}

#[tokio::test]
async fn pairing_flow_gives_the_key_once() {
    let h = harness(false).await;
    let r = h.call("POST", "/x/v1/pair", None, Some(json!({"name": "Debrief"})), &[]).await;
    assert_eq!(r.status, 202, "{}", r.body);
    let (pid, code) = (
        r.json()["pairingId"].as_str().unwrap().to_string(),
        r.json()["code"].as_str().unwrap().to_string(),
    );
    assert_eq!(code.len(), 6);
    assert_eq!(
        h.call("GET", &format!("/x/v1/pair/{pid}"), None, None, &[]).await.json()["status"],
        "pending"
    );
    let shown = h.rpc("pairings", json!({})).await.json();
    assert_eq!(shown[0]["code"], code);
    h.rpc("approve_pairing", json!({"id": pid, "permissions": {"models": null, "maxPriority": "interactive", "tools": true, "extendedApis": true, "limits": {}}})).await;
    let got = h.call("GET", &format!("/x/v1/pair/{pid}"), None, None, &[]).await.json();
    assert_eq!(got["status"], "approved");
    let key = got["key"].as_str().unwrap().to_string();
    assert_eq!(h.call("GET", "/v1/models", Some(&key), None, &[]).await.status, 200);
    assert!(h.call("GET", &format!("/x/v1/pair/{pid}"), None, None, &[]).await.json()["key"].is_null());
    // Web pages cannot start a pairing.
    let r = h
        .call(
            "POST",
            "/x/v1/pair",
            None,
            Some(json!({"name": "Evil"})),
            &[("origin", "https://evil.example")],
        )
        .await;
    assert_eq!(r.status, 403);
}

#[tokio::test]
async fn engine_failures_are_reported_not_fatal() {
    let h = harness(true).await;
    let key = h.make_key("Unlucky").await;
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(r.status, 500, "{}", r.body);
    assert!(r.json()["error"]["message"].as_str().unwrap().contains("could not load"));
    // The server is still up and the next request tries again.
    assert_eq!(h.call("GET", "/v1/models", Some(&key), None, &[]).await.status, 200);
    let _ = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(h.starts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn pause_release_and_job_queue() {
    let h = harness(false).await;
    let key = h.make_key("Queue").await;
    h.call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(h.rpc("overview", json!({})).await.json()["models"].as_array().unwrap().len(), 1);
    assert_eq!(h.rpc("release_memory", json!({})).await.json(), 1);
    assert!(h.st.serving.loaded().is_empty());
    h.rpc("set_serving", json!({"paused": true})).await;
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(r.status, 503);
    assert!(r.json()["error"]["message"].as_str().unwrap().contains("paused"));
    h.rpc("set_serving", json!({"paused": false})).await;
    assert_eq!(
        h.call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
            .await
            .status,
        200
    );
    assert_eq!(h.call("DELETE", "/x/v1/jobs/9999", Some(&key), None, &[]).await.status, 404);
    // Pin keeps a model through the idle policy.
    h.rpc("pin_model", json!({"id": "tiny-q4_k_m", "pinned": true})).await;
    assert!(h.rpc("local_models", json!({})).await.json()[0]["pinned"].as_bool().unwrap());
}

#[tokio::test]
async fn offline_mode_blocks_every_download_path() {
    let h = harness(false).await;
    let mut ui = h.rpc("settings", json!({})).await.json();
    ui["network"]["offline"] = json!(true);
    assert_eq!(h.rpc("save_settings", json!({"settings": ui})).await.status, 200);
    for (m, a) in [
        ("search_models", json!({"query": "qwen"})),
        ("repo_files", json!({"repo": "a/b"})),
        ("install_build", json!({"buildId": "cpu"})),
    ] {
        let r = h.rpc(m, a).await;
        assert_eq!(r.status, 403, "{m}: {}", r.body);
        assert_eq!(r.json()["code"], "offline");
    }
    // Serving local models still works offline.
    let key = h.make_key("Offline").await;
    assert_eq!(
        h.call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
            .await
            .status,
        200
    );
}

#[tokio::test]
async fn embeddings_need_an_embedding_model() {
    let h = harness(false).await;
    let key = h.make_key("Embedder").await;
    let r = h
        .call(
            "POST",
            "/v1/embeddings",
            Some(&key),
            Some(json!({"model": "auto:embeddings", "input": "x"})),
            &[],
        )
        .await;
    assert_eq!(r.status, 404, "{}", r.body);
    let emb = h.dir.path().join("models/e/m");
    std::fs::create_dir_all(&emb).unwrap();
    std::fs::write(emb.join("embed-Q8_0.gguf"), common::gguf("bert")).unwrap();
    let r = h
        .call(
            "POST",
            "/v1/embeddings",
            Some(&key),
            Some(json!({"model": "embed-q8_0", "input": "x"})),
            &[],
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.json()["data"][0]["embedding"], json!([1.0, 0.25]));
    let r = h
        .call(
            "POST",
            "/api/embed",
            Some(&key),
            Some(json!({"model": "embed-q8_0", "input": "x"})),
            &[],
        )
        .await;
    assert_eq!(r.json()["embeddings"][0], json!([1.0, 0.25]));
}

#[tokio::test]
async fn the_llama_cpp_page_lists_only_llama_cpp_and_offers_to_remove_a_leftover_llamafile() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("engines/pack-llamafile");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("llamafile"), vec![0u8; 2048]).unwrap();
    let h = common::harness_in(dir, false).await;
    let engines = h.rpc("engines", json!({})).await.json();
    let list = engines.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["kind"], "llama.cpp");
    assert_eq!(list[0]["leftovers"][0]["id"], "pack-llamafile");
    assert_eq!(list[0]["leftovers"][0]["sizeBytes"], 2048);
    assert!(old.exists(), "never deleted by itself");
    assert!(h.rpc("packs", json!({})).await.json().get("engines").is_none());
    assert_eq!(h.rpc("remove_leftover", json!({"id": "metal"})).await.status, 404);
    assert_eq!(h.rpc("remove_leftover", json!({"id": "pack-llamafile"})).await.status, 200);
    assert!(!old.exists());
    assert!(h.rpc("engines", json!({})).await.json()[0]["leftovers"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        h.rpc("set_model_engine", json!({"id": "tiny-q4_k_m", "engine": "x"})).await.status,
        501,
        "no per-model engine"
    );
}

#[tokio::test]
async fn recommendations_come_from_the_catalog_packs() {
    let h = harness(false).await;
    let p = h.rpc("packs", json!({"tag": "code"})).await.json();
    let rec = p["recommendations"].as_array().unwrap();
    assert!(rec.iter().all(|r| r["tags"].as_array().unwrap().iter().any(|t| t == "code")));
    assert!(!h.rpc("packs", json!({})).await.json()["recommendations"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn task_and_token_endpoints() {
    let h = harness(false).await;
    let key = h.make_key("Tasker").await;
    let r = h
        .call(
            "POST",
            "/x/v1/tasks/summarize",
            Some(&key),
            Some(json!({"text": "Long text here.", "style": "bullets"})),
            &[],
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.json()["result"], "Hello world");
    assert_eq!(r.json()["template"], "summarize@1");
    assert_eq!(
        h.call("POST", "/x/v1/tasks/nope", Some(&key), Some(json!({})), &[]).await.status,
        404
    );
    assert_eq!(
        h.call("POST", "/x/v1/tasks/summarize", Some(&key), Some(json!({})), &[])
            .await
            .status,
        400
    );
    let r = h
        .call("POST", "/x/v1/tokens/count", Some(&key), Some(json!({"text": "hello there"})), &[])
        .await;
    assert!(r.json()["tokens"].as_u64().unwrap() > 0);
    let r = h
        .call(
            "POST",
            "/x/v1/tokens/truncate",
            Some(&key),
            Some(json!({"text": "word ".repeat(500), "maxTokens": 20, "keep": "middle"})),
            &[],
        )
        .await;
    assert_eq!(r.json()["truncated"], true);
    assert_eq!(
        h.call("POST", "/x/v1/tokens/truncate", Some(&key), Some(json!({"text": "x"})), &[])
            .await
            .status,
        400
    );
}

/// Extends `p` to 4 TiB without using disk: sparse on Unix; on Windows the file is marked sparse first (NTFS).
#[allow(clippy::disallowed_methods)] // a test helper calling the OS tool directly
fn make_sparse_huge(p: &std::path::Path) -> bool {
    #[cfg(windows)]
    if !std::process::Command::new("fsutil")
        .args(["sparse", "setflag"])
        .arg(p)
        .status()
        .is_ok_and(|s| s.success())
    {
        return false;
    }
    std::fs::OpenOptions::new()
        .write(true)
        .open(p)
        .and_then(|f| f.set_len(4 << 40))
        .is_ok()
}

#[tokio::test]
async fn a_model_that_cannot_fit_is_refused_with_a_typed_error_and_nothing_starts() {
    let h = harness(false).await;
    let key = h.make_key("Greedy").await;
    // A sparse 4 TiB "model": parses as GGUF but can never fit in memory.
    let p = h.dir.path().join("models/big/m/huge-Q4_K_M.gguf");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    let mut b = common::gguf("llama");
    b.truncate(70_000);
    std::fs::write(&p, &b).unwrap();
    if !make_sparse_huge(&p) {
        eprintln!("skipped: this volume cannot hold a sparse 4 TiB file");
        return;
    }
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("huge-q4_k_m", false)), &[])
        .await;
    assert_eq!(r.status, 503, "{}", r.body);
    assert!(r.json()["error"]["message"].as_str().unwrap().contains("needs about"), "{}", r.body);
    assert_eq!(h.starts.load(Ordering::SeqCst), 0, "no engine was started for it");
    let events = h.rpc("events", json!({"limit": 5})).await.json();
    assert!(events.as_array().unwrap().iter().any(|e| e["kind"] == "requestFailed"));
    let m = h.rpc("local_models", json!({})).await.json();
    assert!(m
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["id"] == "huge-q4_k_m" && x["fit"]["verdict"] == "wontFit"));
}

#[tokio::test]
async fn a_dead_engine_is_forgotten_and_reloaded() {
    let h = harness(false).await;
    let key = h.make_key("Phoenix").await;
    h.call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(h.st.serving.loaded().len(), 1);
    h.st.serving.release_all();
    h.call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(h.starts.load(Ordering::SeqCst), 2, "reloaded after release");
}

/// A taken port is retried on a fresh port inside one load: it is one model load, however many start attempts it took.
/// (This was the flaky CI count of 3 loads: parallel tests raced for the probed port and the retry was counted as a load.)
#[tokio::test]
async fn a_start_retried_after_a_taken_port_is_still_one_model_load() {
    let h = harness(false).await;
    h.port_taken.store(1, Ordering::SeqCst);
    let key = h.make_key("Racer").await;
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        h.attempts.load(Ordering::SeqCst),
        2,
        "the first attempt lost the port, the retry won it"
    );
    assert_eq!(h.starts.load(Ordering::SeqCst), 1, "one load");
}

/// Regression (a_dead_engine_is_forgotten_and_reloaded counted 3 loads in 2 of 6 runs, 2026-10-25): a parallel test can
/// take the probed port between the fake's own check and its real bind; the bind fails, serving retries on a fresh
/// port, and that retry is still one model load.
#[tokio::test]
async fn a_start_whose_port_is_taken_at_the_real_bind_is_still_one_model_load() {
    let h = harness(false).await;
    h.bind_stolen.store(1, Ordering::SeqCst);
    let key = h.make_key("Racer").await;
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        h.attempts.load(Ordering::SeqCst),
        2,
        "the first bind lost the port, the retry won it"
    );
    assert_eq!(h.starts.load(Ordering::SeqCst), 1, "one load");
}

#[tokio::test]
async fn a_dead_engine_is_forgotten_and_reloaded_despite_a_port_race() {
    let h = harness(false).await;
    let key = h.make_key("Phoenix").await;
    h.port_taken.store(1, Ordering::SeqCst);
    h.call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    h.st.serving.release_all();
    h.port_taken.store(1, Ordering::SeqCst);
    h.call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(h.starts.load(Ordering::SeqCst), 2, "two loads, whatever the port races");
}

#[tokio::test]
async fn benchmarks_are_kept_and_models_report_their_engine() {
    let h = harness(false).await;
    let b = h.rpc("run_benchmark", json!({"modelId": "tiny-q4_k_m"})).await;
    assert_eq!(b.status, 200, "{}", b.body);
    assert!(b.json()["genTps"].as_f64().unwrap() > 0.0);
    assert_eq!(h.rpc("benchmarks", json!({})).await.json().as_array().unwrap().len(), 1);
}

async fn tick_until(h: &common::Harness, what: &str, mut done: impl FnMut(&common::Harness) -> bool) {
    // Generous: loaded CI runners are slow (docs/suite-lessons.md).
    for _ in 0..900 {
        h.st.serving.tick();
        if done(h) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for: {what}");
}

/// CI run 7775: the first start of a freshly built binary on a busy Mac took longer than the child engine's fixed
/// budget of 200 health polls, so the load failed with "did not become healthy" while the engine was only slow. Like
/// the real engine, the child is waited for until it is ready or exits. The child reports itself loading (503) until
/// the test creates its ready file, which the test does only after more polls than that old budget: the verdict does
/// not depend on the host's speed.
#[tokio::test]
async fn a_child_engine_that_is_slow_to_become_ready_is_waited_for() {
    const OLD_BUDGET: usize = 200;
    let dir = tempfile::tempdir().unwrap();
    let ready = dir.path().join("engine-ready");
    let (polls, mut polled) = tokio::sync::watch::channel(0usize);
    let engine = common::ChildEngine {
        ready_when: Some(ready.clone()),
        poll_every: std::time::Duration::from_millis(1),
        polls: Some(polls),
        ..common::ChildEngine::new(common::runnable_server(dir.path()))
    };
    let h = common::harness_with(dir, false, Some(std::sync::Arc::new(engine))).await;
    // The load is awaited in process: an HTTP request would add its own client time budget.
    let id = pitwall_core::ids::ModelId::parse("tiny-q4_k_m").unwrap();
    let load = h.st.serving.ensure(&id);
    tokio::pin!(load);
    let loaded = tokio::select! {
        r = &mut load => panic!("the load ended before the engine was ready: {:?}", r.map(|(_, cold)| cold)),
        _ = polled.wait_for(|n| *n > OLD_BUDGET) => {
            std::fs::write(&ready, b"").unwrap();
            load.await
        }
    };
    let (busy, cold) = loaded.unwrap_or_else(|e| panic!("{e:?}"));
    assert!(cold.is_some(), "the model was loaded by this call");
    drop(busy);
    assert!(h.st.serving.is_loaded(&id));
    h.st.serving.release_all();
}

fn launch_on_a_free_port() -> pitwall_core::llama::Launch {
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    pitwall_core::llama::Launch {
        model: "tiny.gguf".into(),
        mmproj: None,
        alias: "tiny-q4_k_m".into(),
        port,
        context: 4096,
        gpu_layers: None,
        threads: 0,
        parallel: 1,
        kind: pitwall_core::llama::ServeKind::Chat,
        cache_ram_mib: 0,
        slot_save_path: None,
    }
}

/// A child engine whose port was taken exits; that is reported at once with what it printed, and the product's own
/// classification reads it as a taken port on this OS ("Address already in use" on Unix, "Only one usage of each
/// socket address" on Windows).
#[tokio::test]
async fn a_child_engine_that_exits_before_it_answers_is_reported_with_its_reason() {
    use pitwall_server::engines::Engine as _;
    let dir = tempfile::tempdir().unwrap();
    let engine = common::ChildEngine {
        take_first_port: true,
        ..common::ChildEngine::new(common::runnable_server(dir.path()))
    };
    let e = engine
        .start(&launch_on_a_free_port())
        .await
        .err()
        .expect("the port is held, so the child cannot listen");
    assert!(e.to_string().contains("exited"), "{e}");
    assert!(pitwall_server::serving::port_taken(&e), "not read as a taken port: {e}");
}

/// The whole path: the first start finds its port taken, the child exits, and the load retries on a fresh port and
/// succeeds, as one load.
#[tokio::test]
async fn a_load_whose_child_finds_its_port_taken_retries_on_a_fresh_port() {
    let dir = tempfile::tempdir().unwrap();
    let engine = std::sync::Arc::new(common::ChildEngine {
        take_first_port: true,
        ..common::ChildEngine::new(common::runnable_server(dir.path()))
    });
    let ports = engine.ports.clone();
    let h = common::harness_with(dir, false, Some(engine)).await;
    let id = pitwall_core::ids::ModelId::parse("tiny-q4_k_m").unwrap();
    let (busy, cold) = h.st.serving.ensure(&id).await.unwrap_or_else(|e| panic!("{e:?}"));
    assert!(cold.is_some());
    drop(busy);
    let ports = ports.lock().unwrap().clone();
    assert_eq!(ports.len(), 2, "one taken port, one retry: {ports:?}");
    assert_ne!(ports[0], ports[1], "the retry asked for a fresh port");
    h.st.serving.release_all();
}

#[tokio::test]
async fn a_crashing_engine_is_restarted_with_backoff_then_given_up_on_until_retry() {
    let dir = tempfile::tempdir().unwrap();
    let exe = common::runnable_server(dir.path());
    let h = common::harness_with(
        dir,
        false,
        Some(std::sync::Arc::new(common::ChildEngine {
            die_after_ms: 900,
            ..common::ChildEngine::new(exe)
        })),
    )
    .await;
    h.st.serving.set_restart_limits(pitwall_core::backoff::Limits {
        initial: 50,
        max: 100,
        reset_after: 60_000,
        max_crashes: 2,
        window: 600_000,
    });
    let key = h.make_key("Crash Test").await;
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(h.st.serving.issues().is_empty());

    // The process exits: the next tick notices, plans a restart and the model comes back by itself.
    tick_until(&h, "a restart to be planned", |h| !h.st.serving.issues().is_empty()).await;
    let ov = h.rpc("overview", json!({})).await.json();
    assert_eq!(ov["issues"][0]["state"], "restarting");
    assert_eq!(ov["issues"][0]["modelId"], "tiny-q4_k_m");
    tick_until(&h, "the model to be restarted", |h| {
        h.st.serving.issues().is_empty() && h.st.serving.is_loaded(&pitwall_core::ids::ModelId::parse("tiny-q4_k_m").unwrap())
    })
    .await;

    // It keeps dying: after the limit the loop guard stops the restarts and says why.
    tick_until(&h, "the crash loop to be detected", |h| {
        h.st.serving
            .issues()
            .iter()
            .any(|i| i.state == pitwall_core::mgmt::IssueState::Failed)
    })
    .await;
    let ov = h.rpc("overview", json!({})).await.json();
    assert_eq!(ov["issues"][0]["state"], "failed");
    assert!(ov["issues"][0]["message"].as_str().unwrap().contains("Retry"));
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(r.status, 500, "{}", r.body);
    assert!(r.json()["error"]["message"].as_str().unwrap().contains("stopped restarting"));
    h.st.serving.tick();
    assert!(
        !h.st.serving.is_loaded(&pitwall_core::ids::ModelId::parse("tiny-q4_k_m").unwrap()),
        "no more restarts"
    );

    // Retry (loading by hand) clears the guard and tries again.
    assert_eq!(h.rpc("load_model", json!({"id": "tiny-q4_k_m"})).await.status, 200);
    assert!(h.st.serving.issues().is_empty());
    h.st.serving.release_all();
}

#[tokio::test]
async fn an_idle_models_context_is_cleared_before_its_weights_are_released() {
    let h = harness(false).await;
    let key = h.make_key("Context Test").await;
    assert_eq!(h.rpc("apply_preset", json!({"preset": "lowMemory"})).await.status, 200);
    assert_eq!(
        h.rpc("set_model_idle", json!({"id": "tiny-q4_k_m", "idleSecs": 2})).await.status,
        200
    );
    let ask = || h.call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[]);
    assert_eq!(ask().await.status, 200);
    h.st.serving.tick();
    assert_eq!(h.erases.load(Ordering::SeqCst), 0, "not idle long enough");

    // Half the idle time has passed: the context goes, the weights stay.
    tokio::time::sleep(std::time::Duration::from_millis(1300)).await;
    h.st.serving.tick();
    for _ in 0..100 {
        if h.erases.load(Ordering::SeqCst) >= 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let erased = h.erases.load(Ordering::SeqCst);
    assert!(erased >= 1, "the engine's slots were erased");
    assert_eq!(h.st.serving.loaded().len(), 1, "still loaded");
    h.st.serving.tick();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(h.erases.load(Ordering::SeqCst), erased, "only once per idle period");

    // The next request makes it eligible again; past the idle time the weights go too.
    assert_eq!(ask().await.status, 200);
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    h.st.serving.tick();
    assert!(h.st.serving.loaded().is_empty(), "unloaded after the full idle time");
}

#[tokio::test]
async fn request_content_is_kept_only_when_asked_redacted_bounded_and_local() {
    let h = harness(false).await;
    let key = h.make_key("Inspector").await;
    let secret = format!("pw_{}", "Zk9Qx2Lm".repeat(5));
    let body = |text: &str| json!({"model": "tiny-q4_k_m", "messages": [{"role": "user", "content": text}]});

    // Off by default: nothing is kept and the answer says how to turn it on.
    h.call("POST", "/v1/chat/completions", Some(&key), Some(body("first, unkept")), &[])
        .await;
    let list = h.rpc("requests", json!({"limit": 10})).await.json();
    assert_eq!(list[0]["hasContent"], false);
    let id = list[0]["id"].as_str().unwrap().to_string();
    let r = h.rpc("request_content", json!({"id": id})).await;
    assert_eq!(r.status, 404);
    assert!(r.body.contains("Keep request content"), "{}", r.body);

    // On: the prompt and the answer are kept, secrets masked.
    let mut s = h.rpc("settings", json!({})).await.json();
    s["privacy"]["keepRequestContent"] = json!(true);
    assert_eq!(h.rpc("save_settings", json!({"settings": s})).await.status, 200);
    h.call(
        "POST",
        "/v1/chat/completions",
        Some(&key),
        Some(body(&format!("my key is {secret}, summarise"))),
        &[],
    )
    .await;
    let list = h.rpc("requests", json!({"limit": 10})).await.json();
    assert_eq!(list[0]["hasContent"], true);
    let c = h.rpc("request_content", json!({"id": list[0]["id"]})).await.json();
    assert!(c["request"].as_str().unwrap().contains("my key is [redacted], summarise"), "{c}");
    assert!(!c.to_string().contains(&secret));
    assert_eq!(c["response"], "Hello world");

    // Bounded: only the latest 50 are kept.
    for i in 0..55 {
        h.call("POST", "/v1/chat/completions", Some(&key), Some(body(&format!("n{i}"))), &[])
            .await;
    }
    let list = h.rpc("requests", json!({"limit": 500})).await.json();
    let kept = list.as_array().unwrap().iter().filter(|r| r["hasContent"] == true).count();
    assert!(kept > 0);
    assert!(h.st.contents.lock().unwrap().len() <= 50);
    // Unknown ids are not found.
    assert_eq!(h.rpc("request_content", json!({"id": "nope"})).await.status, 404);
}

#[tokio::test]
async fn benchmark_results_survive_a_restart_and_tolerate_damage() {
    let dir = tempfile::tempdir().unwrap();
    let h = common::harness_in(dir, false).await;
    let b = h.rpc("run_benchmark", json!({"modelId": "tiny-q4_k_m"})).await;
    assert_eq!(b.status, 200, "{}", b.body);
    assert!(h.dir.path().join("benchmarks.json").exists(), "written to disk");

    // A new server over the same folder reads it back.
    let dir = h.dir;
    drop(h.st);
    let again = common::harness_in(dir, false).await;
    let list = again.rpc("benchmarks", json!({})).await.json();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["modelId"], "tiny-q4_k_m");

    // A damaged file is tolerated: the backup, then nothing.
    std::fs::write(again.dir.path().join("benchmarks.json"), "{ not json").unwrap();
    let dir = again.dir;
    drop(again.st);
    let third = common::harness_in(dir, false).await;
    assert!(third.rpc("benchmarks", json!({})).await.json().as_array().unwrap().len() <= 1);
}

#[tokio::test]
async fn stopping_serving_unloads_every_model_ends_the_engine_processes_and_resuming_serves_again() {
    let dir = tempfile::tempdir().unwrap();
    let exe = common::runnable_server(dir.path());
    let pids: std::sync::Arc<std::sync::Mutex<Vec<u32>>> = Default::default();
    let engine = common::ChildEngine {
        pids: pids.clone(),
        ..common::ChildEngine::new(exe)
    };
    let h = common::harness_with(dir, false, Some(std::sync::Arc::new(engine))).await;
    let key = h.make_key("Quit Test").await;
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let pid = pids.lock().unwrap()[0];
    assert!(pitwall_server::system::pid_alive(pid));

    let stopped = h.rpc("stop_serving", json!({})).await;
    assert_eq!(stopped.status, 200, "{}", stopped.body);
    assert_eq!(stopped.json()["unloaded"], 1);
    assert!(h.st.serving.loaded().is_empty());
    for _ in 0..100 {
        if !pitwall_server::system::pid_alive(pid) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(!pitwall_server::system::pid_alive(pid), "the engine process ended");
    assert_eq!(h.rpc("overview", json!({})).await.json()["server"]["serving"], "stopped");
    let refused = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(refused.status, 503, "{}", refused.body);

    // Resuming (what opening Pitwall does) serves again, with the model loaded afresh.
    assert_eq!(h.rpc("set_serving", json!({"paused": false})).await.status, 200);
    assert_eq!(h.rpc("overview", json!({})).await.json()["server"]["serving"], "serving");
    let again = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(again.status, 200, "{}", again.body);
    h.st.serving.release_all();
}
