//! Model presets and aliases end to end over the fake llama-server (ADR 0014): listing, resolution in the three API
//! families, defaults and the system prompt, provenance, validation, a removed target model, shadowing, and
//! persistence that survives damage.

use crate::common::{harness_in, Harness};
use serde_json::{json, Value};

/// A server with two models: `tiny-q4_k_m` and `big-q4_k_m`.
async fn two_models() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let m = dir.path().join("models/o/r");
    std::fs::create_dir_all(&m).unwrap();
    std::fs::write(m.join("tiny-Q4_K_M.gguf"), crate::common::gguf("llama")).unwrap();
    std::fs::write(m.join("big-Q4_K_M.gguf"), crate::common::gguf("llama")).unwrap();
    harness_in(dir, false).await
}

fn draft(alias: &str, target: &str) -> Value {
    json!({"alias": alias, "target": target})
}

async fn create(h: &Harness, d: Value) -> crate::common::Reply {
    h.rpc("create_preset", json!({"preset": d})).await
}

fn chat(model: &str) -> Value {
    json!({"model": model, "stream": false, "messages": [{"role": "user", "content": "hi"}]})
}

fn last_body(h: &Harness) -> Value {
    h.bodies.lock().unwrap().last().cloned().expect("the engine received a request")
}

#[tokio::test]
async fn an_alias_is_listed_and_works_as_the_model_in_every_family() {
    let h = two_models().await;
    let key = h.make_key("Preset Probe").await;
    let mut d = draft("debrief-explain", "tiny-q4_k_m");
    d["temperature"] = json!(0.3);
    d["maxTokens"] = json!(99);
    d["systemPrompt"] = json!("Explain plainly.");
    let r = create(&h, d).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.json()["alias"], "debrief-explain");

    // Listed next to the models (OpenAI and Ollama), and fetchable by name.
    let ids: Vec<String> = h.call("GET", "/v1/models", Some(&key), None, &[]).await.json()["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();
    assert!(
        ids.contains(&"debrief-explain".to_string()) && ids.contains(&"tiny-q4_k_m".to_string()),
        "{ids:?}"
    );
    let tags = h.call("GET", "/api/tags", Some(&key), None, &[]).await.json();
    assert!(tags["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["name"].as_str().is_some_and(|n| n.starts_with("debrief-explain"))));
    assert_eq!(h.call("GET", "/v1/models/debrief-explain", Some(&key), None, &[]).await.status, 200);
    let ps = h.call("GET", "/api/ps", Some(&key), None, &[]).await.json();
    assert!(!ps.to_string().contains("debrief-explain"), "running models are real models only");

    // OpenAI: provenance names the alias and the model, defaults and the system prompt reach the engine.
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("Debrief-Explain")), &[])
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.header("x-pitwall-alias"), Some("debrief-explain"));
    assert_eq!(r.header("x-pitwall-model"), Some("tiny-q4_k_m"));
    assert_eq!(r.header("x-pitwall-chosen-by"), Some("explicit"));
    let body = last_body(&h);
    assert_eq!(body["messages"][0], json!({"role": "system", "content": "Explain plainly."}));
    assert_eq!(body["messages"].as_array().unwrap().len(), 2);
    assert!((body["temperature"].as_f64().unwrap() - 0.3).abs() < 1e-6);
    assert_eq!(body["max_tokens"], 99);

    // A plain model request carries no alias header.
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m")), &[])
        .await;
    assert_eq!(r.header("x-pitwall-alias"), None);
    assert!(
        last_body(&h)["messages"].as_array().unwrap().len() == 1,
        "no system prompt without the alias"
    );

    // Request values win; the system prompt is added only when the request has none.
    let mut c = chat("debrief-explain");
    c["temperature"] = json!(1.0);
    c["max_tokens"] = json!(5);
    c["messages"] = json!([{"role": "system", "content": "Mine."}, {"role": "user", "content": "hi"}]);
    assert_eq!(h.call("POST", "/v1/chat/completions", Some(&key), Some(c), &[]).await.status, 200);
    let body = last_body(&h);
    assert_eq!(
        (body["temperature"].as_f64().unwrap(), body["max_tokens"].as_u64().unwrap()),
        (1.0, 5)
    );
    assert_eq!(body["messages"][0]["content"], "Mine.");
    assert_eq!(body["messages"].as_array().unwrap().len(), 2);

    // Anthropic and Ollama resolve it too.
    let r = h
        .call(
            "POST",
            "/v1/messages",
            Some(&key),
            Some(json!({"model": "debrief-explain", "max_tokens": 16, "messages": [{"role": "user", "content": "hi"}]})),
            &[],
        )
        .await;
    assert_eq!(r.json()["content"][0]["text"], "Hello world");
    assert_eq!(r.header("x-pitwall-alias"), Some("debrief-explain"));
    let r = h
        .call(
            "POST",
            "/api/chat",
            Some(&key),
            Some(json!({"model": "debrief-explain", "stream": false, "messages": [{"role": "user", "content": "hi"}]})),
            &[],
        )
        .await;
    assert_eq!(r.json()["message"]["content"], "Hello world");
    assert_eq!(
        h.call("POST", "/api/show", Some(&key), Some(json!({"model": "debrief-explain"})), &[])
            .await
            .status,
        200
    );

    // The request log and the inspector's record say which alias was used and what it resolved to.
    let reqs = h.rpc("requests", json!({"limit": 10})).await.json();
    let first_alias = reqs.as_array().unwrap().iter().find(|r| r["alias"] == "debrief-explain").unwrap();
    assert_eq!(first_alias["modelId"], "tiny-q4_k_m");
    assert!(
        reqs.as_array().unwrap().iter().any(|r| r["alias"].is_null()),
        "plain requests have no alias"
    );
}

