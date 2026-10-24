//! Typed requests: what an app asks of Pitwall, and how it becomes the OpenAI-compatible body. Nothing here touches the
//! network, so the body shapes are table-tested.

use crate::model::{CallOptions, ModelSel};
use serde_json::{Value, json};

/// Who says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The instructions.
    System,
    /// The user.
    User,
    /// The model's earlier answers.
    Assistant,
}

impl Role {
    /// The wire name.
    pub fn name(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

/// A piece of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// Text.
    Text(String),
    /// An image sent inline as a `data:` URL (the app has sanitised it).
    Image {
        /// `image/png`, `image/jpeg`, ...
        mime: String,
        /// The image bytes, base64 (the caller encodes; this crate adds no codec).
        base64: String,
    },
}

/// One message of a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Who says it.
    pub role: Role,
    /// What is said, in order.
    pub parts: Vec<Part>,
}

impl Message {
    /// A text message.
    pub fn text(role: Role, text: impl Into<String>) -> Message {
        Message {
            role,
            parts: vec![Part::Text(text.into())],
        }
    }

    /// A user message with images before its text.
    pub fn user_with_images(text: impl Into<String>, images: Vec<(String, String)>) -> Message {
        let mut parts: Vec<Part> = images
            .into_iter()
            .map(|(mime, base64)| Part::Image { mime, base64 })
            .collect();
        parts.push(Part::Text(text.into()));
        Message {
            role: Role::User,
            parts,
        }
    }

    /// Whether the message carries an image.
    pub fn has_images(&self) -> bool {
        self.parts.iter().any(|p| matches!(p, Part::Image { .. }))
    }

    fn to_json(&self) -> Value {
        let content = match self.parts.as_slice() {
            [Part::Text(t)] => json!(t),
            parts => Value::Array(
                parts
                    .iter()
                    .map(|p| match p {
                        Part::Text(t) => json!({"type": "text", "text": t}),
                        Part::Image { mime, base64 } => json!({"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{base64}")}}),
                    })
                    .collect(),
            ),
        };
        json!({"role": self.role.name(), "content": content})
    }
}

/// A chat completion.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    /// Which model.
    pub model: ModelSel,
    /// The conversation.
    pub messages: Vec<Message>,
    /// OpenAI tool definitions, passed through as they are (most apps use their own prompt protocol instead).
    pub tools: Vec<Value>,
    /// A JSON schema the answer must follow (`response_format: json_schema`).
    pub json_schema: Option<Value>,
    /// Most tokens to generate.
    pub max_tokens: Option<u32>,
    /// Sampling temperature.
    pub temperature: Option<f32>,
    /// Priority, cache, cancel and limits.
    pub options: CallOptions,
}

impl ChatRequest {
    /// A request for `model` with these messages and default options.
    pub fn new(model: ModelSel, messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            model,
            messages,
            tools: vec![],
            json_schema: None,
            max_tokens: None,
            temperature: None,
            options: CallOptions::default(),
        }
    }

    /// Whether any message carries an image.
    pub fn has_images(&self) -> bool {
        self.messages.iter().any(Message::has_images)
    }

    /// The model to ask for: images add `vision` to a capability selection.
    pub fn model_text(&self) -> String {
        if self.has_images() {
            self.model.with_vision().text()
        } else {
            self.model.text()
        }
    }

    /// The `POST /v1/chat/completions` body.
    pub fn body(&self, stream: bool) -> Value {
        let mut body = json!({
            "model": self.model_text(),
            "messages": self.messages.iter().map(Message::to_json).collect::<Vec<_>>(),
            "stream": stream,
        });
        if let Some(n) = self.max_tokens {
            body["max_tokens"] = json!(n);
        }
        if let Some(t) = self.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(schema) = &self.json_schema {
            body["response_format"] = json!({"type": "json_schema", "json_schema": {"name": "answer", "schema": schema, "strict": true}});
        }
        if !self.tools.is_empty() {
            body["tools"] = Value::Array(self.tools.clone());
        }
        body
    }
}

/// An embeddings request.
#[derive(Debug, Clone)]
pub struct EmbedRequest {
    /// Which model; an embedding-capable selection by default.
    pub model: ModelSel,
    /// The texts, answered in the same order.
    pub input: Vec<String>,
    /// Priority, cache, cancel and limits.
    pub options: CallOptions,
}

