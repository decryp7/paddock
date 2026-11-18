//! The Pitwall adapter against the in-process fake Pitwall of `paddock-pitwall` (loopback, an ephemeral port, no real
//! Pitwall, no credential store, no sleeping): the whole assistant over a real `Link`.

use crate::support;
use paddock_assistant::pitwall::needs_ai;
use paddock_assistant::*;
use paddock_health::FakeClock;
use paddock_pitwall::retry::Sleeper;
use paddock_pitwall::testing::{ChatScript, Failure, FakePitwall, FakeState, PairMode};
use paddock_pitwall::{
    AiState, AppIdentity, CancelToken, KeyVault, Link, LinkConfig, Locator, PitwallError,
};
use paddock_secrets::MemorySecrets;
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::*;

const KEY: &str = "pw_testkey";

struct NoSleep;

impl Sleeper for NoSleep {
    fn sleep(&self, _: Duration, _: &CancelToken) {}
}

struct World {
    fake: FakePitwall,
    link: Arc<Link>,
    assistant: Arc<Assistant>,
    rx: std::sync::mpsc::Receiver<Envelope>,
    sink: Arc<ChannelSink>,
    /// When the Pitwall provider's chat returned (the generation ended), apart from saving the turn.
    watched: Arc<Watched>,
    /// The store's saves, which a test can hold.
    saves: Arc<HeldSaves>,
    _dir: tempfile::TempDir,
}

/// The Pitwall provider, telling when its chat returned: the generation ended. The turn's end event comes later, once the
/// conversation is saved (an fsync whose time depends on the disk, not on Stop: CI run 8241, Windows).
struct Watched {
    inner: PitwallProvider,
    returned: Mutex<Option<std::sync::mpsc::Sender<()>>>,
}

