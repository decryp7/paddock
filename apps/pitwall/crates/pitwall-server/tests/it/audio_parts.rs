//! Audio clips for models that hear (ADR 0023 Decision 5, WP-7): an OpenAI `input_audio` part is inline data only, and it
//! reaches the engine only when the loaded model says it hears (llama-server's `/props` `modalities.audio`). The test model
//! does not, so the request is refused in words and nothing reaches the engine; before, every `input_audio` part was refused
//! as "not supported", whatever the model.

use crate::common;
use common::harness;
use serde_json::json;

fn with_audio(audio: serde_json::Value) -> serde_json::Value {
    json!({"model": "tiny-q4_k_m", "messages": [{"role": "user", "content": [
        {"type": "text", "text": "what is said here?"},
        {"type": "input_audio", "input_audio": audio},
    ]}]})
}

#[tokio::test]
async fn a_clip_goes_to_the_hearing_check_and_never_reaches_a_model_that_cannot_hear() {
    let h = harness(false).await;
    let key = h.make_key("Test App").await;
    let r = h
        .call(
            "POST",
            "/v1/chat/completions",
            Some(&key),
            Some(with_audio(json!({"data": "UklGRiQAAABXQVZF", "format": "wav"}))),
            &[],
        )
        .await;
    assert_eq!(r.status, 400, "{}", r.body);
    let words = r.json()["error"]["message"].as_str().unwrap().to_string();
    assert!(words.contains("cannot hear audio") && words.contains("audio projector"), "{words}");
    assert!(h.bodies.lock().unwrap().is_empty(), "no request reached the engine");
}

#[tokio::test]
async fn a_clip_by_url_is_refused_before_anything_loads() {
    let h = harness(false).await;
    let key = h.make_key("Test App").await;
    let r = h
        .call(
            "POST",
            "/v1/chat/completions",
            Some(&key),
            Some(with_audio(json!({"url": "https://example.com/a.wav", "format": "wav"}))),
            &[],
        )
        .await;
    assert_eq!(r.status, 400, "{}", r.body);
    assert!(
        r.json()["error"]["message"].as_str().unwrap().contains("never fetches"),
        "{}",
        r.body
    );
    assert_eq!(h.starts.load(std::sync::atomic::Ordering::SeqCst), 0, "nothing was loaded");
}
