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
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
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
    /// Engines actually started (a model load); a start refused because the port was taken is not one.
    pub starts: Arc<AtomicUsize>,
    /// Every call of `start`, including ones refused with "address in use".
    pub attempts: Arc<AtomicUsize>,
    /// How many of the next starts fail as if another program had taken the port (a race `free_port` cannot rule out).
    pub port_taken: Arc<AtomicUsize>,
    /// `POST /slots/{n}?action=erase` calls received (the context release).
    pub erases: Arc<AtomicUsize>,
    pub fail: bool,
    /// The bodies of the chat requests the engine received (what the model was actually sent).
    pub bodies: Arc<std::sync::Mutex<Vec<Value>>>,
    /// The context each start asked for.
    pub contexts: Arc<std::sync::Mutex<Vec<u32>>>,
    /// The `--parallel` each start asked for.
    pub parallels: Arc<std::sync::Mutex<Vec<u32>>>,
    /// How long a chat request takes (milliseconds), to hold slots busy.
    pub delay_ms: Arc<AtomicU64>,
    /// Chat requests being served right now and the most that were at once.
    pub live: Arc<AtomicUsize>,
    pub peak: Arc<AtomicUsize>,
    /// What `timings.cache_n` (prompt tokens reused from the prompt cache) reports.
    pub cache_n: Arc<AtomicU64>,
    /// The inputs of every embeddings request the engine received.
    pub embeds: Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    /// `POST /slots/{n}?action=...` calls: `"{n}:{action}:{body}"`.
    pub slot_calls: Arc<std::sync::Mutex<Vec<String>>>,
}

fn sse(cache_n: u64) -> String {
    format!(
        "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"Hello\"}},\"finish_reason\":null}}]}}\n\ndata: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\" world\"}},\"finish_reason\":null}}]}}\n\ndata: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":5,\"completion_tokens\":2}},\"timings\":{{\"prompt_per_second\":100.0,\"predicted_per_second\":50.0,\"cache_n\":{cache_n}}}}}\n\ndata: [DONE]\n\n"
    )
}