impl EmbedRequest {
    /// Texts to embed with the default model selection.
    pub fn new(input: Vec<String>) -> EmbedRequest {
        EmbedRequest {
            model: ModelSel::Auto(crate::model::Caps::embeddings()),
            input,
            options: CallOptions::default(),
        }
    }
}

/// The extended one-call tasks (`POST /x/v1/tasks/{task}`), when the server has them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// Shorten a text.
    Summarize,
    /// Pick a label.
    Classify,
    /// Pull out fields.
    Extract,
    /// Translate.
    Translate,
}

impl Task {
    /// The path segment.
    pub fn name(self) -> &'static str {
        match self {
            Task::Summarize => "summarize",
            Task::Classify => "classify",
            Task::Extract => "extract",
            Task::Translate => "translate",
        }
    }
}

/// Which part of a long text `truncate` keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// The start.
    Start,
    /// The end.
    End,
    /// Both ends.
    Ends,
}

impl Keep {
    /// The wire name.
    pub fn name(self) -> &'static str {
        match self {
            Keep::Start => "start",
            Keep::End => "end",
            Keep::Ends => "ends",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Caps;

    #[test]
    fn the_body_has_system_turns_images_and_schema() {
        let mut req = ChatRequest::new(
            ModelSel::Auto(Caps::chat(8192)),
            vec![
                Message::text(Role::System, "sys"),
                Message::user_with_images("look", vec![("image/png".into(), "AQID".into())]),
                Message::text(Role::Assistant, "ok"),
            ],
        );
        req.json_schema = Some(json!({"type": "object"}));
        req.max_tokens = Some(9);
        req.temperature = Some(0.5);
        let b = req.body(true);
        assert_eq!(
            b["model"], "auto:chat,vision,8k",
            "images add vision to a capability selection"
        );
        assert_eq!(
            b["messages"][0],
            json!({"role": "system", "content": "sys"})
        );
        assert_eq!(
            b["messages"][1]["content"][0]["image_url"]["url"],
            "data:image/png;base64,AQID"
        );
        assert_eq!(
            b["messages"][1]["content"][1],
            json!({"type": "text", "text": "look"})
        );
        assert_eq!(
            b["messages"][2],
            json!({"role": "assistant", "content": "ok"})
        );
        assert_eq!(
            (
                b["stream"].as_bool(),
                b["max_tokens"].as_u64(),
                b["temperature"].as_f64()
            ),
            (Some(true), Some(9), Some(0.5))
        );
        assert_eq!(
            b["response_format"]["json_schema"]["schema"],
            json!({"type": "object"})
        );
        assert!(b.get("tools").is_none());
    }

    #[test]
    fn a_plain_request_sends_only_what_was_set() {
        let mut req = ChatRequest::new(
            ModelSel::Alias("debrief".into()),
            vec![Message::text(Role::User, "hi")],
        );
        let b = req.body(false);
        assert_eq!(
            b,
            json!({"model": "debrief", "messages": [{"role": "user", "content": "hi"}], "stream": false})
        );
        assert!(!req.has_images());
        req.tools = vec![json!({"type": "function"})];
        assert_eq!(req.body(false)["tools"][0]["type"], "function");
        // A named alias is not rewritten for images (Pitwall refuses it when it cannot see).
        req.messages.push(Message::user_with_images(
            "x",
            vec![("image/png".into(), "AA".into())],
        ));
        assert_eq!(req.model_text(), "debrief");
    }

    #[test]
    fn task_keep_and_embed_defaults() {
        assert_eq!(
            [
                Task::Summarize,
                Task::Classify,
                Task::Extract,
                Task::Translate
            ]
            .map(Task::name),
            ["summarize", "classify", "extract", "translate"]
        );
        assert_eq!(
            [Keep::Start, Keep::End, Keep::Ends].map(Keep::name),
            ["start", "end", "ends"]
        );
        assert_eq!(
            EmbedRequest::new(vec!["a".into()]).model.text(),
            "auto:embeddings"
        );
        assert_eq!(
            [Role::System, Role::User, Role::Assistant].map(Role::name),
            ["system", "user", "assistant"]
        );
    }
}
