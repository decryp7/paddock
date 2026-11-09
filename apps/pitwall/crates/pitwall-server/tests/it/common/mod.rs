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
    /// How many of the next starts find their port taken by another listener at the real bind (the race with a parallel
    /// test that `port_taken` does not cover: the fake gets past its own check, then the bind fails).
    pub bind_stolen: Arc<AtomicUsize>,
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
    /// While true, a model load (`start`) waits: a cold load that the test releases when it has seen what it came for.
    pub start_hold: Arc<std::sync::atomic::AtomicBool>,
    /// While true, the chat answer is one tool call instead of text.
    pub tool_call: Arc<std::sync::atomic::AtomicBool>,
    /// Milliseconds between the first text chunk and the rest of the answer (0: all at once).
    pub tail_delay_ms: Arc<AtomicU64>,
}

fn sse_tool_call() -> String {
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"cpu_memory\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"top\\\":3}\"}}]},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n".to_string()
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
            while self.start_hold.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            let taken = loop {
                let n = self.port_taken.load(Ordering::SeqCst);
                if n == 0 {
                    break false;
                }
                if self
                    .port_taken
                    .compare_exchange(n, n - 1, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    break true;
                }
            };
            if taken {
                return Err(EngineError::Start("bind: Address already in use (os error 98)".into()));
            }
            self.contexts.lock().unwrap().push(l.context);
            self.parallels.lock().unwrap().push(l.parallel);
            let (delay, live, peak) = (self.delay_ms.clone(), self.live.clone(), self.peak.clone());
            let bodies = self.bodies.clone();
            if self.fail {
                self.starts.fetch_add(1, Ordering::SeqCst);
                return Err(EngineError::Start("the fake engine refuses to start".into()));
            }
            let erases = self.erases.clone();
            let (tool_call, tail_delay) = (self.tool_call.clone(), self.tail_delay_ms.clone());
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
                        let (tool_call, tail_delay) = (tool_call.clone(), tail_delay.clone());
                        async move {
                            let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                            peak.fetch_max(now, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(delay.load(Ordering::SeqCst))).await;
                            live.fetch_sub(1, Ordering::SeqCst);
                            if let Ok(v) = serde_json::from_slice::<Value>(&body) {
                                bodies.lock().unwrap().push(v);
                            }
                            if tool_call.load(Ordering::SeqCst) {
                                return ([("content-type", "text/event-stream")], sse_tool_call()).into_response();
                            }
                            let all = sse(cache_n.load(Ordering::SeqCst));
                            let tail = tail_delay.load(Ordering::SeqCst);
                            if tail == 0 {
                                return ([("content-type", "text/event-stream")], all).into_response();
                            }
                            // The first chunk now, the rest after `tail` ms: a first token long before the end.
                            let (head, rest) = all.split_at(all.find("\n\n").map_or(0, |i| i + 2));
                            let (head, rest) = (head.to_string(), rest.to_string());
                            let body = futures_util::stream::unfold(0u8, move |step| {
                                let (head, rest) = (head.clone(), rest.clone());
                                async move {
                                    match step {
                                        0 => Some((Ok::<_, std::convert::Infallible>(axum::body::Bytes::from(head)), 1)),
                                        1 => {
                                            tokio::time::sleep(Duration::from_millis(tail)).await;
                                            Some((Ok(axum::body::Bytes::from(rest)), 2))
                                        }
                                        _ => None,
                                    }
                                }
                            });
                            ([("content-type", "text/event-stream")], axum::body::Body::from_stream(body)).into_response()
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
            // Another listener on the port at the real bind (bind_stolen): held across the bind, so it fails as the race does.
            let steal = loop {
                let n = self.bind_stolen.load(Ordering::SeqCst);
                if n == 0 {
                    break false;
                }
                if self
                    .bind_stolen
                    .compare_exchange(n, n - 1, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    break true;
                }
            };
            let thief = if steal {
                Some(std::net::TcpListener::bind(("127.0.0.1", l.port)).map_err(|e| EngineError::Start(e.to_string()))?)
            } else {
                None
            };
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", l.port))
                .await
                .map_err(|e| EngineError::Start(e.to_string()))?;
            drop(thief);
            // A start is a model load: counted once the engine serves, never for a bind that lost the port.
            self.starts.fetch_add(1, Ordering::SeqCst);
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
    pub bind_stolen: Arc<AtomicUsize>,
    pub erases: Arc<AtomicUsize>,
    pub bodies: Arc<std::sync::Mutex<Vec<Value>>>,
    pub contexts: Arc<std::sync::Mutex<Vec<u32>>>,
    pub parallels: Arc<std::sync::Mutex<Vec<u32>>>,
    pub delay_ms: Arc<AtomicU64>,
    pub peak: Arc<AtomicUsize>,
    pub cache_n: Arc<AtomicU64>,
    pub embeds: Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    pub slot_calls: Arc<std::sync::Mutex<Vec<String>>>,
    pub start_hold: Arc<std::sync::atomic::AtomicBool>,
    pub tool_call: Arc<std::sync::atomic::AtomicBool>,
    pub tail_delay_ms: Arc<AtomicU64>,
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
    let bind_stolen = Arc::new(AtomicUsize::new(0));
    let erases = Arc::new(AtomicUsize::new(0));
    let bodies = Arc::new(std::sync::Mutex::new(vec![]));
    let contexts = Arc::new(std::sync::Mutex::new(vec![]));
    let parallels = Arc::new(std::sync::Mutex::new(vec![]));
    let delay_ms = Arc::new(AtomicU64::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let cache_n = Arc::new(AtomicU64::new(0));
    let embeds = Arc::new(std::sync::Mutex::new(vec![]));
    let slot_calls = Arc::new(std::sync::Mutex::new(vec![]));
    let start_hold = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let tool_call = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let tail_delay_ms = Arc::new(AtomicU64::new(0));
    let engine: Arc<dyn Engine> = other.unwrap_or_else(|| {
        Arc::new(FakeEngine {
            starts: starts.clone(),
            attempts: attempts.clone(),
            port_taken: port_taken.clone(),
            bind_stolen: bind_stolen.clone(),
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
            start_hold: start_hold.clone(),
            tool_call: tool_call.clone(),
            tail_delay_ms: tail_delay_ms.clone(),
        })
    });
    let st = AppState::open(dirs.clone(), Some(engine)).await;
    // Loads are planned against a roomy machine, never the CI runner's free memory (slots.rs pins small ones on purpose).
    st.serving
        .set_memory_reader(std::sync::Arc::new(|_: &Hardware| pitwall_core::memory::System {
            total: 64 << 30,
            available: 48 << 30,
            ..Default::default()
        }));
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
        bind_stolen,
        erases,
        bodies,
        contexts,
        parallels,
        delay_ms,
        peak,
        cache_n,
        embeds,
        slot_calls,
        start_hold,
        tool_call,
        tail_delay_ms,
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
        bind_stolen: Arc::new(AtomicUsize::new(0)),
        erases: Arc::new(AtomicUsize::new(0)),
        bodies: Arc::new(std::sync::Mutex::new(vec![])),
        contexts: Arc::new(std::sync::Mutex::new(vec![])),
        parallels: Arc::new(std::sync::Mutex::new(vec![])),
        delay_ms: Arc::new(AtomicU64::new(0)),
        peak: Arc::new(AtomicUsize::new(0)),
        cache_n: Arc::new(AtomicU64::new(0)),
        embeds: Arc::new(std::sync::Mutex::new(vec![])),
        slot_calls: Arc::new(std::sync::Mutex::new(vec![])),
        start_hold: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        tool_call: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        tail_delay_ms: Arc::new(AtomicU64::new(0)),
        dir,
    }
}

/// The freshly built `pitwall-fake-engine` (ADR 0073: a test-only bin of this package, built through the self
/// dev-dependency), for tests that launch an engine as a real child process.
///
/// Building the Tauri shell (`pitwall-app`) overwrites `target/debug/pitwall-server` with an empty placeholder sidecar;
/// the fake engine is a different file and is not touched by it, but the size check stays to name any such cause.
///
/// The binary is started once (`--version`) and waited for here, before any test step that has a time budget: macOS
/// checks a freshly linked program on its first start, and while other jobs on the machine start fresh binaries too
/// (three CI runners on one Mac, CI run 7775) that check queues behind theirs for many seconds (`syspolicyd` busy, the
/// child still at `_dyld_start`). Later starts of the same file are not checked again.
#[allow(clippy::disallowed_methods)] // a test starting its own helper binary and waiting for it to end
pub fn runnable_fake_engine() -> String {
    let exe = env!("CARGO_BIN_EXE_pitwall-fake-engine");
    let len = std::fs::metadata(exe).map(|m| m.len()).unwrap_or(0);
    assert!(len > 100_000, "{exe} is {len} bytes: it was replaced by a placeholder; rebuild it");
    let started = std::time::Instant::now();
    let out = std::process::Command::new(exe)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap_or_else(|e| panic!("{exe} cannot be started: {e}"));
    let printed = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && printed.starts_with("pitwall-fake-engine "),
        "{exe} --version: {} {printed}{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    eprintln!("the first start of {exe} took {:.2} s", started.elapsed().as_secs_f64());
    exe.to_string()
}

/// How long [`ChildEngine`] waits for a child that neither answers nor exits before it reports a hang. Never a verdict
/// on a slow start: it only turns a hang into a message (pid, port, what the child printed) before the runner's kill
/// (nextest `ci`: 60 s), and bounds a run under plain `cargo test`.
pub const CHILD_HANG_GUARD: Duration = Duration::from_secs(45);

/// An engine that is a real child process: `pitwall-fake-engine`, which exits `die_after_ms` after it starts
/// serving (a crashing engine). Used to test restarts, crash-loop detection and port races against a real process.
/// Build it with [`ChildEngine::new`] and override what a test needs.
pub struct ChildEngine {
    pub exe: String,
    pub die_after_ms: u64,
    /// The child's `/health` answers 503 "loading" until this file exists (`--ready-when`): an engine that is slow
    /// to become ready, released by the test on an event instead of after a duration.
    pub ready_when: Option<std::path::PathBuf>,
    /// The first start finds its port taken (held by the test process until that start ends): another program took
    /// the probed port before the engine bound it.
    pub take_first_port: bool,
    /// The pause between two health polls.
    pub poll_every: Duration,
    /// Counts the health polls of all starts together, so a test can act after a given number of them.
    pub polls: Option<tokio::sync::watch::Sender<usize>>,
    /// How each start ended, in order.
    pub outcomes: Arc<std::sync::Mutex<Vec<StartOutcome>>>,
    /// The process ids started, so a test can check they ended.
    pub pids: Arc<std::sync::Mutex<Vec<u32>>>,
    /// The port of every start, in order.
    pub ports: Arc<std::sync::Mutex<Vec<u16>>>,
}

impl ChildEngine {
    /// A child that serves at once and does not crash on its own.
    pub fn new(exe: String) -> Self {
        Self {
            exe,
            die_after_ms: 600_000,
            ready_when: None,
            take_first_port: false,
            poll_every: Duration::from_millis(50),
            polls: None,
            outcomes: Default::default(),
            pids: Default::default(),
            ports: Default::default(),
        }
    }
}

/// How one [`ChildEngine`] start ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    /// The child answered `/health` with 200.
    Healthy,
    /// The child exited before it answered.
    Exited,
    /// [`CHILD_HANG_GUARD`] ran out: neither of the above.
    HangGuard,
}

