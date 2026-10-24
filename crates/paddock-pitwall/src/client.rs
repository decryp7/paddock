//! The calls of Pitwall's client contract an app makes (Pitwall ADRs 0003, 0007, 0009): health, pairing, selection, model
//! and job lists, chat (whole or streamed), embeddings, tasks and token helpers. Blocking and bounded; apps call it off
//! the UI thread (Paddock rule 10). Answers are parsed into typed values at once and Pitwall's typed errors become
//! [`PitwallError`]s that say what to do.

use crate::error::{PitwallError, status_error};
use crate::loopback::{self, Request, Timeouts};
use crate::model::{CallOptions, ModelSel};
use crate::request::{ChatRequest, EmbedRequest, Keep, Task};
use crate::retry::{self, Policy, Sleeper, ThreadSleeper};
use crate::stream::ChatStream;
use crate::wire::{Health, JobInfo, ModelInfo, PairPoll, Pairing, Provenance, Selected, Usage};
use paddock_secrets::Secret;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Instant;

/// Largest JSON answer of a quick call.
const MAX_JSON: usize = 1024 * 1024;
/// Largest embeddings answer.
const MAX_EMBEDDINGS: usize = 32 * 1024 * 1024;
/// Texts per embeddings request.
pub const EMBED_BATCH: usize = 64;
/// Longest display name an app pairs with.
const MAX_NAME: usize = 64;

/// Who the app is to Pitwall: the stable id (the key's owner, `debrief`) and the name the user sees in Pitwall's
/// "Apps & keys" ("Debrief").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIdentity {
    id: String,
    display_name: String,
}

impl AppIdentity {
    /// `id`: 1-32 lowercase letters, digits or `-`, a letter first. `display_name`: 1-64 printable characters.
    pub fn new(id: &str, display_name: &str) -> Result<AppIdentity, PitwallError> {
        let id_ok = (1..=32).contains(&id.len())
            && id.starts_with(|c: char| c.is_ascii_lowercase())
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        let name_ok = !display_name.trim().is_empty()
            && display_name.chars().count() <= MAX_NAME
            && !display_name.chars().any(char::is_control)
            && display_name.trim() == display_name;
        if !id_ok || !name_ok {
            return Err(PitwallError::Invalid(
                "the app's id or display name is not valid".into(),
            ));
        }
        Ok(AppIdentity {
            id: id.into(),
            display_name: display_name.into(),
        })
    }

    /// The id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The display name.
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
}

/// A finished chat answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatReply {
    /// The whole text.
    pub text: String,
    /// `stop`, `length`, ... (`None` when the answer ended without one).
    pub finish_reason: Option<String>,
    /// Token counts.
    pub usage: Usage,
    /// Who answered.
    pub provenance: Provenance,
}

/// Embedding vectors, in the order of the inputs.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbedReply {
    /// One vector per input.
    pub vectors: Vec<Vec<f32>>,
    /// Token counts summed over the batches.
    pub usage: Usage,
    /// Who answered the last batch.
    pub provenance: Provenance,
}

/// The answer of a task call.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskReply {
    /// A string, or the parsed JSON for `classify` and `extract`.
    pub result: Value,
    /// The model that answered.
    pub model: Option<String>,
    /// Token counts.
    pub usage: Usage,
    /// Who answered.
    pub provenance: Provenance,
}

struct Raw {
    provenance: Provenance,
    body: Vec<u8>,
}

/// A client of one Pitwall server on `127.0.0.1:port`.
#[derive(Clone)]
pub struct Client {
    port: u16,
    app: AppIdentity,
    key: Option<Secret>,
    sleeper: Arc<dyn Sleeper>,
    retry: Option<Policy>,
    quick: Timeouts,
    chat: Timeouts,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("port", &self.port)
            .field("app", &self.app.id)
            .field("key", &self.key)
            .finish()
    }
}

impl Client {
    /// A client without a key (health and pairing work without one).
    pub fn new(port: u16, app: AppIdentity) -> Client {
        Client {
            port,
            app,
            key: None,
            sleeper: Arc::new(ThreadSleeper),
            retry: None,
            quick: Timeouts::QUICK,
            chat: Timeouts::CHAT,
        }
    }