#[tokio::test]
async fn a_capability_target_chooses_the_model_per_request() {
    let h = two_models().await;
    let key = h.make_key("Auto Probe").await;
    assert_eq!(create(&h, draft("spotter-summarise", "auto:chat,small")).await.status, 200);
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("spotter-summarise")), &[])
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.header("x-pitwall-alias"), Some("spotter-summarise"));
    assert_eq!(r.header("x-pitwall-chosen-by"), Some("capabilities"));
    assert!(r.header("x-pitwall-model").is_some());
    let list = h.rpc("presets", json!({})).await.json();
    assert_eq!(list[0]["status"]["state"], "ready");
    assert_eq!(list[0]["status"]["chosenBy"], "capabilities");
    // No installed model offers vision: a clear typed error, in the family's shape.
    assert_eq!(create(&h, draft("pitcrew-vision", "auto:vision")).await.status, 200);
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("pitcrew-vision")), &[])
        .await;
    assert_eq!(r.status, 404);
    assert_eq!(r.json()["error"]["code"], "preset_target_unavailable");
    assert!(r.json()["error"]["message"].as_str().unwrap().contains("vision"));
    let status = h.rpc("presets", json!({})).await.json();
    let vision = status
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["preset"]["alias"] == "pitcrew-vision")
        .unwrap();
    assert_eq!(vision["status"]["state"], "noMatch");
    let ids = h.call("GET", "/v1/models", Some(&key), None, &[]).await.body;
    assert!(!ids.contains("pitcrew-vision"), "an alias that cannot be served is not offered");
}

