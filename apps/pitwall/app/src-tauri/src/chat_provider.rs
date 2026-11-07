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
//! [`SessionChats`] keeps a conversation for the session only: its store lives in a folder of its own, under a parent the host
//! names, that is removed when the value is dropped. The saved store, attachments and "Don't save this chat" are WP-3b.

use crate::server_client::{FrameStream, ServerClient};
use paddock_assistant::protocol::{DEFAULT_MAX_TOKENS, DEFAULT_TEMPERATURE};
use paddock_assistant::{
    Cancel, ChatMessage, ChatRequest, ChatResponse, ConversationStore, LlmProvider, ProviderError, ProviderPhase, Role, StopReason,
    ToolCall, Usage,
};
use pitwall_core::chat::Role as WireRole;
use pitwall_core::mgmt::{
    PlaygroundEvent, PlaygroundFinish, PlaygroundMessage, PlaygroundPhase, PlaygroundRequest, PlaygroundTool, PlaygroundToolCall,
    PlaygroundToolChoice,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How soon a set Stop flag is noticed while the stream is quiet.
pub const CANCEL_POLL: Duration = Duration::from_millis(50);
/// The head of the stream must arrive within this; the server answers at once and labels a cold load as a phase (WP-1), so
/// this bounds only a server that is not there, never a running job (ER-19).
const OPEN_TIMEOUT: Duration = Duration::from_secs(30);

/// The Playground chat as an [`LlmProvider`].
pub struct ServerChatProvider {
    client: Arc<dyn ServerClient>,
    runtime: tokio::runtime::Handle,
    model: Mutex<String>,
    context: AtomicUsize,
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
        }
    }

    /// Changes the model for the next turn.
    pub fn set_model(&self, model: impl Into<String>) {
        *self.model.lock().unwrap_or_else(PoisonError::into_inner) = model.into();
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
pub fn playground_request(model: &str, req: &ChatRequest) -> Result<PlaygroundRequest, ProviderError> {
    if req.json_schema.is_some() {
        return Err(ProviderError::Failed(
            "The Playground chat cannot constrain an answer to a JSON schema. Ask without one.".into(),
        ));
    }
    let mut messages = Vec::with_capacity(req.messages.len() + 1);
    if !req.system.is_empty() {
        messages.push(PlaygroundMessage::text(WireRole::System, req.system.clone()));
    }
    messages.extend(req.messages.iter().map(wire_message));
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

fn wire_message(m: &ChatMessage) -> PlaygroundMessage {
    PlaygroundMessage {
        role: match m.role {
            Role::User => WireRole::User,
            Role::Assistant => WireRole::Assistant,
            Role::Tool => WireRole::Tool,
        },
        content: m.content.clone(),
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
        let body = playground_request(model, req)?;
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

/// A conversation store for the session only: its file lives in a folder of its own, removed when this is dropped.
pub struct SessionChats {
    dir: PathBuf,
    store: Arc<ConversationStore>,
}

impl SessionChats {
    /// A new, empty store in a folder of its own under `parent` (the app's cache folder from `paddock-env`; this crate never
    /// reaches for the shared temporary folder).
    pub fn new(parent: &std::path::Path) -> std::io::Result<SessionChats> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = parent.join(format!(
            "chat-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir)?;
        let store = Arc::new(ConversationStore::open(
            dir.join("chats.json"),
            Arc::new(paddock_health::SystemClock),
        ));
        Ok(SessionChats { dir, store })
    }

    /// The store the assistant is built over.
    pub fn store(&self) -> Arc<ConversationStore> {
        self.store.clone()
    }
}

impl Drop for SessionChats {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server_client::fake::FakeServer;
    use crate::server_client::BoxFuture;
    use paddock_assistant::{StoredMessage, ToolSpec};
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
        let w = playground_request("m", &req).unwrap();
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
        let w = playground_request("m", &req).unwrap();
        assert_eq!((w.messages.len(), w.temperature, w.max_tokens), (3, DEFAULT_TEMPERATURE, 99));
        req.json_schema = Some(json!({}));
        assert!(playground_request("m", &req).unwrap_err().to_string().contains("JSON schema"));
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

    #[test]
    fn a_session_chat_store_lives_in_its_own_folder_and_leaves_nothing_behind() {
        let parent = tempfile::tempdir().unwrap();
        let chats = SessionChats::new(parent.path()).unwrap();
        let store = chats.store();
        let (id, _, saved) = store.create(StoredMessage::user("hello", vec![], 1));
        saved.unwrap();
        assert!(store.get(&id).is_some());
        let dir = chats.dir.clone();
        assert!(dir.join("chats.json").exists());
        drop(store);
        drop(chats);
        assert!(!dir.exists(), "the session's folder is removed");
        assert_ne!(
            SessionChats::new(parent.path()).unwrap().dir,
            SessionChats::new(parent.path()).unwrap().dir
        );
    }
}
