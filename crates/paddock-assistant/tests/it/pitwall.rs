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
use std::sync::Arc;
use std::time::{Duration, Instant};
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
    _dir: tempfile::TempDir,
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
    let assistant = Assistant::new(
        Arc::new(PitwallProvider::new(link.clone())),
        Tools::new(tools).unwrap(),
        store,
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

#[test]
fn the_tool_protocol_runs_over_pitwalls_chat_api() {
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
        vec![Arc::new(Echo(runs.clone()))],
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

#[test]
fn stop_ends_a_slow_pitwall_within_a_read_slice_in_every_phase() {
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
    let asked = Instant::now();
    w.assistant.stop(&ack.conversation_id);
    let events = until_end(&w.rx);
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "{:?}",
        asked.elapsed()
    );
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
    let asked = Instant::now();
    w.assistant.stop(&ack.conversation_id);
    let events = until_end(&w.rx);
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "{:?}",
        asked.elapsed()
    );
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
