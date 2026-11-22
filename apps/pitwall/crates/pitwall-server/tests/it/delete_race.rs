//! The delete mark against a load in flight (ADR 0108, PW-1; Dyno ADR 0034: newer-only state).

use crate::common::harness;

const TINY: &str = "tiny-q4_k_m";

/// The insert-time check of a load (`Serving::ensure_as`): a delete that marks the model while its engine is still starting
/// wins. The load answers "being deleted", its engine ends, the model is not left loaded, and the mark's end lets a later load
/// through. The engine start is held, so the order is the test's, not a race.
#[tokio::test]
async fn a_delete_that_marks_a_model_while_it_loads_wins_at_the_insert() {
    use std::sync::atomic::Ordering;
    let h = harness(false).await;
    let id = pitwall_core::ids::ModelId::parse(TINY).unwrap();
    h.start_hold.close();
    let load = tokio::spawn({
        let (st, id) = (h.st.clone(), id.clone());
        async move { st.serving.ensure(&id).await.map(|_| ()).map_err(|e| e.message) }
    });
    crate::common::until("the engine start began", || h.attempts.load(Ordering::SeqCst) >= 1).await;
    let guard = h.st.serving.begin_delete(&id).expect("nothing is loaded yet, so the mark is taken");
    h.start_hold.release();
    let why = load.await.unwrap().expect_err("the load lost to the delete");
    assert!(why.contains("being deleted"), "{why}");
    assert!(!h.st.serving.is_loaded(&id), "the engine that lost is not left in the loaded set");
    drop(guard);
    assert!(h.st.serving.ensure(&id).await.is_ok(), "the mark ended with the guard");
}
