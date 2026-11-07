//! Cancelling a model load and a benchmark by the caller's `opId` (`cancel_op`): the work stops, nothing is left behind and
//! the server stays consistent (ER-18).

use crate::common;
use common::{harness, harness_with};
use pitwall_core::ids::ModelId;
use pitwall_core::llama::Launch;
use pitwall_core::mgmt::{Availability, EngineKind};
use pitwall_server::engines::{BoxFuture, Engine, EngineError, Running};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const TINY: &str = "tiny-q4_k_m";
/// Only a bound on how long a broken test waits; a passing one never reaches it.
const GUARD: Duration = Duration::from_secs(60);

/// An engine whose model never finishes loading.
struct NeverLoads;

impl Engine for NeverLoads {
    fn kind(&self) -> EngineKind {
        EngineKind::LlamaCpp
    }
    fn availability(&self) -> Availability {
        Availability::Available
    }
    fn start<'a>(&'a self, _: &'a Launch) -> BoxFuture<'a, Result<Running, EngineError>> {
        Box::pin(std::future::pending())
    }
}

#[tokio::test]
async fn cancelling_a_load_stops_it_and_leaves_no_model_and_no_stuck_load() {
    let h = Arc::new(harness_with(tempfile::tempdir().unwrap(), false, Some(Arc::new(NeverLoads))).await);
    let id = ModelId::parse(TINY).unwrap();
    // Twice: the second proves the first released the load gate (a stuck load would block it for ever).
    for op in ["load-1", "load-2"] {
        let h2 = h.clone();
        let load = tokio::spawn(async move { h2.rpc("load_model", json!({ "id": TINY, "opId": op })).await });
        // The cancel may arrive before or after the load has begun: both end the same way (the "before" case is
        // pinned down deterministically in `a_cancel_that_arrives_before_the_operation_registers_stops_it_as_it_starts`).
        assert_eq!(h.rpc("cancel_op", json!({ "opId": op })).await.status, 200);
        let r = tokio::time::timeout(GUARD, load).await.expect("the load ends").unwrap();
        assert_eq!(r.status, 200, "{}", r.body);
        assert_eq!(r.json(), json!({ "cancelled": true }));
        assert!(!h.st.serving.is_loaded(&id), "no model is left behind");
    }
    assert!(h.st.ops.lock().unwrap().is_empty(), "finished operations are forgotten");
}

#[tokio::test]
async fn a_cancel_that_arrives_before_the_operation_registers_stops_it_as_it_starts() {
    let h = harness_with(tempfile::tempdir().unwrap(), false, Some(Arc::new(NeverLoads))).await;
    let id = ModelId::parse(TINY).unwrap();
    // The cancel is answered and stored before the load exists: the order is fixed by awaiting it.
    assert_eq!(h.rpc("cancel_op", json!({ "opId": "early" })).await.status, 200);
    assert_eq!(h.st.ops.lock().unwrap().len(), 1, "the permit is kept for the operation to come");
    let r = tokio::time::timeout(GUARD, h.rpc("load_model", json!({ "id": TINY, "opId": "early" })))
        .await
        .expect("the load ends at once");
    assert_eq!(r.json(), json!({ "cancelled": true }), "{}", r.body);
    assert!(!h.st.serving.is_loaded(&id));
    assert!(h.st.ops.lock().unwrap().is_empty(), "the permit was used up");
}

#[tokio::test]
async fn a_cancelled_load_leaves_a_model_that_another_request_loaded_or_is_using() {
    let h = harness(false).await;
    let id = ModelId::parse(TINY).unwrap();
    assert_eq!(h.rpc("load_model", json!({ "id": TINY })).await.status, 200);
    assert!(h.st.serving.is_loaded(&id));
    // This operation did not load it: cancelling it must not unload it.
    pitwall_server::mgmt::undo_cancelled_load(&h.st, &id, false);
    assert!(h.st.serving.is_loaded(&id), "loaded by someone else: left alone");
    // It did load it, but a request is using it right now: still left alone.
    let (busy, _) = h.st.serving.ensure(&id).await.unwrap();
    pitwall_server::mgmt::undo_cancelled_load(&h.st, &id, true);
    assert!(h.st.serving.is_loaded(&id), "in use: left alone");
    drop(busy);
    // It loaded it and nobody holds it: unloaded.
    pitwall_server::mgmt::undo_cancelled_load(&h.st, &id, true);
    assert!(!h.st.serving.is_loaded(&id));
}

#[tokio::test]
async fn cancelling_a_benchmark_stops_it_and_records_nothing() {
    let h = Arc::new(harness(false).await);
    // The fake model takes "for ever" to answer, so the benchmark is mid-run when the cancel arrives.
    h.delay_ms.store(10 * 60 * 1000, std::sync::atomic::Ordering::SeqCst);
    let id = ModelId::parse(TINY).unwrap();
    let h2 = h.clone();
    let bench = tokio::spawn(async move { h2.rpc("run_benchmark", json!({ "modelId": TINY, "opId": "bench-1" })).await });
    tokio::time::timeout(GUARD, async {
        while !h.st.serving.is_loaded(&id) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the model loads for the benchmark");
    assert_eq!(h.rpc("cancel_op", json!({ "opId": "bench-1" })).await.status, 200);
    let r = tokio::time::timeout(GUARD, bench).await.expect("the benchmark ends").unwrap();
    assert_eq!(r.json(), json!({ "cancelled": true }), "{}", r.body);
    assert_eq!(h.rpc("benchmarks", json!({})).await.json(), json!([]), "no result is stored");
    // The server is consistent: a benchmark without an opId still runs to the end.
    h.delay_ms.store(0, std::sync::atomic::Ordering::SeqCst);
    let again = h.rpc("run_benchmark", json!({ "modelId": TINY })).await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert!(again.json()["genTps"].as_f64().unwrap() > 0.0);
}

#[tokio::test]
async fn a_cancel_for_an_unknown_operation_is_fine_and_the_stored_marks_are_bounded() {
    let h = harness(false).await;
    for n in 0..600 {
        assert_eq!(h.rpc("cancel_op", json!({ "opId": format!("x{n}") })).await.status, 200);
    }
    assert!(h.st.ops.lock().unwrap().len() <= pitwall_server::ops::MAX_OPS);
    assert_eq!(h.rpc("cancel_op", json!({})).await.status, 400);
}
