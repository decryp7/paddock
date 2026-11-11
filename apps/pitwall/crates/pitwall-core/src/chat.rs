//! The neutral chat request every API maps to, the events an engine produces, and the collector that turns a stream
//! of events into one answer (ADR 0003). The OpenAI, Anthropic and Ollama front-ends parse into [`ChatRequest`] and
//! format [`ChatEvent`]s; engines only ever see these types.

use crate::scheduler::Priority;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Most messages in one request.
pub const MAX_MESSAGES: usize = 4096;
/// Most images in one request.
pub const MAX_IMAGES: usize = 16;
/// Most audio clips in one request (judgement: a model that hears takes one clip a turn; a few allow a short history).
pub const MAX_AUDIO_CLIPS: usize = 4;
/// Most tools in one request.
pub const MAX_TOOLS: usize = 256;
/// Most stop sequences in one request.
pub const MAX_STOPS: usize = 16;

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Instructions.
    System,
    /// The user.
    User,
    /// The model.
    Assistant,
    /// The result of a tool call.
    Tool,
}

/// An image's encoding. Only inline data is accepted: Pitwall never fetches a URL a request names (ADR 0001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageMime {
    /// `image/png`.
    Png,
    /// `image/jpeg`.
    Jpeg,
    /// `image/webp`.
    Webp,
    /// `image/gif`.
    Gif,
}

impl ImageMime {
    /// From a media type.
    pub fn parse(mime: &str) -> Option<ImageMime> {
        match mime.trim().to_ascii_lowercase().as_str() {
            "image/png" => Some(ImageMime::Png),
            "image/jpeg" | "image/jpg" => Some(ImageMime::Jpeg),
            "image/webp" => Some(ImageMime::Webp),
            "image/gif" => Some(ImageMime::Gif),
            _ => None,
        }
    }

    /// The media type.
    pub fn as_str(self) -> &'static str {
        match self {
            ImageMime::Png => "image/png",
            ImageMime::Jpeg => "image/jpeg",
            ImageMime::Webp => "image/webp",
            ImageMime::Gif => "image/gif",
        }
    }

    /// Guesses the type from the first bytes of base64 data (Ollama sends bare base64).
    pub fn sniff_base64(data: &str) -> Option<ImageMime> {
        if data.starts_with("iVBORw0KGgo") {
            Some(ImageMime::Png)
        } else if data.starts_with("/9j/") {
            Some(ImageMime::Jpeg)
        } else if data.starts_with("UklGR") {
            Some(ImageMime::Webp)
        } else if data.starts_with("R0lGOD") {
            Some(ImageMime::Gif)
        } else {
            None
        }
    }
}

/// Inline image data (standard base64).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    /// Its encoding.
    pub mime: ImageMime,
    /// Standard base64, validated.
    pub data: String,
}

impl Image {
    /// Parses a `data:<mime>;base64,<data>` URL. Any other URL is refused with a message: remote images are never
    /// fetched.
    pub fn from_data_url(url: &str) -> Result<Image, ChatError> {
        let rest = url
            .strip_prefix("data:")
            .ok_or_else(|| ChatError::Invalid("images must be inline data: URLs (Pitwall never fetches remote content)".into()))?;
        let (meta, data) = rest
            .split_once(',')
            .ok_or_else(|| ChatError::Invalid("the image data URL has no data".into()))?;
        let mime = meta
            .strip_suffix(";base64")
            .and_then(ImageMime::parse)
            .ok_or_else(|| ChatError::Invalid("images must be base64 PNG, JPEG, WebP or GIF".into()))?;
        Image::from_base64(mime, data)
    }

    /// Validates base64 image data.
    pub fn from_base64(mime: ImageMime, data: &str) -> Result<Image, ChatError> {
        use base64::Engine;
        let data = data.trim();
        if data.is_empty() || base64::engine::general_purpose::STANDARD.decode(data).is_err() {
            return Err(ChatError::Invalid("an image is not valid base64".into()));
        }
        Ok(Image {
            mime,
            data: data.to_string(),
        })
    }

