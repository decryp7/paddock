//! Engine version pins, rollback and capability gating end to end (ADR 0018) over the fake llama-server and
//! hand-made installed builds: the gate's typed errors in all three API families, selection order, the busy-model
//! rule, persistence and damage.

use crate::common::{harness_in, Harness};
use pitwall_core::runtimes::BuildId;
use serde_json::{json, Value};

const TINY: &str = "tiny-q4_k_m";

/// Installs a build the way the manager leaves it: a folder with `build.json` and the server file.
fn install(h: &Harness, id: &str, tag: &str, self_test: &str) {
    let dir = h.dir.path().join("engines").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    let info = json!({"id": id, "tag": tag, "self_test": self_test, "message": null, "devices": [], "size": 1});
    std::fs::write(dir.join("build.json"), info.to_string()).unwrap();
    std::fs::write(dir.join(if cfg!(windows) { "llama-server.exe" } else { "llama-server" }), b"").unwrap();
}

async fn fresh() -> Harness {
    harness_in(tempfile::tempdir().unwrap(), false).await
}

/// An app key that may use tools and the extended APIs.
async fn key(h: &Harness) -> String {
    let r = h.rpc("create_app", json!({"name": "Gate Probe"})).await;
    let id = r.json()["id"].as_str().unwrap().to_string();
    let k = h.rpc("create_key", json!({"appId": id})).await.json()["secret"]
        .as_str()
        .unwrap()
        .to_string();
    let perms = json!({"models": null, "maxPriority": "interactive", "tools": true, "extendedApis": true, "limits": {}});
    assert_eq!(
        h.rpc("set_permissions", json!({"appId": id, "permissions": perms})).await.status,
        200
    );
    k
}

fn bid(s: &str) -> BuildId {
    BuildId::parse(s).unwrap()
}

fn tool_chat() -> Value {
    json!({"model": TINY, "stream": false, "messages": [{"role": "user", "content": "hi"}],
           "tools": [{"type": "function", "function": {"name": "f", "description": "d", "parameters": {"type": "object", "properties": {}}}}]})
}

async fn state(h: &Harness) -> Value {
    h.rpc("engine_pins", json!({})).await.json()
}

#[tokio::test]
async fn an_old_build_refuses_tools_in_every_api_family_naming_the_build_needed() {
    let h = fresh().await;
    install(&h, "vulkan", "b4000", "passed");
    let key = key(&h).await;

    let r = h.call("POST", "/v1/chat/completions", Some(&key), Some(tool_chat()), &[]).await;
    assert_eq!(r.status, 503, "{}", r.body);
    let e = &r.json()["error"];
    assert_eq!(
        (e["code"].clone(), e["type"].clone()),
        (json!("engine_build_too_old"), json!("server_error"))
    );
    let m = e["message"].as_str().unwrap();
    assert!(
        m.contains("b4599") && m.contains("vulkan b4000") && m.contains("llama.cpp page") && m.contains(TINY),
        "{m}"
    );

    let anthropic = json!({"model": TINY, "max_tokens": 8, "messages": [{"role": "user", "content": "hi"}],
                           "tools": [{"name": "f", "description": "d", "input_schema": {"type": "object", "properties": {}}}]});
    let r = h.call("POST", "/v1/messages", Some(&key), Some(anthropic), &[]).await;
    assert_eq!(r.status, 503, "{}", r.body);
    assert_eq!(r.json()["error"]["type"], "overloaded_error");
    assert!(r.json()["error"]["message"].as_str().unwrap().contains("b4599"));

    let ollama = json!({"model": TINY, "stream": false, "messages": [{"role": "user", "content": "hi"}],
                        "tools": [{"type": "function", "function": {"name": "f", "description": "d", "parameters": {"type": "object", "properties": {}}}}]});
    let r = h.call("POST", "/api/chat", Some(&key), Some(ollama), &[]).await;
    assert_eq!(r.status, 503, "{}", r.body);
    assert!(r.json()["error"].as_str().unwrap().contains("b4599"));

    // The model never loaded: the request was refused before the queue.
    assert_eq!(h.starts.load(std::sync::atomic::Ordering::SeqCst), 0);
    // Plain chat still works on the same build.
    let plain = json!({"model": TINY, "stream": false, "messages": [{"role": "user", "content": "hi"}]});
    assert_eq!(
        h.call("POST", "/v1/chat/completions", Some(&key), Some(plain), &[]).await.status,
        200
    );
    h.st.serving.release_all();
}