#[tokio::test]
async fn context_applies_when_the_model_loads() {
    let h = two_models().await;
    let key = h.make_key("Context Probe").await;
    let mut d = draft("small-context", "tiny-q4_k_m");
    d["context"] = json!(2048);
    assert_eq!(create(&h, d).await.status, 200);
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("small-context")), &[])
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.header("x-pitwall-engine"), Some("llama.cpp"));
    // `--ctx-size` is the total: 2048 tokens for each of the default two slots.
    assert_eq!(*h.contexts.lock().unwrap(), vec![4096]);
    assert_eq!(*h.parallels.lock().unwrap(), vec![2]);
    // The loaded model keeps its context until it is released.
    h.call("POST", "/v1/chat/completions", Some(&key), Some(chat("tiny-q4_k_m")), &[])
        .await;
    assert_eq!(h.contexts.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn invalid_presets_are_refused_with_what_to_fix() {
    let h = two_models().await;
    assert_eq!(create(&h, draft("ok-one", "tiny-q4_k_m")).await.status, 200);
    let refused = |r: crate::common::Reply, status: u16, word: &str| {
        assert_eq!(r.status, status, "{}", r.body);
        let m = r.json()["message"].as_str().unwrap().to_lowercase();
        assert!(m.contains(word), "{m} should mention {word}");
    };
    refused(create(&h, draft("ok-one", "tiny-q4_k_m")).await, 409, "already exists");
    refused(create(&h, draft("tiny-q4_k_m", "big-q4_k_m")).await, 409, "installed model");
    refused(create(&h, draft("new-one", "gone-model")).await, 400, "not installed");
    refused(create(&h, draft("bad name", "tiny-q4_k_m")).await, 400, "valid preset name");
    refused(create(&h, draft("auto", "tiny-q4_k_m")).await, 400, "reserved");
    refused(create(&h, draft("new-one", "auto:telepathy")).await, 400, "capability");
    for (field, value, word) in [
        ("temperature", json!(3.5), "temperature"),
        ("topP", json!(2.0), "top-p"),
        ("maxTokens", json!(0), "max tokens"),
        ("context", json!(10), "context"),
        ("systemPrompt", json!("x".repeat(9000)), "system prompt"),
    ] {
        let mut d = draft("new-one", "tiny-q4_k_m");
        d[field] = value;
        refused(create(&h, d).await, 400, word);
    }
    assert_eq!(h.rpc("create_preset", json!({"preset": {"alias": 5}})).await.status, 400);
    assert_eq!(h.rpc("create_preset", json!({})).await.status, 400);
    // Update and delete.
    refused(
        h.rpc("update_preset", json!({"alias": "nothing", "preset": draft("x", "tiny-q4_k_m")}))
            .await,
        404,
        "no preset",
    );
    refused(h.rpc("delete_preset", json!({"alias": "nothing"})).await, 404, "no preset");
    assert_eq!(create(&h, draft("two", "tiny-q4_k_m")).await.status, 200);
    refused(
        h.rpc("update_preset", json!({"alias": "two", "preset": draft("ok-one", "tiny-q4_k_m")}))
            .await,
        409,
        "already exists",
    );
    let r = h
        .rpc("update_preset", json!({"alias": "two", "preset": draft("three", "big-q4_k_m")}))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    let names: Vec<String> = h
        .rpc("presets", json!({}))
        .await
        .json()
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["preset"]["alias"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["ok-one", "three"]);
    assert_eq!(h.rpc("delete_preset", json!({"alias": "three"})).await.status, 200);
    // The count is capped.
    for i in 0..pitwall_core::presets::MAX_PRESETS - 1 {
        assert_eq!(create(&h, draft(&format!("p{i}"), "tiny-q4_k_m")).await.status, 200, "{i}");
    }
    refused(create(&h, draft("one-too-many", "tiny-q4_k_m")).await, 409, "delete one");
}

#[tokio::test]
async fn a_removed_target_model_gives_a_clear_typed_error_until_the_preset_is_fixed() {
    let h = two_models().await;
    let key = h.make_key("Stale Probe").await;
    assert_eq!(create(&h, draft("pitcrew-triage", "big-q4_k_m")).await.status, 200);
    assert_eq!(
        h.call("POST", "/v1/chat/completions", Some(&key), Some(chat("pitcrew-triage")), &[])
            .await
            .status,
        200
    );
    std::fs::remove_file(h.dir.path().join("models/o/r/big-Q4_K_M.gguf")).unwrap();

    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("pitcrew-triage")), &[])
        .await;
    assert_eq!(r.status, 404);
    assert_eq!(r.json()["error"]["code"], "preset_target_unavailable");
    let m = r.json()["error"]["message"].as_str().unwrap().to_string();
    assert!(
        m.contains("pitcrew-triage") && m.contains("big-q4_k_m") && m.contains("no longer installed"),
        "{m}"
    );
    let r = h
        .call(
            "POST",
            "/v1/messages",
            Some(&key),
            Some(json!({"model": "pitcrew-triage", "max_tokens": 8, "messages": [{"role": "user", "content": "hi"}]})),
            &[],
        )
        .await;
    assert_eq!(
        (r.status, r.json()["error"]["type"].as_str().unwrap().to_string()),
        (404, "not_found_error".to_string())
    );
    let r = h
        .call(
            "POST",
            "/api/chat",
            Some(&key),
            Some(json!({"model": "pitcrew-triage", "messages": [{"role": "user", "content": "hi"}]})),
            &[],
        )
        .await;
    assert_eq!(r.status, 404);
    assert!(r.json()["error"].as_str().unwrap().contains("no longer installed"));

    let list = h.rpc("presets", json!({})).await.json();
    assert_eq!(list[0]["status"]["state"], "stale");
    assert!(!h
        .call("GET", "/v1/models", Some(&key), None, &[])
        .await
        .body
        .contains("pitcrew-triage"));

    // Pointing it at an installed model fixes it.
    let r = h
        .rpc(
            "update_preset",
            json!({"alias": "pitcrew-triage", "preset": draft("pitcrew-triage", "tiny-q4_k_m")}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        h.call("POST", "/v1/chat/completions", Some(&key), Some(chat("pitcrew-triage")), &[])
            .await
            .status,
        200
    );
}

