//! Pitwall's own chat as the assistant's model (ADR 0023 WP-3a): an [`LlmProvider`] over the management API's
//! `POST /mgmt/v1/chat` ([`ServerClient`]), not over `paddock_pitwall::Link`, because the Playground is the server's own window
//! and needs no pairing. Tools go to the engine natively (the stream's `toolCall` pieces become [`ToolCall::native`], parsed
//! strictly), so the assistant's loop, approvals and budgets run on top unchanged.
//!
//! The provider blocks (the assistant calls it on a worker thread) and bridges to the async client with the handle of a
//! multi-thread runtime (the app's own; a current-thread runtime is not driven by `Handle::block_on`).
//! Stop is honoured within [`CANCEL_POLL`], also while the model is still loading or the server is silent; the connection is
//! dropped then, which makes the server cancel the work.
//!
//! The chat's conversations are kept by `chat_host::chats` (WP-3b); this module only talks to the server. A text file the user
//! attached (WP-4b) is read by its SHA-256 through the reader given with [`ServerChatProvider::with_attachments`] and sent
//! after the question's text, fenced and labelled as data (`chat_host::attach::with_files`). A picture (WP-5) goes as an image
//! of its message when the chosen model reads pictures, newest first within the request's limits; every picture that is not
//! sent says why in the message's text, so the model and the user both know.

