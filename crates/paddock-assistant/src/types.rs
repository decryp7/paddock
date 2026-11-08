//! Provider-neutral request and response types of the [`LlmProvider`](crate::LlmProvider) port.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

#[derive(Debug, Default)]
struct CancelInner {
    flag: Arc<AtomicBool>,
    lock: Mutex<()>,
    cv: Condvar,
}

/// The flag that stops a running turn. Cloning shares it; a provider checks it between tokens (Pitwall's client acts on it
/// within a 50 ms read slice, even while the server is silent). [`wait`](Cancel::wait) blocks until it is set, for a provider
/// that has nothing to poll.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<CancelInner>);

impl Cancel {
    /// A flag that is not set.
    pub fn new() -> Cancel {
        Cancel::default()
    }

    /// Sets the flag and wakes everything that waits for it.
    pub fn cancel(&self) {
        self.0.flag.store(true, Ordering::SeqCst);
        let _guard = self.0.lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.0.cv.notify_all();
    }

    /// Whether it is set.
    pub fn is_cancelled(&self) -> bool {
        self.0.flag.load(Ordering::SeqCst)
    }

    /// Blocks until the flag is set.
    pub fn wait(&self) {
        let mut guard = self.0.lock.lock().unwrap_or_else(PoisonError::into_inner);
        while !self.is_cancelled() {
            guard = self
                .0
                .cv
                .wait(guard)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// The shared flag itself, for APIs that take an `Arc<AtomicBool>` (`paddock_pitwall::CancelToken::from_flag`).
    pub fn flag(&self) -> Arc<AtomicBool> {
        self.0.flag.clone()
    }
}

/// Who says a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The user.
    User,
    /// The model.
    Assistant,
    /// The result of a tool call (answers one [`ToolCall::id`]).
    Tool,
}

/// A tool call the model made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ToolCall {
    /// Identifies the call within its turn (`call_1`); the result answers it.
    pub id: String,
    /// The tool's name; empty when the call could not be read.
    pub name: String,
    /// The arguments; `{"_invalid_json": "<raw>"}` when the call could not be read.
    #[cfg_attr(test, ts(type = "unknown"))]
    pub input: Value,
}

impl ToolCall {
    /// A call as an engine's native tool calling reports it: the arguments are a JSON text, parsed strictly (an engine that
    /// constrains its output to JSON gives no reason for the prompted protocol's repairs). Blank arguments are an empty
    /// object; text that is not one JSON value, or longer than [`MAX_CALL_LEN`](crate::protocol::MAX_CALL_LEN), becomes
    /// the marker of a failed parse (`{"_invalid_json": "<raw, at most 500 characters>"}`), so the loop tells the model and
    /// never runs it. An empty id is fine: the loop gives the call one.
    pub fn native(id: impl Into<String>, name: impl Into<String>, arguments: &str) -> ToolCall {
        let raw = arguments.trim();
        let parsed = if raw.is_empty() {
            Some(Value::Object(serde_json::Map::new()))
        } else if raw.len() > crate::protocol::MAX_CALL_LEN {
            None
        } else {
            serde_json::from_str::<Value>(raw).ok()
        };
        let input = parsed.unwrap_or_else(|| {
            serde_json::json!({"_invalid_json": arguments.chars().take(500).collect::<String>()})
        });
        ToolCall {
            id: id.into(),
            name: name.into(),
            input,
        }
    }

    /// Whether the call could not be read (no name, or the arguments are the marker of a failed parse).
    pub fn is_malformed(&self) -> bool {
        self.name.is_empty() || self.input.get("_invalid_json").is_some()
    }
}

/// Most attachments one question carries (the engine's own limit is 16 images per request).
pub const MAX_ATTACHMENTS: usize = 16;
/// Longest attachment file name.
pub const MAX_ATTACHMENT_NAME: usize = 255;

/// A part of a question besides its text (ADR 0023 WP-2b): a reference to a file the app keeps by its SHA-256. The bytes
/// never pass through the assistant; the app's provider reads them when it builds the model's request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Attachment {
    /// An image (PNG, JPEG, WebP or GIF), as prepared by the app.
    Image {
        /// SHA-256 of the prepared file, lowercase hex.
        sha256: String,
        /// Its type.
        mime: String,
        /// Its size in pixels.
        width: u32,
        /// Its size in pixels.
        height: u32,
    },
    /// An audio clip.
    Audio {
        /// SHA-256 of the clip, lowercase hex.
        sha256: String,
        /// Its length.
        seconds: u32,
    },
    /// A text file, attached as text.
    File {
        /// SHA-256 of the file, lowercase hex.
        sha256: String,
        /// Its name as the user knows it.
        name: String,
    },
}

const IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];

impl Attachment {
    /// The reference's file key.
    pub fn sha256(&self) -> &str {
        match self {
            Attachment::Image { sha256, .. }
            | Attachment::Audio { sha256, .. }
            | Attachment::File { sha256, .. } => sha256,
        }
    }

    /// What is wrong with it, in words for the user; `None` when it is sound.
    pub fn problem(&self) -> Option<String> {
        let sha = self.sha256();
        if sha.len() != 64
            || !sha
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            return Some("An attachment is not a file the app keeps. Attach it again.".into());
        }
        match self {
            Attachment::Image { mime, .. } if !IMAGE_TYPES.contains(&mime.as_str()) => {
                Some("An image must be PNG, JPEG, WebP or GIF.".into())
            }
            Attachment::File { name, .. }
                if name.trim().is_empty()
                    || name.chars().count() > MAX_ATTACHMENT_NAME
                    || name.contains(['/', '\\'])
                    || name.chars().any(char::is_control) =>
            {
                Some("An attachment's file name is not usable. Attach it again.".into())
            }
            _ => None,
        }
    }
}

/// One message of the conversation as the model sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    /// Who says it.
    pub role: Role,
    /// What is said.
    pub content: String,
    /// The tool calls of an assistant message.
    pub tool_calls: Vec<ToolCall>,
    /// For [`Role::Tool`]: the call this answers.
    pub tool_call_id: Option<String>,
    /// For a user message: what was attached, by reference.
    pub attachments: Vec<Attachment>,
}

impl ChatMessage {
    /// A user message with its attachments.
    #[must_use]
    pub fn with_attachments(mut self, attachments: Vec<Attachment>) -> ChatMessage {
        self.attachments = attachments;
        self
    }

    /// A user message.
    pub fn user(content: impl Into<String>) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            content: content.into(),
            tool_calls: vec![],
            tool_call_id: None,
            attachments: vec![],
        }
    }

    /// An assistant message, with the calls it made.
    pub fn assistant(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            content: content.into(),
            tool_calls,
            tool_call_id: None,
            attachments: vec![],
        }
    }

    /// The result of a call.
    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> ChatMessage {
        ChatMessage {
            role: Role::Tool,
            content: content.into(),
            tool_calls: vec![],
            tool_call_id: Some(call_id.into()),
            attachments: vec![],
        }
    }
}

/// A tool the model may call; `input_schema` is a JSON schema object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    /// `lower_snake_case`.
    pub name: String,
    /// What it does, for the model.
    pub description: String,
    /// The arguments as a JSON schema (`type: object`).
    pub input_schema: Value,
}

/// A request to a provider.
#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    /// The system prompt.
    pub system: String,
    /// The conversation.
    pub messages: Vec<ChatMessage>,
    /// Hard cap on generated tokens; 0 means [`DEFAULT_MAX_TOKENS`](crate::protocol::DEFAULT_MAX_TOKENS).
    pub max_tokens: usize,
    /// The tools offered (the tool protocol goes into the system prompt).
    pub tools: Vec<ToolSpec>,
    /// Sampling temperature; `None` means [`DEFAULT_TEMPERATURE`](crate::protocol::DEFAULT_TEMPERATURE).
    pub temperature: Option<f32>,
    /// Constrain the answer to JSON matching this schema (no tools then).
    pub json_schema: Option<Value>,
    /// Stops the generation.
    pub cancel: Cancel,
}

/// Why a generation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The model finished its answer.
    EndTurn,
    /// The token cap was reached.
    MaxTokens,
    /// The model called tools.
    ToolUse,
    /// The cancel flag was set; the text is what was generated until then.
    Cancelled,
}

/// Token counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Tokens in the prompts.
    #[cfg_attr(test, ts(type = "number"))]
    pub input_tokens: u64,
    /// Tokens generated.
    #[cfg_attr(test, ts(type = "number"))]
    pub output_tokens: u64,
}

impl Usage {
    /// Adds `other` (saturating).
    pub fn add(&mut self, other: Usage) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
    }

    /// Input plus output.
    pub fn total(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

/// A provider's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatResponse {
    /// The visible text (thinking and protocol markup removed).
    pub text: String,
    /// The tool calls made.
    pub tool_calls: Vec<ToolCall>,
    /// Why it ended.
    pub stop_reason: StopReason,
    /// Token counts, when the engine reports them.
    pub usage: Option<Usage>,
}
