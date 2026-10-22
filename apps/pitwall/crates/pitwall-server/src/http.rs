//! The HTTP APIs on 127.0.0.1: OpenAI-, Anthropic- and Ollama-compatible endpoints plus `/x/v1` extensions
//! (ADR 0003, ADR 0009). Handlers parse with `pitwall_core::api`, run the request pipeline in [`crate::app`] and
//! write the family's own shapes, errors and streaming format. No handler can open an outbound connection: they
//! reach only the serving core (loopback engines) and in-memory state.

use crate::app::{AppState, Meta, Run};
use crate::auth::{AuthError, PairStatus, Principal};
use crate::paths::now_ms;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::Router;
use pitwall_core::api::{anthropic, ollama, openai, ApiError, ApiFamily, ErrorKind, LoadedEntry, ModelEntry};
use pitwall_core::chat::{ChatEvent, ChatOutput, ModelRef};
use pitwall_core::events::{EventKind, Severity};
use pitwall_core::ids::AppId;
use pitwall_core::privacy;
use pitwall_core::scheduler::{JobKind, Priority};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

type S = State<Arc<AppState>>;

/// The router for the whole server (standard APIs, `/x/v1`, and the management API under `/mgmt/v1`).
pub fn router(st: Arc<AppState>) -> Router {
    let max = usize::try_from(st.cfg().server.max_request_bytes).unwrap_or(32 << 20);
    Router::new()
        .route("/v1/models", get(openai_models))
        .route("/v1/models/{id}", get(openai_model))
        .route("/v1/chat/completions", post(openai_chat))
        .route("/v1/responses", post(openai_responses))
        .route("/v1/embeddings", post(openai_embeddings))
        .route("/v1/audio/transcriptions", post(not_available))
        .route("/v1/messages", post(anthropic_messages))
        .route("/v1/messages/count_tokens", post(anthropic_count))
        .route("/api/chat", post(ollama_chat))
        .route("/api/generate", post(ollama_generate))
        .route("/api/tags", get(ollama_tags))
        .route("/api/ps", get(ollama_ps))
        .route("/api/show", post(ollama_show))
        .route("/api/embed", post(ollama_embed))
        .route("/api/embeddings", post(ollama_embed))
        .route("/api/version", get(ollama_version))
        .route("/x/v1/health", get(x_health))
        .route("/x/v1/pair", post(x_pair))
        .route("/x/v1/pair/{id}", get(x_pair_poll))
        .route("/x/v1/select", post(x_select))
        .route("/x/v1/models", get(x_models))
        .route("/x/v1/tasks/{task}", post(x_task))
        .route("/x/v1/tokens/count", post(x_tokens_count))
        .route("/x/v1/tokens/truncate", post(x_tokens_truncate))
        .route("/x/v1/jobs", get(x_jobs))
        .route("/x/v1/jobs/{id}", delete(x_cancel))
        .merge(crate::mgmt::routes())
        .layer(DefaultBodyLimit::max(max))
        .with_state(st)
}

// ---------------------------------------------------------------------------------------------------------------
// Edge: errors, gate, helpers

fn fail(e: &ApiError, family: ApiFamily) -> Response {
    let mut r = (
        StatusCode::from_u16(e.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        axum::Json(e.body(family)),
    )
        .into_response();
    if e.kind == ErrorKind::RateLimit || e.kind == ErrorKind::Overloaded {
        r.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from_static("5"));
    }
    r
}

fn header_str<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    h.get(name).and_then(|v| v.to_str().ok())
}

/// Check Host/Origin (DNS rebinding, cross-site pages) and the credential. `extended` marks `/x/` calls.
pub(crate) fn gate(st: &AppState, h: &HeaderMap) -> Result<Principal, ApiError> {
    let cfg = st.cfg();
    let lan: Vec<String> = vec![];
    if !privacy::host_allowed(header_str(h, "host"), &lan) {
        return Err(ApiError::new(
            ErrorKind::Permission,
            "this request's Host header is not a local address; Pitwall only answers on 127.0.0.1",
        ));
    }
    let cred = pitwall_core::keys::credential(header_str(h, "authorization"), header_str(h, "x-api-key"));
    let origin = header_str(h, "origin");
    // A web page can send requests to localhost; with no key it is refused, and with a foreign origin always.
    if origin.is_some() && !privacy::origin_allowed_without_key(origin) {
        return Err(ApiError::new(
            ErrorKind::Permission,
            "requests from web pages on other sites are refused",
        ));
    }
    let keyless = cfg.server.allow_keyless_local && origin.is_none();
    st.auth
        .authenticate(cred, keyless, now_ms())
        .map_err(|e| ApiError::new(ErrorKind::Authentication, e.to_string()))
}