use crate::chat_host::attach::{with_files, KeptFile};
use crate::chat_host::models::ChatImages;
use crate::chat_host::pictures::MAX_PREPARED_BYTES;
use crate::server_client::{FrameStream, ServerClient};
use base64::Engine;
use paddock_assistant::protocol::{DEFAULT_MAX_TOKENS, DEFAULT_TEMPERATURE};
use paddock_assistant::{
    Cancel, ChatMessage, ChatRequest, ChatResponse, LlmProvider, ProviderError, ProviderPhase, Role, StopReason, ToolCall, Usage,
};
use pitwall_core::chat::{Image, ImageMime, Role as WireRole, MAX_IMAGES};
use pitwall_core::mgmt::{
    PlaygroundEvent, PlaygroundFinish, PlaygroundMessage, PlaygroundPhase, PlaygroundRequest, PlaygroundTool, PlaygroundToolCall,
    PlaygroundToolChoice,
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How soon a set Stop flag is noticed while the stream is quiet.
pub const CANCEL_POLL: Duration = Duration::from_millis(50);
/// The head of the stream must arrive within this; the server answers at once and labels a cold load as a phase (WP-1), so
/// this bounds only a server that is not there, never a running job (ER-19).
const OPEN_TIMEOUT: Duration = Duration::from_secs(30);

/// Most bytes of pictures (as base64) one request carries; the server refuses a request over 32 MiB, so older pictures give way.
pub const MAX_WIRE_IMAGE_BYTES: usize = 24 * 1024 * 1024;

/// Reads an attachment by its SHA-256, within the size of the largest picture the chat keeps ([`MAX_PREPARED_BYTES`]).
pub type AttachmentReader = Arc<dyn Fn(&str) -> KeptFile + Send + Sync>;

/// The Playground chat as an [`LlmProvider`].
pub struct ServerChatProvider {
    client: Arc<dyn ServerClient>,
    runtime: tokio::runtime::Handle,
    model: Mutex<String>,
    context: AtomicUsize,
    attachments: AttachmentReader,
    images: Mutex<ChatImages>,
}

impl ServerChatProvider {
    /// A provider that asks `model` (an id or alias the server resolves); `runtime` drives the async client from the
    /// assistant's worker thread.
    pub fn new(client: Arc<dyn ServerClient>, runtime: tokio::runtime::Handle, model: impl Into<String>) -> ServerChatProvider {
        ServerChatProvider {
            client,
            runtime,
            model: Mutex::new(model.into()),
            context: AtomicUsize::new(0),
            attachments: Arc::new(|_| KeptFile::Gone),
            images: Mutex::new(ChatImages::default()),
        }
    }

    /// Reads the files a question carries through `read` (the chats' store); without it a file is said to be gone.
    #[must_use]
    pub fn with_attachments(mut self, read: AttachmentReader) -> ServerChatProvider {
        self.attachments = read;
        self
    }

    /// Changes the model for the next turn.
    pub fn set_model(&self, model: impl Into<String>) {
        *self.model.lock().unwrap_or_else(PoisonError::into_inner) = model.into();
    }

    /// Tells the provider what the chosen model means for pictures (the picker decides; see `chat_host::models::chat_images`).
    pub fn set_images(&self, images: ChatImages) {
        *self.images.lock().unwrap_or_else(PoisonError::into_inner) = images;
    }

    /// What the chosen model means for pictures.
    pub fn images(&self) -> ChatImages {
        self.images.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Tells the provider how many tokens the model holds (0: unknown).
    pub fn set_context_length(&self, tokens: usize) {
        self.context.store(tokens, Ordering::Relaxed);
    }

    pub(crate) fn model(&self) -> String {
        self.model.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// The wire request for `req`: the system prompt first, tool calls and results as the wire carries them, the tools offered
/// natively. Structured JSON answers are not part of the Playground chat.
/// A user message's attached files are read through `read` and sent after its text; its pictures go as images when
/// `reads_images` (the chosen model has a vision projector), within [`MAX_IMAGES`] and [`MAX_WIRE_IMAGE_BYTES`], newest first.
pub fn playground_request(
    model: &str,
    req: &ChatRequest,
    read: &dyn Fn(&str) -> KeptFile,
    reads_images: bool,
) -> Result<PlaygroundRequest, ProviderError> {
    if req.json_schema.is_some() {
        return Err(ProviderError::Failed(
            "The Playground chat cannot constrain an answer to a JSON schema. Ask without one.".into(),
        ));
    }
    let mut messages = Vec::with_capacity(req.messages.len() + 1);
    if !req.system.is_empty() {
        messages.push(PlaygroundMessage::text(WireRole::System, req.system.clone()));
    }
    let mut pictures = pictures_of(&req.messages, read, reads_images);
    messages.extend(req.messages.iter().zip(pictures.drain(..)).map(|(m, p)| wire_message(m, read, p)));
    let max_tokens = if req.max_tokens == 0 { DEFAULT_MAX_TOKENS } else { req.max_tokens };
    Ok(PlaygroundRequest {
        model_id: model.to_string(),
        messages,
        temperature: req.temperature.unwrap_or(DEFAULT_TEMPERATURE),
        max_tokens: u32::try_from(max_tokens).unwrap_or(u32::MAX),
        tools: req
            .tools
            .iter()
            .map(|t| PlaygroundTool {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.input_schema.clone(),
            })
            .collect(),
        tool_choice: PlaygroundToolChoice::Auto,
    })
}

/// What became of one picture of a question.
enum Picture {
    Sent(Image),
    /// Not sent, with the kind of reason (its words are [`Pictures::notes`]).
    Left(Why),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Why {
    CannotRead,
    Gone,
    TooLarge,
    Older,
}

/// The pictures of one message, in order, and the pictures it carries that were left out.
#[derive(Default)]
struct Pictures {
    sent: Vec<Image>,
    left: Vec<Why>,
}

impl Pictures {
    /// The sentences that say which pictures were not sent and why (one per reason, with its count).
    fn notes(&self) -> Vec<String> {
        [Why::CannotRead, Why::Gone, Why::TooLarge, Why::Older]
            .into_iter()
            .filter_map(|why| {
                let n = self.left.iter().filter(|w| **w == why).count();
                let (one, many) = match why {
                    Why::CannotRead => (
                        "An image is not sent: the chosen model can't read images.",
                        "{n} images are not sent: the chosen model can't read images.",
                    ),
                    Why::Gone => (
                        "An attached image is no longer available on this computer.",
                        "{n} attached images are no longer available on this computer.",
                    ),
                    Why::TooLarge => (
                        "An attached image is too large to send.",
                        "{n} attached images are too large to send.",
                    ),
                    Why::Older => (
                        "An older image is not sent: a request carries at most 16 images and 24 MiB of them.",
                        "{n} older images are not sent: a request carries at most 16 images and 24 MiB of them.",
                    ),
                };
                match n {
                    0 => None,
                    1 => Some(one.to_string()),
                    _ => Some(many.replace("{n}", &n.to_string())),
                }
            })
            .collect()
    }
}

/// Reads the pictures every message carries, newest first, so the request keeps the latest ones within its limits and the older
/// ones give way. Without a model that reads pictures none is read.
fn pictures_of(messages: &[ChatMessage], read: &dyn Fn(&str) -> KeptFile, reads_images: bool) -> Vec<Pictures> {
    let mut out: Vec<Pictures> = messages.iter().map(|_| Pictures::default()).collect();
    let (mut count, mut bytes) = (0usize, 0usize);
    for (i, m) in messages.iter().enumerate().rev() {
        if m.role != Role::User {
            continue;
        }
        let mut slots: Vec<Picture> = Vec::new();
        for a in m.attachments.iter().rev() {
            let paddock_assistant::Attachment::Image { sha256, mime, .. } = a else {
                continue;
            };
            if !reads_images {
                slots.push(Picture::Left(Why::CannotRead));
            } else if count >= MAX_IMAGES {
                slots.push(Picture::Left(Why::Older));
            } else {
                match (read(sha256), ImageMime::parse(mime)) {
                    (KeptFile::Bytes(b), Some(mime)) if b.len() <= MAX_PREPARED_BYTES => {
                        let data = base64::engine::general_purpose::STANDARD.encode(&b);
                        if bytes + data.len() > MAX_WIRE_IMAGE_BYTES {
                            slots.push(Picture::Left(Why::Older));
                        } else {
                            bytes += data.len();
                            count += 1;
                            slots.push(Picture::Sent(Image { mime, data }));
                        }
                    }
                    (KeptFile::Bytes(_), _) | (KeptFile::TooLarge, _) => slots.push(Picture::Left(Why::TooLarge)),
                    (KeptFile::Gone, _) => slots.push(Picture::Left(Why::Gone)),
                }
            }
        }
        // Collected newest first: put them back in the order the user attached them.
        for slot in slots.into_iter().rev() {
            match slot {
                Picture::Sent(img) => out[i].sent.push(img),
                Picture::Left(why) => out[i].left.push(why),
            }
        }
    }
    out
}

fn wire_message(m: &ChatMessage, read: &dyn Fn(&str) -> KeptFile, pictures: Pictures) -> PlaygroundMessage {
    let mut content = if m.role == Role::User && !m.attachments.is_empty() {
        with_files(&m.content, &m.attachments, read)
    } else {
        m.content.clone()
    };
    for note in pictures.notes() {
        content.push_str("\n\n[");
        content.push_str(&note);
        content.push(']');
    }
    PlaygroundMessage {
        role: match m.role {
            Role::User => WireRole::User,
            Role::Assistant => WireRole::Assistant,
            Role::Tool => WireRole::Tool,
        },
        content,
        tool_calls: m
            .tool_calls
            .iter()
            .map(|c| PlaygroundToolCall {
                id: c.id.clone(),
                name: c.name.clone(),
                arguments: c.input.to_string(),
            })
            .collect(),
        tool_call_id: m.tool_call_id.clone(),
        images: pictures.sent,
    }
}

/// One tool call being assembled from its pieces: id, name, arguments text.
type Piece = (Option<String>, Option<String>, String);

/// Folds the stream's events into the answer.
#[derive(Default)]
struct Answer {
    text: String,
    calls: BTreeMap<u32, Piece>,
}

enum Step {
    More,
    Finished(Result<ChatResponse, ProviderError>),
}

impl Answer {
    fn fold(&mut self, event: PlaygroundEvent, on_delta: &mut dyn FnMut(&str), on_phase: &mut dyn FnMut(ProviderPhase)) -> Step {
        match event {
            // Loading and generating are the Chat tab's live label (ER-18).
            PlaygroundEvent::Phase { phase } => {
                on_phase(match phase {
                    PlaygroundPhase::Loading { elapsed_ms } => ProviderPhase::Loading { elapsed_ms },
                    PlaygroundPhase::Generating => ProviderPhase::Generating,
                });
                Step::More
            }
            // Reasoning is not part of the visible answer.
            PlaygroundEvent::Reasoning { .. } => Step::More,
            PlaygroundEvent::Delta { text } => {
                on_delta(&text);
                self.text.push_str(&text);
                Step::More
            }
            PlaygroundEvent::ToolCall {
                index,
                id,
                name,
                arguments,
            } => {
                let piece = self.calls.entry(index).or_default();
                piece.0 = piece.0.take().or(id);
                piece.1 = piece.1.take().or(name);
                piece.2.push_str(&arguments);
                Step::More
            }
            PlaygroundEvent::Done {
                finish,
                prompt_tokens,
                completion_tokens,
                ..
            } => {
                let tool_calls = std::mem::take(&mut self.calls)
                    .into_values()
                    .map(|(id, name, args)| ToolCall::native(id.unwrap_or_default(), name.unwrap_or_default(), &args))
                    .collect();
                Step::Finished(Ok(ChatResponse {
                    text: std::mem::take(&mut self.text),
                    tool_calls,
                    stop_reason: match finish {
                        PlaygroundFinish::Stop => StopReason::EndTurn,
                        PlaygroundFinish::Length => StopReason::MaxTokens,
                        PlaygroundFinish::ToolCalls => StopReason::ToolUse,
                        PlaygroundFinish::Cancelled => StopReason::Cancelled,
                    },
                    usage: Some(Usage {
                        input_tokens: u64::from(prompt_tokens),
                        output_tokens: u64::from(completion_tokens),
                    }),
                }))
            }
            PlaygroundEvent::Error { error } => Step::Finished(Err(ProviderError::Failed(error.message))),
        }
    }

    /// What Stop leaves: the text so far, or the cancellation when there is none.
    fn cancelled(self) -> Result<ChatResponse, ProviderError> {
        if self.text.is_empty() {
            return Err(ProviderError::Cancelled);
        }
        Ok(ChatResponse {
            text: self.text,
            tool_calls: vec![],
            stop_reason: StopReason::Cancelled,
            usage: None,
        })
    }
}

async fn stop_requested(cancel: &Cancel) {
    while !cancel.is_cancelled() {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}

impl LlmProvider for ServerChatProvider {
    fn id(&self) -> &str {
        "pitwall-server"
    }

    fn display_name(&self) -> String {
        format!("Pitwall ({})", self.model())
    }

    fn context_length(&self) -> usize {
        self.context.load(Ordering::Relaxed)
    }

    fn chat(&self, req: &ChatRequest, on_delta: &mut dyn FnMut(&str)) -> Result<ChatResponse, ProviderError> {
        self.chat_phased(req, on_delta, &mut |_| {})
    }

    fn chat_phased(
        &self,
        req: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
        on_phase: &mut dyn FnMut(ProviderPhase),
    ) -> Result<ChatResponse, ProviderError> {
        let model = self.model();
        let started = std::time::Instant::now();
        log::info!(target: "pitwall_chat", "model call started: model {model}, {} messages, {} tools", req.messages.len(), req.tools.len());
        let result = self.run(&model, req, on_delta, on_phase);
        let ms = started.elapsed().as_millis();
        match &result {
            Ok(r) => log::info!(
                target: "pitwall_chat",
                "model call ended: model {model}, finish {:?}, {} tool calls, {} tokens in, {} out, {ms} ms",
                r.stop_reason,
                r.tool_calls.len(),
                r.usage.map_or(0, |u| u.input_tokens),
                r.usage.map_or(0, |u| u.output_tokens),
            ),
            Err(e) => log::warn!(target: "pitwall_chat", "model call failed: model {model}, {ms} ms, cause: {e}"),
        }
        result
    }
}

impl ServerChatProvider {
    /// One model call, without its log lines (see [`LlmProvider::chat`]).
    fn run(
        &self,
        model: &str,
        req: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
        on_phase: &mut dyn FnMut(ProviderPhase),
    ) -> Result<ChatResponse, ProviderError> {
        let reads_images = self.images().allowed;
        let body = playground_request(model, req, self.attachments.as_ref(), reads_images)?;
        let body = serde_json::to_value(&body).map_err(|e| ProviderError::Failed(format!("The app built an unusable request ({e}).")))?;
        let cancel = req.cancel.clone();
        self.runtime.block_on(async {
            let mut stream: Box<dyn FrameStream> = tokio::select! {
                s = self.client.open_stream("/mgmt/v1/chat", Some(body), OPEN_TIMEOUT) => s.map_err(|e| ProviderError::Failed(e.message))?,
                () = stop_requested(&cancel) => return Err(ProviderError::Cancelled),
            };
            let mut answer = Answer::default();
            loop {
                if cancel.is_cancelled() {
                    return answer.cancelled();
                }
                let frame = tokio::select! {
                    f = stream.next_frame() => f,
                    () = stop_requested(&cancel) => return answer.cancelled(),
                };
                let Some(frame) = frame.map_err(|e| ProviderError::Failed(e.message))? else {
                    return Err(ProviderError::Failed(
                        "Pitwall's server ended the answer early. Retry, or restart the service.".into(),
                    ));
                };
                let event: PlaygroundEvent = serde_json::from_str(&frame).map_err(|_| {
                    ProviderError::Failed(
                        "Pitwall's server sent an answer this app does not understand. Update both to the same release.".into(),
                    )
                })?;
                if let Step::Finished(done) = answer.fold(event, on_delta, on_phase) {
                    return done;
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server_client::fake::FakeServer;
    use crate::server_client::BoxFuture;
    use paddock_assistant::ToolSpec;
    use pitwall_core::mgmt::{CommandError, ErrorCode, PlaygroundPhase};
    use serde_json::{json, Value};

    fn runtime() -> tokio::runtime::Runtime {
        // A multi-thread runtime, as the app's: a current-thread one is not driven by `Handle::block_on`, so its timers never fire.
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
    }

    fn frame(e: &PlaygroundEvent) -> String {
        serde_json::to_string(e).unwrap()
    }

    fn done(finish: PlaygroundFinish) -> PlaygroundEvent {
        PlaygroundEvent::Done {
            finish,
            prompt_tokens: 7,
            completion_tokens: 3,
            ttft_ms: 1,
            gen_tps: 2.0,
            cold_start_ms: None,
        }
    }

    fn request() -> ChatRequest {
        ChatRequest {
            system: "Be brief.".into(),
            messages: vec![ChatMessage::user("hi")],
            ..Default::default()
        }
    }

    fn provider(server: Arc<FakeServer>, rt: &tokio::runtime::Runtime) -> ServerChatProvider {
        ServerChatProvider::new(server, rt.handle().clone(), "tiny")
    }

    #[test]
    fn text_streams_through_and_the_usage_and_stop_reason_come_back() {
        let rt = runtime();
        let server = Arc::new(FakeServer::new());
        server.stream(Ok(vec![
            frame(&PlaygroundEvent::Phase {
                phase: PlaygroundPhase::Loading { elapsed_ms: 0 },
            }),
            frame(&PlaygroundEvent::Phase {
                phase: PlaygroundPhase::Generating,
            }),
            frame(&PlaygroundEvent::Reasoning { text: "hmm".into() }),
            frame(&PlaygroundEvent::Delta { text: "Hel".into() }),
            frame(&PlaygroundEvent::Delta { text: "lo".into() }),
            frame(&done(PlaygroundFinish::Stop)),
        ]));
        let p = provider(server, &rt);
        let mut seen = String::new();
        let out = p.chat(&request(), &mut |d| seen.push_str(d)).unwrap();
        assert_eq!(
            (out.text.as_str(), seen.as_str(), out.stop_reason),
            ("Hello", "Hello", StopReason::EndTurn)
        );
        assert_eq!(
            out.usage,
            Some(Usage {
                input_tokens: 7,
                output_tokens: 3
            })
        );
        assert!(out.tool_calls.is_empty());
        assert_eq!(
            (p.id(), p.display_name().as_str(), p.context_length()),
            ("pitwall-server", "Pitwall (tiny)", 0)
        );
        p.set_model("other");
        p.set_context_length(4096);
        assert_eq!((p.display_name().as_str(), p.context_length()), ("Pitwall (other)", 4096));
    }

    /// The Chat tab's live label (ER-18): the server's load and generate phases reach the assistant as provider phases,
    /// before the first word.
    #[test]
    fn the_servers_phases_reach_the_chat_before_the_words() {
        let rt = runtime();
        let server = Arc::new(FakeServer::new());
        server.stream(Ok(vec![
            frame(&PlaygroundEvent::Phase {
                phase: PlaygroundPhase::Loading { elapsed_ms: 0 },
            }),
            frame(&PlaygroundEvent::Phase {
                phase: PlaygroundPhase::Loading { elapsed_ms: 1200 },
            }),
            frame(&PlaygroundEvent::Phase {
                phase: PlaygroundPhase::Generating,
            }),
            frame(&PlaygroundEvent::Delta { text: "Hi".into() }),
            frame(&done(PlaygroundFinish::Stop)),
        ]));
        let p = provider(server, &rt);
        let seen = std::cell::RefCell::new(Vec::new());
        let out = p
            .chat_phased(&request(), &mut |d| seen.borrow_mut().push(format!("delta {d}")), &mut |ph| {
                seen.borrow_mut().push(format!("{ph:?}"))
            })
            .unwrap();
        assert_eq!(out.text, "Hi");
        assert_eq!(
            *seen.borrow(),
            [
                "Loading { elapsed_ms: 0 }",
                "Loading { elapsed_ms: 1200 }",
                "Generating",
                "delta Hi"
            ]
        );
    }

    #[test]
    fn tool_call_pieces_become_native_calls_and_a_bad_one_is_marked_not_dropped() {
        let rt = runtime();
        let server = Arc::new(FakeServer::new());
        let piece = |index, id: Option<&str>, name: Option<&str>, arguments: &str| {
            frame(&PlaygroundEvent::ToolCall {
                index,
                id: id.map(Into::into),
                name: name.map(Into::into),
                arguments: arguments.into(),
            })
        };
        server.stream(Ok(vec![
            piece(0, Some("c1"), Some("cpu_memory"), ""),
            piece(1, Some("c2"), Some("disks"), "{oops"),
            piece(0, None, None, "{\"top\":3}"),
            frame(&done(PlaygroundFinish::ToolCalls)),
        ]));
        let out = provider(server, &rt).chat(&request(), &mut |_| {}).unwrap();
        assert_eq!(out.stop_reason, StopReason::ToolUse);
        assert_eq!(out.tool_calls.len(), 2);
        assert_eq!(
            (
                out.tool_calls[0].id.as_str(),
                out.tool_calls[0].name.as_str(),
                &out.tool_calls[0].input
            ),
            ("c1", "cpu_memory", &json!({"top": 3}))
        );
        assert!(
            out.tool_calls[1].is_malformed(),
            "unparseable arguments are marked and never run: {:?}",
            out.tool_calls[1]
        );
    }

    /// ADR 0023 WP-4b: an attached text file reaches the model after the question, fenced and labelled as data; the
    /// conversation's own text stays as the user typed it.
    #[test]
    fn an_attached_file_is_sent_after_the_question_as_data() {
        let file = paddock_assistant::Attachment::File {
            sha256: "a".repeat(64),
            name: "notes.txt".into(),
        };
        let req = ChatRequest {
            messages: vec![ChatMessage::user("Summarise").with_attachments(vec![file])],
            ..Default::default()
        };
        let kept = "a".repeat(64);
        let read = move |sha: &str| {
            if sha == kept {
                KeptFile::Bytes(b"three lines\nof notes\n".to_vec())
            } else {
                KeptFile::Gone
            }
        };
        let w = playground_request("m", &req, &read, true).unwrap();
        let content = &w.messages[0].content;
        assert!(content.starts_with("Summarise\n\nAttached file \"notes.txt\""), "{content}");
        assert!(content.contains("```\nthree lines\nof notes\n\n```"), "{content}");
        assert_eq!(req.messages[0].content, "Summarise");
        let gone = playground_request("m", &req, &|_| KeptFile::Gone, true).unwrap();
        assert!(gone.messages[0].content.ends_with("is no longer available on this computer."));
    }

    fn picture(n: usize) -> paddock_assistant::Attachment {
        paddock_assistant::Attachment::Image {
            sha256: format!("{n:064x}"),
            mime: "image/png".into(),
            width: 4,
            height: 3,
        }
    }

    fn picture_bytes(sha: &str) -> KeptFile {
        // The picture's bytes are its own number, so a test can tell which one reached the wire.
        let n = usize::from_str_radix(sha, 16).unwrap_or(0);
        KeptFile::Bytes(format!("pic{n}").into_bytes())
    }

    fn data_of(n: usize) -> String {
        base64::engine::general_purpose::STANDARD.encode(format!("pic{n}"))
    }

    /// WP-5: a question's pictures go as images of its message, in the order attached, after the text; before, an image
    /// attachment was dropped on the floor.
    #[test]
    fn a_pictures_of_a_question_go_to_the_engine_as_images_of_its_message() {
        let req = ChatRequest {
            messages: vec![ChatMessage::user("what is this?").with_attachments(vec![picture(1), picture(2)])],
            ..Default::default()
        };
        let w = playground_request("m", &req, &picture_bytes, true).unwrap();
        let m = &w.messages[0];
        assert_eq!(m.content, "what is this?");
        let sent: Vec<(ImageMime, String)> = m.images.iter().map(|i| (i.mime, i.data.clone())).collect();
        assert_eq!(sent, [(ImageMime::Png, data_of(1)), (ImageMime::Png, data_of(2))]);
    }

    /// A model that cannot read pictures gets none, and the turn says so (the images stay in the conversation).
    #[test]
    fn a_model_without_vision_gets_no_pictures_and_the_turn_says_so() {
        let req = ChatRequest {
            messages: vec![
                ChatMessage::user("look").with_attachments(vec![picture(1)]),
                ChatMessage::assistant("a cat", vec![]),
                ChatMessage::user("and these").with_attachments(vec![picture(2), picture(3)]),
            ],
            ..Default::default()
        };
        let w = playground_request("m", &req, &picture_bytes, false).unwrap();
        assert!(w.messages.iter().all(|m| m.images.is_empty()));
        assert_eq!(
            w.messages[0].content,
            "look\n\n[An image is not sent: the chosen model can't read images.]"
        );
        assert_eq!(
            w.messages[2].content,
            "and these\n\n[2 images are not sent: the chosen model can't read images.]"
        );
    }

    /// The request carries at most 16 pictures, the newest ones; older ones give way with a note on their message.
    #[test]
    fn at_most_sixteen_pictures_go_and_the_older_ones_give_way() {
        let first: Vec<_> = (0..10).map(picture).collect();
        let second: Vec<_> = (10..20).map(picture).collect();
        let req = ChatRequest {
            messages: vec![
                ChatMessage::user("old").with_attachments(first),
                ChatMessage::assistant("ok", vec![]),
                ChatMessage::user("new").with_attachments(second),
            ],
            ..Default::default()
        };
        let w = playground_request("m", &req, &picture_bytes, true).unwrap();
        assert_eq!(w.messages[2].images.len(), 10, "the newest question keeps all of its pictures");
        assert_eq!(w.messages[0].images.len(), 6, "the older one keeps the last six");
        assert_eq!(
            w.messages[0].images[0].data,
            data_of(4),
            "in the order attached, the first four gave way"
        );
        assert!(
            w.messages[0]
                .content
                .contains("4 older images are not sent: a request carries at most 16 images"),
            "{}",
            w.messages[0].content
        );
        assert!(w.messages.iter().map(|m| m.images.len()).sum::<usize>() <= MAX_IMAGES);
    }

    /// The request's bytes are bounded too: pictures past 24 MiB of base64 give way, newest kept.
    #[test]
    fn pictures_past_the_byte_budget_give_way() {
        let big = |_: &str| KeptFile::Bytes(vec![7u8; 9 * 1024 * 1024]);
        let req = ChatRequest {
            messages: vec![ChatMessage::user("x").with_attachments(vec![picture(1), picture(2), picture(3)])],
            ..Default::default()
        };
        let w = playground_request("m", &req, &big, true).unwrap();
        // 9 MiB is 12 MiB as base64: two fit in 24 MiB, the third (the first attached) gives way.
        assert_eq!(w.messages[0].images.len(), 2);
        assert!(
            w.messages[0].content.contains("An older image is not sent"),
            "{}",
            w.messages[0].content
        );
    }

    /// A kept picture that is gone or too large is said so, and the rest still go.
    #[test]
    fn a_picture_that_is_gone_or_too_large_is_said_so() {
        let read = |sha: &str| match usize::from_str_radix(sha, 16).unwrap_or(0) {
            1 => KeptFile::Gone,
            2 => KeptFile::TooLarge,
            n => picture_bytes(&format!("{n:064x}")),
        };
        let req = ChatRequest {
            messages: vec![ChatMessage::user("x").with_attachments(vec![picture(1), picture(2), picture(3)])],
            ..Default::default()
        };
        let w = playground_request("m", &req, &read, true).unwrap();
        assert_eq!(w.messages[0].images.len(), 1);
        let c = &w.messages[0].content;
        assert!(c.contains("An attached image is no longer available on this computer."), "{c}");
        assert!(c.contains("An attached image is too large to send."), "{c}");
    }

    #[test]
    fn the_wire_request_carries_the_system_prompt_tools_calls_and_results() {
        let mut req = ChatRequest {
            system: "S".into(),
            messages: vec![
                ChatMessage::user("how busy?"),
                ChatMessage::assistant(
                    "",
                    vec![ToolCall {
                        id: "c1".into(),
                        name: "cpu_memory".into(),
                        input: json!({"top": 3}),
                    }],
                ),
                ChatMessage::tool_result("c1", "cpu 12%"),
            ],
            tools: vec![ToolSpec {
                name: "cpu_memory".into(),
                description: "d".into(),
                input_schema: json!({"type": "object"}),
            }],
            temperature: Some(0.5),
            max_tokens: 0,
            ..Default::default()
        };
        let w = playground_request("m", &req, &|_| KeptFile::Gone, true).unwrap();
        assert_eq!(w.model_id, "m");
        assert_eq!((w.temperature, w.max_tokens), (0.5, u32::try_from(DEFAULT_MAX_TOKENS).unwrap()));
        let roles: Vec<_> = w.messages.iter().map(|m| m.role).collect();
        assert_eq!(roles, [WireRole::System, WireRole::User, WireRole::Assistant, WireRole::Tool]);
        assert_eq!(w.messages[2].tool_calls[0].arguments, "{\"top\":3}");
        assert_eq!(w.messages[3].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(
            (w.tools[0].name.as_str(), &w.tools[0].parameters),
            ("cpu_memory", &json!({"type": "object"}))
        );
        assert_eq!(w.tool_choice, PlaygroundToolChoice::Auto);
        req.system.clear();
        req.temperature = None;
        req.max_tokens = 99;
        let w = playground_request("m", &req, &|_| KeptFile::Gone, true).unwrap();
        assert_eq!((w.messages.len(), w.temperature, w.max_tokens), (3, DEFAULT_TEMPERATURE, 99));
        req.json_schema = Some(json!({}));
        assert!(playground_request("m", &req, &|_| KeptFile::Gone, true)
            .unwrap_err()
            .to_string()
            .contains("JSON schema"));
    }

    #[test]
    fn failures_say_what_happened() {
        let rt = runtime();
        let cases: Vec<(Result<Vec<String>, CommandError>, &str)> = vec![
            (
                Err(CommandError::new(ErrorCode::ServerUnavailable, "not running; use Repair")),
                "use Repair",
            ),
            (
                Ok(vec![frame(&PlaygroundEvent::Error {
                    error: CommandError::new(ErrorCode::InsufficientMemory, "not enough memory"),
                })]),
                "not enough memory",
            ),
            (
                Ok(vec![frame(&PlaygroundEvent::Delta { text: "partial".into() })]),
                "ended the answer early",
            ),
            (Ok(vec!["not json".into()]), "does not understand"),
        ];
        for (script, want) in cases {
            let server = Arc::new(FakeServer::new());
            server.stream(script);
            let err = provider(server, &rt).chat(&request(), &mut |_| {}).unwrap_err();
            assert!(matches!(&err, ProviderError::Failed(m) if m.contains(want)), "{err:?} for {want}");
        }
    }

    /// A stream that gives its frames and then stays silent, as a model that is still thinking, with a hook on every frame.
    struct Quiet(std::collections::VecDeque<String>);

    impl FrameStream for Quiet {
        fn next_frame(&mut self) -> BoxFuture<'_, Result<Option<String>, CommandError>> {
            Box::pin(async move {
                match self.0.pop_front() {
                    Some(f) => Ok(Some(f)),
                    None => std::future::pending().await,
                }
            })
        }
    }

    struct QuietClient(Vec<String>);

    impl ServerClient for QuietClient {
        fn call<'a>(&'a self, _: &'a str, _: Value, _: Option<Duration>) -> BoxFuture<'a, Result<Value, CommandError>> {
            Box::pin(async { Ok(Value::Null) })
        }
        fn open_stream<'a>(
            &'a self,
            _: &'a str,
            _: Option<Value>,
            _: Duration,
        ) -> BoxFuture<'a, Result<Box<dyn FrameStream>, CommandError>> {
            let frames = self.0.clone().into();
            Box::pin(async move { Ok(Box::new(Quiet(frames)) as Box<dyn FrameStream>) })
        }
    }

    #[test]
    fn stop_ends_a_silent_stream_with_the_text_so_far_or_as_cancelled() {
        let rt = runtime();
        let p = ServerChatProvider::new(
            Arc::new(QuietClient(vec![frame(&PlaygroundEvent::Delta { text: "part".into() })])),
            rt.handle().clone(),
            "m",
        );
        let req = request();
        let cancel = req.cancel.clone();
        // Stop is pressed while the server is silent, from the first delta (a condition, not a timer).
        let out = p.chat(&req, &mut |_| cancel.cancel()).unwrap();
        assert_eq!((out.text.as_str(), out.stop_reason), ("part", StopReason::Cancelled));

        let silent = ServerChatProvider::new(Arc::new(QuietClient(vec![])), rt.handle().clone(), "m");
        let req = request();
        let cancel = req.cancel.clone();
        let stopper = std::thread::spawn(move || cancel.cancel());
        assert_eq!(silent.chat(&req, &mut |_| {}).unwrap_err(), ProviderError::Cancelled);
        stopper.join().unwrap();
        // Already stopped before the call: nothing is asked.
        let req = request();
        req.cancel.cancel();
        assert_eq!(silent.chat(&req, &mut |_| {}).unwrap_err(), ProviderError::Cancelled);
    }

    struct Opening;

    impl ServerClient for Opening {
        fn call<'a>(&'a self, _: &'a str, _: Value, _: Option<Duration>) -> BoxFuture<'a, Result<Value, CommandError>> {
            Box::pin(async { Ok(Value::Null) })
        }
        fn open_stream<'a>(
            &'a self,
            _: &'a str,
            _: Option<Value>,
            _: Duration,
        ) -> BoxFuture<'a, Result<Box<dyn FrameStream>, CommandError>> {
            Box::pin(std::future::pending())
        }
    }

    #[test]
    fn stop_also_ends_the_wait_for_the_stream_to_open() {
        let rt = runtime();
        let p = ServerChatProvider::new(Arc::new(Opening), rt.handle().clone(), "m");
        let req = request();
        let cancel = req.cancel.clone();
        let stopper = std::thread::spawn(move || cancel.cancel());
        assert_eq!(p.chat(&req, &mut |_| {}).unwrap_err(), ProviderError::Cancelled);
        stopper.join().unwrap();
    }

    static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    struct Capture;

    impl log::Log for Capture {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, r: &log::Record<'_>) {
            if r.target() == "pitwall_chat" {
                LOGGED.lock().unwrap_or_else(PoisonError::into_inner).push(r.args().to_string());
            }
        }
        fn flush(&self) {}
    }

    /// The `pitwall_chat` lines that mention `model` (each test uses a model name of its own).
    fn logged(model: &str) -> Vec<String> {
        LOGGED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|l| l.contains(model))
            .cloned()
            .collect()
    }

    #[test]
    fn every_call_logs_its_start_and_how_it_ended_without_content() {
        let _ = log::set_logger(&Capture);
        log::set_max_level(log::LevelFilter::Info);
        let rt = runtime();

        let ok = Arc::new(FakeServer::new());
        ok.stream(Ok(vec![
            frame(&PlaygroundEvent::Delta {
                text: "secret-answer-text".into(),
            }),
            frame(&done(PlaygroundFinish::Stop)),
        ]));
        let p = ServerChatProvider::new(ok, rt.handle().clone(), "log-model-ok");
        let mut req = request();
        req.messages = vec![ChatMessage::user("secret-question-text")];
        p.chat(&req, &mut |_| {}).unwrap();
        let lines = logged("log-model-ok");
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].contains("started") && lines[0].contains("1 messages"), "{lines:?}");
        assert!(
            lines[1].contains("ended")
                && lines[1].contains("finish EndTurn")
                && lines[1].contains("7 tokens in, 3 out")
                && lines[1].contains(" ms"),
            "{lines:?}"
        );

        let bad = Arc::new(FakeServer::new());
        bad.stream(Err(CommandError::new(ErrorCode::ServerUnavailable, "not running; use Repair")));
        let p = ServerChatProvider::new(bad, rt.handle().clone(), "log-model-bad");
        p.chat(&req, &mut |_| {}).unwrap_err();
        let lines = logged("log-model-bad");
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[1].contains("failed") && lines[1].contains("cause: not running; use Repair") && lines[1].contains(" ms"),
            "{lines:?}"
        );

        let all = LOGGED.lock().unwrap_or_else(PoisonError::into_inner).join("\n");
        assert!(!all.contains("secret-"), "no content in the log: {all}");
    }
}
