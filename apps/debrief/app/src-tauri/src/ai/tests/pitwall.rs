//! Pitwall as the only engine through AiCore and the IPC (ADR 0059, ADR 0070), against the fake Pitwall of `debrief-ai`
//! on an ephemeral port with a temporary discovery folder and an in-memory key store: never the user's Pitwall, port,
//! folder or keychain.

use super::*;
use crate::ai::EngineStateDto;
use crate::health::commands::ai_health_of;
use crate::health::hub::AiHealth;
use debrief_ai::engine::pitwall::fake::{ChatScript, FakePitwall, FakeState};
use debrief_ai::engine::pitwall::{Locator, MemoryKeys, Pitwall, Tuning};
use debrief_ai::llm::{ChatMessage, ChatRequest};

struct World {
    f: Fixture,
    fake: FakePitwall,
    pw: Arc<Pitwall>,
    core: Arc<AiCore>,
}

fn world(tag: &str, state: FakeState, key: Option<&str>) -> World {
    let f = Fixture::new(tag);
    let fake = FakePitwall::start(state);
    let dir = f.tmp.0.join("pitwall-data");
    fake.write_discovery(&dir, std::process::id(), 1);
    let locator = Locator {
        data_dir: dir,
        fallback_port: None,
        pid_alive: Arc::new(|pid| pid == std::process::id()),
    };
    let keys: Arc<dyn debrief_ai::engine::pitwall::KeyStore> = Arc::new(key.map(MemoryKeys::with).unwrap_or_default());
    let pw = Pitwall::new(
        locator,
        keys,
        Tuning {
            poll_every: Duration::from_millis(10),
            ..Tuning::default()
        },
    );
    let core = Arc::new(AiCore::new(Some(f.tmp.0.join("ai")), false, AiDeps { pitwall: Some(pw.clone()) }));
    World { f, fake, pw, core }
}

fn paired() -> FakeState {
    FakeState {
        keys: vec!["pw_ok".into()],
        ..FakeState::default()
    }
}

fn hello() -> ChatRequest {
    ChatRequest {
        messages: vec![ChatMessage::user("hi")],
        max_tokens: 64,
        ..ChatRequest::default()
    }
}

#[test]
fn a_paired_pitwall_answers_and_background_work_goes_behind_the_user() {
    let w = world("pw-paired", paired(), Some("pw_ok"));
    assert_eq!(w.core.status().state, EngineStateDto::NoModel, "before the first look");
    w.pw.probe();
    let s = w.core.status();
    assert_eq!((s.available, s.state), (true, EngineStateDto::Ready));
    assert_eq!(s.model.as_deref(), Some("qwen3-8b-q4km"));
    let json = serde_json::to_value(&s).unwrap();
    assert_eq!(json["pitwall"]["found"], "running");
    assert_eq!(json["pitwall"]["usable"], true);
    assert!(json.get("engine").is_none(), "no engine choice any more");
    let r = w.core.provider().unwrap().chat(&hello(), &mut |_| {}).unwrap();
    assert_eq!(r.text, "Hello from Pitwall.");
    assert_eq!(ai_health_of(&s), AiHealth::Fine);
    let prio: Vec<String> = w
        .fake
        .requests_to("/v1/chat/completions")
        .into_iter()
        .filter_map(|(_, h, _)| h.into_iter().find(|(n, _)| n == "x-pitwall-priority").map(|(_, v)| v))
        .collect();
    assert_eq!(prio, ["interactive"]);
}

#[test]
fn without_pairing_there_is_no_model_and_no_fallback() {
    let w = world("pw-unpaired", FakeState::default(), None);
    let v = w.pw.probe();
    assert!(!v.paired && !v.usable);
    assert_eq!(w.core.provider().err().unwrap().code, super::super::error::AiErrorCode::Engine);
    let s = w.core.status();
    assert_eq!((s.state, s.pitwall.usable), (EngineStateDto::NoModel, false));
    assert!(s.reason.is_some(), "the status says why");
}

#[test]
fn pitwall_going_away_mid_session_is_degraded_never_a_silent_switch() {
    let state = FakeState {
        chat: ChatScript::Answer {
            pieces: vec!["ok".into()],
            finish: "stop".into(),
        },
        ..paired()
    };
    let w = world("pw-lost", state, Some("pw_ok"));
    w.pw.probe();
    w.core.provider().unwrap().chat(&hello(), &mut |_| {}).unwrap();
    w.fake.state().chat = ChatScript::DieAfter(vec!["Half".into()]);
    let err = w.core.provider().unwrap().chat(&hello(), &mut |_| {}).unwrap_err();
    assert!(err.to_string().contains("stopped answering"), "{err}");
    let s = w.core.status();
    assert_eq!(s.state, EngineStateDto::NoModel);
    assert!(s.reason.as_deref().unwrap().contains("stopped answering"));
    assert_eq!(ai_health_of(&s), AiHealth::EngineLost);
    let e = w.core.provider().err().unwrap();
    assert_eq!(e.code, super::super::error::AiErrorCode::Engine);
    let e = w.core.provider().err().unwrap();
    assert!(e.message.contains("stopped answering"), "{}", e.message);
}

#[test]
fn a_pitwall_that_is_gone_says_so() {
    let w = world("pw-gone", paired(), Some("pw_ok"));
    w.pw.probe();
    w.fake.kill();
    std::thread::sleep(Duration::from_millis(50));
    w.pw.probe();
    assert_eq!(w.core.status().state, EngineStateDto::NoModel);
    assert!(w.core.provider().is_err());
}

#[test]
fn connect_pairs_through_the_ipc_and_the_status_event_follows() {
    let w = world("pw-ipc", FakeState::default(), None);
    let app = w.f.app(w.core.clone());
    super::super::pitwall::start(app.handle(), w.core.clone());
    let main = window(&app, "main");
    let statuses = Arc::new(Mutex::new(Vec::<Value>::new()));
    let s2 = statuses.clone();
    app.listen_any(super::super::STATUS_EVENT, move |e| {
        s2.lock().unwrap().push(serde_json::from_str(e.payload()).unwrap());
    });
    wait_until("the first look at Pitwall", || {
        w.pw.view().found == debrief_ai::engine::pitwall::FoundKind::Running
    });
    let v = ipc(&main, "ai_pitwall_connect", json!({})).unwrap();
    assert_eq!(v["pairing"]["code"], "482913");
    wait_until("the approval", || w.core.status().state == EngineStateDto::Ready);
    wait_until("a status event for the served model", || {
        statuses
            .lock()
            .unwrap()
            .iter()
            .any(|s| s["state"] == "ready" && s["pitwall"]["paired"] == true)
    });
    let status = ipc(&main, "ai_pitwall_refresh", json!({})).unwrap();
    assert_eq!(status["pitwall"]["paired"], true);
    assert_eq!(ipc(&main, "ai_pitwall_cancel", json!({})).unwrap(), json!(false));
    let status = ipc(&main, "ai_pitwall_forget", json!({})).unwrap();
    assert_eq!(status["pitwall"]["paired"], false);
    assert_eq!(status["state"], "noModel");
}