#[tokio::test]
async fn a_model_that_later_takes_an_alias_name_wins_and_the_preset_is_flagged() {
    let h = two_models().await;
    let key = h.make_key("Shadow Probe").await;
    let mut d = draft("late-model", "tiny-q4_k_m");
    d["systemPrompt"] = json!("From the preset.");
    assert_eq!(create(&h, d).await.status, 200);
    std::fs::write(h.dir.path().join("models/o/r/late-model-Q4_K_M.gguf"), crate::common::gguf("llama")).unwrap();
    // The alias is not the model's id (it has the quant suffix); now install one that is exactly the name.
    std::fs::write(h.dir.path().join("models/o/r/late-model.gguf"), crate::common::gguf("llama")).unwrap();
    let r = h
        .call("POST", "/v1/chat/completions", Some(&key), Some(chat("late-model")), &[])
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.header("x-pitwall-model"), Some("late-model"));
    assert_eq!(r.header("x-pitwall-alias"), None, "the real model answers, not the preset");
    assert_eq!(last_body(&h)["messages"].as_array().unwrap().len(), 1);
    assert_eq!(h.rpc("presets", json!({})).await.json()[0]["status"]["state"], "shadowed");
    let models = h.call("GET", "/v1/models", Some(&key), None, &[]).await.json();
    assert_eq!(
        models["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["id"] == "late-model")
            .count(),
        1
    );
}

