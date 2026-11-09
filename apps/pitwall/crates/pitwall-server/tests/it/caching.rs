//! Caching end to end over the fake llama-server (ADR 0019): the opt-in response cache (hit, miss, bypass, streaming
//! replay, determinism rules, invalidation, clearing, persistence across a restart), the embeddings cache, slot
//! affinity and the prompt-cache numbers.

use crate::common;
use common::{harness, harness_in, Harness, Reply};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

const TINY: &str = "tiny-q4_k_m";

fn chat(content: &str, temperature: f64) -> Value {
    json!({"model": TINY, "stream": false, "temperature": temperature, "messages": [{"role": "user", "content": content}]})
}

async fn post(h: &Harness, key: &str, body: Value, extra: &[(&str, &str)]) -> Reply {
    h.call("POST", "/v1/chat/completions", Some(key), Some(body), extra).await
}

/// An app that opted into the response cache.
async fn caching_app(h: &Harness, name: &str) -> (String, String) {
    let key = h.make_key(name).await;
    let id = name.to_lowercase().replace(' ', "-");
    let perms =
        json!({"models": null, "maxPriority": "interactive", "tools": false, "extendedApis": true, "cacheResponses": true, "limits": {}});
    assert_eq!(
        h.rpc("set_permissions", json!({"appId": id, "permissions": perms})).await.status,
        200
    );
    (id, key)
}

fn calls(h: &Harness) -> usize {
    h.bodies.lock().unwrap().len()
}

#[tokio::test]
async fn nothing_is_cached_until_an_app_or_preset_opts_in() {
    let h = harness(false).await;
    let key = h.make_key("Plain App").await;
    for _ in 0..2 {
        let r = post(&h, &key, chat("hi", 0.0), &[]).await;
        assert_eq!((r.status, r.header("x-pitwall-cache")), (200, None));
    }
    assert_eq!(calls(&h), 2);
}

#[tokio::test]
async fn an_opted_in_app_gets_the_stored_answer_for_an_identical_deterministic_request() {
    let h = harness(false).await;
    let (id, key) = caching_app(&h, "Cache Probe").await;
    let first = post(&h, &key, chat("hi", 0.0), &[]).await;
    assert_eq!((first.status, first.header("x-pitwall-cache")), (200, Some("miss")));
    let second = post(&h, &key, chat("hi", 0.0), &[]).await;
    assert_eq!(second.header("x-pitwall-cache"), Some("hit"));
    assert_eq!(second.header("x-pitwall-engine"), Some("cache"));
    assert_eq!(
        first.json()["choices"][0]["message"]["content"],
        second.json()["choices"][0]["message"]["content"]
    );
    assert_eq!(second.json()["usage"], first.json()["usage"]);
    assert_eq!(calls(&h), 1, "the engine ran once");

    // Anything that changes the answer is a miss.
    for body in [
        chat("hi!", 0.0),
        json!({"max_tokens": 5, "temperature": 0, "model": TINY, "messages": [{"role": "user", "content": "hi"}]}),
    ] {
        assert_eq!(post(&h, &key, body, &[]).await.header("x-pitwall-cache"), Some("miss"));
    }
    // Not deterministic: not eligible, no header, never stored.
    for _ in 0..2 {
        assert_eq!(post(&h, &key, chat("hi", 0.7), &[]).await.header("x-pitwall-cache"), None);
    }
    // A fixed seed is deterministic too.
    let seeded = json!({"model": TINY, "seed": 7, "messages": [{"role": "user", "content": "hi"}]});
    assert_eq!(post(&h, &key, seeded.clone(), &[]).await.header("x-pitwall-cache"), Some("miss"));
    assert_eq!(post(&h, &key, seeded, &[]).await.header("x-pitwall-cache"), Some("hit"));
    // Tools to run: never cached.
    let tools = json!({"model": TINY, "temperature": 0, "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function", "function": {"name": "t", "parameters": {"type": "object"}}}]});
    assert_ne!(post(&h, &key, tools, &[]).await.header("x-pitwall-cache"), Some("hit"));

    // The numbers: hits, misses, tokens saved (7 = 5 prompt + 2 generated) and the request records.
    let day = h.rpc("usage_series", json!({"range": "24h", "appId": id})).await.json();
    let t = &day["totals"];
    assert_eq!((t["cacheHits"].as_u64().unwrap(), t["cacheMisses"].as_u64().unwrap()), (2, 4));
    assert_eq!(t["savedTokens"], 14);
    let recent = h.rpc("requests", json!({"limit": 50})).await.json();
    assert!(recent.as_array().unwrap().iter().any(|r| r["cache"] == "hit"));
    assert!(recent.as_array().unwrap().iter().any(|r| r["cache"] == "miss"));
}