/// Takes a fresh loopback port for the life of the handle, closing every connection at once (so a health poll fails
/// fast). The port is read off a listener that stays open, never off one that is dropped (C39).
pub async fn held_port() -> (u16, tokio::task::JoinHandle<()>) {
    let l = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind a loopback port");
    let port = l.local_addr().expect("the bound address").port();
    let h = tokio::spawn(async move {
        while let Ok((s, _)) = l.accept().await {
            drop(s);
        }
    });
    (port, h)
}

/// Takes `port` on 127.0.0.1 until the handle is aborted, closing every connection at once (so a health poll fails
/// fast instead of waiting for an answer that never comes). `None` when the bind fails: then someone else already has
/// the port, which is the state the test wants anyway.
async fn hold_port(port: u16) -> Option<tokio::task::JoinHandle<()>> {
    let l = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.ok()?;
    Some(tokio::spawn(async move {
        while let Ok((s, _)) = l.accept().await {
            drop(s);
        }
    }))
}

/// Runs `fut` (a start or a load of `engine`, whose `polls` sender belongs to `polled`) and fails at once if the first
/// child has ended but the double keeps polling it instead of reporting the exit: a missed exit would otherwise show
/// only as a slow test ending in the hang guard. Allows a few polls for the child's end to be seen.
pub async fn noticing_the_first_exit<F: std::future::Future>(
    engine: &ChildEngine,
    mut polled: tokio::sync::watch::Receiver<usize>,
    fut: F,
) -> F::Output {
    const SLACK: usize = 5;
    let (pids, outcomes) = (engine.pids.clone(), engine.outcomes.clone());
    let watch = async move {
        let seen = loop {
            if polled.changed().await.is_err() {
                return std::future::pending().await;
            }
            let first = pids.lock().unwrap().first().copied();
            if first.is_some_and(|pid| !pitwall_server::system::pid_alive(pid)) {
                break *polled.borrow();
            }
        };
        let _ = polled.wait_for(|n| *n > seen + SLACK).await;
        if outcomes.lock().unwrap().is_empty() {
            panic!("the first child ended, yet the double polled it {SLACK} more times without reporting the exit");
        }
        std::future::pending::<()>().await
    };
    tokio::select! {
        r = fut => r,
        () = watch => unreachable!(),
    }
}

