//! The standard APIs (ADR 0003): OpenAI-compatible ([`openai`]), Anthropic Messages-compatible ([`anthropic`]) and
//! Ollama-compatible ([`ollama`]). Each parses its requests into the neutral [`crate::chat`] types once, at the edge,
//! and formats answers, stream chunks and errors in its own shape. Shapes follow the official SDKs' documented
//! request and response objects (pinned by the contract tests in each module and in the server).

pub mod anthropic;
pub mod ollama;
pub mod openai;

use crate::chat::ChatError;
use crate::ids::ModelId;
use crate::time::Millis;
use serde_json::{json, Value};

/// Which API a request came in through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApiFamily {
    /// `/v1/chat/completions`, `/v1/responses`, `/v1/embeddings`, `/v1/models`, `/v1/audio/transcriptions`.
    #[serde(rename = "openai")]
    OpenAi,
    /// `/v1/messages`.
    Anthropic,
    /// `/api/…`.
    Ollama,
    /// `/x/…`.
    Extended,
}

/// What went wrong, independent of the API's error shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The request is malformed (400).
    InvalidRequest,
    /// No or a wrong key (401).
    Authentication,
    /// The app may not do this (403).
    Permission,
    /// No such model or route (404).
    NotFound,
    /// The body is too large (413).
    TooLarge,
    /// Rate limit or token budget (429).
    RateLimit,
    /// The queue is full or serving is paused (503).
    Overloaded,
    /// The model does not fit in memory now (503, typed).
    InsufficientMemory,
    /// The feature or engine is not available here (501).
    NotAvailable,
    /// The request took too long (504).
    Timeout,
    /// A preset's target model is gone or matches nothing (404, typed; ADR 0014).
    PresetTarget,
    /// The engine failed (500).
    Engine,
    /// The engine build in use is too old for what the request needs (503, typed; ADR 0018).
    EngineBuildTooOld,
    /// No engine build of this kind can serve what the request needs (400, typed; ADR 0018).
    CapabilityUnsupported,
    /// The engine build pinned for the model is not installed or failed its self-test (503, typed; ADR 0018).
    EnginePin,
    /// The job was cancelled by `DELETE /x/v1/jobs/{id}` before it ran (499, typed).
    Cancelled,
}

/// An error, ready to be written in any API's shape.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ApiError {
    /// What kind.
    pub kind: ErrorKind,
    /// A message that tells the client what to do.
    pub message: String,
}

impl ApiError {
    /// An error of a kind.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> ApiError {
        ApiError {
            kind,
            message: message.into(),
        }
    }

    /// A 400.
    pub fn invalid(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorKind::InvalidRequest, message)
    }

    /// The HTTP status.
    pub fn status(&self) -> u16 {
        match self.kind {
            ErrorKind::InvalidRequest => 400,
            ErrorKind::Authentication => 401,
            ErrorKind::Permission => 403,
            ErrorKind::NotFound | ErrorKind::PresetTarget => 404,
            ErrorKind::TooLarge => 413,
            ErrorKind::RateLimit => 429,
            ErrorKind::Overloaded | ErrorKind::InsufficientMemory | ErrorKind::EngineBuildTooOld | ErrorKind::EnginePin => 503,
            ErrorKind::CapabilityUnsupported => 400,
            ErrorKind::NotAvailable => 501,
            ErrorKind::Timeout => 504,
            ErrorKind::Engine => 500,
            ErrorKind::Cancelled => 499,
        }
    }

    /// Pitwall's own code for the kind (`X-Pitwall-Error`, the extended APIs' `error.code`).
    pub fn code(&self) -> &'static str {
        match self.kind {
            ErrorKind::InvalidRequest => "invalid_request",
            ErrorKind::Authentication => "unauthorized",
            ErrorKind::Permission => "forbidden",
            ErrorKind::NotFound => "not_found",
            ErrorKind::TooLarge => "request_too_large",
            ErrorKind::RateLimit => "rate_limited",
            ErrorKind::Overloaded => "overloaded",
            ErrorKind::InsufficientMemory => "insufficient_memory",
            ErrorKind::NotAvailable => "not_available",
            ErrorKind::Timeout => "timeout",
            ErrorKind::PresetTarget => "preset_target_unavailable",
            ErrorKind::Engine => "engine_error",
            ErrorKind::EngineBuildTooOld => "engine_build_too_old",
            ErrorKind::CapabilityUnsupported => "capability_unsupported",
            ErrorKind::EnginePin => "engine_pin_unavailable",
            ErrorKind::Cancelled => "cancelled",
        }
    }

    /// The error body in an API family's shape.
    pub fn body(&self, family: ApiFamily) -> Value {
        match family {
            ApiFamily::OpenAi => {
                let t = match self.kind {
                    ErrorKind::InvalidRequest
                    | ErrorKind::TooLarge
                    | ErrorKind::NotFound
                    | ErrorKind::PresetTarget
                    | ErrorKind::CapabilityUnsupported => "invalid_request_error",
                    ErrorKind::Authentication => "authentication_error",
                    ErrorKind::Permission => "permission_error",
                    ErrorKind::RateLimit => "rate_limit_error",
                    _ => "server_error",
                };
                json!({"error": {"message": self.message, "type": t, "param": null, "code": self.code()}})
            }
            ApiFamily::Anthropic => {
                let t = match self.kind {
                    ErrorKind::InvalidRequest | ErrorKind::CapabilityUnsupported => "invalid_request_error",
                    ErrorKind::Authentication => "authentication_error",
                    ErrorKind::Permission => "permission_error",
                    ErrorKind::NotFound | ErrorKind::PresetTarget => "not_found_error",
                    ErrorKind::TooLarge => "request_too_large",
                    ErrorKind::RateLimit => "rate_limit_error",
                    ErrorKind::Overloaded | ErrorKind::InsufficientMemory | ErrorKind::EngineBuildTooOld | ErrorKind::EnginePin => {
                        "overloaded_error"
                    }
                    _ => "api_error",
                };
                json!({"type": "error", "error": {"type": t, "message": self.message}})
            }
            ApiFamily::Ollama => json!({"error": self.message}),
            ApiFamily::Extended => json!({"error": {"code": self.code(), "message": self.message}}),
        }
    }
}

