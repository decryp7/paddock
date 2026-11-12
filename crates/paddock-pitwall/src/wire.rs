//! The answers of Pitwall's client contract (`/x/v1`, ADR 0003/0009), parsed into typed values at once. Unknown fields are
//! ignored (the contract grows additively); every field an older Pitwall may lack has a default.

use crate::loopback::Response;
use serde::Deserialize;
use serde_json::Value;

/// What `GET /x/v1/health` says.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    /// `pitwall`.
    #[serde(default)]
    pub name: String,
    /// Pitwall's version.
    #[serde(default)]
    pub version: String,
    /// The client contract version.
    pub api_version: u32,
    /// `serving`, `paused` or `stopped`.
    #[serde(default)]
    pub serving: String,
    /// Models in memory now.
    #[serde(default)]
    pub loaded_models: u32,
}

impl Health {
    /// Pitwall is serving requests.
    pub fn is_serving(&self) -> bool {
        self.serving == "serving"
    }
}

/// A pairing request Pitwall accepted: the code the user compares in Pitwall.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    /// The id to poll with. It is shown in Pitwall's window, so it proves nothing by itself.
    pub pairing_id: String,
    /// Sent with every poll (the `X-Pitwall-Pair-Token` header): 256 random bits only this caller got, without which
    /// Pitwall never hands the key over. Never logged or shown.
    pub poll_token: String,
    /// The six-digit code both sides show.
    pub code: String,
    /// Seconds until the request expires.
    #[serde(default = "five_minutes")]
    pub expires_in_secs: u64,
}

/// `poll_token` is a credential, so `Debug` never prints it.
impl std::fmt::Debug for Pairing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pairing")
            .field("pairing_id", &self.pairing_id)
            .field("poll_token", &"<redacted>")
            .field("code", &self.code)
            .field("expires_in_secs", &self.expires_in_secs)
            .finish()
    }
}

fn five_minutes() -> u64 {
    300
}

/// How a pairing stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairPoll {
    /// The user has not decided yet.
    Pending,
    /// Approved; the key comes exactly once (`None` on later polls).
    Approved(Option<String>),
    /// Denied, expired or unknown.
    Denied,
}

/// The model Pitwall would choose (`POST /x/v1/select`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    /// The model's id.
    pub model: String,
    /// `explicit`, `capabilities` or `default`.
    pub chosen_by: String,
    /// Already in memory.
    pub loaded: bool,
}

/// Why a model cannot do something now (`unavailable` in `GET /x/v1/models`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Blocked {
    /// The capability.
    #[serde(default)]
    pub capability: String,
    /// Pitwall's reason ("needs an engine build with ...").
    #[serde(default)]
    pub reason: String,
}

/// One model of `GET /x/v1/models`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// The id apps send.
    pub id: String,
    /// What it can do now (`chat`, `vision`, `tools`, `json`, `embeddings`).
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// What the engine in use cannot serve, with the reason.
    #[serde(default)]
    pub unavailable: Vec<Blocked>,
    /// The trained context.
    #[serde(default)]
    pub context_max: Option<u64>,
    /// The context each slot gets when loaded (a newer Pitwall; the app sizes its prompts by it).
    #[serde(default)]
    pub context_per_slot: Option<u64>,
    /// File size.
    #[serde(default)]
    pub size_bytes: Option<u64>,
    /// In memory now.
    #[serde(default)]
    pub loaded: bool,
}

impl ModelInfo {
    /// Whether it lists `cap` as servable now.
    pub fn can(&self, cap: &str) -> bool {
        self.capabilities.iter().any(|c| c == cap)
    }

    /// The context an app can plan prompts for: the per-slot value when Pitwall reports it, else the trained maximum.
    pub fn usable_context(&self) -> Option<u64> {
        self.context_per_slot.or(self.context_max)
    }

    /// Pitwall's reason a capability is blocked, if it is.
    pub fn blocked_reason(&self, cap: &str) -> Option<&str> {
        self.unavailable
            .iter()
            .find(|b| b.capability == cap)
            .map(|b| b.reason.as_str())
    }
}

/// A queued or running request (`GET /x/v1/jobs`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    /// The id (`DELETE /x/v1/jobs/{id}` cancels it).
    pub id: String,
    /// The app that submitted it.
    #[serde(default)]
    pub app_id: String,
    /// The model, once chosen.
    #[serde(default)]
    pub model_id: Option<String>,
    /// `queued` or `running`.
    #[serde(default)]
    pub state: String,
    /// Progress 0 to 1 when known.
    #[serde(default)]
    pub progress: Option<f32>,
    /// Tokens so far.
    #[serde(default)]
    pub tokens: u64,
}

/// What the response cache did for a request (`X-Pitwall-Cache`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStatus {
    /// Answered from the cache.
    Hit,
    /// Eligible, not found.
    Miss,
    /// The caller asked not to use the cache.
    Bypass,
}