impl Engine for ChildEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::LlamaCpp
    }
    fn availability(&self) -> Availability {
        Availability::Available
    }
    /// Waits for health or exit, like `LlamaCpp::start`, never for a fixed number of polls: the first start of a
    /// freshly built binary on a busy Mac took longer than such a budget (CI run 7775). Instead of the product's
    /// 5-minute deadline it has [`CHILD_HANG_GUARD`], and the runner's kill bounds everything. An exit is reported with
    /// what the child printed, so a taken port reads as one and the load retries on a fresh port. The child's stderr
    /// goes to the test's own stderr as it comes, the last 4000 bytes kept for that report (as `engines.rs` does).
    #[allow(clippy::disallowed_methods)] // a test starting its own helper binary
    fn start<'a>(&'a self, l: &'a Launch) -> BoxFuture<'a, Result<Running, EngineError>> {
        Box::pin(async move {
            self.ports.lock().unwrap().push(l.port);
            let first = self.ports.lock().unwrap().len() == 1;
            let held = if self.take_first_port && first {
                hold_port(l.port).await
            } else {
                None
            };
            let mut args = vec![
                "--die-after-ms".to_string(),
                self.die_after_ms.to_string(),
                "--port".into(),
                l.port.to_string(),
            ];
            if let Some(gate) = &self.ready_when {
                args.extend(["--ready-when".to_string(), gate.to_string_lossy().into_owned()]);
            }
            let mut child = tokio::process::Command::new(&self.exe)
                .args(&args)
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| EngineError::Start(e.to_string()))?;
            let pid = child.id();
            if let Some(pid) = pid {
                self.pids.lock().unwrap().push(pid);
            }
            let tail = Arc::new(std::sync::Mutex::new(String::new()));
            let reader = child.stderr.take().map(|mut err| {
                let tail = tail.clone();
                tokio::spawn(async move {
                    use tokio::io::AsyncReadExt;
                    let mut buf = [0u8; 1024];
                    while let Ok(n) = err.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                        let text = String::from_utf8_lossy(&buf[..n]);
                        eprint!("{text}");
                        let mut g = tail.lock().unwrap();
                        g.push_str(&text);
                        if g.len() > 4000 {
                            let cut = g.char_indices().map(|(i, _)| i).find(|i| *i >= g.len() - 4000).unwrap_or(0);
                            g.drain(..cut);
                        }
                    }
                })
            });
            let said = |tail: &Arc<std::sync::Mutex<String>>| tail.lock().unwrap().trim().to_string();
            let port = LoopbackPort::new(l.port).ok_or_else(|| EngineError::Start("bad port".into()))?;
            let client = Client::new(port, None);
            let started = std::time::Instant::now();
            let mut next_note = Duration::from_secs(10);
            let outcome = loop {
                if let Ok(Some(status)) = child.try_wait() {
                    if let Some(r) = reader {
                        let _ = r.await;
                    }
                    self.outcomes.lock().unwrap().push(StartOutcome::Exited);
                    break Err(EngineError::Start(format!("the child engine exited ({status}): {}", said(&tail))));
                }
                if let Ok((200, _)) = client.json("GET", "/health", None, Duration::from_secs(2)).await {
                    self.outcomes.lock().unwrap().push(StartOutcome::Healthy);
                    break Ok(Running {
                        child: Some(child),
                        guard: None,
                        client,
                        port: l.port,
                        props: pitwall_core::llama::Props::default(),
                    });
                }
                if let Some(p) = &self.polls {
                    p.send_modify(|n| *n += 1);
                }
                let waited = started.elapsed();
                if waited >= CHILD_HANG_GUARD {
                    // Deliberately without the child's output (it is in the test's stderr above): its words, such as a
                    // bind error, must not make the product read a hang as a taken port.
                    self.outcomes.lock().unwrap().push(StartOutcome::HangGuard);
                    break Err(EngineError::Start(format!(
                        "hang guard: the child engine (pid {pid:?}, port {}) neither answered /health nor ended within {} s; its output is in the test's stderr",
                        l.port,
                        CHILD_HANG_GUARD.as_secs()
                    )));
                }
                if waited >= next_note {
                    eprintln!(
                        "the child engine (pid {pid:?}, port {}) is still starting after {} s; waiting until it answers or exits",
                        l.port,
                        waited.as_secs()
                    );
                    next_note += Duration::from_secs(10);
                }
                // conformance: hang-guard — poll interval for an external process's /health; no in-process event exists; bounded by a 45 s hang guard
                tokio::time::sleep(self.poll_every).await;
            };
            if let Some(h) = held {
                h.abort();
            }
            outcome
        })
    }
}