    /// As a data URL.
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.mime.as_str(), self.data)
    }
}

/// An audio clip's encoding: what llama-server's `input_audio` takes (`wav`, `mp3`; OpenAI's two formats).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioFormat {
    /// WAV.
    Wav,
    /// MP3.
    Mp3,
}

impl AudioFormat {
    /// From OpenAI's `format` value.
    pub fn parse(format: &str) -> Option<AudioFormat> {
        match format.trim().to_ascii_lowercase().as_str() {
            "wav" => Some(AudioFormat::Wav),
            "mp3" => Some(AudioFormat::Mp3),
            _ => None,
        }
    }

    /// OpenAI's `format` value.
    pub fn as_str(self) -> &'static str {
        match self {
            AudioFormat::Wav => "wav",
            AudioFormat::Mp3 => "mp3",
        }
    }
}

/// Inline audio data (standard base64), for a model that hears. Only inline data is accepted: Pitwall never fetches a URL a
/// request names (ADR 0001), and never sets llama-server's `--media-path`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Audio {
    /// Its encoding.
    pub format: AudioFormat,
    /// Standard base64, validated.
    pub data: String,
}

impl std::fmt::Debug for Audio {
    // Never the data: it is a recording (no user audio in logs).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Audio")
            .field("format", &self.format)
            .field("base64_bytes", &self.data.len())
            .finish()
    }
}

impl Audio {
    /// Validates base64 audio data.
    pub fn from_base64(format: AudioFormat, data: &str) -> Result<Audio, ChatError> {
        use base64::Engine;
        let data = data.trim();
        if data.is_empty() || base64::engine::general_purpose::STANDARD.decode(data).is_err() {
            return Err(ChatError::Invalid("an audio clip is not valid base64".into()));
        }
        Ok(Audio {
            format,
            data: data.to_string(),
        })
    }
}

/// A piece of a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Part {
    /// Text.
    Text(String),
    /// An image.
    Image(Image),
    /// An audio clip, for a model that hears.
    Audio(Audio),
}

/// A tool call the model made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// The call's id (answered by a `Tool` message's `tool_call_id`).
    pub id: String,
    /// The tool's name.
    pub name: String,
    /// The arguments as JSON text.
    pub arguments: String,
}

/// One message of the conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Who wrote it.
    pub role: Role,
    /// Its content.
    pub parts: Vec<Part>,
    /// Tool calls of an assistant message.
    pub tool_calls: Vec<ToolCall>,
    /// The call a tool message answers.
    pub tool_call_id: Option<String>,
}

impl Message {
    /// A text message.
    pub fn text(role: Role, text: impl Into<String>) -> Message {
        Message {
            role,
            parts: vec![Part::Text(text.into())],
            tool_calls: vec![],
            tool_call_id: None,
        }
    }

    /// All text parts joined.
    pub fn text_content(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| match p {
                Part::Text(t) => Some(t.as_str()),
                Part::Image(_) | Part::Audio(_) => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// The images of the message.
    pub fn images(&self) -> impl Iterator<Item = &Image> {
        self.parts.iter().filter_map(|p| match p {
            Part::Image(i) => Some(i),
            Part::Text(_) | Part::Audio(_) => None,
        })
    }

    /// The audio clips.
    pub fn audio(&self) -> impl Iterator<Item = &Audio> {
        self.parts.iter().filter_map(|p| match p {
            Part::Audio(a) => Some(a),
            Part::Text(_) | Part::Image(_) => None,
        })
    }
}

/// A tool the model may call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    /// Its name.
    pub name: String,
    /// What it does.
    pub description: String,
    /// A JSON schema object for its arguments.
    pub parameters: Value,
}

/// Whether and which tools the model must call.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ToolChoice {
    /// The model decides.
    #[default]
    Auto,
    /// No tools.
    None,
    /// At least one tool.
    Required,
    /// This tool.
    Named(String),
}