impl LlmProvider for Watched {
    fn id(&self) -> &str {
        self.inner.id()
    }
    fn display_name(&self) -> String {
        self.inner.display_name()
    }
    fn context_length(&self) -> usize {
        self.inner.context_length()
    }
    fn chat(
        &self,
        req: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse, ProviderError> {
        let result = self.inner.chat(req, on_delta);
        if let Some(tx) = lock(&self.returned).as_ref() {
            let _ = tx.send(());
        }
        result
    }
}

impl Watched {
    /// From now on, each return of chat is sent here.
    fn returns(&self) -> std::sync::mpsc::Receiver<()> {
        let (tx, rx) = std::sync::mpsc::channel();
        *lock(&self.returned) = Some(tx);
        rx
    }
}

/// The file store, with saves of a turn (`append`) that wait while a test holds them. Open by default.
struct HeldSaves {
    inner: Arc<ConversationStore>,
    held: (Mutex<bool>, std::sync::Condvar),
}

impl HeldSaves {
    fn hold(&self) {
        *lock(&self.held.0) = true;
    }
    fn open(&self) {
        *lock(&self.held.0) = false;
        self.held.1.notify_all();
    }
}

impl Conversations for HeldSaves {
    fn summaries(&self) -> Vec<ConversationSummary> {
        Conversations::summaries(&*self.inner)
    }
    fn get(&self, id: &str) -> Option<Conversation> {
        Conversations::get(&*self.inner, id)
    }
    fn create(&self, first: StoredMessage) -> (String, String, Result<(), SaveError>) {
        Conversations::create(&*self.inner, first)
    }
    fn create_saved(
        &self,
        first: StoredMessage,
        save: bool,
    ) -> (String, String, Result<(), SaveError>) {
        Conversations::create_saved(&*self.inner, first, save)
    }
    fn append(&self, id: &str, messages: Vec<StoredMessage>) -> Option<Result<(), SaveError>> {
        let mut held = lock(&self.held.0);
        while *held {
            held = self.held.1.wait(held).unwrap_or_else(|p| p.into_inner());
        }
        drop(held);
        Conversations::append(&*self.inner, id, messages)
    }
    fn delete(&self, id: &str) -> Result<bool, SaveError> {
        Conversations::delete(&*self.inner, id)
    }
    fn history(&self, id: &str, history_chars: usize) -> Vec<ChatMessage> {
        Conversations::history(&*self.inner, id, history_chars)
    }
    fn take_problem(&self) -> Option<String> {
        Conversations::take_problem(&*self.inner)
    }
    fn admits(&self, conversation_id: Option<&str>, save: bool) -> Result<(), SaveError> {
        Conversations::admits(&*self.inner, conversation_id, save)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn world(state: FakeState, store_key: bool, api: u32, tools: Vec<Arc<dyn AssistantTool>>) -> World {
    let fake = FakePitwall::start(FakeState {
        keys: vec![KEY.into()],
        ..state
    });
    let dir = tempfile::tempdir().unwrap();
    let disco = dir.path().join("pitwall");
    fake.write_discovery(&disco, 42, api);
    let secrets = Arc::new(MemorySecrets::new("Demo"));
    if store_key {
        KeyVault::new(secrets.clone()).set(KEY).unwrap();
    }
    let mut cfg = LinkConfig::new(
        AppIdentity::new("demo", "Demo").unwrap(),
        Locator::in_dir(&disco, None, Arc::new(|p| p == 42)),
        secrets,
    );
    cfg.sleeper = Arc::new(NoSleep);
    let link = Link::new(cfg);
    link.probe();
    let clock = FakeClock::at_wall_ms(1_700_000_000_000);
    let store = Arc::new(ConversationStore::open(
        dir.path().join("assistant.json"),
        Arc::new(clock.clone()),
    ));
    let watched = Arc::new(Watched {
        inner: PitwallProvider::new(link.clone()),
        returned: Mutex::new(None),
    });
    let saves = Arc::new(HeldSaves {
        inner: store,
        held: (Mutex::new(false), std::sync::Condvar::new()),
    });
    let assistant = Assistant::with_conversations(
        watched.clone(),
        Tools::new(tools).unwrap(),
        saves.clone(),
        Arc::new(clock),
        AssistantConfig::new("Demo", "You help."),
    );
    let (sink, rx) = sink();
    World {
        fake,
        link,
        assistant,
        rx,
        sink,
        watched,
        saves,
        _dir: dir,
    }
}

impl World {
    fn turn(&self, text: &str) -> (String, Vec<AssistantEvent>) {
        let ack = self.assistant.send(ask(text), self.sink.clone()).unwrap();
        (ack.conversation_id, until_end(&self.rx))
    }
}

fn answer(pieces: &[&str]) -> ChatScript {
    ChatScript::Answer {
        pieces: pieces.iter().map(|s| s.to_string()).collect(),
        finish: "stop".into(),
    }
}

#[test]
fn a_paired_pitwall_streams_an_answer_through_the_assistant() {
    let w = world(FakeState::default(), true, 1, vec![]);
    let (id, events) = w.turn("Hello?");
    assert_eq!(done(&events).0, "Hello from Pitwall.");
    let deltas: Vec<&str> = events
        .iter()
        .filter_map(|e| {
            if let AssistantEvent::Delta { text } = e {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(deltas, ["Hello", " from", " Pitwall."]);
    // The link chose the model; the request carries the system prompt and the question.
    let (_, _, body) = w.fake.requests_to("/v1/chat/completions").remove(0);
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["model"], "auto:chat,8k");
    assert_eq!(
        body["messages"][0],
        json!({"role": "system", "content": "You help."})
    );
    assert_eq!(
        body["messages"][1],
        json!({"role": "user", "content": "Hello?"})
    );
    assert_eq!(w.assistant.load(&id).unwrap().messages.len(), 2);
    assert!(
        w.assistant
            .info()
            .provider
            .starts_with("Pitwall (qwen3-8b-q4km")
    );
    w.assistant.shutdown();
}

/// `Echo`, but it runs only once the test opens the gate: the engine asks Pitwall for the next turn right after the
/// tool returns, so the test must have scripted that answer first (otherwise the fake repeats the tool call).
struct GatedEcho {
    echo: Echo,
    gate: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}

impl AssistantTool for GatedEcho {
    fn spec(&self) -> ToolSpec {
        self.echo.spec()
    }
    fn budget(&self) -> ToolBudget {
        self.echo.budget()
    }
    fn call(&self, input: &serde_json::Value, cancel: &Cancel) -> ToolOutput {
        // A dropped sender (the test failed) also opens the gate.
        let _ = self.gate.lock().unwrap().recv();
        self.echo.call(input, cancel)
    }
}

#[test]
fn the_tool_protocol_runs_over_pitwalls_chat_api() {
    let (open_gate, gate) = std::sync::mpsc::channel();
    let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let w = world(
        FakeState {
            chat: answer(&[
                "Checking. <tool",
                "_call>{\"name\":\"echo\",\"arguments\":{\"word\":\"hi\"}}</tool_call>",
            ]),
            ..FakeState::default()
        },
        true,
        1,
        vec![Arc::new(GatedEcho {
            echo: Echo(runs.clone()),
            gate: std::sync::Mutex::new(gate),
        })],
    );
    let ack = w.assistant.send(ask("echo hi"), w.sink.clone()).unwrap();
    let first = until(&w.rx, |e| matches!(e, AssistantEvent::ToolStarted { .. }));
    // The protocol markup never reaches the text.
    let shown: String = first
        .iter()
        .filter_map(|e| {
            if let AssistantEvent::Delta { text } = e {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(shown, "Checking. ");
    w.fake.state().chat = answer(&["It said hi."]);
    open_gate.send(()).unwrap();
    let rest = until_end(&w.rx);
    assert_eq!(done(&rest).0, "It said hi.");
    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1);
    let second = w.fake.requests_to("/v1/chat/completions").remove(1).2;
    assert!(second.contains("tool_response"), "{second}");
    let first_body = w.fake.requests_to("/v1/chat/completions").remove(0).2;
    assert!(first_body.contains("<tools>"));
    assert_eq!(
        w.assistant
            .load(&ack.conversation_id)
            .unwrap()
            .messages
            .len(),
        4
    );
    w.assistant.shutdown();
}

fn needs(events: &[AssistantEvent]) -> AiState {
    match events.last().unwrap() {
        AssistantEvent::Error {
            kind: ErrorKind::NeedsAi,
            needs_ai: Some(state),
            ..
        } => state.clone(),
        other => panic!("not a needs-AI error: {other:?}"),
    }
}

#[test]
fn a_pairing_that_was_refused_is_the_needs_pairing_state_not_an_error() {
    let w = world(
        FakeState {
            pair: PairMode::Deny,
            ..FakeState::default()
        },
        false,
        1,
        vec![],
    );
    assert_eq!(needs(&w.turn("hi").1), AiState::NeedsPairing);
    // The key was revoked while the app ran: the same state, and the useless key is gone.
    let w = world(FakeState::default(), true, 1, vec![]);
    w.fake.state().keys.clear();
    assert_eq!(needs(&w.turn("hi").1), AiState::NeedsPairing);
    w.assistant.shutdown();
}

#[test]
fn a_pitwall_with_another_connection_version_is_incompatible() {
    let w = world(
        FakeState {
            api_version: 2,
            ..FakeState::default()
        },
        true,
        2,
        vec![],
    );
    match needs(&w.turn("hi").1) {
        AiState::Incompatible {
            message,
            update_app,
        } => {
            assert!(
                message.contains("newer connection version") || message.contains("Update"),
                "{message}"
            );
            assert!(update_app);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn pitwall_not_running_and_no_model_are_needs_ai_states() {
    let w = world(FakeState::default(), true, 1, vec![]);
    w.fake.kill();
    assert_eq!(needs(&w.turn("hi").1), AiState::NotRunning);
    let w = world(
        FakeState {
            models: vec![],
            ..FakeState::default()
        },
        true,
        1,
        vec![],
    );
    assert!(matches!(
        needs(&w.turn("hi").1),
        AiState::NoSuitableModel { .. }
    ));
}

#[test]
fn pitwall_dying_mid_stream_keeps_what_was_said_and_says_not_running() {
    let w = world(
        FakeState {
            chat: ChatScript::DieAfter(vec!["The answer begins".into()]),
            ..FakeState::default()
        },
        true,
        1,
        vec![],
    );
    let (id, events) = w.turn("hi");
    assert_eq!(needs(&events), AiState::NotRunning);
    let c = w.assistant.load(&id).unwrap();
    assert_eq!(
        (c.messages[1].text.as_str(), c.messages[1].end),
        ("The answer begins", Some(EndReason::Failed))
    );
}

#[test]
fn an_engine_error_in_the_stream_is_a_plain_error_and_overload_is_unavailable() {
    let w = world(
        FakeState {
            chat: ChatScript::ErrorEvent("the engine fell over".into()),
            ..FakeState::default()
        },
        true,
        1,
        vec![],
    );
    let (_, events) = w.turn("hi");
    assert!(
        matches!(events.last().unwrap(), AssistantEvent::Error { kind: ErrorKind::Failed, message, needs_ai: None } if message.contains("the engine fell over"))
    );
    let w = world(
        FakeState {
            failures: (0..8)
                .map(|_| ("/v1/chat/completions".to_string(), Failure::overloaded()))
                .collect(),
            ..FakeState::default()
        },
        true,
        1,
        vec![],
    );
    assert!(matches!(
        needs(&w.turn("hi").1),
        AiState::Unavailable { .. }
    ));
}

/// Stops the turn of `id` while the fake Pitwall stays silent and the turn's save is held, and checks that the generation
/// ends anyway: Stop acts on its own, without an answer from Pitwall and before the conversation is saved. Then lets the save
/// go and returns the turn's events up to its end.
///
/// No wall-clock limit is asserted (rule 40, C30): how fast a cancel acts is the loopback client's read slice, tested by
/// `paddock_pitwall`'s `cancel_acts_while_the_server_is_silent`. This test once asserted "under 2 s" from Stop to the turn's end
/// event, which also timed the save (an fsync): 2.8 s on the loaded Windows runner (CI run 8241), while Stop itself took at most
/// 65 ms there in 240 probe runs under CPU and disk load (docs/lessons.md, 2026-11-18). The wait is the testkit's hang guard only.
fn stop_with_the_save_held(w: &World, id: &str) -> Vec<AssistantEvent> {
    w.saves.hold();
    let returned = w.watched.returns();
    assert!(w.assistant.stop(id));
    returned
        .recv_timeout(paddock_testkit::wait::HANG_GUARD)
        .expect("the generation never ended after Stop (Pitwall silent, the save held)");
    // The turn has not ended yet: its end event waits for the held save.
    let mut events: Vec<AssistantEvent> = w.rx.try_iter().map(|e| e.event).collect();
    assert!(
        !events.iter().any(|e| matches!(
            e,
            AssistantEvent::Done { .. } | AssistantEvent::Error { .. }
        )),
        "the turn ended before its save: {events:?}"
    );
    w.saves.open();
    events.extend(until_end(&w.rx));
    events
}

#[test]
fn stop_ends_a_silent_pitwall_before_the_turn_is_saved_in_every_phase() {
    // Silent before the first byte.
    let w = world(
        FakeState {
            chat: ChatScript::HoldFirst,
            ..FakeState::default()
        },
        true,
        1,
        vec![],
    );
    let ack = w.assistant.send(ask("slow"), w.sink.clone()).unwrap();
    until(&w.rx, |e| matches!(e, AssistantEvent::Started { .. }));
    // The request must be on its way before stopping.
    while w.fake.requests_to("/v1/chat/completions").is_empty() {
        std::thread::yield_now();
    }
    let events = stop_with_the_save_held(&w, &ack.conversation_id);
    assert_eq!(done(&events).1, EndReason::Cancelled);
    w.fake.release();

    // Silent after some text: the text is kept.
    let w = world(
        FakeState {
            chat: ChatScript::HoldAfter(vec!["Part one".into()]),
            ..FakeState::default()
        },
        true,
        1,
        vec![],
    );
    let ack = w.assistant.send(ask("slow"), w.sink.clone()).unwrap();
    until(&w.rx, |e| matches!(e, AssistantEvent::Delta { .. }));
    let events = stop_with_the_save_held(&w, &ack.conversation_id);
    assert_eq!(
        done(&events),
        ("Part one".into(), EndReason::Cancelled, None)
    );
    w.fake.release();
}

#[test]
fn every_pitwall_error_maps_to_a_state_or_a_plain_failure() {
    let w = world(FakeState::default(), true, 1, vec![]);
    let l = &w.link;
    let e = |x: PitwallError| needs_ai(l, &x);
    assert_eq!(e(PitwallError::NotRunning), Some(AiState::NotRunning));
    assert_eq!(e(PitwallError::Stopped), Some(AiState::NotRunning));
    for x in [
        PitwallError::NeedsPairing,
        PitwallError::KeyRevoked,
        PitwallError::PairingDenied,
        PitwallError::PairingExpired,
    ] {
        assert_eq!(e(x), Some(AiState::NeedsPairing));
    }
    assert!(matches!(
        e(PitwallError::Incompatible {
            version: "9".into(),
            found: 0
        }),
        Some(AiState::Incompatible {
            update_app: false,
            ..
        })
    ));
    assert!(matches!(
        e(PitwallError::NoModelFits("x".into())),
        Some(AiState::NoSuitableModel { .. })
    ));
    // A ready link with a "not ready" error: the error's own sentence.
    assert!(
        matches!(e(PitwallError::NotReady("loading".into())), Some(AiState::Unavailable { message }) if message == "loading")
    );
    for x in [
        PitwallError::Forbidden("no".into()),
        PitwallError::Memory("m".into()),
        PitwallError::Overloaded {
            message: "o".into(),
            retry_after: None,
        },
        PitwallError::RateLimited {
            message: "r".into(),
            retry_after: None,
        },
    ] {
        assert!(matches!(e(x), Some(AiState::Unavailable { .. })));
    }
    for x in [
        PitwallError::Cancelled,
        PitwallError::Unsupported {
            cap: "vision".into(),
            hint: "h".into(),
        },
        PitwallError::Invalid("i".into()),
        PitwallError::Timeout("first byte"),
        PitwallError::Protocol("p".into()),
        PitwallError::Server {
            status: 500,
            code: String::new(),
            message: "s".into(),
        },
    ] {
        assert_eq!(e(x), None);
    }
    // A link that has not found Pitwall says why instead.
    let fresh = Link::new(LinkConfig::new(
        AppIdentity::new("demo", "Demo").unwrap(),
        Locator::in_dir(&w._dir.path().join("none"), Some(9), Arc::new(|_| false)),
        Arc::new(MemorySecrets::new("Demo")),
    ));
    assert_eq!(
        needs_ai(&fresh, &PitwallError::NotReady("x".into())),
        Some(AiState::NotInstalled)
    );
    let p = PitwallProvider::new(fresh);
    assert_eq!(
        (p.id(), p.display_name().as_str(), p.context_length()),
        ("pitwall", "Pitwall", 0)
    );
}

#[test]
fn a_finished_by_length_answer_is_reported_as_cut_short() {
    let w = world(
        FakeState {
            chat: ChatScript::Answer {
                pieces: vec!["cut".into()],
                finish: "length".into(),
            },
            ..FakeState::default()
        },
        true,
        1,
        vec![],
    );
    let (_, events) = w.turn("hi");
    assert_eq!(done(&events).0, "cut");
}
