//! A fake Pitwall for tests: contract-shaped answers on an ephemeral `127.0.0.1` port, for this crate's contract tests and
//! for the apps that use it (feature `testing`). It serves exactly the routes of [`PITWALL_ROUTES`], which mirror the
//! routes of Pitwall's own router that the client contract uses, with Pitwall's answer and error shapes and `X-Pitwall-*`
//! headers. Scriptable: pairing approved, denied or pending; another `apiVersion`; a revoked key; a server that dies
//! mid-stream; one that holds its answer until released; typed failures with `Retry-After`. It never uses Pitwall's real
//! port, data folder or credential store, and it waits on conditions, never on the clock.

use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

/// The routes of Pitwall's router (`crates/pitwall-server/src/http.rs`) that the client contract uses: method and path, with
/// `{id}` and `{task}` for path parameters. The contract test checks that the fake serves each and that Pitwall's router, when
/// its source is at hand, still has each.
pub const PITWALL_ROUTES: &[(&str, &str)] = &[
    ("GET", "/v1/models"),
    ("POST", "/v1/chat/completions"),
    ("POST", "/v1/embeddings"),
    ("GET", "/x/v1/health"),
    ("POST", "/x/v1/pair"),
    ("GET", "/x/v1/pair/{id}"),
    ("POST", "/x/v1/select"),
    ("GET", "/x/v1/models"),
    ("POST", "/x/v1/tasks/{task}"),
    ("POST", "/x/v1/tokens/count"),
    ("POST", "/x/v1/tokens/truncate"),
    ("GET", "/x/v1/jobs"),
    ("DELETE", "/x/v1/jobs/{id}"),
];

/// Request headers, names lowercased.
pub type Headers = Vec<(String, String)>;
/// One request the fake saw: method, path, headers, body.
pub type Seen = (String, String, Headers, String);

/// How the fake answers a chat.
#[derive(Debug, Clone)]
pub enum ChatScript {
    /// Streams these pieces, then `finish_reason`.
    Answer {
        /// The text deltas.
        pieces: Vec<String>,
        /// The finish reason.
        finish: String,
    },
    /// Streams these pieces, then drops the connection (Pitwall stopped).
    DieAfter(Vec<String>),
    /// Sends an error event in the middle of the stream.
    ErrorEvent(String),
    /// Streams the pieces, then holds the stream open until [`FakePitwall::release`] (cancel and idle tests).
    HoldAfter(Vec<String>),
    /// Holds the answer before its first byte until [`FakePitwall::release`] (first-byte and cancel tests).
    HoldFirst,
    /// Sends this text as the body of the stream, as it is, then ends it (malformed, oversized or unfinished streams).
    Raw(String),
}

/// A scripted failure of the next request to a route.
#[derive(Debug, Clone)]
pub struct Failure {
    /// The HTTP status.
    pub status: u16,
    /// Pitwall's error code.
    pub code: String,
    /// The message.
    pub message: String,
    /// `Retry-After` seconds, when sent.
    pub retry_after: Option<u32>,
}

impl Failure {
    /// `503 overloaded` with `Retry-After: 5`, as Pitwall sends it.
    pub fn overloaded() -> Failure {
        Failure {
            status: 503,
            code: "overloaded".into(),
            message: "the queue is full".into(),
            retry_after: Some(5),
        }
    }
}

/// How the fake answers pairing polls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairMode {
    /// Approve with this key (delivered once).
    Approve(String),
    /// Deny.
    Deny,
    /// Never decide.
    Pending,
    /// Approved, but the key was already delivered earlier (`key: null`).
    ApprovedWithoutKey,
}

/// A model of the fake's library.
#[derive(Debug, Clone)]
pub struct FakeModel {
    /// The id.
    pub id: String,
    /// What it can do.
    pub capabilities: Vec<String>,
    /// Reasons a capability is blocked, `(capability, reason)`.
    pub blocked: Vec<(String, String)>,
    /// The trained context.
    pub context_max: u64,
    /// The context per slot, when the fake plays a newer Pitwall.
    pub context_per_slot: Option<u64>,
}

impl FakeModel {
    /// A chat/json/tools model.
    pub fn chat(id: &str, context_max: u64) -> FakeModel {
        FakeModel {
            id: id.into(),
            capabilities: vec!["chat".into(), "json".into(), "tools".into()],
            blocked: vec![],
            context_max,
            context_per_slot: None,
        }
    }
}