/// The shape of the answer.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum OutputFormat {
    /// Free text.
    #[default]
    Text,
    /// Any JSON object.
    JsonObject,
    /// JSON matching this schema (grammar-constrained on llama.cpp).
    JsonSchema(Value),
}

/// Which model answers.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum ModelRef {
    /// The model the client named (an id or an alias).
    Named(String),
    /// Pitwall's default model.
    #[default]
    Default,
}

/// The neutral chat request.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ChatRequest {
    /// Which model.
    pub model: ModelRef,
    /// The conversation (system messages included, in order).
    pub messages: Vec<Message>,
    /// Tools the model may call.
    pub tools: Vec<Tool>,
    /// Tool use.
    pub tool_choice: ToolChoice,
    /// Most tokens to generate.
    pub max_tokens: Option<u32>,
    /// Sampling temperature.
    pub temperature: Option<f32>,
    /// Nucleus sampling.
    pub top_p: Option<f32>,
    /// Top-k sampling.
    pub top_k: Option<u32>,
    /// Stop sequences.
    pub stop: Vec<String>,
    /// Sampling seed.
    pub seed: Option<i64>,
    /// The answer's shape.
    pub format: OutputFormat,
    /// Stream the answer.
    pub stream: bool,
    /// The context the client asks for (Ollama `num_ctx`).
    pub context: Option<u32>,
    /// The priority the client asked for (capped by its permissions).
    pub priority: Option<Priority>,
}

/// A request that cannot be served as sent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChatError {
    /// The request is malformed or over a limit; the message says what to fix.
    #[error("{0}")]
    Invalid(String),
}

impl ChatRequest {
    /// Checks the limits every front-end shares: messages, images, tools, stops, sampling ranges.
    pub fn check(&self) -> Result<(), ChatError> {
        let bad = |m: &str| Err(ChatError::Invalid(m.to_string()));
        if self.messages.is_empty() {
            return bad("the request has no messages");
        }
        if self.messages.len() > MAX_MESSAGES {
            return bad("the request has too many messages");
        }
        if self.messages.iter().map(|m| m.images().count()).sum::<usize>() > MAX_IMAGES {
            return bad("the request has too many images");
        }
        if self.messages.iter().map(|m| m.audio().count()).sum::<usize>() > MAX_AUDIO_CLIPS {
            return bad("the request has too many audio clips");
        }
        if self.tools.len() > MAX_TOOLS {
            return bad("the request has too many tools");
        }
        if self.stop.len() > MAX_STOPS {
            return bad("the request has too many stop sequences");
        }
        if self.temperature.is_some_and(|t| !(0.0..=2.0).contains(&t)) {
            return bad("temperature must be between 0 and 2");
        }
        if self.top_p.is_some_and(|p| !(0.0..=1.0).contains(&p)) {
            return bad("top_p must be between 0 and 1");
        }
        if let ToolChoice::Named(n) = &self.tool_choice {
            if !self.tools.iter().any(|t| &t.name == n) {
                return Err(ChatError::Invalid(format!("tool_choice names {n:?}, which is not among the tools")));
            }
        }
        Ok(())
    }

    /// Whether any message carries an image.
    pub fn has_images(&self) -> bool {
        self.messages.iter().any(|m| m.images().next().is_some())
    }

    /// Whether any message carries an audio clip.
    pub fn has_audio(&self) -> bool {
        self.messages.iter().any(|m| m.audio().next().is_some())
    }
}

/// Why generation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Finish {
    /// The model ended its turn (or hit a stop sequence).
    Stop,
    /// `max_tokens` reached.
    Length,
    /// The model called tools.
    ToolCalls,
    /// The client went away or cancelled.
    Cancelled,
}

/// Token counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Prompt tokens.
    pub prompt_tokens: u32,
    /// Generated tokens.
    pub completion_tokens: u32,
}