impl Engine for FakeEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::LlamaCpp
    }
    fn availability(&self) -> Availability {
        Availability::Available
    }
    fn start<'a>(&'a self, l: &'a Launch) -> BoxFuture<'a, Result<Running, EngineError>> {
        Box::pin(async move {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            let taken = self.port_taken.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok();
            if taken {
                return Err(EngineError::Start("bind: Address already in use (os error 98)".into()));
            }
            self.starts.fetch_add(1, Ordering::SeqCst);
            self.contexts.lock().unwrap().push(l.context);
            self.parallels.lock().unwrap().push(l.parallel);
            let (delay, live, peak) = (self.delay_ms.clone(), self.live.clone(), self.peak.clone());
            let bodies = self.bodies.clone();
            if self.fail {
                return Err(EngineError::Start("the fake engine refuses to start".into()));
            }
            let erases = self.erases.clone();
            let (cache_n, embeds, slot_calls) = (self.cache_n.clone(), self.embeds.clone(), self.slot_calls.clone());
            let app = Router::new()
                .route(
                    "/slots/{slot}",
                    post(
                        move |axum::extract::Path(slot): axum::extract::Path<u32>,
                              axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
                              body: axum::body::Bytes| {
                            let (erases, slot_calls) = (erases.clone(), slot_calls.clone());
                            async move {
                                let action = q.get("action").cloned().unwrap_or_default();
                                slot_calls
                                    .lock()
                                    .unwrap()
                                    .push(format!("{slot}:{action}:{}", String::from_utf8_lossy(&body)));
                                if action == "erase" {
                                    erases.fetch_add(1, Ordering::SeqCst);
                                }
                                axum::Json(json!({"id_slot": slot, "n_erased": 1}))
                            }
                        },
                    ),
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
                    post(move |body: axum::body::Bytes| {
                        let bodies = bodies.clone();
                        let (delay, live, peak, cache_n) = (delay.clone(), live.clone(), peak.clone(), cache_n.clone());
                        async move {
                            let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                            peak.fetch_max(now, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(delay.load(Ordering::SeqCst))).await;
                            live.fetch_sub(1, Ordering::SeqCst);
                            if let Ok(v) = serde_json::from_slice::<Value>(&body) {
                                bodies.lock().unwrap().push(v);
                            }
                            ([("content-type", "text/event-stream")], sse(cache_n.load(Ordering::SeqCst))).into_response()
                        }
                    }),
                )
                .route(
                    "/v1/embeddings",
                    post(move |body: axum::body::Bytes| {
                        let embeds = embeds.clone();
                        async move {
                            let inputs: Vec<String> = serde_json::from_slice::<Value>(&body)
                                .ok()
                                .and_then(|v| {
                                    v["input"]
                                        .as_array()
                                        .map(|a| a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect())
                                })
                                .unwrap_or_default();
                            let data: Vec<Value> = inputs
                                .iter()
                                .enumerate()
                                .map(|(i, t)| json!({"index": i, "embedding": [t.len() as f64, 0.25]}))
                                .collect();
                            let n = inputs.len() as u64;
                            embeds.lock().unwrap().push(inputs);
                            axum::Json(json!({"data": data, "usage": {"prompt_tokens": 3 * n}}))
                        }
                    }),
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
    pub attempts: Arc<AtomicUsize>,
    pub port_taken: Arc<AtomicUsize>,
    pub erases: Arc<AtomicUsize>,
    pub bodies: Arc<std::sync::Mutex<Vec<Value>>>,
    pub contexts: Arc<std::sync::Mutex<Vec<u32>>>,
    pub parallels: Arc<std::sync::Mutex<Vec<u32>>>,
    pub delay_ms: Arc<AtomicU64>,
    pub peak: Arc<AtomicUsize>,
    pub cache_n: Arc<AtomicU64>,
    pub embeds: Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    pub slot_calls: Arc<std::sync::Mutex<Vec<String>>>,
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
    harness_with(dir, fail, None).await
}

/// [`harness_in`] with another engine than the in-process fake (for example [`ChildEngine`]).
pub async fn harness_with(dir: tempfile::TempDir, fail: bool, other: Option<Arc<dyn Engine>>) -> Harness {
    let dirs = Dirs::at(dir.path());
    let m = dirs.models().join("o/r");
    std::fs::create_dir_all(&m).unwrap();
    if !m.join("tiny-Q4_K_M.gguf").exists() {
        std::fs::write(m.join("tiny-Q4_K_M.gguf"), gguf("llama")).unwrap();
    }
    let starts = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::new(AtomicUsize::new(0));
    let port_taken = Arc::new(AtomicUsize::new(0));
    let erases = Arc::new(AtomicUsize::new(0));
    let bodies = Arc::new(std::sync::Mutex::new(vec![]));
    let contexts = Arc::new(std::sync::Mutex::new(vec![]));
    let parallels = Arc::new(std::sync::Mutex::new(vec![]));
    let delay_ms = Arc::new(AtomicU64::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let cache_n = Arc::new(AtomicU64::new(0));
    let embeds = Arc::new(std::sync::Mutex::new(vec![]));
    let slot_calls = Arc::new(std::sync::Mutex::new(vec![]));
    let engine: Arc<dyn Engine> = other.unwrap_or_else(|| {
        Arc::new(FakeEngine {
            starts: starts.clone(),
            attempts: attempts.clone(),
            port_taken: port_taken.clone(),
            erases: erases.clone(),
            fail,
            bodies: bodies.clone(),
            contexts: contexts.clone(),
            parallels: parallels.clone(),
            delay_ms: delay_ms.clone(),
            live: Arc::new(AtomicUsize::new(0)),
            peak: peak.clone(),
            cache_n: cache_n.clone(),
            embeds: embeds.clone(),
            slot_calls: slot_calls.clone(),
        })
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
        attempts,
        port_taken,
        erases,
        bodies,
        contexts,
        parallels,
        delay_ms,
        peak,
        cache_n,
        embeds,
        slot_calls,
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

/// A server over a temp folder with the *real* engine manager (no fake): for the opt-in end-to-end test that installs
/// llama.cpp and a tiny model (`PITWALL_LLAMA_E2E=1`).
pub async fn real_harness(dir: tempfile::TempDir) -> Harness {
    let dirs = Dirs::at(dir.path());
    let st = AppState::open(dirs.clone(), None).await;
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
        starts: Arc::new(AtomicUsize::new(0)),
        attempts: Arc::new(AtomicUsize::new(0)),
        port_taken: Arc::new(AtomicUsize::new(0)),
        erases: Arc::new(AtomicUsize::new(0)),
        bodies: Arc::new(std::sync::Mutex::new(vec![])),
        contexts: Arc::new(std::sync::Mutex::new(vec![])),
        parallels: Arc::new(std::sync::Mutex::new(vec![])),
        delay_ms: Arc::new(AtomicU64::new(0)),
        peak: Arc::new(AtomicUsize::new(0)),
        cache_n: Arc::new(AtomicU64::new(0)),
        embeds: Arc::new(std::sync::Mutex::new(vec![])),
        slot_calls: Arc::new(std::sync::Mutex::new(vec![])),
        dir,
    }
}

/// The freshly built `pitwall-server`, for tests that launch it as a real child process.
///
/// Building the Tauri shell (`pitwall-app`) before these tests overwrites `target/debug/pitwall-server` with an empty
/// placeholder sidecar (`Permission denied`, then `Exec format error`); the assertion names that cause. Copying the
/// binary elsewhere does not help and is slow on Windows (virus scanners), so the built file is used in place.
pub fn runnable_server(_dir: &std::path::Path) -> String {
    let exe = env!("CARGO_BIN_EXE_pitwall-server");
    let len = std::fs::metadata(exe).map(|m| m.len()).unwrap_or(0);
    assert!(
        len > 100_000,
        "{exe} is {len} bytes: the Tauri shell was built first and replaced it with its placeholder sidecar; run these tests before building pitwall-app"
    );
    exe.to_string()
}

/// An engine that is a real child process: `pitwall-server --fake-engine`, which exits `die_after_ms` after it starts
/// serving (a crashing engine). Used to test restarts and crash-loop detection against a real process.
pub struct ChildEngine {
    pub exe: String,
    pub die_after_ms: u64,
    /// The process ids started, so a test can check they ended.
    pub pids: Arc<std::sync::Mutex<Vec<u32>>>,
}

impl Engine for ChildEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::LlamaCpp
    }
    fn availability(&self) -> Availability {
        Availability::Available
    }
    #[allow(clippy::disallowed_methods)] // a test starting its own helper binary
    fn start<'a>(&'a self, l: &'a Launch) -> BoxFuture<'a, Result<Running, EngineError>> {
        Box::pin(async move {
            let child = tokio::process::Command::new(&self.exe)
                .args([
                    "--fake-engine",
                    "--die-after-ms",
                    &self.die_after_ms.to_string(),
                    "--port",
                    &l.port.to_string(),
                ])
                .stdin(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| EngineError::Start(e.to_string()))?;
            if let Some(pid) = child.id() {
                self.pids.lock().unwrap().push(pid);
            }
            let port = LoopbackPort::new(l.port).ok_or_else(|| EngineError::Start("bad port".into()))?;
            let client = Client::new(port, None);
            for _ in 0..200 {
                if client.json("GET", "/health", None, Duration::from_secs(2)).await.is_ok() {
                    return Ok(Running {
                        child: Some(child),
                        guard: None,
                        client,
                        port: l.port,
                        props: pitwall_core::llama::Props::default(),
                    });
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(EngineError::Start("the child engine did not become healthy".into()))
        })
    }
}