fn priority(h: &HeaderMap) -> Result<Option<Priority>, ApiError> {
    match header_str(h, "x-pitwall-priority") {
        None => Ok(None),
        Some(p) => Priority::parse(p)
            .map(Some)
            .ok_or_else(|| ApiError::invalid("X-Pitwall-Priority must be interactive, background or batch")),
    }
}

fn json_body(b: &Bytes) -> Result<Value, ApiError> {
    serde_json::from_slice(b).map_err(|e| ApiError::invalid(format!("the request body is not valid JSON: {e}")))
}

fn provenance(r: &mut Response, run_model: &str, engine: &str, by: pitwall_core::mgmt::ChosenBy, cold: Option<u64>) {
    let h = r.headers_mut();
    let mut put = |k: &'static str, v: String| {
        if let Ok(v) = HeaderValue::from_str(&v) {
            h.insert(HeaderName::from_static(k), v);
        }
    };
    put("x-pitwall-model", run_model.to_string());
    put("x-pitwall-engine", engine.to_string());
    put(
        "x-pitwall-chosen-by",
        serde_json::to_value(by)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default(),
    );
    if let Some(c) = cold {
        put("x-pitwall-cold-start-ms", c.to_string());
    }
}

fn entries(st: &AppState) -> Vec<ModelEntry> {
    let loaded = st.serving.loaded();
    st.library
        .local()
        .iter()
        .map(|f| ModelEntry {
            id: f.id.clone(),
            size: f.size,
            family: Some(f.info.architecture.clone()),
            parameter_size: Some(f.info.params_b).filter(|p| *p > 0.0).map(|p| format!("{p:.1}B")),
            quant: f.info.quant.clone(),
            modified: f.modified,
            sha256: None,
            loaded: loaded.iter().find(|l| l.model_id == f.id.as_str()).map(|l| LoadedEntry {
                ram: l.ram_bytes,
                vram: l.vram_bytes,
                expires_at: l.idle_release_at,
            }),
        })
        .collect()
}

macro_rules! gate {
    ($st:expr, $h:expr, $fam:expr) => {
        match gate(&$st, &$h) {
            Ok(p) => p,
            Err(e) => return fail(&e, $fam),
        }
    };
}