/// Engine timings of one answer, when it reports them.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Timings {
    /// Prompt tokens per second.
    pub prompt_tps: f64,
    /// Generated tokens per second.
    pub gen_tps: f64,
    /// Prompt tokens the engine reused from its prompt cache instead of processing again.
    pub cached_tokens: u32,
}

/// What an engine streams.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChatEvent {
    /// More answer text.
    Delta(String),
    /// More reasoning text (`<think>` content the engine separated).
    Reasoning(String),
    /// A piece of a tool call; `index` groups the pieces of one call.
    ToolCall {
        /// Which call.
        index: u32,
        /// The id, in the first piece.
        id: Option<String>,
        /// The name, in the first piece.
        name: Option<String>,
        /// More of the arguments' JSON text.
        arguments: String,
    },
    /// The end.
    Done {
        /// Why.
        finish: Finish,
        /// Token counts.
        usage: Usage,
        /// Speeds, when known.
        timings: Option<Timings>,
    },
}

/// A whole answer.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ChatOutput {
    /// The text.
    pub text: String,
    /// Reasoning text, if the engine separated it.
    pub reasoning: String,
    /// Tool calls.
    pub tool_calls: Vec<ToolCall>,
    /// Why it ended (`None` until the stream ended).
    pub finish: Option<Finish>,
    /// Token counts.
    pub usage: Usage,
    /// Speeds, when known.
    pub timings: Option<Timings>,
}

impl ChatOutput {
    /// Folds one event in. Tool call pieces are joined by index; ids are generated when the engine sent none.
    pub fn push(&mut self, e: &ChatEvent) {
        match e {
            ChatEvent::Delta(t) => self.text.push_str(t),
            ChatEvent::Reasoning(t) => self.reasoning.push_str(t),
            ChatEvent::ToolCall {
                index,
                id,
                name,
                arguments,
            } => {
                let i = *index as usize;
                if i >= MAX_TOOLS {
                    return;
                }
                while self.tool_calls.len() <= i {
                    let n = self.tool_calls.len();
                    self.tool_calls.push(ToolCall {
                        id: format!("call_{n}"),
                        name: String::new(),
                        arguments: String::new(),
                    });
                }
                let c = &mut self.tool_calls[i];
                if let Some(id) = id.as_ref().filter(|s| !s.is_empty()) {
                    c.id.clone_from(id);
                }
                if let Some(n) = name {
                    c.name.push_str(n);
                }
                c.arguments.push_str(arguments);
            }
            ChatEvent::Done { finish, usage, timings } => {
                self.finish = Some(if *finish == Finish::Stop && !self.tool_calls.is_empty() {
                    Finish::ToolCalls
                } else {
                    *finish
                });
                self.usage = *usage;
                self.timings = *timings;
            }
        }
    }

    /// Collects a whole stream.
    pub fn collect<'a>(events: impl IntoIterator<Item = &'a ChatEvent>) -> ChatOutput {
        let mut o = ChatOutput::default();
        for e in events {
            o.push(e);
        }
        o
    }
}

/// The neutral embedding request.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EmbedRequest {
    /// Which model.
    pub model: ModelRef,
    /// The texts.
    pub input: Vec<String>,
}

/// Most texts in one embedding request.
pub const MAX_EMBED_INPUTS: usize = 2048;

impl EmbedRequest {
    /// Checks the limits.
    pub fn check(&self) -> Result<(), ChatError> {
        if self.input.is_empty() {
            return Err(ChatError::Invalid("input is empty".into()));
        }
        if self.input.len() > MAX_EMBED_INPUTS {
            return Err(ChatError::Invalid(format!("at most {MAX_EMBED_INPUTS} inputs per request")));
        }
        Ok(())
    }
}