#[tokio::test]
async fn cache_control_bypasses_and_a_stream_is_replayed() {
    let h = harness(false).await;
    let (_, key) = caching_app(&h, "Stream Probe").await;
    assert_eq!(post(&h, &key, chat("hi", 0.0), &[]).await.header("x-pitwall-cache"), Some("miss"));
    let bypass = post(&h, &key, chat("hi", 0.0), &[("cache-control", "no-cache")]).await;
    assert_eq!(bypass.header("x-pitwall-cache"), Some("bypass"));
    assert_eq!(calls(&h), 2, "a bypass asks the model");
    let mut streamed = chat("hi", 0.0);
    streamed["stream"] = json!(true);
    let s = post(&h, &key, streamed, &[]).await;
    assert_eq!(s.header("x-pitwall-cache"), Some("hit"));
    assert!(
        s.body.contains("Hello") && s.body.contains(" world") && s.body.contains("[DONE]"),
        "{}",
        s.body
    );
    assert_eq!(calls(&h), 2);
    // A streamed miss is stored for later non-streaming requests.
    let mut other = chat("another", 0.0);
    other["stream"] = json!(true);
    assert_eq!(post(&h, &key, other, &[]).await.header("x-pitwall-cache"), Some("miss"));
    assert_eq!(
        post(&h, &key, chat("another", 0.0), &[]).await.header("x-pitwall-cache"),
        Some("hit")
    );
}

#[tokio::test]
async fn a_preset_can_opt_in_for_apps_that_did_not() {
    let h = harness(false).await;
    let key = h.make_key("Preset Cache").await;
    let d = json!({"alias": "cached-hi", "target": TINY, "temperature": 0.0, "cacheResponses": true, "keepWarm": false});
    assert_eq!(h.rpc("create_preset", json!({"preset": d})).await.status, 200);
    let body = json!({"model": "cached-hi", "messages": [{"role": "user", "content": "hi"}]});
    assert_eq!(post(&h, &key, body.clone(), &[]).await.header("x-pitwall-cache"), Some("miss"));
    assert_eq!(post(&h, &key, body.clone(), &[]).await.header("x-pitwall-cache"), Some("hit"));
    // Changing the preset drops what was cached under it at once, even if the change would not alter the key.
    let d2 = json!({"alias": "cached-hi", "target": TINY, "temperature": 0.0, "cacheResponses": true, "keepWarm": true});
    assert_eq!(
        h.rpc("update_preset", json!({"alias": "cached-hi", "preset": d2})).await.status,
        200
    );
    assert_eq!(post(&h, &key, body, &[]).await.header("x-pitwall-cache"), Some("miss"));
}

#[tokio::test]
async fn clearing_and_removing_a_model_drop_cached_answers() {
    let h = harness(false).await;
    let (id, key) = caching_app(&h, "Clear Probe").await;
    let (_, other_key) = caching_app(&h, "Other Probe").await;
    post(&h, &key, chat("a", 0.0), &[]).await;
    post(&h, &other_key, chat("b", 0.0), &[]).await;
    h.st.caches.flush();
    let info = h.rpc("cache_info", json!({})).await.json();
    assert_eq!(info["responses"]["entries"], 2);
    assert_eq!(info["responses"]["storage"], "disk");
    assert_eq!(info["responses"]["expiresAfterSecs"], 7 * 24 * 3600);
    assert_eq!(info["embeddings"]["sliding"], true);
    let n = h.rpc("clear_cache", json!({"scope": {"kind": "app", "appId": id}})).await.json();
    assert_eq!(n, 1);
    assert_eq!(post(&h, &key, chat("a", 0.0), &[]).await.header("x-pitwall-cache"), Some("miss"));
    assert_eq!(
        post(&h, &other_key, chat("b", 0.0), &[]).await.header("x-pitwall-cache"),
        Some("hit")
    );
    assert_eq!(h.rpc("clear_cache", json!({"scope": {"kind": "all"}})).await.json(), 2);
    assert_eq!(h.rpc("clear_cache", json!({"scope": {"kind": "nonsense"}})).await.status, 400);
    post(&h, &key, chat("a", 0.0), &[]).await;
    assert_eq!(h.rpc("delete_model", json!({"id": TINY})).await.status, 200);
    h.st.caches.flush();
    assert_eq!(h.rpc("cache_info", json!({})).await.json()["responses"]["entries"], 0);
}