/// Who answered, from the `X-Pitwall-*` headers, so an app can show "answered by model X (cached)".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Provenance {
    /// `X-Pitwall-Model`.
    pub model: Option<String>,
    /// `X-Pitwall-Engine`.
    pub engine: Option<String>,
    /// `X-Pitwall-Chosen-By`.
    pub chosen_by: Option<String>,
    /// `X-Pitwall-Cold-Start-Ms`: the model had to be loaded first.
    pub cold_start_ms: Option<u64>,
    /// `X-Pitwall-Cache`.
    pub cache: Option<CacheStatus>,
    /// `X-Pitwall-Alias`: the preset that resolved.
    pub alias: Option<String>,
    /// `X-Pitwall-Job-Id` (a newer Pitwall): cancels a queued request without closing the connection.
    pub job_id: Option<String>,
}

impl Provenance {
    /// Reads the headers of a response; values are bounded and unknown ones ignored.
    pub fn from_response(r: &Response) -> Provenance {
        let text = |n: &str| r.header(n).map(|v| v.chars().take(200).collect::<String>());
        Provenance {
            model: text("x-pitwall-model"),
            engine: text("x-pitwall-engine"),
            chosen_by: text("x-pitwall-chosen-by"),
            cold_start_ms: r
                .header("x-pitwall-cold-start-ms")
                .and_then(|v| v.trim().parse().ok()),
            cache: match r
                .header("x-pitwall-cache")
                .map(|v| v.trim().to_ascii_lowercase())
                .as_deref()
            {
                Some("hit") => Some(CacheStatus::Hit),
                Some("miss") => Some(CacheStatus::Miss),
                Some("bypass") => Some(CacheStatus::Bypass),
                _ => None,
            },
            alias: text("x-pitwall-alias"),
            job_id: text("x-pitwall-job-id").filter(|j| {
                j.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            }),
        }
    }
}

/// Token counts of an answer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// Tokens in the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated.
    pub completion_tokens: u64,
}

impl Usage {
    /// Reads an OpenAI `usage` object (snake case) or an extended one (camel case).
    pub fn from_value(u: &Value) -> Usage {
        let n = |a: &str, b: &str| {
            u.get(a)
                .or_else(|| u.get(b))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        };
        Usage {
            prompt_tokens: n("prompt_tokens", "promptTokens"),
            completion_tokens: n("completion_tokens", "completionTokens"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_pairing_and_models_ignore_unknown_fields_and_default_missing_ones() {
        let h: Health = serde_json::from_str(r#"{"ok":true,"name":"pitwall","version":"1.0.4","apiVersion":1,"serving":"paused","offline":true,"future":1}"#).unwrap();
        assert_eq!(
            (h.api_version, h.is_serving(), h.loaded_models),
            (1, false, 0)
        );
        assert!(
            serde_json::from_str::<Health>(r#"{"version":"1"}"#).is_err(),
            "apiVersion is required"
        );
        let p: Pairing =
            serde_json::from_str(r#"{"pairingId":"a","pollToken":"t","code":"123456"}"#).unwrap();
        assert_eq!(p.expires_in_secs, 300);
        let m: ModelInfo = serde_json::from_str(
            r#"{"id":"m","capabilities":["chat"],"unavailable":[{"capability":"vision","reason":"needs mtmd"}],"contextMax":32768}"#,
        )
        .unwrap();
        assert!(m.can("chat") && !m.can("vision"));
        assert_eq!(
            (
                m.usable_context(),
                m.blocked_reason("vision"),
                m.blocked_reason("tools")
            ),
            (Some(32768), Some("needs mtmd"), None)
        );
        let m2: ModelInfo =
            serde_json::from_str(r#"{"id":"m","contextMax":32768,"contextPerSlot":8192}"#).unwrap();
        assert_eq!(m2.usable_context(), Some(8192));
        let j: JobInfo =
            serde_json::from_str(r#"{"id":"7","appId":"debrief","state":"running","tokens":3}"#)
                .unwrap();
        assert_eq!((j.id.as_str(), j.tokens, j.model_id), ("7", 3, None));
    }

    #[test]
    fn usage_reads_both_spellings() {
        let a =
            Usage::from_value(&serde_json::json!({"prompt_tokens": 11, "completion_tokens": 3}));
        let b = Usage::from_value(&serde_json::json!({"promptTokens": 11, "completionTokens": 3}));
        assert_eq!(
            (a, b),
            (
                Usage {
                    prompt_tokens: 11,
                    completion_tokens: 3
                },
                Usage {
                    prompt_tokens: 11,
                    completion_tokens: 3
                }
            )
        );
        assert_eq!(Usage::from_value(&Value::Null), Usage::default());
    }

    /// The poll token is a credential: `{:?}` (a log line, a panic message) never shows it.
    #[test]
    fn debug_output_redacts_the_poll_token() {
        let p: Pairing = serde_json::from_str(
            r#"{"pairingId":"a","pollToken":"SECRET-TOKEN-1","code":"123456"}"#,
        )
        .unwrap();
        let shown = format!("{p:?} {p:#?}");
        assert!(!shown.contains("SECRET-TOKEN-1"), "{shown}");
        assert!(shown.contains("poll_token: \"<redacted>\""), "{shown}");
        assert!(shown.contains("123456"), "the rest stays visible: {shown}");
    }
}
