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

/// A model with a request running is not deleted: its id answers `busy`, its files stay and it stays loaded; once the request
/// is over the same call deletes it. The single `delete_model` refuses the same way (one code for both paths).
#[tokio::test]
async fn a_model_with_a_running_request_is_busy_and_stays_on_disk() {
    let h = harness(false).await;
    let id = pitwall_core::ids::ModelId::parse(TINY).unwrap();
    let (busy, _) = h.st.serving.ensure(&id).await.unwrap();
    let r = h.rpc("delete_models", json!({"ids": [TINY]})).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.json()[0]["err"]["code"], "busy", "{}", r.body);
    assert_eq!(h.st.library.local().len(), 1, "the files stay");
    assert!(h.st.serving.is_loaded(&id), "still loaded for the running request");
    let single = h.rpc("delete_model", json!({"id": TINY})).await;
    assert_ne!(single.status, 200, "{}", single.body);
    assert!(single.body.contains("busy"), "{}", single.body);
    assert_eq!(h.st.library.local().len(), 1);
    drop(busy);
    assert_eq!(h.rpc("delete_models", json!({"ids": [TINY]})).await.json(), json!([{"ok": null}]));
    assert!(h.st.library.local().is_empty());
}

/// While a model is being deleted a load of it is refused, so the unload, the scan and the delete cannot lose to a load in
/// between (Dyno ADR 0034: newer-only state). The mark ends with the delete.
#[tokio::test]
async fn a_model_being_deleted_cannot_be_loaded() {
    let h = harness(false).await;
    let id = pitwall_core::ids::ModelId::parse(TINY).unwrap();
    let guard = h.st.serving.begin_delete(&id).expect("an idle model can be marked");
    assert!(
        h.st.serving.begin_delete(&id).is_err(),
        "a second delete of it is refused while the first runs"
    );
    let e = h.st.serving.ensure(&id).await.err().expect("refused");
    assert!(e.message.contains("being deleted"), "{}", e.message);
    drop(guard);
    assert!(h.st.serving.ensure(&id).await.is_ok(), "the mark ended with the guard");
}

/// The same id twice in one call: the second is a result of its own, and a refused one is not unloaded by its twin.
#[tokio::test]
async fn a_repeated_id_in_one_call_answers_not_found_the_second_time() {
    let h = harness(false).await;
    let r = h.rpc("delete_models", json!({"ids": [TINY, TINY]})).await;
    assert_eq!(r.json()[0], json!({"ok": null}));
    assert_eq!(r.json()[1]["err"]["code"], "notFound");
}