impl From<ChatError> for ApiError {
    fn from(e: ChatError) -> Self {
        match e {
            ChatError::Invalid(m) => ApiError::invalid(m),
        }
    }
}

/// A model as the listing endpoints show it.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelEntry {
    /// Its id (the `model` clients send).
    pub id: ModelId,
    /// File size.
    pub size: u64,
    /// Architecture, e.g. `qwen3`.
    pub family: Option<String>,
    /// e.g. `8B`.
    pub parameter_size: Option<String>,
    /// e.g. `Q4_K_M`.
    pub quant: Option<String>,
    /// When it was added.
    pub modified: Millis,
    /// SHA-256 of the file, when known.
    pub sha256: Option<String>,
    /// While loaded: memory and when the idle rule releases it.
    pub loaded: Option<LoadedEntry>,
}

/// A loaded model's listing details (Ollama `/api/ps`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadedEntry {
    /// Resident memory.
    pub ram: u64,
    /// Video memory.
    pub vram: u64,
    /// When it will be released (`None`: pinned).
    pub expires_at: Option<Millis>,
}

/// A JSON object or a 400 that says what was expected.
pub(crate) fn object<'a>(v: &'a Value, what: &str) -> Result<&'a serde_json::Map<String, Value>, ApiError> {
    v.as_object()
        .ok_or_else(|| ApiError::invalid(format!("{what} must be a JSON object")))
}

/// An optional field of a type, with a 400 naming it when it has the wrong type.
pub(crate) fn opt<'a, T>(
    o: &'a serde_json::Map<String, Value>,
    key: &str,
    f: impl Fn(&'a Value) -> Option<T>,
    want: &str,
) -> Result<Option<T>, ApiError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => f(v).map(Some).ok_or_else(|| ApiError::invalid(format!("`{key}` must be {want}"))),
    }
}

/// A float field.
pub(crate) fn opt_f32(o: &serde_json::Map<String, Value>, key: &str) -> Result<Option<f32>, ApiError> {
    opt(o, key, |v| v.as_f64().map(|f| f as f32), "a number")
}

/// A non-negative integer field (saturating at `u32::MAX`).
pub(crate) fn opt_u32(o: &serde_json::Map<String, Value>, key: &str) -> Result<Option<u32>, ApiError> {
    opt(
        o,
        key,
        |v| v.as_u64().map(|n| n.min(u64::from(u32::MAX)) as u32),
        "a non-negative integer",
    )
}

/// `stop`: a string or an array of strings.
pub(crate) fn stops(o: &serde_json::Map<String, Value>, key: &str) -> Result<Vec<String>, ApiError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::String(s)) => Ok(vec![s.clone()]),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| {
                x.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| ApiError::invalid(format!("`{key}` must hold strings")))
            })
            .collect(),
        Some(_) => Err(ApiError::invalid(format!("`{key}` must be a string or an array of strings"))),
    }
}