#[tokio::test]
async fn json_schema_vision_and_embeddings_are_gated_too() {
    let h = fresh().await;
    install(&h, "cpu", "b1900", "passed");
    let key = key(&h).await;
    let schema = json!({"model": TINY, "messages": [{"role": "user", "content": "hi"}],
        "response_format": {"type": "json_schema", "json_schema": {"name": "o", "schema": {"type": "object"}}}});
    let r = h.call("POST", "/v1/chat/completions", Some(&key), Some(schema), &[]).await;
    assert_eq!(r.status, 503, "{}", r.body);
    assert!(r.json()["error"]["message"].as_str().unwrap().contains("b3782"));

    let png = "data:image/png;base64,iVBORw0KGgo=";
    let vision = json!({"model": TINY, "messages": [{"role": "user", "content": [
        {"type": "text", "text": "what"}, {"type": "image_url", "image_url": {"url": png}}]}]});
    let r = h.call("POST", "/v1/chat/completions", Some(&key), Some(vision), &[]).await;
    assert_eq!(r.status, 503, "{}", r.body);
    assert!(r.json()["error"]["message"].as_str().unwrap().contains("b5331"));

    let emb = h.dir.path().join("models/e/m");
    std::fs::create_dir_all(&emb).unwrap();
    std::fs::write(emb.join("embed-Q8_0.gguf"), crate::common::gguf("bert")).unwrap();
    let r = h
        .call(
            "POST",
            "/v1/embeddings",
            Some(&key),
            Some(json!({"model": "embed-q8_0", "input": "x"})),
            &[],
        )
        .await;
    assert_eq!(r.status, 503, "{}", r.body);
    assert!(r.json()["error"]["message"].as_str().unwrap().contains("embeddings"));
    let r = h
        .call(
            "POST",
            "/api/embed",
            Some(&key),
            Some(json!({"model": "embed-q8_0", "input": "x"})),
            &[],
        )
        .await;
    assert_eq!(r.status, 503, "{}", r.body);
}

#[tokio::test]
async fn a_new_enough_build_and_a_machine_without_builds_serve_everything() {
    let h = fresh().await;
    let key = key(&h).await;
    // No engine installed (the fake engine stands in): the engine's own availability speaks, the gate does not.
    assert_eq!(
        h.call("POST", "/v1/chat/completions", Some(&key), Some(tool_chat()), &[])
            .await
            .status,
        200
    );
    install(&h, "vulkan", "b11669", "passed");
    assert_eq!(
        h.call("POST", "/v1/chat/completions", Some(&key), Some(tool_chat()), &[])
            .await
            .status,
        200
    );
    h.st.serving.release_all();
}

