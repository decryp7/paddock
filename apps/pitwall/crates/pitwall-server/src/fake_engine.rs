//! A tiny OpenAI-compatible server for tests (`pitwall-fake-engine --port N`, a test-only bin of this package): `/health`, streamed chat
//! completions and embeddings with canned answers. The crash and restart tests launch that program as an engine, so the
//! whole launch, health and request path runs against a real child process.

use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use serde_json::json;

/// The canned stream.
pub const SSE: &str = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2},\"timings\":{\"prompt_per_second\":100.0,\"predicted_per_second\":50.0}}\n\ndata: [DONE]\n\n";

/// The router.
pub fn router() -> Router {
    router_with(None)
}

/// The router; with `ready_when`, `/health` answers 503 "loading" (as llama-server does while it loads a model) until
/// that file exists, then 200.
pub fn router_with(ready_when: Option<std::path::PathBuf>) -> Router {
    Router::new()
        .route(
            "/health",
            get(move || {
                let ready = ready_when.as_ref().is_none_or(|p| p.exists());
                async move {
                    if ready {
                        (axum::http::StatusCode::OK, axum::Json(json!({"status": "ok"})))
                    } else {
                        (
                            axum::http::StatusCode::SERVICE_UNAVAILABLE,
                            axum::Json(json!({"error": {"code": 503, "message": "Loading model"}})),
                        )
                    }
                }
            }),
        )
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

/// Parses the value of `--ready-when`: a file path, not empty, not another option, at most 4096 bytes.
pub fn parse_ready_when(value: Option<String>) -> Result<std::path::PathBuf, String> {
    match value {
        Some(v) if !v.is_empty() && !v.starts_with("--") && v.len() <= 4096 => Ok(v.into()),
        _ => Err("--ready-when takes the path of the file whose existence makes /health answer 200 (at most 4096 bytes)".into()),
    }
}

/// Serve on 127.0.0.1:`port` until the process ends; with `die_after_ms` the process exits that long after it started
/// listening (a crashing engine, for the restart and crash-loop tests); with `ready_when` it reports itself loading
/// until that file exists (an engine that is slow to become ready, released by the test on an event).
pub async fn serve(port: u16, die_after_ms: Option<u64>, ready_when: Option<std::path::PathBuf>) -> std::io::Result<()> {
    let l = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    if let Some(ms) = die_after_ms {
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            std::process::exit(3);
        });
    }
    axum::serve(l, router_with(ready_when)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_when_takes_a_path_and_refuses_anything_else() {
        assert_eq!(parse_ready_when(Some("/tmp/x/ready".into())), Ok("/tmp/x/ready".into()));
        for bad in [None, Some(String::new()), Some("--port".into()), Some("a".repeat(4097))] {
            assert!(parse_ready_when(bad.clone()).is_err(), "{bad:?}");
        }
    }
}
