//! Bulk management commands (ADR 0108): N items in one call, one result per item, one shared piece of work.

use crate::common::harness;
use serde_json::json;

const TINY: &str = "tiny-q4_k_m";

/// PW-1: `delete_models` scans the library once for all its ids and answers one `ItemResult` per id in request order; a bad id
/// and a model that is gone fail alone.
#[tokio::test]
async fn delete_models_scans_once_and_answers_one_result_per_id() {
    let h = harness(false).await;
    let before = h.st.library.scans();
    let r = h
        .rpc("delete_models", json!({"ids": [TINY, "ghost-q4_k_m", "not a model id!", TINY]}))
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(h.st.library.scans() - before, 1, "one scan for four ids");
    let v = r.json();
    let items = v.as_array().unwrap();
    assert_eq!(items.len(), 4);
    assert_eq!(items[0], json!({"ok": null}));
    assert_eq!(items[1]["err"]["code"], "notFound");
    assert_eq!(items[2]["err"]["code"], "invalid");
    // The same model twice in one call: the second is a result of its own, not a failure of the call.
    assert_eq!(items[3]["err"]["code"], "notFound");
    assert!(h.st.library.local().is_empty(), "the model is gone from disk");
}

#[tokio::test]
async fn delete_models_refuses_a_batch_over_its_bound_and_takes_an_empty_one() {
    let h = harness(false).await;
    let ids: Vec<String> = (0..=pitwall_core::mgmt::MAX_BULK_IDS).map(|i| format!("m{i}-q4_k_m")).collect();
    assert_eq!(h.rpc("delete_models", json!({"ids": ids})).await.status, 400);
    let r = h.rpc("delete_models", json!({"ids": []})).await;
    assert_eq!((r.status, r.json()), (200, json!([])));
}