/// The fake's script and what it saw.
#[derive(Debug, Clone)]
pub struct FakeState {
    /// Pitwall's version.
    pub version: String,
    /// The `apiVersion` it reports.
    pub api_version: u32,
    /// `serving`, `paused` or `stopped`.
    pub serving: String,
    /// Keys it accepts.
    pub keys: Vec<String>,
    /// How pairing ends.
    pub pair: PairMode,
    /// Polls answered `pending` before the outcome.
    pub pending_polls: usize,
    /// The library.
    pub models: Vec<FakeModel>,
    /// A preset the user defined, listed in `/v1/models`.
    pub preset: Option<String>,
    /// How chat answers.
    pub chat: ChatScript,
    /// Failures for the next requests to a path (consumed one per request, in order).
    pub failures: Vec<(String, Failure)>,
    /// `X-Pitwall-Job-Id` on chat answers (a newer Pitwall).
    pub job_id: Option<String>,
    /// `X-Pitwall-Cold-Start-Ms` on chat answers.
    pub cold_start_ms: Option<u64>,
    /// `X-Pitwall-Cache` on chat and embeddings answers.
    pub cache: Option<String>,
    /// Queued/running jobs the fake lists and lets be cancelled.
    pub jobs: Vec<String>,
    /// Answer embeddings with one vector too few.
    pub embed_short: bool,
    /// Answer embeddings malformed: 1 a vector is missing, 2 a vector holds a non-number.
    pub embed_bad: u8,
    /// The pairing code Pitwall shows.
    pub pair_code: String,
    /// Pairing requests started so far (their ids are `pr1-abc`, `pr2-abc`, ...).
    pub pairs: usize,
    /// `select` answers this model although it is not in the list (a library that changed between two calls).
    pub select_model: Option<String>,
    /// Every request: (method, path, headers lowercased, body).
    pub seen: Vec<Seen>,
}

impl Default for FakeState {
    fn default() -> Self {
        FakeState {
            version: "1.0.4".into(),
            api_version: 1,
            serving: "serving".into(),
            keys: vec![],
            pair: PairMode::Approve("pw_fakekey".into()),
            pending_polls: 1,
            models: vec![FakeModel::chat("qwen3-8b-q4km", 32768)],
            preset: None,
            chat: ChatScript::Answer {
                pieces: vec!["Hello".into(), " from".into(), " Pitwall.".into()],
                finish: "stop".into(),
            },
            failures: vec![],
            job_id: None,
            cold_start_ms: None,
            cache: None,
            jobs: vec![],
            embed_short: false,
            embed_bad: 0,
            pair_code: "482913".into(),
            pairs: 0,
            select_model: None,
            seen: vec![],
        }
    }
}

struct Shared {
    state: Mutex<FakeState>,
    stop: AtomicBool,
    /// Set by `release`; `HoldFirst` and `HoldAfter` wait on it.
    gate: (Mutex<bool>, Condvar),
}

/// A running fake server.
pub struct FakePitwall {
    /// The loopback port.
    pub port: u16,
    shared: Arc<Shared>,
    /// The accept loop; `kill` waits for it, because the port is closed only when it drops the listener.
    acceptor: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Drop for FakePitwall {
    fn drop(&mut self) {
        self.kill();
    }
}

impl FakePitwall {
    /// Starts a fake with this script on an ephemeral loopback port.
    pub fn start(state: FakeState) -> FakePitwall {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
        let port = listener.local_addr().expect("local address").port();
        let shared = Arc::new(Shared {
            state: Mutex::new(state),
            stop: AtomicBool::new(false),
            gate: (Mutex::new(false), Condvar::new()),
        });
        let s = shared.clone();
        let acceptor = std::thread::spawn(move || {
            // Blocks in `accept`; `kill` connects once to wake it.
            for conn in listener.incoming() {
                if s.stop.load(Ordering::Relaxed) {
                    return;
                }
                let Ok(conn) = conn else { continue };
                let s = s.clone();
                std::thread::spawn(move || serve(conn, &s));
            }
        });
        FakePitwall {
            port,
            shared,
            acceptor: Mutex::new(Some(acceptor)),
        }
    }

