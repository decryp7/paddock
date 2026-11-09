//! Parallel slots per model end to end over the fake llama-server (ADR 0015): launch arguments, the queue beyond the
//! slots, the fit table, validation, persistence across restarts and damage, and a busy model never being touched.

use crate::common;
use common::{harness_in, Harness};
use pitwall_core::ids::ModelId;
use pitwall_core::memory::System;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

const TINY: &str = "tiny-q4_k_m";

fn chat() -> Value {
    json!({"model": TINY, "stream": false, "messages": [{"role": "user", "content": "hi"}]})
}

fn model() -> ModelId {
    ModelId::parse(TINY).unwrap()
}

async fn fresh() -> Harness {
    harness_in(tempfile::tempdir().unwrap(), false).await
}

async fn set_slots(h: &Harness, slots: Value) -> common::Reply {
    h.rpc("set_model_slots", json!({"id": TINY, "slots": slots})).await
}

#[tokio::test]
async fn the_launch_gets_parallel_and_the_total_context() {
    let h = fresh().await;
    let key = h.make_key("Slots Probe").await;
    assert_eq!(set_slots(&h, json!(4)).await.status, 200);
    assert_eq!(
        h.call("POST", "/v1/chat/completions", Some(&key), Some(chat()), &[]).await.status,
        200
    );
    // 8192 tokens for each of 4 slots: llama-server splits --ctx-size evenly.
    assert_eq!(*h.contexts.lock().unwrap(), vec![8192 * 4]);
    assert_eq!(*h.parallels.lock().unwrap(), vec![4]);
    let loaded = h.rpc("overview", json!({})).await.json();
    assert_eq!(loaded["models"][0]["slots"], 4);
    let models = h.rpc("local_models", json!({})).await.json();
    assert_eq!(
        (
            models[0]["slots"].clone(),
            models[0]["slotsCustom"].clone(),
            models[0]["loadedSlots"].clone()
        ),
        (json!(4), json!(true), json!(4))
    );
    // What one request may hold (the chat's window): the context of one slot, not the trained maximum.
    assert_eq!(models[0]["contextPerSlot"], 8192);
    h.st.serving.release_all();
}

#[tokio::test]
async fn the_default_comes_from_the_settings_and_null_returns_to_it() {
    let h = fresh().await;
    let m = h.rpc("local_models", json!({})).await.json();
    assert_eq!(
        (m[0]["slots"].clone(), m[0]["slotsCustom"].clone(), m[0]["loadedSlots"].clone()),
        (json!(2), json!(false), Value::Null)
    );
    assert_eq!(m[0]["contextPerSlot"], 8192, "not loaded: the context it will be loaded with");
    assert_eq!(set_slots(&h, json!(8)).await.status, 200);
    assert_eq!(h.rpc("local_models", json!({})).await.json()[0]["slots"], 8);
    let back = set_slots(&h, Value::Null).await;
    assert_eq!(back.status, 200);
    assert_eq!(back.json()["slots"], 2);
    assert_eq!(h.rpc("local_models", json!({})).await.json()[0]["slotsCustom"], false);
}

#[tokio::test]
async fn invalid_slots_and_unknown_models_are_refused_with_what_to_fix() {
    let h = fresh().await;
    for bad in [json!(0), json!(17), json!(1000), json!(-1), json!("many"), json!(2.5)] {
        let r = set_slots(&h, bad.clone()).await;
        assert_eq!(r.status, 400, "{bad}: {}", r.body);
    }
    assert!(set_slots(&h, json!(99)).await.body.contains("between 1 and 16"));
    let r = h.rpc("set_model_slots", json!({"id": "nope-q4", "slots": 2})).await;
    assert_eq!(r.status, 404);
    assert_eq!(
        h.rpc("local_models", json!({})).await.json()[0]["slotsCustom"],
        false,
        "nothing was saved"
    );
    // The fit query validates too.
    assert_eq!(h.rpc("model_fit", json!({"id": TINY, "slots": 40})).await.status, 400);
    assert_eq!(h.rpc("model_fit", json!({"id": TINY, "context": 10})).await.status, 400);
    assert_eq!(h.rpc("model_fit", json!({"id": TINY, "context": 99_999_999})).await.status, 400);
    assert_eq!(h.rpc("model_fit", json!({"id": "nope-q4"})).await.status, 404);
}

