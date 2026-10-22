//! Test harness: a server over a temp data folder with a fake llama-server (an in-process OpenAI-compatible
//! HTTP server), so the API surface is exercised end to end without a model.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use pitwall_core::hardware::Hardware;
use pitwall_core::llama::Launch;
use pitwall_core::mgmt::{Availability, EngineKind};
use pitwall_loopback::{Client, LoopbackPort};
use pitwall_server::app::AppState;
use pitwall_server::engines::{BoxFuture, Engine, EngineError, Running};
use pitwall_server::paths::Dirs;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct AbortOnDrop(pub tokio::task::JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// A fake llama-server: /health, /props, streamed chat completions, embeddings.
pub struct FakeEngine {
    pub starts: Arc<AtomicUsize>,
    /// `POST /slots/{n}?action=erase` calls received (the context release).
    pub erases: Arc<AtomicUsize>,
    pub fail: bool,
}

const SSE: &str = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2},\"timings\":{\"prompt_per_second\":100.0,\"predicted_per_second\":50.0}}\n\ndata: [DONE]\n\n";

impl Engine for FakeEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::LlamaCpp
    }
    fn availability(&self) -> Availability {
        Availability::Available
    }
    fn start<'a>(&'a self, l: &'a Launch) -> BoxFuture<'a, Result<Running, EngineError>> {
        Box::pin(async move {
            self.starts.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(EngineError::Start("the fake engine refuses to start".into()));
            }
            let erases = self.erases.clone();
            let app = Router::new()
                .route(
                    "/slots/{slot}",
                    post(move || {
                        let erases = erases.clone();
                        async move {
                            erases.fetch_add(1, Ordering::SeqCst);
                            axum::Json(json!({"id_slot": 0, "n_erased": 1}))
                        }
                    }),
                )
                .route("/health", get(|| async { axum::Json(json!({"status": "ok"})) }))
                .route(
                    "/props",
                    get(|| async {
                        axum::Json(
                            json!({"default_generation_settings": {"n_ctx": 4096}, "modalities": {"vision": false}, "build_info": "fake"}),
                        )
                    }),
                )
                .route(
                    "/v1/chat/completions",
                    post(|| async { ([("content-type", "text/event-stream")], SSE).into_response() }),
                )
                .route(
                    "/v1/embeddings",
                    post(|| async { axum::Json(json!({"data": [{"index": 0, "embedding": [0.5, 0.25]}], "usage": {"prompt_tokens": 3}})) }),
                );
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", l.port))
                .await
                .map_err(|e| EngineError::Start(e.to_string()))?;
            let h = tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            Ok(Running {
                child: None,
                guard: Some(Box::new(AbortOnDrop(h))),
                client: Client::new(LoopbackPort::new(l.port).unwrap(), None),
                port: l.port,
                props: pitwall_core::llama::Props {
                    n_ctx: Some(4096),
                    vision: Some(false),
                    ..Default::default()
                },
            })
        })
    }
}

pub struct Harness {
    pub st: Arc<AppState>,
    pub client: Client,
    pub admin: String,
    pub starts: Arc<AtomicUsize>,
    pub erases: Arc<AtomicUsize>,
    pub dir: tempfile::TempDir,
}

pub fn gguf(arch: &str) -> Vec<u8> {
    let mut b = b"GGUF".to_vec();
    b.extend(3u32.to_le_bytes());
    b.extend(0u64.to_le_bytes());
    b.extend(1u64.to_le_bytes());
    b.extend(("general.architecture".len() as u64).to_le_bytes());
    b.extend(b"general.architecture");
    b.extend(8u32.to_le_bytes());
    b.extend((arch.len() as u64).to_le_bytes());
    b.extend(arch.as_bytes());
    b.extend(vec![0u8; 70_000]);
    b
}

pub async fn harness(fail: bool) -> Harness {
    harness_in(tempfile::tempdir().unwrap(), fail).await
}

pub async fn harness_in(dir: tempfile::TempDir, fail: bool) -> Harness {
    let dirs = Dirs::at(dir.path());
    let m = dirs.models().join("o/r");
    std::fs::create_dir_all(&m).unwrap();
    if !m.join("tiny-Q4_K_M.gguf").exists() {
        std::fs::write(m.join("tiny-Q4_K_M.gguf"), gguf("llama")).unwrap();
    }
    let starts = Arc::new(AtomicUsize::new(0));
    let erases = Arc::new(AtomicUsize::new(0));
    let engine: Arc<dyn Engine> = Arc::new(FakeEngine {
        starts: starts.clone(),
        erases: erases.clone(),
        fail,
    });
    let st = AppState::open(dirs.clone(), Some(engine)).await;
    let _ = Hardware::default();
    let app = pitwall_server::http::router(st.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let admin = std::fs::read_to_string(dirs.admin_key()).unwrap().trim().to_string();
    Harness {
        st,
        client: Client::new(LoopbackPort::new(port).unwrap(), None),
        admin,
        starts,
        erases,
        dir,
    }
}

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.body))
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

impl Harness {
    pub async fn call(&self, method: &str, path: &str, key: Option<&str>, body: Option<Value>, extra: &[(&str, &str)]) -> Reply {
        let mut headers: Vec<(&str, String)> = extra.iter().map(|(k, v)| (*k, (*v).to_string())).collect();
        if let Some(k) = key {
            headers.push(("authorization", format!("Bearer {k}")));
        }
        let hs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let bytes = body.map(|b| serde_json::to_vec(&b).unwrap());
        let r = self
            .client
            .send_with(method, path, bytes, &hs, Duration::from_secs(20))
            .await
            .unwrap();
        let status = r.status;
        let headers = r.headers.clone();
        let raw = r.bytes(8 << 20).await.unwrap();
        Reply {
            status,
            headers,
            body: String::from_utf8_lossy(&raw).into_owned(),
        }
    }

    /// A management call as the admin.
    pub async fn rpc(&self, method: &str, args: Value) -> Reply {
        self.call("POST", &format!("/mgmt/v1/rpc/{method}"), Some(&self.admin), Some(args), &[])
            .await
    }

    pub async fn make_key(&self, name: &str) -> String {
        let r = self.rpc("create_app", json!({"name": name})).await;
        assert_eq!(r.status, 200, "{}", r.body);
        let id = r.json()["id"].as_str().unwrap().to_string();
        let k = self.rpc("create_key", json!({"appId": id})).await;
        assert_eq!(k.status, 200, "{}", k.body);
        k.json()["secret"].as_str().unwrap().to_string()
    }
}