    /// The script and the log.
    pub fn state(&self) -> MutexGuard<'_, FakeState> {
        self.shared.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Writes a `discovery.json` for this server into `dir` with process id `pid`.
    pub fn write_discovery(&self, dir: &Path, pid: u32, api_version: u32) {
        let port = self.port;
        std::fs::create_dir_all(dir).expect("discovery folder");
        let text = json!({"format": 1, "version": self.state().version, "apiVersion": api_version, "pid": pid, "port": port,
            "baseUrl": format!("http://127.0.0.1:{port}"), "startedAt": 1});
        std::fs::write(dir.join(crate::discovery::FILE_NAME), text.to_string())
            .expect("discovery file");
    }

    /// Lets held answers go on (`HoldFirst`, `HoldAfter`).
    pub fn release(&self) {
        let (m, c) = &self.shared.gate;
        *m.lock().unwrap_or_else(|p| p.into_inner()) = true;
        c.notify_all();
    }

    /// Stops listening and drops the connections it is serving (Pitwall quit).
    pub fn kill(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.release();
        // Wake the accept loop until it has ended: once it drops the listener the port refuses connections, so a request
        // made after `kill` returns never reaches a half-closed server (it would read as "stopped answering", not "not
        // running"). A wake-up that is lost (a full backlog) is repeated; the wait is bounded.
        let acceptor = self
            .acceptor
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(acceptor) = acceptor {
            let give_up = std::time::Instant::now() + Duration::from_secs(10);
            while !acceptor.is_finished() && std::time::Instant::now() < give_up {
                let _ = TcpStream::connect(("127.0.0.1", self.port));
                std::thread::yield_now();
            }
            if acceptor.is_finished() {
                let _ = acceptor.join();
            }
        }
    }