#[tokio::test]
async fn requests_beyond_the_slots_queue_and_all_are_answered() {
    let h = fresh().await;
    let key = h.make_key("Queue Probe").await;
    h.hold.close();
    assert_eq!(set_slots(&h, json!(1)).await.status, 200);
    let ask = || h.call("POST", "/v1/chat/completions", Some(&key), Some(chat()), &[]);
    // The engine holds its answers until one request runs and the other two wait in the queue.
    let release = async {
        common::until("one request running and two queued", || {
            h.st.serving.queue().1 == 1 && h.st.serving.jobs().len() == 3
        })
        .await;
        common::until("the running request reaches the engine", || h.peak.load(Ordering::SeqCst) == 1).await;
        h.hold.release();
    };
    let (a, b, c, ()) = tokio::join!(ask(), ask(), ask(), release);
    assert_eq!([a.status, b.status, c.status], [200, 200, 200]);
    assert_eq!(
        h.peak.load(Ordering::SeqCst),
        1,
        "one slot: one request at a time, the others waited"
    );

    // Three slots (applied on the next load): three at once.
    h.st.serving.release_all();
    assert_eq!(set_slots(&h, json!(3)).await.status, 200);
    h.peak.store(0, Ordering::SeqCst);
    h.hold.close();
    let release = async {
        common::until("three requests running and one queued", || {
            h.st.serving.queue().1 == 3 && h.st.serving.jobs().len() == 4
        })
        .await;
        common::until("the three reach the engine", || h.peak.load(Ordering::SeqCst) == 3).await;
        h.hold.release();
    };
    let (a, b, c, d, ()) = tokio::join!(ask(), ask(), ask(), ask(), release);
    assert_eq!([a.status, b.status, c.status, d.status], [200; 4]);
    assert_eq!(h.peak.load(Ordering::SeqCst), 3, "three slots run three, the fourth queues");
    h.st.serving.release_all();
}

#[tokio::test]
async fn changing_slots_never_unloads_a_busy_model_and_applies_on_the_next_load() {
    let h = fresh().await;
    let key = h.make_key("Busy Probe").await;
    h.hold.close();
    assert_eq!(set_slots(&h, json!(2)).await.status, 200);
    let ask = h.call("POST", "/v1/chat/completions", Some(&key), Some(chat()), &[]);
    let change = async {
        // Wait until the request is being served (the engine holds its answer).
        common::until("the request is being served", || {
            h.st.serving.loaded().first().is_some_and(|m| m.active_requests > 0)
        })
        .await;
        assert_eq!(h.st.serving.loaded()[0].active_requests, 1, "busy");
        let r = set_slots(&h, json!(8)).await;
        assert_eq!(r.status, 200, "{}", r.body);
        assert_eq!(r.json()["appliesOnNextLoad"], true);
        assert_eq!(r.json()["loadedSlots"], 2);
        // Neither the change nor the tick (which applies the memory policy) touches the busy model.
        h.st.serving.tick();
        assert!(
            !h.st.serving.unload(&model(), pitwall_core::memory::ReleaseReason::Manual),
            "busy models are refused"
        );
        assert!(h.st.serving.is_loaded(&model()));
        assert_eq!(h.rpc("unload_model", json!({"id": TINY})).await.status, 409);
        h.hold.release();
    };
    let (reply, ()) = tokio::join!(ask, change);
    assert_eq!(reply.status, 200);
    assert_eq!(h.starts.load(Ordering::SeqCst), 1, "not restarted");
    assert_eq!(h.st.serving.loaded()[0].slots, 2, "still loaded with the old count");
    assert_eq!(h.st.serving.slots_for(&model()).get(), 2);

    // Reloaded, it uses the new count.
    assert_eq!(h.rpc("unload_model", json!({"id": TINY})).await.status, 200);
    let reloaded = h.call("POST", "/v1/chat/completions", Some(&key), Some(chat()), &[]).await;
    assert_eq!(reloaded.status, 200, "{}", reloaded.body);
    assert_eq!(*h.parallels.lock().unwrap(), vec![2, 8]);
    assert_eq!(h.st.serving.slots_for(&model()).get(), 8);
    h.st.serving.release_all();
}