/// The model field (required for the standard APIs; `"auto"` and `"default"` mean Pitwall's choice).
pub(crate) fn model_ref(o: &serde_json::Map<String, Value>) -> Result<crate::chat::ModelRef, ApiError> {
    match o.get("model") {
        Some(Value::String(s)) if s.trim().is_empty() || s == "auto" || s == "default" => Ok(crate::chat::ModelRef::Default),
        Some(Value::String(s)) => Ok(crate::chat::ModelRef::Named(s.trim().to_string())),
        None | Some(Value::Null) => Ok(crate::chat::ModelRef::Default),
        Some(_) => Err(ApiError::invalid("`model` must be a string")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_in_every_shape() {
        let all = [
            ErrorKind::InvalidRequest,
            ErrorKind::Authentication,
            ErrorKind::Permission,
            ErrorKind::NotFound,
            ErrorKind::TooLarge,
            ErrorKind::RateLimit,
            ErrorKind::Overloaded,
            ErrorKind::InsufficientMemory,
            ErrorKind::NotAvailable,
            ErrorKind::Timeout,
            ErrorKind::PresetTarget,
            ErrorKind::Engine,
            ErrorKind::EngineBuildTooOld,
            ErrorKind::CapabilityUnsupported,
            ErrorKind::EnginePin,
        ];
        let statuses: Vec<u16> = all.iter().map(|k| ApiError::new(*k, "m").status()).collect();
        assert_eq!(
            statuses,
            vec![400, 401, 403, 404, 413, 429, 503, 503, 501, 504, 404, 500, 503, 400, 503]
        );
        for k in all {
            let e = ApiError::new(k, "msg");
            assert_eq!(e.body(ApiFamily::OpenAi)["error"]["message"], "msg");
            assert_eq!(e.body(ApiFamily::OpenAi)["error"]["code"], e.code());
            assert_eq!(e.body(ApiFamily::Anthropic)["type"], "error");
            assert!(e.body(ApiFamily::Anthropic)["error"]["type"].as_str().unwrap().ends_with("error") || k == ErrorKind::TooLarge);
            assert_eq!(e.body(ApiFamily::Ollama)["error"], "msg");
            assert_eq!(e.body(ApiFamily::Extended)["error"]["code"], e.code());
        }
        assert_eq!(
            ApiError::new(ErrorKind::RateLimit, "x").body(ApiFamily::OpenAi)["error"]["type"],
            "rate_limit_error"
        );
        assert_eq!(
            ApiError::new(ErrorKind::Overloaded, "x").body(ApiFamily::Anthropic)["error"]["type"],
            "overloaded_error"
        );
        let shapes = |k| {
            let e = ApiError::new(k, "x");
            (
                e.body(ApiFamily::OpenAi)["error"]["type"].clone(),
                e.body(ApiFamily::Anthropic)["error"]["type"].clone(),
            )
        };
        assert_eq!(
            shapes(ErrorKind::EngineBuildTooOld),
            (json!("server_error"), json!("overloaded_error"))
        );
        assert_eq!(shapes(ErrorKind::EnginePin), (json!("server_error"), json!("overloaded_error")));
        assert_eq!(
            shapes(ErrorKind::CapabilityUnsupported),
            (json!("invalid_request_error"), json!("invalid_request_error"))
        );
        let e: ApiError = ChatError::Invalid("bad".into()).into();
        assert_eq!((e.status(), e.to_string()), (400, "bad".to_string()));
        assert_eq!(serde_json::to_value(ApiFamily::OpenAi).unwrap(), "openai");
    }

    #[test]
    fn field_helpers() {
        let v = json!({"a": 1.5, "n": 7, "s": "x", "arr": ["a", "b"], "bad": [1], "obj": {}, "model": "auto"});
        let o = object(&v, "body").unwrap();
        assert!(object(&json!(1), "body").unwrap_err().to_string().contains("JSON object"));
        assert_eq!(opt_f32(o, "a").unwrap(), Some(1.5));
        assert_eq!(opt_f32(o, "missing").unwrap(), None);
        assert!(opt_f32(o, "s").unwrap_err().to_string().contains("`s` must be a number"));
        assert_eq!(opt_u32(o, "n").unwrap(), Some(7));
        assert_eq!(stops(o, "s").unwrap(), vec!["x"]);
        assert_eq!(stops(o, "arr").unwrap(), vec!["a", "b"]);
        assert!(stops(o, "bad").is_err());
        assert!(stops(o, "obj").is_err());
        assert!(stops(o, "none").unwrap().is_empty());
        assert_eq!(model_ref(o).unwrap(), crate::chat::ModelRef::Default);
        let o2 = json!({"model": " qwen "});
        assert_eq!(
            model_ref(o2.as_object().unwrap()).unwrap(),
            crate::chat::ModelRef::Named("qwen".into())
        );
        assert!(model_ref(json!({"model": 1}).as_object().unwrap()).is_err());
        assert_eq!(model_ref(json!({}).as_object().unwrap()).unwrap(), crate::chat::ModelRef::Default);
    }
}