    /// The same client with the app's key.
    pub fn with_key(mut self, key: Option<Secret>) -> Client {
        self.key = key;
        self
    }

    /// The same client with another way to wait between retries (tests record instead of sleeping).
    pub fn with_sleeper(mut self, sleeper: Arc<dyn Sleeper>) -> Client {
        self.sleeper = sleeper;
        self
    }

    /// The same client with one retry policy for every call (default: by the call's priority).
    pub fn with_retry(mut self, policy: Policy) -> Client {
        self.retry = Some(policy);
        self
    }

    /// The same client with other default limits for quick and chat calls.
    pub fn with_timeouts(mut self, quick: Timeouts, chat: Timeouts) -> Client {
        (self.quick, self.chat) = (quick, chat);
        self
    }

    /// The port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The app this client speaks for.
    pub fn app(&self) -> &AppIdentity {
        &self.app
    }

    /// Whether a key is held.
    pub fn has_key(&self) -> bool {
        self.key.is_some()
    }

    fn policy(&self, o: &CallOptions) -> Policy {
        self.retry
            .unwrap_or_else(|| Policy::for_priority(o.priority))
    }

    /// One request with the retries of its policy. `auth`: send the key. `chat_limits`: the long chat limits are the
    /// default. The status is checked: a non-2xx answer is a typed error.
    #[allow(clippy::too_many_arguments)]
    fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        o: &CallOptions,
        policy: Policy,
        auth: bool,
        limit: usize,
    ) -> Result<Raw, PitwallError> {
        let deadline = o.deadline.map(|d| Instant::now() + d);
        let t = o.timeouts.unwrap_or(self.quick);
        let bytes = body.map(|b| b.to_string().into_bytes());
        retry::run(policy, &o.cancel, self.sleeper.as_ref(), deadline, || {
            let mut req = match (method, &bytes) {
                ("POST", Some(b)) => Request::post(path, b.clone()),
                ("DELETE", _) => Request::delete(path),
                _ => Request::get(path),
            };
            req = self.decorate(req, o, auth);
            let r = loopback::send_as(self.port, &self.user_agent(), &req, t, Some(&o.cancel))?;
            let status = r.status;
            let retry_after = r.header("retry-after").map(str::to_string);
            let provenance = Provenance::from_response(&r);
            let body = r.read_all(limit)?;
            if !(200..300).contains(&status) {
                return Err(status_error(
                    status,
                    retry_after.as_deref(),
                    auth && self.key.is_some(),
                    &body,
                ));
            }
            Ok(Raw { provenance, body })
        })
    }

    fn user_agent(&self) -> String {
        format!("{}/paddock-pitwall", self.app.id)
    }

    fn decorate<'a>(&self, mut req: Request<'a>, o: &CallOptions, auth: bool) -> Request<'a> {
        if auth && let Some(k) = &self.key {
            req = req.header("Authorization", format!("Bearer {}", k.expose()));
        }
        req = req.header("X-Pitwall-Priority", o.priority.header());
        if let Some(c) = o.cache.header() {
            req = req.header("Cache-Control", c);
        }
        req
    }

    fn json(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        o: &CallOptions,
        auth: bool,
    ) -> Result<(Value, Provenance), PitwallError> {
        let raw = self.call(method, path, body, o, self.policy(o), auth, MAX_JSON)?;
        let v = serde_json::from_slice(&raw.body)
            .map_err(|e| PitwallError::Protocol(format!("not JSON: {e}")))?;
        Ok((v, raw.provenance))
    }

    fn typed<T: serde::de::DeserializeOwned>(
        &self,
        v: Value,
        what: &str,
    ) -> Result<T, PitwallError> {
        serde_json::from_value(v).map_err(|e| PitwallError::Protocol(format!("{what}: {e}")))
    }

    /// `GET /x/v1/health` (no key needed).
    pub fn health(&self) -> Result<Health, PitwallError> {
        // A probe: a refused connection is the answer, not something to retry.
        let raw = self.call(
            "GET",
            "/x/v1/health",
            None,
            &CallOptions::default(),
            Policy::NONE,
            false,
            MAX_JSON,
        )?;
        let v = serde_json::from_slice(&raw.body)
            .map_err(|e| PitwallError::Protocol(format!("not JSON: {e}")))?;
        self.typed(v, "health")
    }

    /// `POST /x/v1/pair`: asks Pitwall to show a pairing request for the app. Never retried (the user would see two).
    pub fn begin_pairing(&self) -> Result<Pairing, PitwallError> {
        let body = json!({ "name": self.app.display_name });
        let raw = self.call(
            "POST",
            "/x/v1/pair",
            Some(&body),
            &CallOptions::default(),
            Policy::NONE,
            false,
            MAX_JSON,
        )?;
        let v = serde_json::from_slice(&raw.body)
            .map_err(|e| PitwallError::Protocol(format!("not JSON: {e}")))?;
        let p: Pairing = self.typed(v, "pairing")?;
        if p.code.is_empty()
            || p.code.len() > 16
            || p.pairing_id.is_empty()
            || p.pairing_id.len() > 128
        {
            return Err(PitwallError::Protocol(
                "a pairing answer without a usable code".into(),
            ));
        }
        Ok(p)
    }

    /// `GET /x/v1/pair/{id}`.
    pub fn poll_pairing(&self, pairing_id: &str) -> Result<PairPoll, PitwallError> {
        if pairing_id.is_empty()
            || !pairing_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(PitwallError::Protocol(
                "a pairing id with unexpected characters".into(),
            ));
        }
        let path = format!("/x/v1/pair/{pairing_id}");
        let raw = self.call(
            "GET",
            &path,
            None,
            &CallOptions::default(),
            Policy::NONE,
            false,
            MAX_JSON,
        )?;
        let v: Value = serde_json::from_slice(&raw.body)
            .map_err(|e| PitwallError::Protocol(format!("not JSON: {e}")))?;
        Ok(match v.get("status").and_then(Value::as_str) {
            Some("pending") => PairPoll::Pending,
            Some("approved") => {
                PairPoll::Approved(v.get("key").and_then(Value::as_str).map(str::to_string))
            }
            _ => PairPoll::Denied,
        })
    }

    /// `POST /x/v1/select`: which model would serve `model`, without running it.
    pub fn select(&self, model: &ModelSel) -> Result<Selected, PitwallError> {
        let (v, _) = self.json(
            "POST",
            "/x/v1/select",
            Some(&json!({ "model": model.text() })),
            &CallOptions::default(),
            true,
        )?;
        let model = v
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| PitwallError::Protocol("select: no model".into()))?;
        Ok(Selected {
            model: model.chars().take(200).collect(),
            chosen_by: v
                .get("chosenBy")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .chars()
                .take(32)
                .collect(),
            loaded: v.get("loaded").and_then(Value::as_bool).unwrap_or(false),
        })
    }

    /// `GET /x/v1/models`: the installed models with their capabilities and the reasons something is blocked.
    pub fn models(&self) -> Result<Vec<ModelInfo>, PitwallError> {
        let (v, _) = self.json("GET", "/x/v1/models", None, &CallOptions::default(), true)?;
        self.typed(
            v.get("models").cloned().unwrap_or(Value::Array(vec![])),
            "models",
        )
    }

    /// `GET /v1/models`: the ids (installed models and the presets that resolve).
    pub fn model_ids(&self) -> Result<Vec<String>, PitwallError> {
        let (v, _) = self.json("GET", "/v1/models", None, &CallOptions::default(), true)?;
        Ok(v.get("data")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|m| m.get("id").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Checks that the model `model` resolves to can do `cap` before a request is sent: a model that cannot gives
    /// [`PitwallError::Unsupported`] locally, with Pitwall's reason ("needs an engine build with ...").
    pub fn ensure_capability(
        &self,
        model: &ModelSel,
        cap: &str,
    ) -> Result<ModelInfo, PitwallError> {
        let chosen = self.select(model)?;
        let info = self
            .models()?
            .into_iter()
            .find(|m| m.id == chosen.model)
            .ok_or_else(|| {
                PitwallError::NoModelFits(format!(
                    "{} is not in Pitwall's model list",
                    chosen.model
                ))
            })?;
        if info.can(cap) {
            return Ok(info);
        }
        let hint = info.blocked_reason(cap).map_or_else(
            || format!("{} cannot do {cap}; pick a model that can", info.id),
            str::to_string,
        );
        Err(PitwallError::Unsupported {
            cap: cap.into(),
            hint,
        })
    }

    /// `GET /x/v1/jobs`: the requests running and waiting.
    pub fn jobs(&self) -> Result<Vec<JobInfo>, PitwallError> {
        let (v, _) = self.json("GET", "/x/v1/jobs", None, &CallOptions::default(), true)?;
        let mut all: Vec<JobInfo> = self.typed(
            v.get("running").cloned().unwrap_or(Value::Array(vec![])),
            "jobs",
        )?;
        all.extend(self.typed::<Vec<JobInfo>>(
            v.get("waiting").cloned().unwrap_or(Value::Array(vec![])),
            "jobs",
        )?);
        Ok(all)
    }

    /// `DELETE /x/v1/jobs/{id}`: cancels a queued or running request of this app. `Ok(false)`: Pitwall does not know it.
    pub fn cancel_job(&self, id: &str) -> Result<bool, PitwallError> {
        if id.is_empty() || id.len() > 20 || !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(PitwallError::Invalid("a job id is a number".into()));
        }
        let path = format!("/x/v1/jobs/{id}");
        match self.json("DELETE", &path, None, &CallOptions::default(), true) {
            Ok((v, _)) => Ok(v.get("cancelled").and_then(Value::as_bool).unwrap_or(true)),
            Err(PitwallError::NoModelFits(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// `POST /x/v1/tokens/count`: an estimate of the tokens in `text`.
    pub fn count_tokens(&self, text: &str) -> Result<u64, PitwallError> {
        let (v, _) = self.json(
            "POST",
            "/x/v1/tokens/count",
            Some(&json!({ "text": text })),
            &CallOptions::default(),
            true,
        )?;
        v.get("tokens")
            .and_then(Value::as_u64)
            .ok_or_else(|| PitwallError::Protocol("tokens: no count".into()))
    }

    /// `POST /x/v1/tokens/truncate`: `text` cut to `max_tokens`, keeping `keep`; the flag says whether it was cut.
    pub fn truncate(
        &self,
        text: &str,
        max_tokens: u32,
        keep: Keep,
    ) -> Result<(String, bool), PitwallError> {
        let body = json!({ "text": text, "maxTokens": max_tokens, "keep": keep.name() });
        let (v, _) = self.json(
            "POST",
            "/x/v1/tokens/truncate",
            Some(&body),
            &CallOptions::default(),
            true,
        )?;
        let t = v
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| PitwallError::Protocol("truncate: no text".into()))?;
        Ok((
            t.to_string(),
            v.get("truncated").and_then(Value::as_bool).unwrap_or(false),
        ))
    }

    /// `POST /x/v1/tasks/{task}`; `request` is the task's body (`text`, `labels`, `to`, ...).
    pub fn task(
        &self,
        task: Task,
        request: &Value,
        options: &CallOptions,
    ) -> Result<TaskReply, PitwallError> {
        let path = format!("/x/v1/tasks/{}", task.name());
        let timeouts = options.timeouts.unwrap_or(self.chat);
        let o = CallOptions {
            timeouts: Some(timeouts),
            ..options.clone()
        };
        let (v, provenance) = self.json("POST", &path, Some(request), &o, true)?;
        Ok(TaskReply {
            result: v.get("result").cloned().unwrap_or(Value::Null),
            model: v.get("model").and_then(Value::as_str).map(str::to_string),
            usage: v.get("usage").map(Usage::from_value).unwrap_or_default(),
            provenance,
        })
    }

    /// `POST /v1/embeddings`, in batches of [`EMBED_BATCH`], one vector per input in order.
    pub fn embed(&self, req: &EmbedRequest) -> Result<EmbedReply, PitwallError> {
        let mut vectors = Vec::with_capacity(req.input.len());
        let mut usage = Usage::default();
        let mut provenance = Provenance::default();
        let o = CallOptions {
            timeouts: Some(req.options.timeouts.unwrap_or(self.chat)),
            ..req.options.clone()
        };
        for batch in req.input.chunks(EMBED_BATCH) {
            let body = json!({ "model": req.model.text(), "input": batch });
            let raw = self.call(
                "POST",
                "/v1/embeddings",
                Some(&body),
                &o,
                self.policy(&o),
                true,
                MAX_EMBEDDINGS,
            )?;
            let v: Value = serde_json::from_slice(&raw.body)
                .map_err(|e| PitwallError::Protocol(format!("not JSON: {e}")))?;
            let data = v
                .get("data")
                .and_then(Value::as_array)
                .ok_or_else(|| PitwallError::Protocol("embeddings: no data".into()))?;
            if data.len() != batch.len() {
                return Err(PitwallError::Protocol(format!(
                    "embeddings: {} vectors for {} inputs",
                    data.len(),
                    batch.len()
                )));
            }
            for item in data {
                let vec = item
                    .get("embedding")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        PitwallError::Protocol("embeddings: a vector is missing".into())
                    })?
                    .iter()
                    .map(|n| {
                        n.as_f64().map(|f| f as f32).ok_or_else(|| {
                            PitwallError::Protocol("embeddings: a vector holds a non-number".into())
                        })
                    })
                    .collect::<Result<Vec<f32>, _>>()?;
                vectors.push(vec);
            }
            if let Some(u) = v.get("usage") {
                let u = Usage::from_value(u);
                usage.prompt_tokens += u.prompt_tokens;
            }
            provenance = raw.provenance;
        }
        Ok(EmbedReply {
            vectors,
            usage,
            provenance,
        })
    }

    /// Opens a streamed chat (`stream: true`). Retries (connect errors, `503 overloaded`, `429` with `Retry-After`) happen
    /// only here, before the first delta. Dropping the stream cancels the request in Pitwall.
    pub fn chat_stream(&self, req: &ChatRequest) -> Result<ChatStream, PitwallError> {
        let o = &req.options;
        let body = req.body(true).to_string().into_bytes();
        let t = o.timeouts.unwrap_or(self.chat);
        let deadline = o.deadline.map(|d| Instant::now() + d);
        let had_key = self.key.is_some();
        retry::run(
            self.policy(o),
            &o.cancel,
            self.sleeper.as_ref(),
            deadline,
            || {
                let r = self
                    .decorate(Request::post("/v1/chat/completions", body.clone()), o, true)
                    .header("Accept", "text/event-stream");
                let resp =
                    loopback::send_as(self.port, &self.user_agent(), &r, t, Some(&o.cancel))?;
                if !(200..300).contains(&resp.status) {
                    let (status, retry_after) =
                        (resp.status, resp.header("retry-after").map(str::to_string));
                    let body = resp.read_all(MAX_JSON)?;
                    return Err(status_error(status, retry_after.as_deref(), had_key, &body));
                }
                let provenance = Provenance::from_response(&resp);
                Ok(ChatStream::new(resp, provenance))
            },
        )
    }

    /// A streamed chat read to its end: `on_delta` receives each piece of text as it arrives. A cancel ends the call with
    /// [`PitwallError::Cancelled`] (the text so far was already delivered).
    pub fn chat_with(
        &self,
        req: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatReply, PitwallError> {
        let mut stream = self.chat_stream(req)?;
        let mut text = String::new();
        for delta in stream.by_ref() {
            let delta = delta?;
            text.push_str(&delta);
            on_delta(&delta);
        }
        Ok(ChatReply {
            text,
            finish_reason: stream.finish_reason().map(str::to_string),
            usage: stream.usage(),
            provenance: stream.provenance().clone(),
        })
    }

    /// A chat answer, whole.
    pub fn chat(&self, req: &ChatRequest) -> Result<ChatReply, PitwallError> {
        self.chat_with(req, &mut |_| {})
    }
}