#[tokio::test]
async fn the_loaded_footprint_reflects_the_slots() {
    let h = fresh().await;
    let key = h.make_key("Footprint Probe").await;
    let ask = || h.call("POST", "/v1/chat/completions", Some(&key), Some(chat()), &[]);
    assert_eq!(set_slots(&h, json!(1)).await.status, 200);
    assert_eq!(ask().await.status, 200);
    let one = h.st.serving.loaded()[0].ram_bytes;
    h.st.serving.release_all();
    assert_eq!(set_slots(&h, json!(8)).await.status, 200);
    let reply = ask().await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    let eight = h.st.serving.loaded()[0].ram_bytes;
    assert!(eight > one, "eight slots hold more KV cache than one ({eight} vs {one})");
    h.st.serving.release_all();
}

#[tokio::test]
async fn the_fit_query_answers_context_and_concurrency() {
    let h = fresh().await;
    let f = h.rpc("model_fit", json!({"id": TINY})).await;
    assert_eq!(f.status, 200, "{}", f.body);
    let v = f.json();
    assert_eq!(v["context"], 8192);
    assert_eq!(v["slots"], 2);
    let rows = v["choices"].as_array().unwrap();
    assert_eq!(
        rows.iter().map(|r| r["slots"].as_u64().unwrap()).collect::<Vec<_>>(),
        vec![1, 2, 4, 8]
    );
    assert_eq!(rows[0]["extraBytes"], 0);
    assert_eq!(rows[1]["extraBytes"], v["perSlotBytes"]);
    // The tiny file has no layout metadata: an estimate, said so.
    assert_eq!(v["kvExact"], false);
    assert!(v["message"].as_str().unwrap().contains("slot"));
    // Asking about other values.
    let g = h.rpc("model_fit", json!({"id": TINY, "context": 2048, "slots": 5})).await.json();
    assert_eq!((g["context"].clone(), g["slots"].clone()), (json!(2048), json!(5)));
    assert!(g["choices"].as_array().unwrap().iter().any(|r| r["slots"] == 5));
    // Querying changes nothing.
    assert_eq!(h.rpc("local_models", json!({})).await.json()[0]["slotsCustom"], false);
}

#[tokio::test]
async fn slots_survive_a_restart_and_damaged_values_read_as_unset() {
    let h = fresh().await;
    assert_eq!(set_slots(&h, json!(6)).await.status, 200);
    let dir = h.dir;
    drop(h.st);
    let again = harness_in(dir, false).await;
    let m = again.rpc("local_models", json!({})).await.json();
    assert_eq!((m[0]["slots"].clone(), m[0]["slotsCustom"].clone()), (json!(6), json!(true)));

    // The file is damaged in one value only: the other settings and the preferences survive.
    let settings = again.dir.path().join("settings.toml");
    let text = std::fs::read_to_string(&settings).unwrap();
    assert!(text.contains("slots = 6"), "{text}");
    std::fs::write(&settings, text.replace("slots = 6", "slots = 600")).unwrap();
    let dir = again.dir;
    drop(again.st);
    let third = harness_in(dir, false).await;
    let m = third.rpc("local_models", json!({})).await.json();
    assert_eq!(
        (m[0]["slots"].clone(), m[0]["slotsCustom"].clone()),
        (json!(2), json!(false)),
        "the default again"
    );
    assert_eq!(third.rpc("settings", json!({})).await.status, 200);
}

