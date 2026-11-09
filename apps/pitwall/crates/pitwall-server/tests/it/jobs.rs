//! `X-Pitwall-Job-Id` and cancelling by id (ADR 0009): a streamed request that has to queue answers at once with the id of
//! its job, `DELETE /x/v1/jobs/{id}` cancels it without closing the connection (typed `cancelled` frame), an app may
//! cancel only its own jobs and the admin any.

use crate::common;
use common::{harness_in, Harness, Reply};
use serde_json::{json, Value};
use std::time::Duration;

const TINY: &str = "tiny-q4_k_m";

fn chat(stream: bool) -> Value {
    json!({"model": TINY, "stream": stream, "messages": [{"role": "user", "content": "hi"}]})
}

fn messages() -> Value {
    json!({"model": TINY, "stream": true, "max_tokens": 16, "messages": [{"role": "user", "content": "hi"}]})
}

/// One slot, a slow engine, and a request occupying the slot.
async fn busy() -> (Harness, String, tokio::task::JoinHandle<Reply>) {
    let h = harness_in(tempfile::tempdir().unwrap(), false).await;
    let key = h.make_key("Owner").await;
    h.hold.close();
    assert_eq!(h.rpc("set_model_slots", json!({"id": TINY, "slots": 1})).await.status, 200);
    let (client, k) = (h.client.clone(), key.clone());
    let running = tokio::spawn(async move {
        let body = serde_json::to_vec(&chat(false)).unwrap();
        let auth = format!("Bearer {k}");
        let r = client
            .send_with(
                "POST",
                "/v1/chat/completions",
                Some(body),
                &[("authorization", auth.as_str())],
                Duration::from_secs(20),
            )
            .await
            .unwrap();
        let (status, headers) = (r.status, r.headers.clone());
        let raw = r.bytes(8 << 20).await.unwrap();
        Reply {
            status,
            headers,
            body: String::from_utf8_lossy(&raw).into_owned(),
        }
    });
    common::until("the request holds the slot", || {
        h.st.serving.loaded().first().is_some_and(|m| m.active_requests > 0)
    })
    .await;
    assert_eq!(h.st.serving.loaded()[0].active_requests, 1, "the slot is taken");
    (h, key, running)
}

/// Sends a streamed request and returns its response as soon as the headers arrive.
async fn open_stream(h: &Harness, path: &str, key: &str, body: Value) -> pitwall_loopback::Response {
    let auth = format!("Bearer {key}");
    h.client
        .send_with(
            "POST",
            path,
            Some(serde_json::to_vec(&body).unwrap()),
            &[("authorization", auth.as_str())],
            Duration::from_secs(20),
        )
        .await
        .unwrap()
}

fn job_id(r: &pitwall_loopback::Response) -> u64 {
    r.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-pitwall-job-id"))
        .expect("X-Pitwall-Job-Id")
        .1
        .parse()
        .unwrap()
}

#[tokio::test]
async fn a_queued_stream_answers_at_once_and_is_cancelled_by_id() {
    let (h, key, running) = busy().await;
    let queued = open_stream(&h, "/v1/chat/completions", &key, chat(true)).await;
    assert_eq!(queued.status, 200, "the answer starts while the job still waits");
    let id = job_id(&queued);
    let del = h.call("DELETE", &format!("/x/v1/jobs/{id}"), Some(&key), None, &[]).await;
    assert_eq!(del.status, 200, "{}", del.body);
    assert_eq!(del.json()["cancelled"], true);
    assert_eq!(del.json()["wasRunning"], false);
    let body = String::from_utf8_lossy(&queued.bytes(1 << 20).await.unwrap()).into_owned();
    assert!(body.contains("\"code\":\"cancelled\""), "typed cancelled frame: {body}");
    assert!(!body.contains("[DONE]") || body.contains("cancelled"));
    // The running request is untouched.
    h.hold.release();
    assert_eq!(running.await.unwrap().status, 200);
    h.st.serving.release_all();
}

#[tokio::test]
async fn another_apps_key_cannot_cancel_but_the_admin_can() {
    let (h, key, running) = busy().await;
    let other = h.make_key("Other").await;
    let queued = open_stream(&h, "/v1/messages", &key, messages()).await;
    assert_eq!(queued.status, 200);
    let id = job_id(&queued);
    let foreign = h.call("DELETE", &format!("/x/v1/jobs/{id}"), Some(&other), None, &[]).await;
    assert_eq!(foreign.status, 403, "{}", foreign.body);
    assert_eq!(foreign.json()["error"]["code"], "forbidden");
    // Still queued: the admin cancels it.
    let admin = h.call("DELETE", &format!("/x/v1/jobs/{id}"), Some(&h.admin), None, &[]).await;
    assert_eq!(admin.status, 200, "{}", admin.body);
    let body = String::from_utf8_lossy(&queued.bytes(1 << 20).await.unwrap()).into_owned();
    assert!(body.contains("event: error") && body.contains("cancelled"), "{body}");
    assert_eq!(
        h.call("DELETE", &format!("/x/v1/jobs/{id}"), Some(&key), None, &[]).await.status,
        404,
        "gone"
    );
    h.hold.release();
    assert_eq!(running.await.unwrap().status, 200);
    h.st.serving.release_all();
}

#[tokio::test]
async fn an_uncontended_stream_carries_the_id_with_its_provenance_and_a_queued_one_still_completes() {
    let h = harness_in(tempfile::tempdir().unwrap(), false).await;
    let key = h.make_key("Plain").await;
    let r = open_stream(&h, "/v1/chat/completions", &key, chat(true)).await;
    assert_eq!(r.status, 200);
    assert!(job_id(&r) > 0);
    assert!(
        r.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("x-pitwall-model")),
        "full provenance when it ran at once"
    );
    assert!(String::from_utf8_lossy(&r.bytes(1 << 20).await.unwrap()).contains("[DONE]"));
    // Non-streamed answers carry it too.
    let json = h.call("POST", "/v1/chat/completions", Some(&key), Some(chat(false)), &[]).await;
    assert_eq!(json.status, 200);
    assert!(json.header("x-pitwall-job-id").is_some());
    h.st.serving.release_all();

    // A queued stream that is not cancelled still gets its answer on the same connection.
    let (h, key, running) = busy().await;
    let queued = open_stream(&h, "/v1/chat/completions", &key, chat(true)).await;
    assert_eq!(queued.status, 200);
    let _ = job_id(&queued);
    h.hold.release();
    let body = String::from_utf8_lossy(&queued.bytes(1 << 20).await.unwrap()).into_owned();
    assert!(body.contains("[DONE]"), "{body}");
    assert_eq!(running.await.unwrap().status, 200);
    h.st.serving.release_all();
}
