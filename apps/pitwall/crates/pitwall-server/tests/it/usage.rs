//! Per-app usage over time and the extended limits, end to end over the fake llama-server: recording, the series and
//! detail calls, 80% warnings, refusals at the boundary, and persistence that survives damage.

use crate::common;
use common::{harness, harness_in, Harness};
use serde_json::{json, Value};

fn chat() -> Value {
    json!({"model": "tiny-q4_k_m", "stream": false, "messages": [{"role": "user", "content": "hi"}]})
}

async fn app_with_limits(h: &Harness, limits: Value) -> (String, String) {
    let id = h.rpc("create_app", json!({"name": "Usage Probe"})).await.json()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let key = h.rpc("create_key", json!({"appId": id})).await.json()["secret"]
        .as_str()
        .unwrap()
        .to_string();
    let perms = json!({"models": null, "maxPriority": "interactive", "tools": false, "extendedApis": false, "limits": limits});
    assert_eq!(
        h.rpc("set_permissions", json!({"appId": id, "permissions": perms})).await.status,
        200
    );
    (id, key)
}

async fn ask(h: &Harness, key: &str) -> common::Reply {
    h.call("POST", "/v1/chat/completions", Some(key), Some(chat()), &[]).await
}

#[tokio::test]
async fn every_request_is_recorded_per_app_with_memory_attributed() {
    let h = harness(false).await;
    let (id, key) = app_with_limits(&h, json!({})).await;
    for _ in 0..3 {
        assert_eq!(ask(&h, &key).await.status, 200);
    }
    let day = h.rpc("usage_series", json!({"range": "24h", "appId": null})).await.json();
    assert_eq!(day["stepMs"], 3_600_000);
    assert_eq!(day["starts"].as_array().unwrap().len(), 24);
    assert_eq!(day["totals"]["requests"], 3);
    assert_eq!(day["apps"][0]["appId"], id);
    assert!(day["totals"]["completionTokens"].as_u64().unwrap() >= 6);
    let footprint = h.st.serving.footprint(&pitwall_core::ids::ModelId::parse("tiny-q4_k_m").unwrap());
    assert_eq!(day["totals"]["peakMemoryBytes"].as_u64().unwrap(), footprint);
    let month = h.rpc("usage_series", json!({"range": "30d", "appId": id})).await.json();
    assert_eq!(
        (month["stepMs"].as_u64().unwrap(), month["totals"]["requests"].as_u64().unwrap()),
        (86_400_000, 3)
    );
    let other = h.rpc("usage_series", json!({"range": "7d", "appId": "nobody"})).await.json();
    assert_eq!(other["totals"]["requests"], 0);
    assert_eq!(other["apps"][0]["points"].as_array().unwrap().len(), 168);
    assert_eq!(h.rpc("usage_series", json!({"range": "1y", "appId": null})).await.status, 400);

    let detail = h.rpc("app_detail", json!({"appId": id})).await.json();
    assert_eq!(detail["app"]["id"], id);
    assert_eq!(detail["month"]["requests"], 3);
    assert_eq!(detail["recent"].as_array().unwrap().len(), 3);
    assert_eq!(detail["recent"][0]["hasContent"], false, "metadata only");
    assert!(detail["limits"].as_array().unwrap().is_empty());
    assert_eq!(h.rpc("app_detail", json!({"appId": "nobody"})).await.status, 404);
}