#[tokio::test]
async fn models_and_select_report_the_context_each_slot_gets() {
    let h = fresh().await;
    let key = h.make_key("Context Probe").await;
    // Not loaded: the default (capped by the model), the same number select reports.
    let models = h.call("GET", "/x/v1/models", Some(&key), None, &[]).await.json();
    let entry = &models["models"][0];
    let per_slot = entry["contextPerSlot"].as_u64().expect("contextPerSlot is a number");
    assert!(per_slot >= 512);
    if let Some(max) = entry["contextMax"].as_u64() {
        assert!(per_slot <= max);
    }
    let sel = h
        .call("POST", "/x/v1/select", Some(&key), Some(json!({"model": "auto"})), &[])
        .await
        .json();
    assert_eq!(sel["contextPerSlot"].as_u64(), Some(per_slot));
    // Loaded with 4 slots: the launch is the total, each slot still reports the per-slot context.
    assert_eq!(set_slots(&h, json!(4)).await.status, 200);
    assert_eq!(
        h.call("POST", "/v1/chat/completions", Some(&key), Some(chat()), &[]).await.status,
        200
    );
    let total = h.contexts.lock().unwrap()[0];
    let models = h.call("GET", "/x/v1/models", Some(&key), None, &[]).await.json();
    assert_eq!(models["models"][0]["contextPerSlot"].as_u64(), Some(u64::from(total / 4)));
    h.st.serving.release_all();
}

/// A machine with `available_gib` of free memory and nothing else going on.
fn machine(available_gib: u64) -> pitwall_server::system::MemoryReader {
    const GIB: u64 = 1 << 30;
    std::sync::Arc::new(move |_| System {
        total: 64 * GIB,
        available: available_gib * GIB,
        vram_free: None,
        os_pressure: false,
        os_critical: false,
        foreground_busy: false,
    })
}

/// The slot tests run on CI machines of any size (a Windows runner with 8 GiB answered 503 to the eight-slot load, whose KV
/// cache needs about 9 GiB): what a load needs is compared with the memory reader serving was given, never with the host.
#[tokio::test]
async fn a_load_is_planned_against_the_memory_serving_reads_not_the_host() {
    let h = fresh().await;
    let key = h.make_key("Memory Probe").await;
    assert_eq!(set_slots(&h, json!(8)).await.status, 200);

    h.st.serving.set_memory_reader(machine(8));
    let refused = h.call("POST", "/v1/chat/completions", Some(&key), Some(chat()), &[]).await;
    assert_eq!(refused.status, 503, "{}", refused.body);
    assert_eq!(refused.json()["error"]["code"], "insufficient_memory");
    assert_eq!(h.starts.load(Ordering::SeqCst), 0, "refused before an engine was started");

    h.st.serving.set_memory_reader(machine(48));
    let served = h.call("POST", "/v1/chat/completions", Some(&key), Some(chat()), &[]).await;
    assert_eq!(served.status, 200, "{}", served.body);
    assert_eq!(*h.parallels.lock().unwrap(), vec![8]);
    h.st.serving.release_all();
}

/// Every reading of memory (the dashboard sample, the limits' machine size) goes through the one reader on the state, so a
/// test's machine is the machine everywhere, never the CI runner's.
#[tokio::test]
async fn every_memory_reading_uses_the_one_reader_on_the_state() {
    let h = fresh().await;
    h.st.serving.set_memory_reader(machine(8));
    let sample = h.st.memory_sample();
    assert_eq!(sample.ram_total, 64 << 30);
    assert_eq!(sample.ram_available, 8 << 30);
}