#[tokio::test]
async fn the_model_list_and_capability_selection_leave_out_what_the_build_cannot_serve() {
    let h = fresh().await;
    let key = key(&h).await;
    let emb = h.dir.path().join("models/e/m");
    std::fs::create_dir_all(&emb).unwrap();
    std::fs::write(emb.join("embed-Q8_0.gguf"), crate::common::gguf("bert")).unwrap();
    install(&h, "cpu", "b1900", "passed");
    let models = h.call("GET", "/x/v1/models", Some(&key), None, &[]).await.json();
    let e = models["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "embed-q8_0")
        .unwrap()
        .clone();
    assert_eq!(e["capabilities"], json!([]));
    assert_eq!(e["unavailable"][0]["capability"], "embeddings");
    assert_eq!(e["unavailable"][0]["minBuild"], 2004);
    assert!(e["unavailable"][0]["reason"].as_str().unwrap().contains("llama.cpp page"));
    // Selection by capability finds nothing for embeddings on this build...
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
    // ...and finds the model once a newer build is pinned.
    install(&h, "vulkan", "b11669", "passed");
    assert_eq!(h.rpc("set_global_pin", json!({"buildId": "vulkan"})).await.status, 200);
    let models = h.call("GET", "/x/v1/models", Some(&key), None, &[]).await.json();
    let e = models["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "embed-q8_0")
        .unwrap()
        .clone();
    assert_eq!(
        (e["capabilities"].clone(), e["unavailable"].clone()),
        (json!(["embeddings"]), json!([]))
    );
    let r = h
        .call(
            "POST",
            "/v1/embeddings",
            Some(&key),
            Some(json!({"model": "auto:embeddings", "input": "x"})),
            &[],
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    h.st.serving.release_all();
}

#[tokio::test]
async fn a_per_model_pin_to_an_old_build_gates_that_model_only() {
    let h = fresh().await;
    install(&h, "vulkan", "b11669", "passed");
    install(&h, "cpu", "b4000", "passed");
    let key = key(&h).await;
    assert_eq!(
        h.call("POST", "/v1/chat/completions", Some(&key), Some(tool_chat()), &[])
            .await
            .status,
        200
    );
    h.st.serving.release_all();
    let r = h.rpc("set_model_pin", json!({"modelId": TINY, "buildId": "cpu"})).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        h.call("POST", "/v1/chat/completions", Some(&key), Some(tool_chat()), &[])
            .await
            .status,
        503
    );
    // Removing the pin restores service.
    assert_eq!(h.rpc("set_model_pin", json!({"modelId": TINY, "buildId": null})).await.status, 200);
    assert_eq!(
        h.call("POST", "/v1/chat/completions", Some(&key), Some(tool_chat()), &[])
            .await
            .status,
        200
    );
    h.st.serving.release_all();
}

#[tokio::test]
async fn a_pin_to_a_build_that_is_gone_is_a_typed_error_in_every_family() {
    let h = fresh().await;
    install(&h, "vulkan", "b11669", "passed");
    install(&h, "cpu", "b11669", "passed");
    let key = key(&h).await;
    assert_eq!(h.rpc("set_global_pin", json!({"buildId": "cpu"})).await.status, 200);
    std::fs::remove_dir_all(h.dir.path().join("engines/cpu")).unwrap();
    let s = state(&h).await;
    assert!(
        s["problems"][0].as_str().unwrap().contains("Install it on the llama.cpp page"),
        "{s}"
    );
    let r = h.call("POST", "/v1/chat/completions", Some(&key), Some(tool_chat()), &[]).await;
    assert_eq!(r.status, 503, "{}", r.body);
    assert_eq!(r.json()["error"]["code"], "engine_pin_unavailable");
    assert!(r.json()["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Install it on the llama.cpp page"));
    let r = h
        .call("POST", "/v1/messages", Some(&key), Some(json!({"model": TINY, "max_tokens": 8, "messages": [{"role": "user", "content": "hi"}, ], "tools": [{"name": "f", "input_schema": {"type": "object"}}]})), &[])
        .await;
    assert_eq!(r.status, 503, "{}", r.body);
    assert_eq!(r.json()["error"]["type"], "overloaded_error");
    let r = h.call("POST", "/api/chat", Some(&key), Some(json!({"model": TINY, "stream": false, "messages": [{"role": "user", "content": "hi"}], "tools": [{"type": "function", "function": {"name": "f", "parameters": {"type": "object"}}}]})), &[]).await;
    assert_eq!(r.status, 503, "{}", r.body);
    // Unpinning is the way out.
    assert_eq!(h.rpc("set_global_pin", json!({"buildId": null})).await.status, 200);
    assert_eq!(
        h.call("POST", "/v1/chat/completions", Some(&key), Some(tool_chat()), &[])
            .await
            .status,
        200
    );
    h.st.serving.release_all();
}

#[tokio::test]
async fn pinning_validates_the_build_and_reports_the_state() {
    let h = fresh().await;
    install(&h, "vulkan", "b11669", "passed");
    install(&h, "cpu", "b11000", "failed");
    let r = h.rpc("set_global_pin", json!({"buildId": "metal"})).await;
    assert_eq!(r.status, 404, "{}", r.body);
    assert!(r.body.contains("Install it on the llama.cpp page"));
    let r = h.rpc("set_global_pin", json!({"buildId": "cpu"})).await;
    assert_eq!(r.status, 409, "{}", r.body);
    assert!(r.body.contains("failed its self-test"));
    assert_eq!(h.rpc("set_global_pin", json!({"buildId": "../etc"})).await.status, 400);
    assert_eq!(
        h.rpc("set_model_pin", json!({"modelId": "nope-q4", "buildId": "vulkan"}))
            .await
            .status,
        200
    );
    assert_eq!(
        h.rpc("set_model_pin", json!({"modelId": "Not A Model", "buildId": "vulkan"}))
            .await
            .status,
        400
    );
    let s = state(&h).await;
    assert_eq!(s["global"], Value::Null);
    assert_eq!(s["active"], "vulkan");
    assert_eq!(s["activeSource"], "chosen automatically");
    assert_eq!(s["models"][0]["buildId"], "vulkan");
    assert_eq!(s["models"][0]["tag"], "b11669");
    let r = h.rpc("set_global_pin", json!({"buildId": "vulkan"})).await.json();
    assert_eq!(r["state"]["global"], "vulkan");
    assert_eq!(r["state"]["activeSource"], "pinned for all models");
    assert!(r["message"].as_str().unwrap().contains("pinned for all models"));
}

#[tokio::test]
async fn rollback_goes_to_the_previous_good_build_and_never_to_a_failed_one() {
    let h = fresh().await;
    install(&h, "cuda-12", "b11669", "passed");
    install(&h, "vulkan", "b11669", "passed");
    install(&h, "cpu", "b11669", "failed");
    // Nothing earlier yet.
    let s = state(&h).await;
    assert_eq!((s["rollbackTo"].clone(), s["active"].clone()), (Value::Null, json!("cuda-12")));
    assert!(s["rollbackBlocked"].as_str().unwrap().contains("no earlier"));
    let r = h.rpc("rollback_build", json!({})).await;
    assert_eq!(r.status, 409, "{}", r.body);
    // The user moves from cuda-12 to vulkan: cuda-12 becomes the rollback target.
    h.rpc("select_build", json!({"choice": "vulkan"})).await;
    let s = state(&h).await;
    assert_eq!(s["active"], "vulkan");
    assert_eq!(s["rollbackTo"], json!({"buildId": "cuda-12", "tag": "b11669"}));
    let engines = h.rpc("engines", json!({})).await.json();
    assert_eq!(engines[0]["rollbackTo"], "cuda-12");
    // A build that failed its self-test is never the target, even if it was in use before.
    {
        let mut pins = h.st.llama.pins();
        pins.remember(bid("cpu"));
        h.st.save_pins(&pins).unwrap();
        h.st.llama.set_pins(pins);
    }
    let s = state(&h).await;
    assert_eq!(
        s["rollbackTo"]["buildId"],
        "cpu".replace("cpu", "cuda-12"),
        "cpu failed its self-test: {s}"
    );
    let r = h.rpc("rollback_build", json!({})).await.json();
    assert_eq!(r["state"]["global"], "cuda-12");
    assert_eq!(r["state"]["active"], "cuda-12");
    assert!(r["message"].as_str().unwrap().contains("Rolled back to cuda-12 b11669"));
    // Now vulkan is the way back, and cpu still is not.
    assert_eq!(state(&h).await["rollbackTo"]["buildId"], "vulkan");
}

#[tokio::test]
async fn rollback_with_only_failed_builds_in_history_says_why() {
    let h = fresh().await;
    install(&h, "vulkan", "b11669", "passed");
    install(&h, "cpu", "b11669", "failed");
    let mut pins = h.st.llama.pins();
    pins.remember(bid("cpu"));
    h.st.save_pins(&pins).unwrap();
    h.st.llama.set_pins(pins);
    let r = h.rpc("rollback_build", json!({})).await;
    assert_eq!(r.status, 409, "{}", r.body);
    assert!(r.body.contains("failed their self-test") && r.body.contains("cpu"), "{}", r.body);
    assert_eq!(state(&h).await["global"], Value::Null);
}

#[tokio::test]
async fn a_loaded_model_keeps_its_engine_and_the_change_applies_on_the_next_load() {
    let h = fresh().await;
    install(&h, "cuda-12", "b11669", "passed");
    install(&h, "vulkan", "b11669", "passed");
    let key = key(&h).await;
    let plain = json!({"model": TINY, "stream": false, "messages": [{"role": "user", "content": "hi"}]});
    assert_eq!(
        h.call("POST", "/v1/chat/completions", Some(&key), Some(plain), &[]).await.status,
        200
    );
    // `start` records the build a model runs on; the fake engine does not, so do what it does.
    h.st.llama.note_started(TINY, bid("cuda-12"));
    let r = h.rpc("set_global_pin", json!({"buildId": "vulkan"})).await.json();
    assert_eq!(r["appliesOnNextLoad"], json!([TINY]));
    assert!(r["message"].as_str().unwrap().contains("until they are loaded again"), "{r}");
    // The model was not touched.
    assert!(h.st.serving.is_loaded(&pitwall_core::ids::ModelId::parse(TINY).unwrap()));
    // A pin that changes nothing for it says nothing.
    let r = h.rpc("set_global_pin", json!({"buildId": "cuda-12"})).await.json();
    assert_eq!(r["appliesOnNextLoad"], json!([]));
    h.st.serving.release_all();
}

#[tokio::test]
async fn pins_survive_a_restart_and_a_damaged_file_degrades_to_none() {
    let h = fresh().await;
    install(&h, "vulkan", "b11669", "passed");
    install(&h, "cpu", "b11669", "passed");
    h.rpc("set_model_pin", json!({"modelId": TINY, "buildId": "cpu"})).await;
    h.rpc("set_global_pin", json!({"buildId": "vulkan"})).await;
    let text = std::fs::read_to_string(h.dir.path().join("engine-pins.json")).unwrap();
    assert!(text.contains("\"version\":1"), "{text}");
    assert!(
        h.dir.path().join("engine-pins.json.bak").exists(),
        "the previous file is kept as a backup"
    );
    let (dir, st) = (h.dir, h.st);
    drop(st);

    let again = harness_in(dir, false).await;
    let s = state(&again).await;
    assert_eq!(
        (s["global"].clone(), s["models"][0]["buildId"].clone()),
        (json!("vulkan"), json!("cpu"))
    );

    // A damaged file falls back to the backup (one change older) and says so.
    std::fs::write(again.dir.path().join("engine-pins.json"), "{ not json").unwrap();
    let dir = again.dir;
    drop(again.st);
    let third = harness_in(dir, false).await;
    let s = state(&third).await;
    assert_eq!(
        (s["global"].clone(), s["models"][0]["buildId"].clone()),
        (Value::Null, json!("cpu")),
        "{s}"
    );
    let events = third.rpc("events", json!({"limit": 50})).await.json().to_string();
    assert!(events.contains("engine pins file was damaged"), "{events}");

    // Both damaged: no pins, and the app still serves.
    std::fs::write(third.dir.path().join("engine-pins.json"), "garbage").unwrap();
    std::fs::write(third.dir.path().join("engine-pins.json.bak"), r#"{"version":99}"#).unwrap();
    let dir = third.dir;
    drop(third.st);
    let fourth = harness_in(dir, false).await;
    let s = state(&fourth).await;
    assert_eq!((s["global"].clone(), s["models"].clone()), (Value::Null, json!([])));
    let key = key(&fourth).await;
    assert_eq!(
        fourth
            .call("POST", "/v1/chat/completions", Some(&key), Some(tool_chat()), &[])
            .await
            .status,
        200
    );
    fourth.st.serving.release_all();

    // One damaged entry is skipped, the rest are kept.
    std::fs::write(
        fourth.dir.path().join("engine-pins.json"),
        r#"{"version":1,"global":"../x","models":{"tiny-q4_k_m":"metal","other":"A B"}}"#,
    )
    .unwrap();
    let dir = fourth.dir;
    drop(fourth.st);
    let fifth = harness_in(dir, false).await;
    let s = state(&fifth).await;
    assert_eq!((s["global"].clone(), s["models"].as_array().unwrap().len()), (Value::Null, 1));
    // The pin points at a build that is not installed here: reported, not fatal.
    assert!(s["problems"][0].as_str().unwrap().contains("not installed"), "{s}");
    let events = fifth.rpc("events", json!({"limit": 50})).await.json().to_string();
    assert!(
        events.contains("2 pin(s) were left out") && events.contains("Install it on the llama.cpp page"),
        "{events}"
    );
}