/// Embeddings in input order.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EmbedOutput {
    /// One vector per input.
    pub vectors: Vec<Vec<f32>>,
    /// Prompt tokens.
    pub prompt_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> ChatRequest {
        ChatRequest {
            messages: vec![Message::text(Role::User, "hi")],
            ..Default::default()
        }
    }

    #[test]
    fn audio_is_checked_counted_and_never_printed() {
        assert_eq!(AudioFormat::parse(" WAV "), Some(AudioFormat::Wav));
        assert_eq!(AudioFormat::parse("mp3").map(AudioFormat::as_str), Some("mp3"));
        assert_eq!(AudioFormat::parse("m4a"), None);
        assert!(Audio::from_base64(AudioFormat::Wav, "***").is_err());
        assert!(Audio::from_base64(AudioFormat::Wav, " ").is_err());
        let a = Audio::from_base64(AudioFormat::Wav, "UklGRiQAAABXQVZF").unwrap();
        assert!(!format!("{a:?}").contains("UklGR"), "no audio in debug output");
        let msg = |n: usize| Message {
            role: Role::User,
            parts: vec![Part::Audio(a.clone()); n],
            tool_calls: vec![],
            tool_call_id: None,
        };
        let mut r = req();
        assert!(!r.has_audio());
        r.messages = vec![msg(MAX_AUDIO_CLIPS)];
        assert!(r.has_audio() && r.check().is_ok());
        assert_eq!(r.messages[0].text_content(), "");
        assert_eq!(r.messages[0].images().count(), 0);
        r.messages = vec![msg(MAX_AUDIO_CLIPS + 1)];
        assert!(r.check().unwrap_err().to_string().contains("too many audio clips"));
    }

    #[test]
    fn images_are_inline_only() {
        let i = Image::from_data_url("data:image/png;base64,iVBORw0KGgo=").unwrap();
        assert_eq!(i.mime, ImageMime::Png);
        assert_eq!(i.data_url(), "data:image/png;base64,iVBORw0KGgo=");
        let e = Image::from_data_url("https://example.com/cat.png").unwrap_err().to_string();
        assert!(e.contains("never fetches"), "{e}");
        assert!(Image::from_data_url("data:image/png;base64").is_err());
        assert!(Image::from_data_url("data:image/tiff;base64,AAAA").is_err());
        assert!(Image::from_data_url("data:image/png,AAAA").is_err());
        assert!(Image::from_data_url("data:image/png;base64,***").is_err());
        assert!(Image::from_base64(ImageMime::Png, " ").is_err());
        for (m, s) in [
            ("image/png", ImageMime::Png),
            ("IMAGE/JPG", ImageMime::Jpeg),
            ("image/jpeg", ImageMime::Jpeg),
            ("image/webp", ImageMime::Webp),
            ("image/gif", ImageMime::Gif),
        ] {
            assert_eq!(ImageMime::parse(m), Some(s));
            assert_eq!(ImageMime::parse(s.as_str()), Some(s));
        }
        assert_eq!(ImageMime::sniff_base64("iVBORw0KGgoAAA"), Some(ImageMime::Png));
        assert_eq!(ImageMime::sniff_base64("/9j/4AAQ"), Some(ImageMime::Jpeg));
        assert_eq!(ImageMime::sniff_base64("UklGRxxx"), Some(ImageMime::Webp));
        assert_eq!(ImageMime::sniff_base64("R0lGODlh"), Some(ImageMime::Gif));
        assert_eq!(ImageMime::sniff_base64("AAAA"), None);
    }

    #[test]
    fn messages_expose_text_and_images() {
        let img = Image::from_base64(ImageMime::Png, "iVBORw0KGgo=").unwrap();
        let m = Message {
            role: Role::User,
            parts: vec![Part::Text("a".into()), Part::Image(img.clone()), Part::Text("b".into())],
            tool_calls: vec![],
            tool_call_id: None,
        };
        assert_eq!(m.text_content(), "ab");
        assert_eq!(m.images().count(), 1);
        let r = ChatRequest {
            messages: vec![m],
            ..Default::default()
        };
        assert!(r.has_images());
        assert!(!req().has_images());
    }

    #[test]
    fn limits_are_checked() {
        assert!(req().check().is_ok());
        let cases: Vec<(ChatRequest, &str)> = vec![
            (ChatRequest::default(), "no messages"),
            (
                ChatRequest {
                    messages: vec![Message::text(Role::User, "x"); MAX_MESSAGES + 1],
                    ..Default::default()
                },
                "too many messages",
            ),
            (
                ChatRequest {
                    temperature: Some(3.0),
                    ..req()
                },
                "temperature",
            ),
            (ChatRequest { top_p: Some(1.5), ..req() }, "top_p"),
            (
                ChatRequest {
                    stop: vec!["x".into(); MAX_STOPS + 1],
                    ..req()
                },
                "stop",
            ),
            (
                ChatRequest {
                    tools: vec![
                        Tool {
                            name: "t".into(),
                            description: String::new(),
                            parameters: Value::Null
                        };
                        MAX_TOOLS + 1
                    ],
                    ..req()
                },
                "too many tools",
            ),
            (
                ChatRequest {
                    tool_choice: ToolChoice::Named("x".into()),
                    ..req()
                },
                "not among",
            ),
        ];
        for (r, needle) in cases {
            let e = r.check().unwrap_err().to_string();
            assert!(e.contains(needle), "{e}");
        }
        let img = Image::from_base64(ImageMime::Png, "iVBORw0KGgo=").unwrap();
        let many = Message {
            role: Role::User,
            parts: vec![Part::Image(img); MAX_IMAGES + 1],
            tool_calls: vec![],
            tool_call_id: None,
        };
        assert!(ChatRequest {
            messages: vec![many],
            ..Default::default()
        }
        .check()
        .unwrap_err()
        .to_string()
        .contains("images"));
        let named = ChatRequest {
            tools: vec![Tool {
                name: "t".into(),
                description: String::new(),
                parameters: Value::Null,
            }],
            tool_choice: ToolChoice::Named("t".into()),
            ..req()
        };
        assert!(named.check().is_ok());
    }

    #[test]
    fn the_collector_joins_streams() {
        let events = vec![
            ChatEvent::Reasoning("hm".into()),
            ChatEvent::Delta("Hel".into()),
            ChatEvent::Delta("lo".into()),
            ChatEvent::ToolCall {
                index: 1,
                id: Some("b".into()),
                name: Some("two".into()),
                arguments: "{}".into(),
            },
            ChatEvent::ToolCall {
                index: 0,
                id: Some(String::new()),
                name: Some("one".into()),
                arguments: "{\"a\":".into(),
            },
            ChatEvent::ToolCall {
                index: 0,
                id: None,
                name: None,
                arguments: "1}".into(),
            },
            ChatEvent::ToolCall {
                index: 9999,
                id: None,
                name: None,
                arguments: "x".into(),
            },
            ChatEvent::Done {
                finish: Finish::Stop,
                usage: Usage {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                },
                timings: None,
            },
        ];
        let o = ChatOutput::collect(&events);
        assert_eq!(o.text, "Hello");
        assert_eq!(o.reasoning, "hm");
        assert_eq!(o.tool_calls.len(), 2);
        assert_eq!(
            o.tool_calls[0],
            ToolCall {
                id: "call_0".into(),
                name: "one".into(),
                arguments: "{\"a\":1}".into()
            }
        );
        assert_eq!(o.tool_calls[1].id, "b");
        assert_eq!(o.finish, Some(Finish::ToolCalls));
        assert_eq!(o.usage.completion_tokens, 2);
        let plain = ChatOutput::collect(&[ChatEvent::Done {
            finish: Finish::Length,
            usage: Usage::default(),
            timings: None,
        }]);
        assert_eq!(plain.finish, Some(Finish::Length));
    }

    #[test]
    fn embed_limits() {
        assert!(EmbedRequest::default().check().is_err());
        assert!(EmbedRequest {
            input: vec!["a".into()],
            ..Default::default()
        }
        .check()
        .is_ok());
        assert!(EmbedRequest {
            input: vec![String::new(); MAX_EMBED_INPUTS + 1],
            ..Default::default()
        }
        .check()
        .is_err());
    }
}
