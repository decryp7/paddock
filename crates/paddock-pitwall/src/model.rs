//! What a request asks of Pitwall's models: a named alias or model, or a capability selection (`auto:chat,8k,vision`), plus
//! how urgent the call is and how its answer may be cached.

use crate::loopback::{CancelToken, Timeouts};
use serde::Serialize;
use std::time::Duration;

/// What a model must be able to do, and how it is preferred (Pitwall's `auto:` selection, ADR 0014).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Caps {
    /// Chat completions.
    pub chat: bool,
    /// Images in the prompt.
    pub vision: bool,
    /// Native tool calls.
    pub tools: bool,
    /// JSON-schema answers.
    pub json: bool,
    /// Embeddings.
    pub embeddings: bool,
    /// At least this many tokens of context.
    pub context_tokens: Option<u32>,
    /// Prefer the fastest model.
    pub fast: bool,
}

impl Caps {
    /// A chat model with at least `context_tokens` of context (rounded up to whole thousands in the request).
    pub fn chat(context_tokens: u32) -> Caps {
        Caps {
            chat: true,
            context_tokens: Some(context_tokens),
            ..Caps::default()
        }
    }

    /// The same, with images.
    pub fn with_vision(mut self) -> Caps {
        self.vision = true;
        self
    }

    /// An embedding model.
    pub fn embeddings() -> Caps {
        Caps {
            embeddings: true,
            ..Caps::default()
        }
    }

    /// The `auto:` text Pitwall parses: capabilities, then the context as `<n>k`, then `fast`.
    pub fn to_auto(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for (on, name) in [
            (self.chat, "chat"),
            (self.vision, "vision"),
            (self.tools, "tools"),
            (self.json, "json"),
            (self.embeddings, "embeddings"),
        ] {
            if on {
                parts.push(name.into());
            }
        }
        if let Some(n) = self.context_tokens {
            parts.push(format!("{}k", n.div_ceil(1024).max(1)));
        }
        if self.fast {
            parts.push("fast".into());
        }
        if parts.is_empty() {
            "auto".into()
        } else {
            format!("auto:{}", parts.join(","))
        }
    }
}

/// Which model a request names (the `model` field).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSel {
    /// A preset name the user defined in Pitwall (Pitwall ADR 0014), for example the app's own (`debrief`).
    Alias(String),
    /// One installed model.
    Exact(String),
    /// Whatever fits these requirements when the request arrives.
    Auto(Caps),
}

impl ModelSel {
    /// The `model` text.
    pub fn text(&self) -> String {
        match self {
            ModelSel::Alias(s) | ModelSel::Exact(s) => s.clone(),
            ModelSel::Auto(c) => c.to_auto(),
        }
    }

    /// The same selection with images: a capability selection gains `vision`; a named model is left as it is (Pitwall
    /// refuses it with `capability_unsupported` if it cannot see).
    pub fn with_vision(&self) -> ModelSel {
        match self {
            ModelSel::Auto(c) => ModelSel::Auto((*c).with_vision()),
            other => other.clone(),
        }
    }
}

/// How urgent a request is for Pitwall's queue (`X-Pitwall-Priority`); the app's permission may clamp it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Priority {
    /// Something the user waits for (chats, use cases the user started).
    #[default]
    Interactive,
    /// Work nobody waits for (name aliases, labelling).
    Background,
    /// Bulk work that yields to everything else (review, extraction over many items).
    Batch,
}

impl Priority {
    /// The header value.
    pub fn header(self) -> &'static str {
        match self {
            Priority::Interactive => "interactive",
            Priority::Background => "background",
            Priority::Batch => "batch",
        }
    }
}

/// How Pitwall's response cache may be used (ADR 0019).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CachePolicy {
    /// Pitwall's default.
    #[default]
    Default,
    /// Do not read the cache (`Cache-Control: no-cache`); the answer may still be stored.
    Bypass,
    /// Neither read nor store (`Cache-Control: no-store`).
    NoStore,
}

impl CachePolicy {
    /// The `Cache-Control` value, when one is sent.
    pub fn header(self) -> Option<&'static str> {
        match self {
            CachePolicy::Default => None,
            CachePolicy::Bypass => Some("no-cache"),
            CachePolicy::NoStore => Some("no-store"),
        }
    }
}

/// The options of one call.
#[derive(Debug, Clone)]
pub struct CallOptions {
    /// Queue priority.
    pub priority: Priority,
    /// Cache use.
    pub cache: CachePolicy,
    /// Cancels the call at once, in every phase.
    pub cancel: CancelToken,
    /// Connect, first-byte and idle limits; `None`: quick limits for plain calls, chat limits for chat.
    pub timeouts: Option<Timeouts>,
    /// The whole call (all retries included) must end by then; `None`: only the per-phase limits apply.
    pub deadline: Option<Duration>,
}

impl Default for CallOptions {
    fn default() -> Self {
        CallOptions {
            priority: Priority::Interactive,
            cache: CachePolicy::Default,
            cancel: CancelToken::new(),
            timeouts: None,
            deadline: None,
        }
    }
}

impl CallOptions {
    /// Options with this priority.
    pub fn with_priority(mut self, priority: Priority) -> CallOptions {
        self.priority = priority;
        self
    }

    /// Options with this cancel token.
    pub fn with_cancel(mut self, cancel: CancelToken) -> CallOptions {
        self.cancel = cancel;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_text_is_what_pitwall_parses() {
        assert_eq!(Caps::chat(8192).to_auto(), "auto:chat,8k");
        assert_eq!(Caps::chat(8000).to_auto(), "auto:chat,8k");
        assert_eq!(Caps::chat(1).to_auto(), "auto:chat,1k");
        assert_eq!(
            Caps::chat(8192).with_vision().to_auto(),
            "auto:chat,vision,8k"
        );
        assert_eq!(Caps::embeddings().to_auto(), "auto:embeddings");
        assert_eq!(Caps::default().to_auto(), "auto");
        let all = Caps {
            chat: true,
            vision: true,
            tools: true,
            json: true,
            embeddings: true,
            context_tokens: Some(4096),
            fast: true,
        };
        assert_eq!(
            all.to_auto(),
            "auto:chat,vision,tools,json,embeddings,4k,fast"
        );
    }

    #[test]
    fn selection_text_and_vision() {
        assert_eq!(ModelSel::Alias("debrief".into()).text(), "debrief");
        assert_eq!(ModelSel::Exact("qwen3-8b".into()).text(), "qwen3-8b");
        assert_eq!(
            ModelSel::Auto(Caps::chat(8192)).with_vision().text(),
            "auto:chat,vision,8k"
        );
        assert_eq!(
            ModelSel::Alias("debrief".into()).with_vision(),
            ModelSel::Alias("debrief".into())
        );
    }

    #[test]
    fn priority_and_cache_headers() {
        assert_eq!(
            [Priority::Interactive, Priority::Background, Priority::Batch].map(Priority::header),
            ["interactive", "background", "batch"]
        );
        assert_eq!(
            [
                CachePolicy::Default,
                CachePolicy::Bypass,
                CachePolicy::NoStore
            ]
            .map(CachePolicy::header),
            [None, Some("no-cache"), Some("no-store")]
        );
        let o = CallOptions::default().with_priority(Priority::Batch);
        assert_eq!(o.priority, Priority::Batch);
        let c = CancelToken::new();
        assert!(
            !CallOptions::default()
                .with_cancel(c.clone())
                .cancel
                .is_cancelled()
        );
        c.cancel();
    }
}