fn meta(family: ApiFamily, endpoint: &str, h: &HeaderMap, kind: JobKind) -> Result<Meta, ApiError> {
    Ok(Meta {
        family,
        endpoint: endpoint.into(),
        priority: priority(h)?,
        extended: family == ApiFamily::Extended,
        kind,
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Running a request and writing it in a family's shape

type Fmt = Box<dyn FnMut(&ChatEvent) -> Vec<String> + Send>;

fn sse(run: Run, mut fmt: Fmt, err_frame: fn(&str) -> String, content_type: &'static str) -> Response {
    let (model, by, cold, engine) = (
        run.resolved.id.to_string(),
        run.resolved.chosen_by,
        run.cold_start_ms,
        run.engine.clone(),
    );
    struct St {
        run: Run,
        pending: VecDeque<String>,
        ended: bool,
    }
    let mut rec_init = run;
    rec_init.rec.chosen(by);
    let state = St {
        run: rec_init,
        pending: VecDeque::new(),
        ended: false,
    };
    let stream = futures_util::stream::unfold(
        (state, std::mem::replace(&mut fmt, Box::new(|_| vec![]))),
        move |(mut s, mut f)| async move {
            loop {
                if let Some(p) = s.pending.pop_front() {
                    return Some((Ok::<_, std::convert::Infallible>(Bytes::from(p)), (s, f)));
                }
                if s.ended {
                    return None;
                }
                match s.run.rx.recv().await {
                    None => s.ended = true,
                    Some(Err(e)) => {
                        s.run.rec.finish(Default::default(), None, Some(e.0.clone()));
                        s.pending.push_back(err_frame(&e.0));
                        s.ended = true;
                    }
                    Some(Ok(ev)) => {
                        s.run.rec.event(&ev);
                        if let ChatEvent::Done { usage, timings, .. } = &ev {
                            s.run.rec.finish(*usage, *timings, None);
                        }
                        s.pending.extend(f(&ev));
                    }
                }
            }
        },
    );
    let mut r = Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(stream))
        .unwrap_or_default();
    provenance(&mut r, &model, &engine, by, cold);
    r
}

/// Drain a run into a whole answer. Returns the output and the timing facts.
async fn collect(mut run: Run) -> Result<(ChatOutput, Run), ApiError> {
    let mut out = ChatOutput::default();
    run.rec.chosen(run.resolved.chosen_by);
    while let Some(ev) = run.rx.recv().await {
        match ev {
            Ok(e) => {
                run.rec.event(&e);
                out.push(&e);
                if let ChatEvent::Done { usage, timings, .. } = &e {
                    run.rec.finish(*usage, *timings, None);
                }
            }
            Err(e) => {
                run.rec.finish(Default::default(), None, Some(e.0.clone()));
                let kind = if e.0.contains("timed out") {
                    ErrorKind::Timeout
                } else {
                    ErrorKind::Engine
                };
                return Err(ApiError::new(kind, format!("the model engine failed: {}", e.0)));
            }
        }
    }
    if out.finish.is_none() {
        return Err(ApiError::new(
            ErrorKind::Engine,
            "the model engine ended the answer without finishing; try again",
        ));
    }
    Ok((out, run))
}

fn json_response(v: Value, run: &Run) -> Response {
    let mut r = axum::Json(v).into_response();
    provenance(
        &mut r,
        run.resolved.id.as_str(),
        &run.engine,
        run.resolved.chosen_by,
        run.cold_start_ms,
    );
    r
}

fn id(prefix: &str) -> String {
    format!("{prefix}-{:x}{:x}", now_ms(), rand_suffix())
}

fn rand_suffix() -> u32 {
    let b = crate::paths::random32();
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

// ---------------------------------------------------------------------------------------------------------------
// OpenAI

async fn openai_models(State(st): S, h: HeaderMap) -> Response {
    gate!(st, h, ApiFamily::OpenAi);
    axum::Json(openai::models(&entries(&st))).into_response()
}

async fn openai_model(State(st): S, h: HeaderMap, Path(name): Path<String>) -> Response {
    gate!(st, h, ApiFamily::OpenAi);
    match entries(&st).iter().find(|e| e.id.as_str() == name) {
        Some(m) => axum::Json(openai::model(m)).into_response(),
        None => fail(
            &ApiError::new(ErrorKind::NotFound, format!("the model \"{name}\" does not exist")),
            ApiFamily::OpenAi,
        ),
    }
}

async fn not_available(State(st): S, h: HeaderMap) -> Response {
    gate!(st, h, ApiFamily::OpenAi);
    fail(
        &ApiError::new(
            ErrorKind::NotAvailable,
            "speech-to-text is not available yet: the whisper.cpp engine is planned. See Engines in Pitwall.",
        ),
        ApiFamily::OpenAi,
    )
}

fn openai_err_frame(m: &str) -> String {
    pitwall_core::sse::data(&json!({"error": {"message": m, "type": "server_error", "code": "engine_error"}}).to_string())
}

async fn openai_chat(State(st): S, h: HeaderMap, body: Bytes) -> Response {
    let who = gate!(st, h, ApiFamily::OpenAi);
    let go = async {
        let req = openai::parse_chat(&json_body(&body)?)?;
        let m = meta(ApiFamily::OpenAi, "/v1/chat/completions", &h, JobKind::Chat)?;
        let stream = req.stream;
        let run = st.start_chat(&who, m, req).await?;
        let name = run.resolved.id.to_string();
        let (cid, created) = (id("chatcmpl"), now_ms() / 1000);
        if stream {
            let mut s = openai::ChatStream::new(&cid, &name, created);
            Ok(sse(run, Box::new(move |e| s.event(e)), openai_err_frame, "text/event-stream"))
        } else {
            let (out, run) = collect(run).await?;
            Ok(json_response(openai::chat_response(&out, &cid, &name, created), &run))
        }
    };
    go.await.unwrap_or_else(|e: ApiError| fail(&e, ApiFamily::OpenAi))
}

async fn openai_responses(State(st): S, h: HeaderMap, body: Bytes) -> Response {
    let who = gate!(st, h, ApiFamily::OpenAi);
    let go = async {
        let req = openai::parse_responses(&json_body(&body)?)?;
        let m = meta(ApiFamily::OpenAi, "/v1/responses", &h, JobKind::Chat)?;
        let stream = req.stream;
        let run = st.start_chat(&who, m, req).await?;
        let name = run.resolved.id.to_string();
        let (rid, created) = (id("resp"), now_ms() / 1000);
        if stream {
            let mut s = openai::ResponsesStream::new(&rid, &name, created);
            Ok(sse(run, Box::new(move |e| s.event(e)), openai_err_frame, "text/event-stream"))
        } else {
            let (out, run) = collect(run).await?;
            Ok(json_response(openai::responses_response(&out, &rid, &name, created), &run))
        }
    };
    go.await.unwrap_or_else(|e: ApiError| fail(&e, ApiFamily::OpenAi))
}

async fn embed(
    st: &Arc<AppState>,
    who: &Principal,
    h: &HeaderMap,
    family: ApiFamily,
    endpoint: &str,
    req: pitwall_core::chat::EmbedRequest,
) -> Result<(pitwall_core::chat::EmbedOutput, String, u64), ApiError> {
    let model = match &req.model {
        ModelRef::Default => ModelRef::Named("auto:embeddings".into()),
        m => m.clone(),
    };
    let resolved = st.serving.resolve(&model, false, false)?;
    let (app, perms) = match who {
        Principal::App { id, permissions } => (id.clone(), Some(permissions.clone())),
        _ => (AppId::parse("pitwall").map_err(|e| ApiError::invalid(e.to_string()))?, None),
    };
    let mut pr = priority(h)?.unwrap_or(Priority::Interactive);
    if let Some(p) = &perms {
        pr = p
            .authorize(Some(&resolved.id), pr, false, family == ApiFamily::Extended)
            .map_err(|d| {
                ApiError::new(
                    if d.is_rate() { ErrorKind::RateLimit } else { ErrorKind::Permission },
                    d.to_string(),
                )
            })?;
        st.auth
            .admit(&app, p.limits.requests_per_min, now_ms())
            .map_err(|d| ApiError::new(ErrorKind::RateLimit, d.to_string()))?;
        let used = crate::app_lock(&st.usage).budget_use(now_ms(), app.as_str());
        pitwall_core::limits::check_budget(&used, &p.limits).map_err(|d| {
            ApiError::new(
                if d.is_rate() { ErrorKind::RateLimit } else { ErrorKind::Permission },
                d.to_string(),
            )
        })?;
    }
    let started = std::time::Instant::now();
    let permit = st
        .serving
        .acquire(app.clone(), resolved.id.clone(), pr, JobKind::Embedding, None)
        .await?;
    let (busy, _cold) = st.serving.ensure(&resolved.id).await?;
    let out = st
        .serving
        .embed(&busy, &req, Duration::from_secs(st.cfg().server.request_timeout_secs))
        .await;
    drop(permit);
    let ms = started.elapsed().as_millis() as u64;
    let out = out.map_err(|e| ApiError::new(ErrorKind::Engine, format!("the embedding model failed: {}", e.0)))?;
    crate::app_lock(&st.usage).record(&pitwall_core::usage::UsageEvent {
        at: now_ms(),
        app: app.as_str(),
        prompt_tokens: u64::from(out.prompt_tokens),
        completion_tokens: 0,
        busy_ms: ms,
        failed: false,
        memory_bytes: st.serving.footprint(&resolved.id),
    });
    let _ = endpoint;
    Ok((out, resolved.id.to_string(), ms))
}

async fn openai_embeddings(State(st): S, h: HeaderMap, body: Bytes) -> Response {
    let who = gate!(st, h, ApiFamily::OpenAi);
    let go = async {
        let req = openai::parse_embeddings(&json_body(&body)?)?;
        let (out, model, _) = embed(&st, &who, &h, ApiFamily::OpenAi, "/v1/embeddings", req).await?;
        Ok(axum::Json(openai::embeddings_response(&out, &model)).into_response())
    };
    go.await.unwrap_or_else(|e: ApiError| fail(&e, ApiFamily::OpenAi))
}

// ---------------------------------------------------------------------------------------------------------------
// Anthropic

fn anthropic_err_frame(m: &str) -> String {
    pitwall_core::sse::named(
        "error",
        &json!({"type": "error", "error": {"type": "api_error", "message": m}}).to_string(),
    )
}

async fn anthropic_messages(State(st): S, h: HeaderMap, body: Bytes) -> Response {
    let who = gate!(st, h, ApiFamily::Anthropic);
    let go = async {
        let req = anthropic::parse_messages(&json_body(&body)?)?;
        let m = meta(ApiFamily::Anthropic, "/v1/messages", &h, JobKind::Chat)?;
        let stream = req.stream;
        let prompt: u32 = req.messages.iter().map(|m| pitwall_core::tokens::estimate(&m.text_content())).sum();
        let run = st.start_chat(&who, m, req).await?;
        let name = run.resolved.id.to_string();
        let mid = id("msg");
        if stream {
            let mut s = anthropic::MessagesStream::new(&mid, &name, prompt);
            Ok(sse(run, Box::new(move |e| s.event(e)), anthropic_err_frame, "text/event-stream"))
        } else {
            let (out, run) = collect(run).await?;
            Ok(json_response(anthropic::messages_response(&out, &mid, &name), &run))
        }
    };
    go.await.unwrap_or_else(|e: ApiError| fail(&e, ApiFamily::Anthropic))
}

async fn anthropic_count(State(st): S, h: HeaderMap, body: Bytes) -> Response {
    gate!(st, h, ApiFamily::Anthropic);
    let go = || -> Result<Response, ApiError> {
        let req = anthropic::parse_messages(&json_body(&body)?)?;
        let n: u32 = req.messages.iter().map(|m| pitwall_core::tokens::estimate(&m.text_content())).sum();
        Ok(axum::Json(anthropic::count_tokens_response(n)).into_response())
    };
    go().unwrap_or_else(|e| fail(&e, ApiFamily::Anthropic))
}

// ---------------------------------------------------------------------------------------------------------------
// Ollama

fn ollama_err_frame(m: &str) -> String {
    format!("{}\n", json!({"error": m}))
}

async fn ollama_run(st: Arc<AppState>, h: HeaderMap, body: Bytes, generate: bool) -> Response {
    let fam = ApiFamily::Ollama;
    let who = gate!(st, h, fam);
    let go = async {
        let parsed = if generate {
            ollama::parse_generate(&json_body(&body)?)?
        } else {
            ollama::parse_chat(&json_body(&body)?)?
        };
        let ollama::OllamaRequest {
            chat,
            keep_alive,
            load_only,
        } = parsed;
        let endpoint = if generate { "/api/generate" } else { "/api/chat" };
        if load_only {
            let resolved = st.serving.resolve(&chat.model, false, false)?;
            let name = resolved.id.to_string();
            let unload = matches!(keep_alive, Some(pitwall_core::api::ollama::KeepAlive::Unload));
            if unload {
                st.serving.unload(&resolved.id, pitwall_core::memory::ReleaseReason::Manual);
            } else {
                st.serving.ensure(&resolved.id).await?;
            }
            return Ok(axum::Json(ollama::load_response(&name, generate, now_ms(), unload)).into_response());
        }
        let m = meta(fam, endpoint, &h, JobKind::Chat)?;
        let stream = chat.stream;
        let run = st.start_chat(&who, m, chat).await?;
        let name = run.resolved.id.to_string();
        let model_id = run.resolved.id.clone();
        let started = std::time::Instant::now();
        let load_ms = run.cold_start_ms.unwrap_or(0);
        let stats = move |t: Option<pitwall_core::chat::Timings>, u: pitwall_core::chat::Usage| {
            let eval_ms = t
                .filter(|t| t.gen_tps > 0.0)
                .map_or(0, |t| (f64::from(u.completion_tokens) / t.gen_tps * 1000.0) as u64);
            let prompt_ms = t
                .filter(|t| t.prompt_tps > 0.0)
                .map_or(0, |t| (f64::from(u.prompt_tokens) / t.prompt_tps * 1000.0) as u64);
            ollama::Stats {
                total_ms: started.elapsed().as_millis() as u64 + load_ms,
                load_ms,
                prompt_ms,
                eval_ms,
            }
        };
        let unload_after = matches!(keep_alive, Some(pitwall_core::api::ollama::KeepAlive::Unload));
        let st2 = st.clone();
        let resp = if stream {
            let mut s = ollama::Stream::new(&name, generate);
            let f: Fmt = Box::new(move |e| {
                let (t, u) = match e {
                    ChatEvent::Done { timings, usage, .. } => (*timings, *usage),
                    _ => (None, Default::default()),
                };
                s.event(e, now_ms(), &stats(t, u))
            });
            sse(run, f, ollama_err_frame, "application/x-ndjson")
        } else {
            let (out, run) = collect(run).await?;
            let stats = stats(out.timings, out.usage);
            json_response(ollama::response(&out, &name, generate, now_ms(), &stats), &run)
        };
        if unload_after {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(1)).await;
                st2.serving.unload(&model_id, pitwall_core::memory::ReleaseReason::Manual);
            });
        }
        Ok(resp)
    };
    go.await.unwrap_or_else(|e: ApiError| fail(&e, fam))
}