#[tokio::test]
async fn answers_survive_a_restart_but_not_a_changed_model_file() {
    let h = harness(false).await;
    let (_, key) = caching_app(&h, "Restart Probe").await;
    post(&h, &key, chat("hi", 0.0), &[]).await;
    h.st.caches.flush();
    let dir = h.dir;
    let h2 = harness_in(dir, false).await;
    let hit = post(&h2, &key, chat("hi", 0.0), &[]).await;
    assert_eq!((hit.status, hit.header("x-pitwall-cache")), (200, Some("hit")), "{}", hit.body);
    assert_eq!(calls(&h2), 0);
    // The model file changed (a new download under the same name): the fingerprint differs, so nothing stale is served.
    let model = h2.dir.path().join("models/o/r/tiny-Q4_K_M.gguf");
    let mut bytes = std::fs::read(&model).unwrap();
    bytes.extend([0u8; 10]);
    std::fs::write(&model, bytes).unwrap();
    assert_eq!(post(&h2, &key, chat("hi", 0.0), &[]).await.header("x-pitwall-cache"), Some("miss"));
}

#[tokio::test]
async fn memory_only_storage_keeps_nothing_on_disk() {
    let h = harness(false).await;
    let (_, key) = caching_app(&h, "Memory Probe").await;
    let mut s = h.rpc("settings", json!({})).await.json();
    s["cache"]["responsesStorage"] = json!("memory");
    assert_eq!(h.rpc("save_settings", json!({"settings": s})).await.status, 200);
    post(&h, &key, chat("hi", 0.0), &[]).await;
    h.st.caches.flush();
    let info = h.rpc("cache_info", json!({})).await.json();
    assert_eq!(
        (info["responses"]["storage"].clone(), info["responses"]["entries"].clone()),
        (json!("memory"), json!(1))
    );
    let dir = h.dir;
    let h2 = harness_in(dir, false).await;
    assert_eq!(post(&h2, &key, chat("hi", 0.0), &[]).await.header("x-pitwall-cache"), Some("miss"));
}

#[tokio::test]
async fn invalid_cache_settings_are_refused_with_the_setting_named() {
    let h = harness(false).await;
    let mut s = h.rpc("settings", json!({})).await.json();
    s["cache"]["responsesMaxMib"] = json!(0);
    let r = h.rpc("save_settings", json!({"settings": s})).await;
    assert_eq!(r.status, 400);
    assert!(r.body.contains("Answers cache"), "{}", r.body);
}

async fn embed(h: &Harness, key: &str, inputs: &[&str], extra: &[(&str, &str)]) -> Reply {
    h.call(
        "POST",
        "/v1/embeddings",
        Some(key),
        Some(json!({"model": TINY, "input": inputs})),
        extra,
    )
    .await
}

#[tokio::test]
async fn embeddings_are_cached_by_default_and_only_missing_inputs_reach_the_model() {
    let h = harness(false).await;
    let key = h.make_key("Embed Probe").await;
    let first = embed(&h, &key, &["alpha"], &[]).await;
    assert_eq!((first.status, first.header("x-pitwall-cache")), (200, Some("miss")));
    let again = embed(&h, &key, &["alpha"], &[]).await;
    assert_eq!(again.header("x-pitwall-cache"), Some("hit"));
    assert_eq!(again.json()["data"], first.json()["data"]);
    assert_eq!(h.embeds.lock().unwrap().len(), 1, "the model ran once");
    let mixed = embed(&h, &key, &["alpha", "bb"], &[]).await;
    assert_eq!(mixed.header("x-pitwall-cache"), Some("miss"));
    assert_eq!(h.embeds.lock().unwrap().last().unwrap(), &vec!["bb".to_string()]);
    let data = mixed.json()["data"].clone();
    assert_eq!(
        (data[0]["embedding"][0].clone(), data[1]["embedding"][0].clone()),
        (json!(5.0), json!(2.0))
    );
    assert_eq!(data[0]["index"], 0);
    assert_eq!(
        embed(&h, &key, &["alpha"], &[("cache-control", "no-cache")])
            .await
            .header("x-pitwall-cache"),
        Some("bypass")
    );
    assert_eq!(h.embeds.lock().unwrap().len(), 3);
    let day = h.rpc("usage_series", json!({"range": "24h", "appId": null})).await.json();
    assert_eq!(day["totals"]["cacheHits"], 1);
    assert!(day["totals"]["savedTokens"].as_u64().unwrap() > 0);
    // Switched off: nothing is looked up or stored, and no header.
    let mut s = h.rpc("settings", json!({})).await.json();
    s["cache"]["embeddings"] = json!(false);
    assert_eq!(h.rpc("save_settings", json!({"settings": s})).await.status, 200);
    assert_eq!(embed(&h, &key, &["alpha"], &[]).await.header("x-pitwall-cache"), None);
    assert_eq!(h.embeds.lock().unwrap().len(), 4);
}

fn slots_of(h: &Harness) -> Vec<Option<u64>> {
    h.bodies.lock().unwrap().iter().map(|b| b["id_slot"].as_u64()).collect()
}