    /// The requests to `path` it saw: (method, headers, body).
    pub fn requests_to(&self, path: &str) -> Vec<(String, Headers, String)> {
        self.state()
            .seen
            .iter()
            .filter(|(_, p, _, _)| p == path)
            .map(|(m, _, h, b)| (m.clone(), h.clone(), b.clone()))
            .collect()
    }
}

fn read_request(conn: &mut TcpStream) -> Option<Seen> {
    conn.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let n = conn.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, path) = (first.next()?.to_string(), first.next()?.to_string());
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let len: usize = headers
        .iter()
        .find(|(n, _)| n == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = conn.read(&mut tmp).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    Some((
        method,
        path,
        headers,
        String::from_utf8_lossy(&body).into_owned(),
    ))
}

fn respond(conn: &mut TcpStream, status: u16, extra: &[(&str, String)], body: &Value) {
    let text = body.to_string();
    let mut head = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        text.len()
    );
    for (n, v) in extra {
        head.push_str(&format!("{n}: {v}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    let _ = conn.write_all(head.as_bytes());
    let _ = conn.write_all(text.as_bytes());
}

/// An error in the shape of the route's API family: OpenAI-shaped for `/v1/`, extended for `/x/`.
fn error(conn: &mut TcpStream, path: &str, f: &Failure) {
    let body = if path.starts_with("/x/") {
        json!({"error": {"code": f.code, "message": f.message}})
    } else {
        json!({"error": {"message": f.message, "type": "x", "param": null, "code": f.code}})
    };
    let extra: Vec<(&str, String)> = f
        .retry_after
        .map(|s| ("Retry-After", s.to_string()))
        .into_iter()
        .collect();
    respond(conn, f.status, &extra, &body);
}

fn fail(conn: &mut TcpStream, path: &str, status: u16, code: &str, message: &str) {
    error(
        conn,
        path,
        &Failure {
            status,
            code: code.into(),
            message: message.into(),
            retry_after: None,
        },
    );
}

fn chunk(conn: &mut TcpStream, data: &str) -> bool {
    let frame = format!("data: {data}\n\n");
    conn.write_all(format!("{:x}\r\n{frame}\r\n", frame.len()).as_bytes())
        .is_ok()
        && conn.flush().is_ok()
}

fn wait_for_release(s: &Shared) {
    let (m, c) = &s.gate;
    let mut released = m.lock().unwrap_or_else(|p| p.into_inner());
    while !*released && !s.stop.load(Ordering::Relaxed) {
        released = c.wait(released).unwrap_or_else(|p| p.into_inner());
    }
}

/// Matches `path` against a route pattern with `{param}` segments.
pub fn matches_route(pattern: &str, path: &str) -> bool {
    let (p, q): (Vec<&str>, Vec<&str>) = (
        pattern.split('/').collect(),
        path.split('?').next().unwrap_or("").split('/').collect(),
    );
    p.len() == q.len()
        && p.iter()
            .zip(&q)
            .all(|(a, b)| (a.starts_with('{') && !b.is_empty()) || a == b)
}

fn serve(mut conn: TcpStream, shared: &Shared) {
    let Some((method, path, headers, body)) = read_request(&mut conn) else {
        return;
    };
    if shared.stop.load(Ordering::Relaxed) {
        return;
    }
    let lock = || shared.state.lock().unwrap_or_else(|p| p.into_inner());
    lock()
        .seen
        .push((method.clone(), path.clone(), headers.clone(), body.clone()));
    let header = |n: &str| headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
    let key = header("authorization").and_then(|v| v.strip_prefix("Bearer ").map(str::to_string));
    let scripted = {
        let mut st = lock();
        st.failures
            .iter()
            .position(|(p, _)| *p == path)
            .map(|i| st.failures.remove(i).1)
    };
    let route = PITWALL_ROUTES
        .iter()
        .find(|(m, p)| *m == method && matches_route(p, &path))
        .map(|(_, p)| *p);
    // Health and pairing need no key; everything else does.
    let open = matches!(
        route,
        Some("/x/v1/health" | "/x/v1/pair" | "/x/v1/pair/{id}")
    );
    if !open && !key.as_ref().is_some_and(|k| lock().keys.contains(k)) {
        return fail(
            &mut conn,
            &path,
            401,
            "unauthorized",
            "a valid Pitwall API key is required",
        );
    }
    if let Some(f) = scripted {
        return error(&mut conn, &path, &f);
    }
    let s = lock().clone();
    let provenance = || -> Vec<(&'static str, String)> {
        let mut h = vec![
            (
                "X-Pitwall-Model",
                s.models.first().map(|m| m.id.clone()).unwrap_or_default(),
            ),
            ("X-Pitwall-Engine", "fake".to_string()),
            ("X-Pitwall-Chosen-By", "capabilities".to_string()),
        ];
        if let Some(c) = s.cold_start_ms {
            h.push(("X-Pitwall-Cold-Start-Ms", c.to_string()));
        }
        if let Some(c) = &s.cache {
            h.push(("X-Pitwall-Cache", c.clone()));
        }
        if let Some(a) = &s.preset {
            h.push(("X-Pitwall-Alias", a.clone()));
        }
        h
    };
    let json_body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    match route {
        Some("/x/v1/health") => respond(
            &mut conn,
            200,
            &[],
            &json!({"ok": true, "name": "pitwall", "version": s.version, "apiVersion": s.api_version, "serving": s.serving, "offline": false, "loadedModels": 0}),
        ),
        Some("/x/v1/pair") => {
            let started = {
                let mut st = lock();
                st.pairs += 1;
                st.pairs
            };
            let name = json_body
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if name.is_empty() {
                return fail(
                    &mut conn,
                    &path,
                    400,
                    "invalid_request",
                    "the app needs a name",
                );
            }
            respond(
                &mut conn,
                202,
                &[],
                &json!({"pairingId": format!("pr{}-abc", started), "code": s.pair_code, "expiresInSecs": 300}),
            );
        }
        Some("/x/v1/pair/{id}") => {
            let answer = {
                let mut st = lock();
                if st.pending_polls > 0 {
                    st.pending_polls -= 1;
                    json!({"status": "pending"})
                } else {
                    match st.pair.clone() {
                        PairMode::Approve(k) => {
                            st.keys.push(k.clone());
                            // Delivered once.
                            st.pair = PairMode::Pending;
                            json!({"status": "approved", "key": k})
                        }
                        PairMode::Deny => json!({"status": "denied"}),
                        PairMode::Pending => json!({"status": "pending"}),
                        PairMode::ApprovedWithoutKey => {
                            json!({"status": "approved", "key": null, "note": "the key was already delivered"})
                        }
                    }
                }
            };
            respond(&mut conn, 200, &[], &answer);
        }
        Some("/v1/models") => {
            let mut data: Vec<Value> = s
                .models
                .iter()
                .map(|m| json!({"id": m.id, "object": "model"}))
                .collect();
            if let Some(p) = &s.preset {
                data.push(json!({"id": p, "object": "model"}));
            }
            respond(
                &mut conn,
                200,
                &[],
                &json!({"object": "list", "data": data}),
            );
        }
        Some("/x/v1/models") => {
            let models: Vec<Value> = s
                .models
                .iter()
                .map(|m| {
                    let blocked: Vec<Value> = m.blocked.iter().map(|(c, r)| json!({"capability": c, "reason": r})).collect();
                    let mut v = json!({"id": m.id, "capabilities": m.capabilities, "unavailable": blocked, "contextMax": m.context_max, "sizeBytes": 1, "loaded": false});
                    if let Some(c) = m.context_per_slot {
                        v["contextPerSlot"] = json!(c);
                    }
                    v
                })
                .collect();
            respond(&mut conn, 200, &[], &json!({ "models": models }));
        }
        Some("/x/v1/select") => {
            let spec = json_body
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("auto")
                .to_string();
            let wants = |cap: &str| spec.contains(cap);
            let pick = s.models.iter().find(|m| {
                ["vision", "embeddings"]
                    .iter()
                    .all(|c| !wants(c) || m.capabilities.iter().any(|x| x == c))
            });
            match (&s.select_model, pick) {
                (Some(over), _) => respond(
                    &mut conn,
                    200,
                    &[],
                    &json!({"model": over, "chosenBy": "capabilities", "loaded": false}),
                ),
                (None, Some(m)) => respond(
                    &mut conn,
                    200,
                    &[],
                    &json!({"model": m.id, "chosenBy": "capabilities", "loaded": false}),
                ),
                (None, None) => fail(
                    &mut conn,
                    &path,
                    404,
                    "not_found",
                    "no installed model has the capabilities asked for",
                ),
            }
        }
        Some("/x/v1/jobs") => {
            let running: Vec<Value> = s.jobs.iter().map(|id| json!({"id": id, "appId": "debrief", "state": "running", "tokens": 0, "priority": "interactive", "kind": "chat", "submittedAt": 1})).collect();
            respond(
                &mut conn,
                200,
                &[],
                &json!({"running": running, "waiting": [], "depth": s.jobs.len()}),
            );
        }
        Some("/x/v1/jobs/{id}") => {
            let id = path.rsplit('/').next().unwrap_or_default().to_string();
            let known = {
                let mut st = lock();
                let n = st.jobs.len();
                st.jobs.retain(|j| *j != id);
                st.jobs.len() != n
            };
            if known {
                respond(
                    &mut conn,
                    200,
                    &[],
                    &json!({"cancelled": true, "wasRunning": true}),
                );
            } else {
                fail(&mut conn, &path, 404, "not_found", "no such job");
            }
        }
        Some("/x/v1/tokens/count") => match json_body.get("text").and_then(Value::as_str) {
            Some(t) => respond(
                &mut conn,
                200,
                &[],
                &json!({"tokens": t.len().div_ceil(4), "estimate": true}),
            ),
            None => fail(
                &mut conn,
                &path,
                400,
                "invalid_request",
                "`text` is required",
            ),
        },
        Some("/x/v1/tokens/truncate") => {
            let (Some(t), Some(max)) = (
                json_body.get("text").and_then(Value::as_str),
                json_body.get("maxTokens").and_then(Value::as_u64),
            ) else {
                return fail(
                    &mut conn,
                    &path,
                    400,
                    "invalid_request",
                    "`text` and `maxTokens` are required",
                );
            };
            let keep = usize::try_from(max).unwrap_or(usize::MAX).saturating_mul(4);
            let cut = t.len() > keep;
            let text: String = if !cut {
                t.to_string()
            } else if json_body.get("keep").and_then(Value::as_str) == Some("end") {
                t.chars()
                    .rev()
                    .take(keep)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect()
            } else {
                t.chars().take(keep).collect()
            };
            respond(
                &mut conn,
                200,
                &[],
                &json!({"text": text, "truncated": cut}),
            );
        }
        Some("/x/v1/tasks/{task}") => {
            let task = path.rsplit('/').next().unwrap_or_default();
            if !["summarize", "classify", "extract", "translate"].contains(&task) {
                return fail(
                    &mut conn,
                    &path,
                    404,
                    "not_found",
                    "tasks are summarize, classify, extract and translate",
                );
            }
            let result = if task == "classify" || task == "extract" {
                json!({"label": "ok"})
            } else {
                json!(format!(
                    "{task} of {} chars",
                    json_body
                        .get("text")
                        .and_then(Value::as_str)
                        .map_or(0, str::len)
                ))
            };
            respond(
                &mut conn,
                200,
                &provenance(),
                &json!({"result": result, "template": task, "model": s.models.first().map(|m| m.id.clone()), "usage": {"promptTokens": 5, "completionTokens": 2}}),
            );
        }
        Some("/v1/embeddings") => {
            let inputs: Vec<String> = json_body
                .get("input")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let n = if s.embed_short {
                inputs.len().saturating_sub(1)
            } else {
                inputs.len()
            };
            let data: Vec<Value> = inputs.iter().take(n).enumerate().map(|(i, t)| match s.embed_bad {
                1 => json!({"object": "embedding", "index": i}),
                2 => json!({"object": "embedding", "index": i, "embedding": ["x"]}),
                _ => json!({"object": "embedding", "index": i, "embedding": [t.len() as f32, i as f32]}),
            }).collect();
            respond(
                &mut conn,
                200,
                &provenance(),
                &json!({"object": "list", "data": data, "model": "embed", "usage": {"prompt_tokens": inputs.len() * 2, "total_tokens": inputs.len() * 2}}),
            );
        }
        Some("/v1/chat/completions") => chat(&mut conn, shared, &s, &provenance()),
        _ => fail(&mut conn, &path, 404, "not_found", "no such route"),
    }
}

fn chat(
    conn: &mut TcpStream,
    shared: &Shared,
    s: &FakeState,
    provenance: &[(&'static str, String)],
) {
    if matches!(s.chat, ChatScript::HoldFirst) {
        wait_for_release(shared);
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
    }
    let pieces = match &s.chat {
        ChatScript::Answer { pieces, .. }
        | ChatScript::DieAfter(pieces)
        | ChatScript::HoldAfter(pieces) => pieces.clone(),
        ChatScript::ErrorEvent(_) => vec!["partial".into()],
        ChatScript::HoldFirst => vec!["late".into()],
        ChatScript::Raw(_) => vec![],
    };
    let mut head = String::from(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n",
    );
    for (n, v) in provenance {
        head.push_str(&format!("{n}: {v}\r\n"));
    }
    if let Some(j) = &s.job_id {
        head.push_str(&format!("X-Pitwall-Job-Id: {j}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    let _ = conn.write_all(head.as_bytes());
    if let ChatScript::Raw(text) = &s.chat {
        // The text goes out as it is, as one chunk, and the stream ends.
        let _ = conn.write_all(format!("{:x}\r\n", text.len()).as_bytes());
        let _ = conn.write_all(text.as_bytes());
        let _ = conn.write_all(b"\r\n0\r\n\r\n");
        return;
    }
    let role = json!({"choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}, "finish_reason": null}]});
    if !chunk(conn, &role.to_string()) {
        return;
    }
    for p in pieces {
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
        let c = json!({"choices": [{"index": 0, "delta": {"content": p}, "finish_reason": null}]});
        if !chunk(conn, &c.to_string()) {
            return;
        }
    }
    match &s.chat {
        ChatScript::DieAfter(_) => {
            let _ = conn.shutdown(Shutdown::Both);
        }
        ChatScript::HoldAfter(_) => wait_for_release(shared),
        ChatScript::ErrorEvent(m) => {
            chunk(
                conn,
                &json!({"error": {"message": m, "type": "server_error", "code": "engine_error"}})
                    .to_string(),
            );
            let _ = conn.write_all(b"0\r\n\r\n");
        }
        ChatScript::Answer { finish, .. } => {
            let end = json!({"choices": [{"index": 0, "delta": {}, "finish_reason": finish}],
                "usage": {"prompt_tokens": 11, "completion_tokens": 3, "total_tokens": 14}});
            chunk(conn, &end.to_string());
            chunk(conn, "[DONE]");
            let _ = conn.write_all(b"0\r\n\r\n");
        }
        ChatScript::Raw(_) => {}
        ChatScript::HoldFirst => {
            let end = json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]});
            chunk(conn, &end.to_string());
            chunk(conn, "[DONE]");
            let _ = conn.write_all(b"0\r\n\r\n");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_patterns_match_segments() {
        assert!(matches_route("/x/v1/pair/{id}", "/x/v1/pair/abc"));
        assert!(matches_route("/x/v1/health", "/x/v1/health?x=1"));
        assert!(!matches_route("/x/v1/pair/{id}", "/x/v1/pair/"));
        assert!(!matches_route("/x/v1/pair/{id}", "/x/v1/pair/a/b"));
        assert!(!matches_route("/x/v1/health", "/x/v1/healthz"));
    }
}