#[tokio::test]
async fn the_editor_preview_resolves_a_draft_from_the_engine() {
    let h = two_models().await;
    let p = h
        .rpc(
            "resolve_preset",
            json!({"preset": draft("new-one", "tiny-q4_k_m"), "editing": null}),
        )
        .await
        .json();
    assert_eq!(p["problem"], Value::Null);
    assert_eq!(
        (p["status"]["state"].as_str(), p["status"]["modelId"].as_str()),
        (Some("ready"), Some("tiny-q4_k_m"))
    );
    let p = h
        .rpc("resolve_preset", json!({"preset": draft("new-one", "auto:chat,small")}))
        .await
        .json();
    assert_eq!(p["status"]["chosenBy"], "capabilities");
    // Problems come back in words; a valid target still resolves when only the name is wrong.
    let p = h
        .rpc("resolve_preset", json!({"preset": draft("tiny-q4_k_m", "big-q4_k_m")}))
        .await
        .json();
    assert!(p["problem"].as_str().unwrap().contains("installed model"));
    assert_eq!(p["status"]["state"], "ready");
    let p = h
        .rpc("resolve_preset", json!({"preset": draft("bad name", "big-q4_k_m")}))
        .await
        .json();
    assert!(p["problem"].as_str().unwrap().contains("preset name"));
    assert_eq!(p["status"]["state"], "ready");
    let p = h
        .rpc("resolve_preset", json!({"preset": draft("ok", "auto:nonsense")}))
        .await
        .json();
    assert!(p["problem"].as_str().unwrap().contains("capability") && p["status"].is_null());
    let p = h.rpc("resolve_preset", json!({"preset": draft("ok", "gone-model")})).await.json();
    assert_eq!(p["status"]["state"], "stale");
    assert!(p["problem"].as_str().unwrap().contains("not installed"));
    // A duplicate is reported unless it is the preset being edited.
    assert_eq!(create(&h, draft("taken", "tiny-q4_k_m")).await.status, 200);
    let p = h
        .rpc("resolve_preset", json!({"preset": draft("taken", "tiny-q4_k_m")}))
        .await
        .json();
    assert!(p["problem"].as_str().unwrap().contains("already exists"));
    let p = h
        .rpc(
            "resolve_preset",
            json!({"preset": draft("taken", "tiny-q4_k_m"), "editing": "taken"}),
        )
        .await
        .json();
    assert_eq!(p["problem"], Value::Null);
    // A preview changes nothing.
    assert_eq!(h.rpc("presets", json!({})).await.json().as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn presets_survive_a_restart_and_tolerate_damage() {
    let h = two_models().await;
    assert_eq!(create(&h, draft("first", "tiny-q4_k_m")).await.status, 200);
    assert_eq!(create(&h, draft("second", "big-q4_k_m")).await.status, 200);
    assert!(
        h.dir.path().join("presets.json.bak").exists(),
        "the previous file is kept as a backup"
    );
    let (dir, st) = (h.dir, h.st);
    drop(st);

    let again = harness_in(dir, false).await;
    assert_eq!(
        again.rpc("presets", json!({})).await.json().as_array().unwrap().len(),
        2,
        "read back from disk"
    );

    // A damaged file falls back to the backup (one preset old) and says so.
    std::fs::write(again.dir.path().join("presets.json"), "{ not json").unwrap();
    let dir = again.dir;
    drop(again.st);
    let third = harness_in(dir, false).await;
    let list = third.rpc("presets", json!({})).await.json();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["preset"]["alias"], "first");
    let events = third.rpc("events", json!({"limit": 50})).await.json().to_string();
    assert!(events.contains("presets file was damaged"), "{events}");

    // Both damaged: nothing, and the app still works.
    std::fs::write(third.dir.path().join("presets.json"), "garbage").unwrap();
    std::fs::write(third.dir.path().join("presets.json.bak"), r#"{"version":99,"presets":[]}"#).unwrap();
    let dir = third.dir;
    drop(third.st);
    let fourth = harness_in(dir, false).await;
    assert!(fourth.rpc("presets", json!({})).await.json().as_array().unwrap().is_empty());
    assert_eq!(create(&fourth, draft("fresh", "tiny-q4_k_m")).await.status, 200);

    // One damaged entry is skipped, the rest are kept.
    let text = r#"{"version":1,"presets":[{"alias":"good","target":"tiny-q4_k_m"},{"alias":"bad name","target":"x"},{"alias":"hot","target":"tiny-q4_k_m","temperature":99}]}"#;
    std::fs::write(fourth.dir.path().join("presets.json"), text).unwrap();
    let dir = fourth.dir;
    drop(fourth.st);
    let fifth = harness_in(dir, false).await;
    assert_eq!(fifth.rpc("presets", json!({})).await.json().as_array().unwrap().len(), 1);
    assert!(fifth
        .rpc("events", json!({"limit": 50}))
        .await
        .json()
        .to_string()
        .contains("2 preset(s) were left out"));
}

#[tokio::test]
async fn a_failed_save_changes_nothing() {
    let h = two_models().await;
    assert_eq!(create(&h, draft("keep", "tiny-q4_k_m")).await.status, 200);
    // A folder where the file belongs makes the save fail.
    let path = h.dir.path().join("presets.json");
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let r = create(&h, draft("lost", "tiny-q4_k_m")).await;
    assert_eq!(r.status, 500, "{}", r.body);
    assert!(r.json()["message"].as_str().unwrap().contains("could not be saved"));
    let list = h.rpc("presets", json!({})).await.json();
    assert_eq!(list.as_array().unwrap().len(), 1, "the failed change was not applied");
    assert_eq!(list[0]["preset"]["alias"], "keep");
}

#[tokio::test]
async fn management_calls_need_the_admin_key() {
    let h = two_models().await;
    let key = h.make_key("Not Admin").await;
    let r = h
        .call(
            "POST",
            "/mgmt/v1/rpc/create_preset",
            Some(&key),
            Some(json!({"preset": draft("sneaky", "tiny-q4_k_m")})),
            &[],
        )
        .await;
    assert_eq!(r.status, 403);
    assert!(h.rpc("presets", json!({})).await.json().as_array().unwrap().is_empty());
}
