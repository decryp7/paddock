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
    assert_eq!(r.json()["data"][0]["embedding"], json!([0.5, 0.25]));
    let r = h
        .call(
            "POST",
            "/api/embed",
            Some(&key),
            Some(json!({"model": "embed-q8_0", "input": "x"})),
            &[],
        )
        .await;
    assert_eq!(r.json()["embeddings"][0], json!([0.5, 0.25]));
}

#[tokio::test]
async fn an_engine_pack_serves_a_model_through_a_real_child_process() {
    // The pack launches this very binary as an OpenAI-compatible engine (`--fake-engine`).
    let dir = tempfile::tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_pitwall-server");
    let packs = dir.path().join("packs");
    std::fs::create_dir_all(&packs).unwrap();
    std::fs::write(
        packs.join("fake.engine.toml"),
        format!(
            "format = 1\nid = \"fakeoai\"\nname = \"Fake OpenAI engine\"\nversion = \"1\"\n[capabilities]\nchat = true\n[launch]\ncommand = {exe:?}\nargs = [\"--fake-engine\", \"--host\", \"{{host}}\", \"--port\", \"{{port}}\", \"--model\", \"{{model}}\"]\n[health]\npath = \"/health\"\n"
        ),
    )
    .unwrap();
    std::fs::write(packs.join("broken.engine.toml"), "format = 1\nid = \"broken\"\n").unwrap();
    let m = dir.path().join("models/o/r");
    std::fs::create_dir_all(&m).unwrap();
    std::fs::write(m.join("tiny-Q4_K_M.gguf"), common::gguf("llama")).unwrap();
    let h = common::harness_in(dir, false).await;
    let key = h.make_key("Pack User").await;

    // The bad pack is reported, the good one listed, the built-in one too.
    let p = h.rpc("packs", json!({})).await.json();
    assert!(p["problems"][0]["file"] == "broken.engine.toml" && p["problems"][0]["message"].as_str().unwrap().contains("name"));
    let ids: Vec<&str> = p["engines"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"fakeoai") && ids.contains(&"llamafile"));
    let engines = h.rpc("engines", json!({})).await.json();
    let fake = engines
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Fake OpenAI engine")
        .unwrap();
    assert_eq!(fake["availability"]["state"], "available");

    // Route the model to the pack and chat through the real child process.
    assert_eq!(
        h.rpc("set_model_engine", json!({"id": "tiny-q4_k_m", "engine": "nope"}))
            .await
            .status,
        404
    );
    assert_eq!(
        h.rpc("set_model_engine", json!({"id": "tiny-q4_k_m", "engine": "fakeoai"}))
            .await
            .status,
        200
    );
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m", false)), &[])
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.json()["choices"][0]["message"]["content"], "Hello world");
    assert_eq!(h.starts.load(Ordering::SeqCst), 0, "the built-in fake engine was not used");
    let loaded = h.rpc("overview", json!({})).await.json()["models"][0].clone();
    assert_eq!(loaded["engine"], "pack");
    assert_eq!(loaded["build"], "fakeoai");
    assert!(
        h.rpc("unload_model", json!({"id": "tiny-q4_k_m"})).await.status == 200,
        "dropping the engine ends the child"
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
    std::fs::OpenOptions::new().write(true).open(&p).unwrap().set_len(4 << 40).unwrap();
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

#[tokio::test]
async fn benchmarks_are_kept_and_models_report_their_engine() {
    let h = harness(false).await;
    let b = h.rpc("run_benchmark", json!({"modelId": "tiny-q4_k_m"})).await;
    assert_eq!(b.status, 200, "{}", b.body);
    assert!(b.json()["genTps"].as_f64().unwrap() > 0.0);
    assert_eq!(h.rpc("benchmarks", json!({})).await.json().as_array().unwrap().len(), 1);
    let m = h.rpc("local_models", json!({})).await.json();
    assert!(m[0]["engine"].is_null());
}