#[tokio::test]
async fn limits_refuse_at_the_boundary_with_an_explanation_and_warn_at_80_percent() {
    let h = harness(false).await;
    let (id, key) = app_with_limits(&h, json!({"requestsPerDay": 5})).await;
    for i in 0..4 {
        assert_eq!(ask(&h, &key).await.status, 200, "request {i}");
    }
    // 4 of 5 is 80%: a warning is raised once.
    let events = h.rpc("events", json!({"limit": 50})).await.json();
    let warns: Vec<_> = events.as_array().unwrap().iter().filter(|e| e["kind"] == "limitWarning").collect();
    assert_eq!(warns.len(), 1, "{events}");
    assert_eq!(warns[0]["appId"], id);
    assert!(warns[0]["message"].as_str().unwrap().contains("80%"));
    let detail = h.rpc("app_detail", json!({"appId": id})).await.json();
    assert_eq!(detail["limits"][0]["kind"], "requestsPerDay");
    assert_eq!(detail["limits"][0]["state"], "warning");
    assert_eq!(
        (detail["limits"][0]["used"].as_u64(), detail["limits"][0]["limit"].as_u64()),
        (Some(4), Some(5))
    );
    assert_eq!(ask(&h, &key).await.status, 200, "the fifth is the last");
    let r = ask(&h, &key).await;
    assert_eq!(r.status, 429, "{}", r.body);
    assert!(
        r.body.contains("5 requests for today") && r.body.contains("midnight UTC"),
        "{}",
        r.body
    );
    let detail = h.rpc("app_detail", json!({"appId": id})).await.json();
    assert_eq!(detail["limits"][0]["state"], "exceeded");
    let events = h.rpc("events", json!({"limit": 50})).await.json();
    assert_eq!(
        events.as_array().unwrap().iter().filter(|e| e["kind"] == "limitWarning").count(),
        1,
        "once per period"
    );

    // The monthly token budget and the memory share are enforced too; the existing daily token budget still is.
    let (_, key2) = app_with_limits_named(&h, "Month Probe", json!({"tokensPerMonth": 1})).await;
    assert_eq!(ask(&h, &key2).await.status, 200);
    let r = ask(&h, &key2).await;
    assert_eq!(r.status, 429);
    assert!(r.body.contains("this month"), "{}", r.body);
    let (_, key3) = app_with_limits_named(&h, "Day Probe", json!({"tokensPerDay": 1})).await;
    assert_eq!(ask(&h, &key3).await.status, 200);
    assert_eq!(ask(&h, &key3).await.status, 429);
    let (_, key4) = app_with_limits_named(&h, "Memory Probe", json!({"maxMemoryPercent": 100})).await;
    assert_eq!(ask(&h, &key4).await.status, 200, "a model within the share is served");
}

async fn app_with_limits_named(h: &Harness, name: &str, limits: Value) -> (String, String) {
    let id = h.rpc("create_app", json!({"name": name})).await.json()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let key = h.rpc("create_key", json!({"appId": id})).await.json()["secret"]
        .as_str()
        .unwrap()
        .to_string();
    let perms = json!({"models": null, "maxPriority": "interactive", "tools": false, "extendedApis": false, "limits": limits});
    assert_eq!(
        h.rpc("set_permissions", json!({"appId": id, "permissions": perms})).await.status,
        200
    );
    (id, key)
}

#[tokio::test]
async fn invalid_limits_are_refused_with_what_to_fix() {
    let h = harness(false).await;
    let (id, _) = app_with_limits(&h, json!({})).await;
    for (limits, needle) in [
        (json!({"requestsPerDay": 0}), "requests per day"),
        (json!({"tokensPerMonth": 0}), "tokens per month"),
        (json!({"maxMemoryPercent": 0}), "memory share"),
        (json!({"maxMemoryPercent": 101}), "memory share"),
    ] {
        let perms = json!({"models": null, "maxPriority": "interactive", "tools": false, "extendedApis": false, "limits": limits});
        let r = h.rpc("set_permissions", json!({"appId": id, "permissions": perms})).await;
        assert_eq!(r.status, 400);
        assert!(r.body.contains(needle), "{}", r.body);
    }
}

#[tokio::test]
async fn usage_survives_a_restart_and_tolerates_damage() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness_in(dir, false).await;
    let (id, key) = app_with_limits(&h, json!({})).await;
    assert_eq!(ask(&h, &key).await.status, 200);
    h.st.save_usage();
    assert!(h.dir.path().join("usage.json").exists());
    assert_eq!(ask(&h, &key).await.status, 200);
    h.st.save_usage(); // the first file becomes the backup
    assert!(h.dir.path().join("usage.json.bak").exists());
    let (dir, st) = (h.dir, h.st);
    drop(st);

    let again = harness_in(dir, false).await;
    let s = again.rpc("usage_series", json!({"range": "24h", "appId": id})).await.json();
    assert_eq!(s["totals"]["requests"], 2, "read back from disk");

    // A damaged file falls back to the backup (one request old), then to nothing.
    std::fs::write(again.dir.path().join("usage.json"), "{ not json").unwrap();
    let dir = again.dir;
    drop(again.st);
    let third = harness_in(dir, false).await;
    let s = third.rpc("usage_series", json!({"range": "24h", "appId": id})).await.json();
    assert_eq!(s["totals"]["requests"], 1);
    std::fs::write(third.dir.path().join("usage.json"), "garbage").unwrap();
    std::fs::write(third.dir.path().join("usage.json.bak"), "{\"version\":99}").unwrap();
    let dir = third.dir;
    drop(third.st);
    let fourth = harness_in(dir, false).await;
    let s = fourth.rpc("usage_series", json!({"range": "24h", "appId": null})).await.json();
    assert_eq!(s["totals"]["requests"], 0);
    assert!(s["apps"].as_array().unwrap().is_empty());
}