async fn ollama_chat(State(st): S, h: HeaderMap, body: Bytes) -> Response {
    ollama_run(st, h, body, false).await
}

async fn ollama_generate(State(st): S, h: HeaderMap, body: Bytes) -> Response {
    ollama_run(st, h, body, true).await
}

async fn ollama_tags(State(st): S, h: HeaderMap) -> Response {
    gate!(st, h, ApiFamily::Ollama);
    axum::Json(ollama::tags(&entries(&st))).into_response()
}

async fn ollama_ps(State(st): S, h: HeaderMap) -> Response {
    gate!(st, h, ApiFamily::Ollama);
    axum::Json(ollama::ps(&entries(&st))).into_response()
}

async fn ollama_show(State(st): S, h: HeaderMap, body: Bytes) -> Response {
    gate!(st, h, ApiFamily::Ollama);
    let go = || -> Result<Response, ApiError> {
        let v = json_body(&body)?;
        let name = v
            .get("model")
            .or_else(|| v.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim_end_matches(":latest")
            .to_string();
        let local = st.library.local();
        let f = local
            .iter()
            .find(|f| f.id.as_str() == name)
            .ok_or_else(|| ApiError::new(ErrorKind::NotFound, format!("model '{name}' not found")))?;
        let e = entries(&st)
            .into_iter()
            .find(|e| e.id == f.id)
            .ok_or_else(|| ApiError::new(ErrorKind::NotFound, "model not found"))?;
        let caps: Vec<&str> = crate::library::capabilities(f)
            .iter()
            .filter_map(|c| match c {
                pitwall_core::select::Capability::Chat => Some("completion"),
                pitwall_core::select::Capability::Vision => Some("vision"),
                pitwall_core::select::Capability::Tools => Some("tools"),
                pitwall_core::select::Capability::Embeddings => Some("embedding"),
                _ => None,
            })
            .collect();
        Ok(axum::Json(ollama::show(&e, &caps)).into_response())
    };
    go().unwrap_or_else(|e| fail(&e, ApiFamily::Ollama))
}

async fn ollama_embed(State(st): S, h: HeaderMap, body: Bytes) -> Response {
    let who = gate!(st, h, ApiFamily::Ollama);
    let go = async {
        let (req, legacy) = ollama::parse_embed(&json_body(&body)?)?;
        let (out, model, ms) = embed(&st, &who, &h, ApiFamily::Ollama, "/api/embed", req).await?;
        Ok(axum::Json(ollama::embed_response(
            &out,
            &model,
            legacy,
            &ollama::Stats {
                total_ms: ms,
                ..Default::default()
            },
        ))
        .into_response())
    };
    go.await.unwrap_or_else(|e: ApiError| fail(&e, ApiFamily::Ollama))
}

async fn ollama_version(State(st): S, h: HeaderMap) -> Response {
    gate!(st, h, ApiFamily::Ollama);
    axum::Json(ollama::version()).into_response()
}

// ---------------------------------------------------------------------------------------------------------------
// /x/v1

async fn x_health(State(st): S, h: HeaderMap) -> Response {
    if !privacy::host_allowed(header_str(&h, "host"), &[]) {
        return fail(&ApiError::new(ErrorKind::Permission, "not a local address"), ApiFamily::Extended);
    }
    let cfg = st.cfg();
    axum::Json(json!({
        "ok": true,
        "name": "pitwall",
        "version": env!("CARGO_PKG_VERSION"),
        "apiVersion": pitwall_core::API_VERSION,
        "serving": if st.serving.paused() { "paused" } else { "serving" },
        "offline": cfg.network.offline,
        "loadedModels": st.serving.loaded().len(),
    }))
    .into_response()
}

async fn x_pair(State(st): S, h: HeaderMap, body: Bytes) -> Response {
    let fam = ApiFamily::Extended;
    if !privacy::host_allowed(header_str(&h, "host"), &[])
        || (header_str(&h, "origin").is_some() && !privacy::origin_allowed_without_key(header_str(&h, "origin")))
    {
        return fail(
            &ApiError::new(ErrorKind::Permission, "pairing is only possible from this machine"),
            fam,
        );
    }
    let go = || -> Result<Response, ApiError> {
        let v = json_body(&body)?;
        let name = v.get("name").and_then(Value::as_str).unwrap_or_default();
        let b = crate::paths::random32();
        let r = st
            .auth
            .request_pairing(name, u32::from_le_bytes([b[0], b[1], b[2], b[3]]), now_ms())
            .map_err(|e| match e {
                AuthError::Invalid(m) => ApiError::invalid(m),
                AuthError::Unauthorized => ApiError::new(ErrorKind::Authentication, e.to_string()),
            })?;
        st.event(
            EventKind::Pairing,
            Severity::Info,
            format!("{} asks to connect (code {})", r.app_name, r.code),
        );
        st.push(&pitwall_core::mgmt::LiveUpdate::Pairing {
            pairings: st.auth.pairings(now_ms()),
        });
        Ok((
            StatusCode::ACCEPTED,
            axum::Json(json!({"pairingId": r.id, "code": r.code, "expiresInSecs": 300})),
        )
            .into_response())
    };
    go().unwrap_or_else(|e| fail(&e, fam))
}

async fn x_pair_poll(State(st): S, h: HeaderMap, Path(pid): Path<String>) -> Response {
    if !privacy::host_allowed(header_str(&h, "host"), &[]) {
        return fail(&ApiError::new(ErrorKind::Permission, "not a local address"), ApiFamily::Extended);
    }
    let v = match st.auth.poll_pairing(&pid, now_ms()) {
        PairStatus::Pending => json!({"status": "pending"}),
        PairStatus::Approved(Some(k)) => json!({"status": "approved", "key": k}),
        PairStatus::Approved(None) => json!({"status": "approved", "key": null, "note": "the key was already delivered"}),
        PairStatus::Denied => json!({"status": "denied"}),
    };
    axum::Json(v).into_response()
}

async fn x_select(State(st): S, h: HeaderMap, body: Bytes) -> Response {
    let who = gate!(st, h, ApiFamily::Extended);
    let go = || -> Result<Response, ApiError> {
        if let Principal::App { permissions, .. } = &who {
            permissions
                .authorize(None, Priority::Interactive, false, true)
                .map_err(|d| ApiError::new(ErrorKind::Permission, d.to_string()))?;
        }
        let v = json_body(&body)?;
        let spec = v.get("model").and_then(Value::as_str).unwrap_or("auto");
        let r = st.serving.resolve(&ModelRef::Named(spec.to_string()), false, false)?;
        Ok(axum::Json(json!({"model": r.id.as_str(), "chosenBy": r.chosen_by, "loaded": st.serving.is_loaded(&r.id)})).into_response())
    };
    go().unwrap_or_else(|e| fail(&e, ApiFamily::Extended))
}

async fn x_models(State(st): S, h: HeaderMap) -> Response {
    gate!(st, h, ApiFamily::Extended);
    let loaded = st.serving.loaded();
    let v: Vec<Value> = st
        .library
        .local()
        .iter()
        .map(|f| json!({"id": f.id.as_str(), "capabilities": crate::library::capabilities(f), "contextMax": f.info.context_length, "sizeBytes": f.size, "loaded": loaded.iter().any(|l| l.model_id == f.id.as_str())}))
        .collect();
    axum::Json(json!({"models": v})).into_response()
}

async fn x_jobs(State(st): S, h: HeaderMap) -> Response {
    gate!(st, h, ApiFamily::Extended);
    axum::Json(st.queue_snapshot()).into_response()
}

async fn x_cancel(State(st): S, h: HeaderMap, Path(job): Path<u64>) -> Response {
    let who = gate!(st, h, ApiFamily::Extended);
    // An app may cancel only its own jobs; the admin any.
    let owner = st.serving.jobs().into_iter().find(|j| j.id == job).map(|j| j.spec.app);
    if let (Principal::App { id, .. }, Some(o)) = (&who, &owner) {
        if id != o {
            return fail(
                &ApiError::new(ErrorKind::Permission, "that job belongs to another app"),
                ApiFamily::Extended,
            );
        }
    }
    match st.serving.cancel(job) {
        pitwall_core::scheduler::Cancelled::Unknown => fail(&ApiError::new(ErrorKind::NotFound, "no such job"), ApiFamily::Extended),
        c => axum::Json(json!({"cancelled": true, "wasRunning": c == pitwall_core::scheduler::Cancelled::Running})).into_response(),
    }
}

async fn x_task(State(st): S, h: HeaderMap, Path(task): Path<String>, body: Bytes) -> Response {
    let who = gate!(st, h, ApiFamily::Extended);
    let go = async {
        let t = pitwall_core::tasks::Task::parse(&task)
            .ok_or_else(|| ApiError::new(ErrorKind::NotFound, "tasks are summarize, classify, extract and translate"))?;
        let v = json_body(&body)?;
        let (mut req, template) = pitwall_core::tasks::build(t, &v)?;
        if let Some(m) = v.get("model").and_then(Value::as_str) {
            req.model = ModelRef::Named(m.to_string());
        }
        req.stream = false;
        let m = meta(ApiFamily::Extended, &format!("/x/v1/tasks/{task}"), &h, JobKind::Task)?;
        let run = st.start_chat(&who, m, req).await?;
        let (out, run) = collect(run).await?;
        let result = if t == pitwall_core::tasks::Task::Summarize || t == pitwall_core::tasks::Task::Translate {
            Value::String(out.text.trim().to_string())
        } else {
            serde_json::from_str(out.text.trim()).unwrap_or(Value::String(out.text.clone()))
        };
        Ok(json_response(
            json!({"result": result, "template": template, "model": run.resolved.id.as_str(), "usage": {"promptTokens": out.usage.prompt_tokens, "completionTokens": out.usage.completion_tokens}}),
            &run,
        ))
    };
    go.await.unwrap_or_else(|e: ApiError| fail(&e, ApiFamily::Extended))
}

async fn x_tokens_count(State(st): S, h: HeaderMap, body: Bytes) -> Response {
    gate!(st, h, ApiFamily::Extended);
    let go = || -> Result<Response, ApiError> {
        let v = json_body(&body)?;
        let text = v
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::invalid("`text` is required"))?;
        Ok(axum::Json(json!({"tokens": pitwall_core::tokens::estimate(text), "estimate": true})).into_response())
    };
    go().unwrap_or_else(|e| fail(&e, ApiFamily::Extended))
}

async fn x_tokens_truncate(State(st): S, h: HeaderMap, body: Bytes) -> Response {
    gate!(st, h, ApiFamily::Extended);
    let go = || -> Result<Response, ApiError> {
        let v = json_body(&body)?;
        let text = v
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::invalid("`text` is required"))?;
        let max = v
            .get("maxTokens")
            .and_then(Value::as_u64)
            .ok_or_else(|| ApiError::invalid("`maxTokens` is required"))?
            .min(u64::from(u32::MAX)) as u32;
        let keep = match v.get("keep").and_then(Value::as_str).unwrap_or("start") {
            "end" => pitwall_core::tokens::Keep::Tail,
            "ends" | "middle" => pitwall_core::tokens::Keep::Middle,
            _ => pitwall_core::tokens::Keep::Head,
        };
        let (t, cut) = pitwall_core::tokens::truncate(text, max, keep);
        Ok(axum::Json(json!({"text": t, "truncated": cut})).into_response())
    };
    go().unwrap_or_else(|e| fail(&e, ApiFamily::Extended))
}
