//! A tiny OpenAI-compatible server for tests (`pitwall-server --fake-engine --port N`): `/health`, streamed chat
//! completions and embeddings with canned answers. Engine packs in the tests launch this binary, so the whole
//! launch, health and request path runs against a real child process.

use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use serde_json::json;

/// The canned stream.
pub const SSE: &str = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2},\"timings\":{\"prompt_per_second\":100.0,\"predicted_per_second\":50.0}}\n\ndata: [DONE]\n\n";

/// The router.
pub fn router() -> Router {
    Router::new()
        .route("/health", get(|| async { axum::Json(json!({"status": "ok"})) }))
        .route(
            "/props",
            get(|| async {
                axum::Json(json!({"default_generation_settings": {"n_ctx": 4096}, "modalities": {"vision": false}, "build_info": "fake"}))
            }),
        )
        .route(
            "/v1/chat/completions",
            post(|| async { ([("content-type", "text/event-stream")], SSE).into_response() }),
        )
        .route(
            "/v1/embeddings",
            post(|| async { axum::Json(json!({"data": [{"index": 0, "embedding": [0.5, 0.25]}], "usage": {"prompt_tokens": 3}})) }),
        )
}

/// Serve on 127.0.0.1:`port` until the process ends; with `die_after_ms` the process exits that long after it started
/// listening (a crashing engine, for the restart and crash-loop tests).
pub async fn serve(port: u16, die_after_ms: Option<u64>) -> std::io::Result<()> {
    let l = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    if let Some(ms) = die_after_ms {
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            std::process::exit(3);
        });
    }
    axum::serve(l, router()).await
}