fn convo(system: &str, user: &str) -> Value {
    json!({"model": TINY, "stream": false, "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}]})
}

#[tokio::test]
async fn a_conversation_goes_back_to_the_slot_that_holds_its_prefix() {
    let h = harness(false).await;
    let key = h.make_key("Affinity Probe").await;
    for body in [
        convo("rules A", "q1"),
        convo("rules B", "q1"),
        convo("rules A", "q1"),
        convo("rules B", "q1"),
    ] {
        assert_eq!(post(&h, &key, body, &[]).await.status, 200);
    }
    assert_eq!(slots_of(&h), vec![Some(0), Some(1), Some(0), Some(1)]);
    // A conversation that grows keeps its slot.
    let grown = json!({"model": TINY, "stream": false, "messages": [
        {"role": "system", "content": "rules A"}, {"role": "user", "content": "q1"},
        {"role": "assistant", "content": "a1"}, {"role": "user", "content": "q2"}]});
    post(&h, &key, grown, &[]).await;
    assert_eq!(slots_of(&h).last(), Some(&Some(0)));
    h.st.serving.release_all();
}

#[tokio::test]
async fn affinity_can_be_switched_off() {
    let h = harness(false).await;
    let key = h.make_key("No Affinity").await;
    let mut s = h.rpc("settings", json!({})).await.json();
    s["cache"]["prefixAffinity"] = json!(false);
    assert_eq!(h.rpc("save_settings", json!({"settings": s})).await.status, 200);
    post(&h, &key, convo("rules", "q"), &[]).await;
    assert_eq!(slots_of(&h), vec![None]);
}

#[tokio::test]
async fn reused_prompt_tokens_show_in_the_request_and_the_usage() {
    let h = harness(false).await;
    let key = h.make_key("Prefix Probe").await;
    h.cache_n.store(4, Ordering::SeqCst);
    post(&h, &key, convo("rules", "q"), &[]).await;
    let recent = h.rpc("requests", json!({"limit": 5})).await.json();
    assert_eq!(recent[0]["cachedTokens"], 4);
    assert!(recent[0]["cache"].is_null());
    let day = h.rpc("usage_series", json!({"range": "24h", "appId": null})).await.json();
    assert_eq!(day["totals"]["prefixTokens"], 4);
}

#[tokio::test]
async fn keep_warm_saves_the_prefix_when_the_model_unloads_and_restores_it_on_load() {
    let h = harness(false).await;
    let key = h.make_key("Warm Probe").await;
    let d = json!({"alias": "warm-chat", "target": TINY, "systemPrompt": "You are terse.", "keepWarm": true});
    assert_eq!(h.rpc("create_preset", json!({"preset": d})).await.status, 200);
    let body = json!({"model": "warm-chat", "stream": false, "messages": [{"role": "user", "content": "hi"}]});
    post(&h, &key, body.clone(), &[]).await;
    assert!(h.st.serving.release_all() >= 1);
    common::until("the prefix is saved on unload", || {
        h.slot_calls.lock().unwrap().iter().any(|c| c.contains(":save:"))
    })
    .await;
    let saved = h
        .slot_calls
        .lock()
        .unwrap()
        .iter()
        .find(|c| c.contains(":save:"))
        .cloned()
        .expect("saved on unload");
    assert!(saved.starts_with("0:save:") && saved.contains(".warm-chat."), "{saved}");
    // The fake engine writes no file, so make one as llama-server would; the next load restores it.
    let file = saved
        .split("\"filename\":\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    let path = h.st.dirs.slots().join(&file);
    std::fs::write(&path, b"prefix").unwrap();
    post(&h, &key, body, &[]).await;
    assert!(h
        .slot_calls
        .lock()
        .unwrap()
        .iter()
        .any(|c| c.starts_with("0:restore:") && c.contains(&file)));
    // Deleting the preset drops its files at once.
    assert_eq!(h.rpc("delete_preset", json!({"alias": "warm-chat"})).await.status, 200);
    assert!(!path.exists());
    h.st.serving.release_all();
}

#[tokio::test]
async fn the_clean_up_pass_removes_stale_prefix_files_and_keeps_fresh_answers() {
    let h = harness(false).await;
    let (_, key) = caching_app(&h, "Sweep Probe").await;
    post(&h, &key, chat("hi", 0.0), &[]).await;
    let stale = h.st.dirs.slots().join("stale.bin");
    std::fs::write(&stale, b"x").unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(40 * 24 * 3600);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&stale)
        .unwrap()
        .set_modified(old)
        .unwrap();
    assert_eq!(h.st.sweep_caches(), 1, "the 40-day-old prefix file went; the fresh answer stayed");
    assert!(!stale.exists());
    assert_eq!(post(&h, &key, chat("hi", 0.0), &[]).await.header("x-pitwall-cache"), Some("hit"));
    let info = h.rpc("cache_info", json!({})).await.json();
    assert!(info["nextSweepAt"].as_u64().unwrap() > 0 && info["keepWarmFiles"] == 0);
}
