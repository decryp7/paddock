//! Contract tests of the Pitwall client against an in-process fake that mirrors Pitwall's routes (loopback, an ephemeral
//! port, no real Pitwall, no real credential store, no sleeping: waits are channel and condition waits).

use paddock_health::{FakeClock, Health, RepairAction, ServiceState};
use paddock_pitwall::discovery::{FILE_NAME, Found};
use paddock_pitwall::engine::{EngineChoice, Routed, Session, run_or_fallback};
use paddock_pitwall::health::{Fallback, SERVICE_ID, report, service_spec};
use paddock_pitwall::retry::{Policy, Sleeper};
use paddock_pitwall::testing::{
    ChatScript, Failure, FakeModel, FakePitwall, FakeState, PITWALL_ROUTES, PairMode, hang_up_port,
    matches_route,
};
use paddock_pitwall::*;
use paddock_secrets::{MemorySecrets, Name, Secrets};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const KEY: &str = "pw_testkey";

fn app() -> AppIdentity {
    AppIdentity::new("debrief", "Debrief").unwrap()
}

fn keyed() -> FakeState {
    FakeState {
        keys: vec![KEY.into()],
        ..FakeState::default()
    }
}

fn client(fake: &FakePitwall) -> Client {
    Client::new(fake.port, app())
        .with_key(Some(paddock_secrets::Secret::new(KEY).unwrap()))
        .with_sleeper(Arc::new(Recorder::default()))
}

/// Records the waits instead of sleeping.
#[derive(Default)]
struct Recorder(Mutex<Vec<Duration>>);

impl Sleeper for Recorder {
    fn sleep(&self, d: Duration, _: &CancelToken) {
        self.0.lock().unwrap().push(d);
    }
}

/// Blocks each wait until the test sends a tick (or drops the sender): the test steps the pairing poll.
struct Ticks(Mutex<Receiver<()>>);

impl Sleeper for Ticks {
    fn sleep(&self, _: Duration, _: &CancelToken) {
        let _ = self.0.lock().unwrap().recv();
    }
}

fn chat_req(text: &str) -> ChatRequest {
    ChatRequest::new(
        ModelSel::Auto(Caps::chat(8192)),
        vec![Message::text(Role::User, text)],
    )
}

// ---------------------------------------------------------------------------------------------------------------
// The contract itself

#[test]
fn the_fake_serves_every_route_the_contract_names() {
    let fake = FakePitwall::start(FakeState {
        jobs: vec!["7".into()],
        ..keyed()
    });
    let c = client(&fake);
    c.health().unwrap();
    assert_eq!(c.begin_pairing().unwrap().code, "482913");
    c.poll_pairing("pr1-abc").unwrap();
    c.select(&ModelSel::Auto(Caps::chat(4096))).unwrap();
    c.models().unwrap();
    c.model_ids().unwrap();
    c.embed(&EmbedRequest::new(vec!["a".into()])).unwrap();
    c.chat(&chat_req("hi")).unwrap();
    c.task(
        Task::Summarize,
        &serde_json::json!({"text": "abc"}),
        &CallOptions::default(),
    )
    .unwrap();
    c.count_tokens("abcd").unwrap();
    c.truncate("abcdefgh", 1, Keep::Start).unwrap();
    c.jobs().unwrap();
    assert!(c.cancel_job("7").unwrap());
    let seen: Vec<(String, String)> = fake
        .state()
        .seen
        .iter()
        .map(|(m, p, _, _)| (m.clone(), p.clone()))
        .collect();
    for (method, pattern) in PITWALL_ROUTES {
        assert!(
            seen.iter()
                .any(|(m, p)| m == method && matches_route(pattern, p)),
            "the client never called {method} {pattern}"
        );
    }
}

/// When Pitwall's source is at hand (`PADDOCK_PITWALL_HTTP_RS=<pitwall>/crates/pitwall-server/src/http.rs`), every route of the
/// contract must still be in its router: the consumer-driven check that catches drift. Without the variable the test passes
/// (CI of this repo has no Pitwall checkout); Pitwall's own CI runs it with the variable set.
#[test]
fn pitwalls_router_still_has_every_route_of_the_contract() {
    let Ok(path) = std::env::var("PADDOCK_PITWALL_HTTP_RS") else {
        return;
    };
    let source = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    for (method, pattern) in PITWALL_ROUTES {
        let verb = method.to_ascii_lowercase();
        let line = source
            .lines()
            .find(|l| l.contains(&format!("\"{pattern}\"")) && l.contains(".route("))
            .unwrap_or_else(|| panic!("Pitwall's router has no route {pattern}"));
        assert!(
            line.contains(&format!("{verb}(")),
            "{pattern} is not a {method} route in Pitwall: {line}"
        );
    }
}

#[test]
fn the_fake_answers_what_pitwall_answers_when_a_key_is_missing_or_wrong() {
    let fake = FakePitwall::start(keyed());
    let nokey = Client::new(fake.port, app());
    assert_eq!(nokey.models().unwrap_err(), PitwallError::NeedsPairing);
    let wrong = Client::new(fake.port, app())
        .with_key(Some(paddock_secrets::Secret::new("pw_other").unwrap()));
    assert_eq!(
        wrong.chat(&chat_req("x")).unwrap_err(),
        PitwallError::KeyRevoked
    );
    assert_eq!(wrong.models().unwrap_err(), PitwallError::KeyRevoked);
    // Health and pairing need no key.
    assert!(nokey.health().unwrap().is_serving());
}

// ---------------------------------------------------------------------------------------------------------------
// Chat, streaming and cancel

#[test]
fn a_streamed_chat_delivers_deltas_provenance_and_usage() {
    let fake = FakePitwall::start(FakeState {
        job_id: Some("12".into()),
        cold_start_ms: Some(900),
        cache: Some("miss".into()),
        preset: Some("debrief".into()),
        ..keyed()
    });
    let c = client(&fake);
    let mut seen = Vec::new();
    let mut req = chat_req("hello");
    req.options.priority = Priority::Background;
    req.options.cache = CachePolicy::Bypass;
    let reply = c
        .chat_with(&req, &mut |d| seen.push(d.to_string()))
        .unwrap();
    assert_eq!(seen, ["Hello", " from", " Pitwall."]);
    assert_eq!(reply.text, "Hello from Pitwall.");
    assert_eq!(
        (reply.finish_reason.as_deref(), reply.usage),
        (
            Some("stop"),
            Usage {
                prompt_tokens: 11,
                completion_tokens: 3
            }
        )
    );
    let p = &reply.provenance;
    assert_eq!(p.model.as_deref(), Some("qwen3-8b-q4km"));
    assert_eq!(
        (p.engine.as_deref(), p.chosen_by.as_deref(), p.cold_start_ms),
        (Some("fake"), Some("capabilities"), Some(900))
    );
    assert_eq!(
        (p.cache, p.alias.as_deref(), p.job_id.as_deref()),
        (Some(CacheStatus::Miss), Some("debrief"), Some("12"))
    );
    let (_, headers, body) = fake.requests_to("/v1/chat/completions").remove(0);
    let h = |n: &str| {
        headers
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(
        (
            h("x-pitwall-priority"),
            h("authorization"),
            h("cache-control")
        ),
        (
            Some("background"),
            Some("Bearer pw_testkey"),
            Some("no-cache")
        )
    );
    assert!(h("user-agent").unwrap().starts_with("debrief/"));
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        (body["model"].as_str(), body["stream"].as_bool()),
        (Some("auto:chat,8k"), Some(true))
    );
}

#[test]
fn dropping_a_stream_cancels_and_cancel_acts_while_the_server_is_silent() {
    // Cancel before the first byte: the server holds its answer.
    let fake = FakePitwall::start(FakeState {
        chat: ChatScript::HoldFirst,
        ..keyed()
    });
    let c = client(&fake).with_timeouts(
        Timeouts::QUICK,
        Timeouts {
            connect: Duration::from_secs(1),
            first_byte: Duration::from_secs(300),
            idle: Duration::from_secs(300),
        },
    );
    let cancel = CancelToken::new();
    let mut req = chat_req("x");
    req.options.cancel = cancel.clone();
    let c2 = c.clone();
    let h = std::thread::spawn(move || c2.chat(&req));
    // Cancel once the server has the request.
    loop {
        if !fake.requests_to("/v1/chat/completions").is_empty() {
            break;
        }
        std::thread::yield_now();
    }
    cancel.cancel();
    assert_eq!(h.join().unwrap().unwrap_err(), PitwallError::Cancelled);
    fake.release();

    // Cancel in the middle of a stream; the pieces so far were delivered.
    let fake = FakePitwall::start(FakeState {
        chat: ChatScript::HoldAfter(vec!["one".into(), "two".into()]),
        ..keyed()
    });
    let c = client(&fake);
    let cancel = CancelToken::new();
    let mut req = chat_req("x");
    req.options.cancel = cancel.clone();
    let mut got = Vec::new();
    let r = c.chat_with(&req, &mut |d| {
        got.push(d.to_string());
        if got.len() == 2 {
            cancel.cancel();
        }
    });
    assert_eq!(r.unwrap_err(), PitwallError::Cancelled);
    assert_eq!(got, ["one", "two"]);
    fake.release();

    // A stream dropped early: the iterator form.
    let fake = FakePitwall::start(keyed());
    let c = client(&fake);
    let mut stream = c.chat_stream(&chat_req("x")).unwrap();
    assert_eq!(stream.next().unwrap().unwrap(), "Hello");
    drop(stream);
}

#[test]
fn a_server_that_dies_mid_stream_is_stopped_not_a_hang() {
    let fake = FakePitwall::start(FakeState {
        chat: ChatScript::DieAfter(vec!["par".into()]),
        ..keyed()
    });
    let c = client(&fake);
    let mut got = String::new();
    let r = c.chat_with(&chat_req("x"), &mut |d| got.push_str(d));
    assert_eq!(r.unwrap_err(), PitwallError::Stopped);
    assert_eq!(got, "par", "what arrived was delivered");
    // The failed stream was not retried after its first delta.
    assert_eq!(fake.requests_to("/v1/chat/completions").len(), 1);
}

#[test]
fn an_error_event_in_the_stream_is_the_engines_error() {
    let fake = FakePitwall::start(FakeState {
        chat: ChatScript::ErrorEvent("the engine crashed".into()),
        ..keyed()
    });
    let r = client(&fake).chat(&chat_req("x"));
    assert!(
        matches!(r, Err(PitwallError::Server { code, message, .. }) if code == "engine_error" && message == "the engine crashed")
    );
}

#[test]
fn slow_answers_time_out_with_the_limit_that_ran_out() {
    let quick = Timeouts {
        connect: Duration::from_secs(1),
        first_byte: Duration::from_millis(100),
        idle: Duration::from_millis(100),
    };
    let fake = FakePitwall::start(FakeState {
        chat: ChatScript::HoldFirst,
        ..keyed()
    });
    let c = client(&fake).with_timeouts(quick, quick);
    assert_eq!(
        c.chat(&chat_req("x")).unwrap_err(),
        PitwallError::Timeout("the first answer")
    );
    fake.release();
    let fake = FakePitwall::start(FakeState {
        chat: ChatScript::HoldAfter(vec!["a".into()]),
        ..keyed()
    });
    let c = client(&fake).with_timeouts(quick, quick);
    assert_eq!(
        c.chat(&chat_req("x")).unwrap_err(),
        PitwallError::Timeout("the next piece of the answer")
    );
    fake.release();
}

#[test]
fn a_calls_deadline_ends_the_retries() {
    let fake = FakePitwall::start(FakeState {
        failures: vec![("/x/v1/models".into(), Failure::overloaded())],
        ..keyed()
    });
    let c = client(&fake);
    let o = CallOptions {
        deadline: Some(Duration::ZERO),
        ..CallOptions::default()
    };
    let r = c.task(Task::Summarize, &serde_json::json!({"text": "x"}), &o);
    assert!(r.is_ok(), "no failure was scripted for this path");
    fake.state()
        .failures
        .push(("/x/v1/tasks/summarize".into(), Failure::overloaded()));
    let r = c.task(Task::Summarize, &serde_json::json!({"text": "x"}), &o);
    assert_eq!(r.unwrap_err(), PitwallError::Timeout("the call's deadline"));
}

// ---------------------------------------------------------------------------------------------------------------
// Errors and retries

#[test]
fn overload_is_retried_with_the_servers_retry_after_and_other_errors_are_not() {
    let rec = Arc::new(Recorder::default());
    let fake = FakePitwall::start(FakeState {
        failures: vec![
            ("/x/v1/models".into(), Failure::overloaded()),
            ("/x/v1/models".into(), Failure::overloaded()),
        ],
        ..keyed()
    });
    let c = client(&fake).with_sleeper(rec.clone());
    assert_eq!(c.models().unwrap().len(), 1);
    assert_eq!(
        *rec.0.lock().unwrap(),
        [Duration::from_secs(5), Duration::from_secs(5)]
    );
    assert_eq!(fake.requests_to("/x/v1/models").len(), 3);

    // A chat is retried before its first delta, the same way.
    fake.state()
        .failures
        .push(("/v1/chat/completions".into(), Failure::overloaded()));
    assert_eq!(c.chat(&chat_req("x")).unwrap().text, "Hello from Pitwall.");

    // A 429 with Retry-After waits as told; a 4xx is never retried.
    fake.state().failures.push((
        "/x/v1/jobs".into(),
        Failure {
            status: 429,
            code: "rate_limited".into(),
            message: "slow".into(),
            retry_after: Some(2),
        },
    ));
    assert!(c.jobs().unwrap().is_empty());
    assert_eq!(rec.0.lock().unwrap().last(), Some(&Duration::from_secs(2)));
    for (status, code, want) in [
        (403, "forbidden", PitwallError::Forbidden("m".into())),
        (
            404,
            "preset_target_unavailable",
            PitwallError::NoModelFits("m".into()),
        ),
        (400, "invalid_request", PitwallError::Invalid("m".into())),
        (413, "request_too_large", PitwallError::Invalid("m".into())),
        (503, "insufficient_memory", PitwallError::Memory("m".into())),
    ] {
        fake.state().failures.push((
            "/x/v1/jobs".into(),
            Failure {
                status,
                code: code.into(),
                message: "m".into(),
                retry_after: None,
            },
        ));
        let before = fake.requests_to("/x/v1/jobs").len();
        let e = c.jobs().unwrap_err();
        assert_eq!(e, want);
        let calls = fake.requests_to("/x/v1/jobs").len() - before;
        assert_eq!(calls, 1, "{code} is not retried");
    }
    // A persistent overload gives up after the policy's attempts, with a recorded wait between each.
    let rec2 = Arc::new(Recorder::default());
    let c2 = client(&fake)
        .with_sleeper(rec2.clone())
        .with_retry(Policy::INTERACTIVE);
    for _ in 0..4 {
        fake.state()
            .failures
            .push(("/x/v1/jobs".into(), Failure::overloaded()));
    }
    assert!(matches!(c2.jobs(), Err(PitwallError::Overloaded { .. })));
    assert_eq!(rec2.0.lock().unwrap().len(), 3);
}

#[test]
fn the_server_gone_is_not_running_and_a_foreign_listener_is_a_protocol_error() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let rec = Arc::new(Recorder::default());
    let c = Client::new(port, app()).with_sleeper(rec.clone());
    assert_eq!(c.health().unwrap_err(), PitwallError::NotRunning);
    assert!(rec.0.lock().unwrap().is_empty(), "a probe is not retried");
    let c = c.with_key(Some(paddock_secrets::Secret::new(KEY).unwrap()));
    assert_eq!(c.models().unwrap_err(), PitwallError::NotRunning);
    assert_eq!(
        rec.0.lock().unwrap().len(),
        3,
        "other calls retry a refused connection"
    );
    // Something that answers HTTP but is not Pitwall.
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let p = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        if let Ok((mut s, _)) = l.accept() {
            let mut b = [0u8; 4096];
            let _ = s.read(&mut b);
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n<html>ok.");
        }
    });
    assert!(matches!(
        Client::new(p, app()).health(),
        Err(PitwallError::Protocol(_))
    ));
}

// ---------------------------------------------------------------------------------------------------------------
// Models, capabilities, tokens, jobs, tasks, embeddings

#[test]
fn capabilities_are_checked_before_a_request_and_carry_pitwalls_reason() {
    let mut blind = FakeModel::chat("blind", 8192);
    blind.blocked = vec![("vision".into(), "needs an engine build with mtmd".into())];
    let mut sighted = FakeModel::chat("sighted", 16384);
    sighted.capabilities.push("vision".into());
    sighted.context_per_slot = Some(4096);
    let fake = FakePitwall::start(FakeState {
        models: vec![blind, sighted],
        ..keyed()
    });
    let c = client(&fake);
    let models = c.models().unwrap();
    assert_eq!(
        models[0].blocked_reason("vision"),
        Some("needs an engine build with mtmd")
    );
    assert_eq!(
        (models[1].usable_context(), models[0].usable_context()),
        (Some(4096), Some(8192))
    );
    // `auto:chat` picks the first model, which cannot see: refused locally with the reason.
    let e = c
        .ensure_capability(&ModelSel::Auto(Caps::chat(4096)), "vision")
        .unwrap_err();
    assert!(
        matches!(&e, PitwallError::Unsupported { cap, hint } if cap == "vision" && hint.contains("mtmd")),
        "{e:?}"
    );
    // With vision in the selection Pitwall picks the model that can.
    let ok = c
        .ensure_capability(&ModelSel::Auto(Caps::chat(4096).with_vision()), "vision")
        .unwrap();
    assert_eq!(ok.id, "sighted");
    // No model at all has embeddings.
    let none = c.select(&ModelSel::Auto(Caps::embeddings())).unwrap_err();
    assert!(matches!(none, PitwallError::NoModelFits(_)));
    assert!(
        matches!(c.ensure_capability(&ModelSel::Auto(Caps::chat(1024)), "tools"), Ok(m) if m.id == "blind")
    );
    // A model without the capability and without a reason still names the capability.
    let e = c
        .ensure_capability(&ModelSel::Auto(Caps::chat(1024)), "rerank")
        .unwrap_err();
    assert!(matches!(e, PitwallError::Unsupported { hint, .. } if hint.contains("rerank")));
}

#[test]
fn embeddings_are_batched_in_order_and_a_short_answer_is_refused() {
    let fake = FakePitwall::start(FakeState {
        cache: Some("hit".into()),
        ..keyed()
    });
    let c = client(&fake);
    let input: Vec<String> = (0..EMBED_BATCH * 2 + 5)
        .map(|i| "x".repeat(i + 1))
        .collect();
    let r = c.embed(&EmbedRequest::new(input.clone())).unwrap();
    assert_eq!(r.vectors.len(), input.len());
    assert_eq!(fake.requests_to("/v1/embeddings").len(), 3, "64 + 64 + 5");
    assert_eq!(r.vectors[0], [1.0, 0.0]);
    assert_eq!(
        r.vectors[input.len() - 1],
        [input.len() as f32, 4.0],
        "the order is the input's"
    );
    assert_eq!(
        (r.usage.prompt_tokens, r.provenance.cache),
        (input.len() as u64 * 2, Some(CacheStatus::Hit))
    );
    assert_eq!(
        c.embed(&EmbedRequest::new(vec![])).unwrap().vectors.len(),
        0
    );
    fake.state().embed_short = true;
    assert!(
        matches!(c.embed(&EmbedRequest::new(vec!["a".into(), "b".into()])), Err(PitwallError::Protocol(m)) if m.contains("1 vectors for 2"))
    );
}

#[test]
fn tokens_tasks_and_jobs() {
    let fake = FakePitwall::start(FakeState {
        jobs: vec!["7".into(), "8".into()],
        ..keyed()
    });
    let c = client(&fake);
    assert_eq!(c.count_tokens("abcdefgh").unwrap(), 2);
    assert_eq!(
        c.truncate("abcdefghijklmnop", 2, Keep::Start).unwrap(),
        ("abcdefgh".to_string(), true)
    );
    assert_eq!(
        c.truncate("abcdefghijklmnop", 2, Keep::End).unwrap(),
        ("ijklmnop".to_string(), true)
    );
    assert_eq!(
        c.truncate("abc", 10, Keep::Ends).unwrap(),
        ("abc".to_string(), false)
    );
    let t = c
        .task(
            Task::Summarize,
            &serde_json::json!({"text": "abcd"}),
            &CallOptions::default(),
        )
        .unwrap();
    assert_eq!(
        (t.result, t.model.as_deref(), t.usage),
        (
            serde_json::json!("summarize of 4 chars"),
            Some("qwen3-8b-q4km"),
            Usage {
                prompt_tokens: 5,
                completion_tokens: 2
            }
        )
    );
    let t = c
        .task(
            Task::Classify,
            &serde_json::json!({"text": "x", "labels": ["a"]}),
            &CallOptions::default(),
        )
        .unwrap();
    assert_eq!(t.result, serde_json::json!({"label": "ok"}));
    let jobs = c.jobs().unwrap();
    assert_eq!(
        jobs.iter().map(|j| j.id.as_str()).collect::<Vec<_>>(),
        ["7", "8"]
    );
    assert!(c.cancel_job("7").unwrap());
    assert!(!c.cancel_job("7").unwrap(), "a job Pitwall does not know");
    // Ids are numbers; anything else never leaves the process.
    for bad in ["", "7/../x", "a", "1 2"] {
        assert!(
            matches!(c.cancel_job(bad), Err(PitwallError::Invalid(_))),
            "{bad:?}"
        );
    }
    assert!(matches!(
        c.poll_pairing("a/b"),
        Err(PitwallError::Protocol(_))
    ));
    assert!(matches!(c.poll_pairing(""), Err(PitwallError::Protocol(_))));
}

// ---------------------------------------------------------------------------------------------------------------
// The link: discovery, probing, pairing, outcomes

struct Rig {
    fake: FakePitwall,
    dir: tempfile::TempDir,
    secrets: Arc<MemorySecrets>,
    link: Arc<Link>,
    events: Receiver<LinkEvent>,
    ticks: Option<Sender<()>>,
    clock: FakeClock,
}

fn rig_with(state: FakeState, stepped: bool, pid: u32, api: u32) -> Rig {
    let fake = FakePitwall::start(state);
    let dir = tempfile::tempdir().unwrap();
    fake.write_discovery(dir.path(), pid, api);
    let secrets = Arc::new(MemorySecrets::new("Debrief"));
    let clock = FakeClock::at_wall_ms(1_000_000);
    let (tx, rx) = channel();
    let sleeper: Arc<dyn Sleeper> = if stepped {
        Arc::new(Ticks(Mutex::new(rx)))
    } else {
        Arc::new(Recorder::default())
    };
    let mut cfg = LinkConfig::new(
        app(),
        Locator::in_dir(dir.path(), None, Arc::new(|p| p == 42)),
        secrets.clone(),
    );
    cfg.clock = Arc::new(clock.clone());
    cfg.sleeper = sleeper;
    let link = Link::new(cfg);
    let (etx, events) = channel();
    link.set_listener(Arc::new(move |e| {
        let _ = etx.send(e);
    }));
    Rig {
        fake,
        dir,
        secrets,
        link,
        events,
        ticks: stepped.then_some(tx),
        clock,
    }
}

fn rig(state: FakeState) -> Rig {
    rig_with(state, false, 42, 1)
}

fn store_key(r: &Rig) {
    KeyVault::new(r.secrets.clone()).set(KEY).unwrap();
}

fn next_event(r: &Rig, want: impl Fn(&LinkEvent) -> bool) -> LinkEvent {
    loop {
        let e = r.events.recv().expect("the link went away");
        if want(&e) {
            return e;
        }
    }
}

#[test]
fn the_link_finds_a_paired_pitwall_and_chats_with_the_model_it_picked() {
    let r = rig(keyed());
    store_key(&r);
    let v = r.link.probe();
    assert_eq!(
        (
            v.found,
            v.paired,
            v.usable,
            v.model.as_deref(),
            v.version.as_deref()
        ),
        (
            FoundKind::Running,
            true,
            true,
            Some("qwen3-8b-q4km"),
            Some("1.0.4")
        )
    );
    assert_eq!(
        (v.context_tokens, v.vision, v.problem.clone()),
        (8192, false, None)
    );
    let state = AiState::from_view(&v, r.link.needs(), false);
    assert!(state.is_ready());
    let mut text = String::new();
    let reply = r
        .link
        .chat_with(chat_req("hi"), &mut |d| text.push_str(d))
        .unwrap();
    assert_eq!(reply.text, text);
    assert!(r.link.served());
    // The link chose the model, not the request.
    let (_, _, body) = r.fake.requests_to("/v1/chat/completions").remove(0);
    assert!(body.contains("\"model\":\"auto:chat,8k\""));
}

#[test]
fn the_users_preset_wins_and_context_follows_the_model() {
    let mut m = FakeModel::chat("big", 131072);
    m.context_per_slot = Some(4000);
    m.capabilities.push("vision".into());
    let r = rig(FakeState {
        preset: Some("debrief".into()),
        models: vec![m],
        ..keyed()
    });
    store_key(&r);
    let v = r.link.probe();
    assert_eq!((v.context_tokens, v.vision), (4000, true));
    r.link.chat_with(chat_req("hi"), &mut |_| {}).unwrap();
    let (_, _, body) = r.fake.requests_to("/v1/chat/completions").remove(0);
    assert!(body.contains("\"model\":\"debrief\""));
}

#[test]
fn every_way_pitwall_can_be_absent_is_a_state_with_a_fix() {
    // Never installed: no folder, no fallback port.
    let nothing = tempfile::tempdir().unwrap();
    let secrets = Arc::new(MemorySecrets::new("Debrief"));
    let link = Link::new(LinkConfig::new(
        app(),
        Locator::in_dir(&nothing.path().join("missing"), None, Arc::new(|_| true)),
        secrets.clone(),
    ));
    let v = link.probe();
    assert_eq!(
        AiState::from_view(&v, link.needs(), false),
        AiState::NotInstalled
    );
    assert_eq!(
        link.ready(Priority::Interactive).unwrap_err(),
        PitwallError::NotRunning
    );
    // Installed (folder exists) but nothing listens.
    let link = Link::new(LinkConfig::new(
        app(),
        Locator::in_dir(nothing.path(), None, Arc::new(|_| true)),
        secrets,
    ));
    assert_eq!(
        AiState::from_view(&link.probe(), link.needs(), false),
        AiState::NotRunning
    );

    // The process of the discovery file is dead and the fallback port is shut.
    let r = rig_with(keyed(), false, 7, 1);
    assert_eq!(r.link.probe().found, FoundKind::Installed);
    // A newer Pitwall.
    let r = rig_with(
        FakeState {
            api_version: 2,
            ..keyed()
        },
        false,
        42,
        2,
    );
    let v = r.link.probe();
    assert_eq!(v.found, FoundKind::Incompatible);
    let s = AiState::from_view(&v, r.link.needs(), false);
    assert!(
        matches!(&s, AiState::Incompatible { update_app: true, message } if message.contains("Update Debrief")),
        "{s:?}"
    );
    assert!(matches!(
        r.link.ready(Priority::Interactive),
        Err(PitwallError::NotReady(_))
    ));
    assert!(matches!(
        r.link.connect(true),
        Err(PitwallError::NotReady(_))
    ));
    // The file says apiVersion 1 but the server answers 3.
    let r = rig_with(
        FakeState {
            api_version: 3,
            ..keyed()
        },
        false,
        42,
        1,
    );
    assert_eq!(r.link.probe().found, FoundKind::Incompatible);
    // An older Pitwall.
    let r = rig_with(
        FakeState {
            api_version: 0,
            ..keyed()
        },
        false,
        42,
        1,
    );
    let s = AiState::from_view(&r.link.probe(), r.link.needs(), false);
    assert!(matches!(
        s,
        AiState::Incompatible {
            update_app: false,
            ..
        }
    ));
}

#[test]
fn paused_stopped_and_no_model_are_states_not_errors() {
    let r = rig(FakeState {
        serving: "paused".into(),
        ..keyed()
    });
    store_key(&r);
    let v = r.link.probe();
    assert_eq!(
        AiState::from_view(&v, r.link.needs(), false),
        AiState::Paused { stopped: false }
    );
    assert!(!v.usable);
    r.fake.state().serving = "stopped".into();
    assert_eq!(
        AiState::from_view(&r.link.probe(), r.link.needs(), false),
        AiState::Paused { stopped: true }
    );
    assert!(
        matches!(r.link.ready(Priority::Interactive), Err(PitwallError::NotReady(m)) if m.contains("stopped"))
    );

    let r = rig(FakeState {
        models: vec![],
        ..keyed()
    });
    store_key(&r);
    let s = AiState::from_view(&r.link.probe(), r.link.needs(), false);
    assert_eq!(
        s,
        AiState::NoSuitableModel {
            needs: Caps::chat(8192)
        }
    );
    assert_eq!(s.actions(), [NextStep::InstallModel]);
}

#[test]
fn a_revoked_key_is_forgotten_and_the_app_asks_to_connect_again() {
    let r = rig(keyed());
    KeyVault::new(r.secrets.clone()).set("pw_revoked").unwrap();
    let v = r.link.probe();
    assert!(!v.paired && !v.usable, "{v:?}");
    assert_eq!(
        AiState::from_view(&v, r.link.needs(), false),
        AiState::NeedsPairing
    );
    assert!(
        KeyVault::new(r.secrets.clone()).get().is_none(),
        "the useless key is deleted"
    );
    assert!(
        v.problem
            .unwrap()
            .contains("no longer accepts Debrief's key")
    );
}

#[test]
fn a_pitwall_that_stops_or_revokes_mid_session_is_noticed_by_the_next_request() {
    let r = rig(FakeState {
        chat: ChatScript::DieAfter(vec!["a".into()]),
        ..keyed()
    });
    store_key(&r);
    r.link.probe();
    r.fake.state().chat = ChatScript::Answer {
        pieces: vec!["ok".into()],
        finish: "stop".into(),
    };
    r.link.chat_with(chat_req("1"), &mut |_| {}).unwrap();
    assert!(r.link.served());
    r.fake.state().chat = ChatScript::DieAfter(vec!["a".into()]);
    let e = r.link.chat_with(chat_req("2"), &mut |_| {}).unwrap_err();
    assert_eq!(e, PitwallError::Stopped);
    let v = r.link.view();
    assert!(!v.usable && v.problem.unwrap().contains("stopped answering"));
    // Automatic does not fall back silently now: it served and went away.
    let session = Session::new(EngineChoice::Auto);
    assert_eq!(session.engine(&r.link), None);
    let routed = run_or_fallback(
        &session,
        &r.link,
        "Ask about this log",
        "Debrief",
        || "pitwall",
        || "built-in",
    );
    let Routed::Unavailable(view) = routed else {
        panic!("nothing may run")
    };
    assert_eq!(view.feature, "Ask about this log");
    assert!(
        matches!(view.reason, AiState::Unavailable { .. }),
        "{:?}",
        view.reason
    );
    // The repair does not depend on Pitwall.
    session.use_built_in_for_session();
    assert!(matches!(
        run_or_fallback(
            &session,
            &r.link,
            "x",
            "Debrief",
            || "pitwall",
            || "built-in"
        ),
        Routed::Ran(EngineKind::BuiltIn, "built-in")
    ));

    // A key revoked while running.
    let r = rig(keyed());
    store_key(&r);
    r.link.probe();
    r.fake.state().keys.clear();
    assert_eq!(
        r.link.chat_with(chat_req("x"), &mut |_| {}).unwrap_err(),
        PitwallError::KeyRevoked
    );
    assert!(KeyVault::new(r.secrets.clone()).get().is_none());
    assert!(!r.link.view().paired);
    // Other failures leave the link as it was.
    let r = rig(keyed());
    store_key(&r);
    r.link.probe();
    r.fake.state().failures.push((
        "/v1/chat/completions".into(),
        Failure {
            status: 400,
            code: "invalid_request".into(),
            message: "bad".into(),
            retry_after: None,
        },
    ));
    assert!(matches!(
        r.link.chat_with(chat_req("x"), &mut |_| {}),
        Err(PitwallError::Invalid(_))
    ));
    assert!(r.link.view().usable);
}

#[test]
fn pairing_needs_the_users_choice_shows_the_code_and_stores_the_key_once_approved() {
    let r = rig_with(
        FakeState {
            pending_polls: 1,
            ..FakeState::default()
        },
        true,
        42,
        1,
    );
    let v = r.link.probe();
    assert_eq!(
        AiState::from_view(&v, r.link.needs(), false),
        AiState::NeedsPairing
    );
    assert!(
        r.fake.requests_to("/x/v1/pair").is_empty(),
        "probing never pairs"
    );
    let v = r.link.connect(true).unwrap();
    assert_eq!(v.pairing.as_ref().map(|p| p.code.as_str()), Some("482913"));
    assert_eq!(v.pairing.unwrap().expires_at_ms, 1_000_000 + 300_000);
    let (_, _, body) = r.fake.requests_to("/x/v1/pair").remove(0);
    assert_eq!(body, r#"{"name":"Debrief"}"#);
    // A second connect while one waits starts nothing new.
    assert!(r.link.connect(true).unwrap().pairing.is_some());
    assert_eq!(r.fake.requests_to("/x/v1/pair").len(), 1);
    let tx = r.ticks.as_ref().unwrap();
    tx.send(()).unwrap(); // first poll: still pending
    tx.send(()).unwrap(); // second poll: approved
    let e = next_event(&r, |e| matches!(e, LinkEvent::Paired { .. }));
    assert_eq!(e, LinkEvent::Paired { use_for_app: true });
    let v = r.link.view();
    assert!(v.paired && v.usable && v.pairing.is_none(), "{v:?}");
    assert_eq!(
        KeyVault::new(r.secrets.clone()).get().unwrap().expose(),
        "pw_fakekey"
    );
    assert_eq!(
        r.secrets
            .get(&Name::new("Pitwall", "API key").unwrap())
            .unwrap()
            .unwrap()
            .expose(),
        "pw_fakekey"
    );
    // Already usable: connect is a no-op that still reports the choice.
    assert!(r.link.connect(true).unwrap().usable);
    assert_eq!(
        next_event(&r, |e| matches!(e, LinkEvent::Paired { .. })),
        LinkEvent::Paired { use_for_app: true }
    );
    assert_eq!(r.fake.requests_to("/x/v1/pair").len(), 1);
}

#[test]
fn a_denied_expired_or_cancelled_pairing_changes_nothing_and_says_so() {
    // Denied.
    let r = rig_with(
        FakeState {
            pair: PairMode::Deny,
            pending_polls: 0,
            ..FakeState::default()
        },
        true,
        42,
        1,
    );
    r.link.connect(false).unwrap();
    r.ticks.as_ref().unwrap().send(()).unwrap();
    next_event(&r, |_| r.link.view().pairing.is_none());
    let v = r.link.view();
    assert!(!v.paired && v.problem.unwrap().contains("denied Debrief"));

    // Expired: the clock passes the request's life while it is pending.
    let r = rig_with(
        FakeState {
            pair: PairMode::Pending,
            pending_polls: 0,
            ..FakeState::default()
        },
        true,
        42,
        1,
    );
    r.link.connect(false).unwrap();
    r.ticks.as_ref().unwrap().send(()).unwrap();
    r.clock.advance(Duration::from_millis(301_000));
    r.ticks.as_ref().unwrap().send(()).unwrap();
    next_event(&r, |_| r.link.view().pairing.is_none());
    assert!(r.link.view().problem.unwrap().contains("expired"));
    assert!(KeyVault::new(r.secrets.clone()).get().is_none());

    // Cancelled by the user: no key, no problem text, the poll ends.
    let r = rig_with(
        FakeState {
            pair: PairMode::Pending,
            pending_polls: 0,
            ..FakeState::default()
        },
        true,
        42,
        1,
    );
    r.link.connect(false).unwrap();
    assert!(r.link.cancel_pairing());
    assert!(!r.link.cancel_pairing());
    assert!(r.link.view().pairing.is_none());
    drop(r.ticks);

    // The credential store is down when the key arrives: said plainly, nothing half-stored.
    let r = rig_with(
        FakeState {
            pending_polls: 0,
            ..FakeState::default()
        },
        true,
        42,
        1,
    );
    r.secrets.set_unavailable(true);
    r.link.connect(false).unwrap();
    r.ticks.as_ref().unwrap().send(()).unwrap();
    next_event(&r, |_| r.link.view().pairing.is_none());
    assert!(
        r.link
            .view()
            .problem
            .unwrap()
            .contains("could not be stored")
    );
}

#[test]
fn connecting_when_pitwall_is_not_running_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let link = Link::new(LinkConfig::new(
        app(),
        Locator::in_dir(dir.path(), None, Arc::new(|_| true)),
        Arc::new(MemorySecrets::new("Debrief")),
    ));
    assert_eq!(link.connect(true).unwrap_err(), PitwallError::NotRunning);
}

#[test]
fn forgetting_the_key_returns_to_needs_pairing() {
    let r = rig(keyed());
    store_key(&r);
    assert!(r.link.probe().usable);
    let v = r.link.forget().unwrap();
    assert!(!v.paired && !v.usable);
    r.secrets.set_unavailable(true);
    assert!(matches!(r.link.forget(), Err(PitwallError::NotReady(_))));
}

#[test]
fn discovery_finds_the_fake_through_its_file_and_ignores_a_damaged_one() {
    let r = rig(keyed());
    let loc = Locator::in_dir(r.dir.path(), None, Arc::new(|p| p == 42));
    assert!(matches!(loc.look(), Found::Discovered(d) if d.port == r.fake.port));
    std::fs::write(r.dir.path().join(FILE_NAME), "{").unwrap();
    assert!(matches!(loc.look(), Found::Guess { note: Some(_), .. }));
    // The well-known-port guess reaches the same server.
    let guess = Locator::in_dir(r.dir.path(), Some(r.fake.port), Arc::new(|_| false));
    store_key(&r);
    let cfg = LinkConfig::new(app(), guess, r.secrets.clone());
    assert!(Link::new(cfg).probe().usable);
}

#[test]
fn an_unreachable_link_changes_state_once_and_tells_the_listener() {
    let r = rig(keyed());
    store_key(&r);
    r.link.probe();
    assert_eq!(next_event(&r, |_| true), LinkEvent::Changed);
    r.link.probe();
    assert!(
        r.events.try_recv().is_err(),
        "the same view announces nothing"
    );
    r.fake.kill();
    let v = r.link.probe();
    assert!(!v.usable);
    assert_eq!(next_event(&r, |_| true), LinkEvent::Changed);
}

// ---------------------------------------------------------------------------------------------------------------
// Health integration: Pitwall is optional, never down, and its repair does not need it

#[test]
fn the_health_entry_follows_the_link_and_repair_works_without_pitwall() {
    let r = rig(keyed());
    store_key(&r);
    let used = Arc::new(Mutex::new(0));
    let u = used.clone();
    let health = Health::new(
        vec![service_spec(SERVICE_ID, "Pitwall", move || {
            *u.lock().unwrap() += 1;
            Ok(())
        })],
        Box::new(|_, _| {}),
    )
    .unwrap();
    let session = Session::new(EngineChoice::Pitwall);
    let state = |r: &Rig| session.state(&r.link);
    assert!(
        report(
            &health,
            SERVICE_ID,
            &state(&r),
            true,
            "Debrief",
            &Fallback::BuiltIn
        ),
        "nothing probed yet: not installed, and wanted"
    );
    r.link.probe();
    report(
        &health,
        SERVICE_ID,
        &state(&r),
        true,
        "Debrief",
        &Fallback::BuiltIn,
    );
    assert_eq!(health.state(SERVICE_ID), Some(ServiceState::Ok));
    r.fake.kill();
    r.link.probe();
    report(
        &health,
        SERVICE_ID,
        &state(&r),
        true,
        "Debrief",
        &Fallback::BuiltIn,
    );
    let entry = health.get(SERVICE_ID).unwrap();
    assert_eq!(
        (entry.state, entry.repair),
        (ServiceState::Degraded, Some(RepairAction::UseFallback))
    );
    health.repair(SERVICE_ID).unwrap();
    // The repair ran on its own thread and did not touch Pitwall (which is gone).
    loop {
        if *used.lock().unwrap() == 1 {
            break;
        }
        std::thread::yield_now();
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Malformed and unfinished answers, odd pairing endings, the fake's own error routes

fn raw_chat(text: &str) -> Result<ChatReply, PitwallError> {
    let fake = FakePitwall::start(FakeState {
        chat: ChatScript::Raw(text.into()),
        ..keyed()
    });
    client(&fake).chat(&chat_req("x"))
}

#[test]
fn unfinished_malformed_and_oversized_streams_are_errors_not_hangs() {
    // The body ends without [DONE] and without a finish reason: a dropped connection.
    assert_eq!(
        raw_chat("data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n").unwrap_err(),
        PitwallError::Stopped
    );
    // The body ends without [DONE] but the finish reason came: complete.
    let ok = raw_chat(
        "data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":\"length\"}]}\n\n",
    )
    .unwrap();
    assert_eq!(
        (ok.text.as_str(), ok.finish_reason.as_deref()),
        ("a", Some("length"))
    );
    // Lines that are not data events are skipped; an event that is not JSON is a protocol error.
    let ok = raw_chat(": keep-alive\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"b\"},\"finish_reason\":\"stop\"}]}\r\n\r\ndata: [DONE]\n\n").unwrap();
    assert_eq!(ok.text, "b");
    assert!(
        matches!(raw_chat("data: nope\n\n"), Err(PitwallError::Protocol(m)) if m.contains("not JSON"))
    );
    // One event larger than the limit.
    let huge = "x".repeat(paddock_pitwall::stream::MAX_EVENT + 10);
    assert!(matches!(raw_chat(&huge), Err(PitwallError::Protocol(m)) if m.contains("too large")));
    // The iterator stops after an error.
    let fake = FakePitwall::start(FakeState {
        chat: ChatScript::Raw("data: nope\n\n".into()),
        ..keyed()
    });
    let mut s = client(&fake).chat_stream(&chat_req("x")).unwrap();
    assert!(s.next().unwrap().is_err());
    assert!(s.next().is_none());
}

#[test]
fn malformed_pairing_and_embedding_answers_are_protocol_errors() {
    let fake = FakePitwall::start(FakeState {
        pair_code: String::new(),
        ..keyed()
    });
    assert!(matches!(
        client(&fake).begin_pairing(),
        Err(PitwallError::Protocol(m)) if m.contains("usable code")
    ));
    for (bad, want) in [(1, "a vector is missing"), (2, "non-number")] {
        let fake = FakePitwall::start(FakeState {
            embed_bad: bad,
            ..keyed()
        });
        let e = client(&fake)
            .embed(&EmbedRequest::new(vec!["a".into()]))
            .unwrap_err();
        assert!(
            matches!(&e, PitwallError::Protocol(m) if m.contains(want)),
            "{e:?}"
        );
    }
    // select names a model the list does not have.
    let fake = FakePitwall::start(FakeState {
        select_model: Some("ghost".into()),
        ..keyed()
    });
    let e = client(&fake)
        .ensure_capability(&ModelSel::Auto(Caps::chat(1024)), "chat")
        .unwrap_err();
    assert!(matches!(e, PitwallError::NoModelFits(m) if m.contains("ghost")));
    // A job cancel Pitwall refuses is an error, not "unknown".
    let fake = FakePitwall::start(FakeState {
        failures: vec![(
            "/x/v1/jobs/7".into(),
            Failure {
                status: 403,
                code: "forbidden".into(),
                message: "not yours".into(),
                retry_after: None,
            },
        )],
        ..keyed()
    });
    assert!(matches!(
        client(&fake).cancel_job("7"),
        Err(PitwallError::Forbidden(_))
    ));
    // Identities and the client's own accessors.
    for (id, name) in [
        ("", "A"),
        ("Debrief", "A"),
        ("a", ""),
        ("a", " A"),
        ("a", "A\n"),
        ("1a", "A"),
    ] {
        assert!(AppIdentity::new(id, name).is_err(), "{id:?} {name:?}");
    }
    let c = client(&fake);
    assert_eq!(
        (c.port(), c.app().id(), c.app().display_name(), c.has_key()),
        (fake.port, "debrief", "Debrief", true)
    );
    assert!(
        !format!("{c:?}").contains("pw_testkey"),
        "the key never prints"
    );
}

#[test]
fn the_fake_refuses_what_pitwall_refuses() {
    use paddock_pitwall::loopback::{Request, send};
    let fake = FakePitwall::start(keyed());
    let call = |method: &str, path: &str, body: &str| {
        let mut req = if method == "POST" {
            Request::post(path, body.as_bytes().to_vec())
        } else {
            Request::get(path)
        };
        req = req.header("Authorization", format!("Bearer {KEY}"));
        let r = send(fake.port, &req, Timeouts::QUICK, None).unwrap();
        (
            r.status,
            String::from_utf8(r.read_all(1 << 16).unwrap()).unwrap(),
        )
    };
    assert_eq!(call("POST", "/x/v1/tokens/count", "{}").0, 400);
    assert_eq!(call("POST", "/x/v1/tokens/truncate", "{}").0, 400);
    assert_eq!(call("POST", "/x/v1/tasks/dance", "{}").0, 404);
    assert_eq!(call("POST", "/x/v1/pair", "{}").0, 400);
    let (status, body) = call("GET", "/nowhere", "");
    assert_eq!(status, 404);
    assert!(body.contains("no such route"));
    // Health and pairing need no key.
    let r = send(
        fake.port,
        &Request::get("/x/v1/health"),
        Timeouts::QUICK,
        None,
    )
    .unwrap();
    assert_eq!(r.status, 200);
}

#[test]
fn a_pairing_that_hiccups_goes_on_and_one_that_loses_pitwall_ends() {
    // One failed poll is not the end of the pairing.
    let r = rig_with(
        FakeState {
            pending_polls: 0,
            failures: vec![(
                "/x/v1/pair/pr1-abc".into(),
                Failure {
                    status: 500,
                    code: "engine_error".into(),
                    message: "hiccup".into(),
                    retry_after: None,
                },
            )],
            ..FakeState::default()
        },
        true,
        42,
        1,
    );
    r.link.connect(true).unwrap();
    let tx = r.ticks.as_ref().unwrap();
    tx.send(()).unwrap();
    tx.send(()).unwrap();
    next_event(&r, |e| matches!(e, LinkEvent::Paired { .. }));
    assert!(r.link.view().usable);

    // Pitwall quits while the code is shown: the pairing ends and says why.
    let r = rig_with(
        FakeState {
            pair: PairMode::Pending,
            pending_polls: 0,
            ..FakeState::default()
        },
        true,
        42,
        1,
    );
    r.link.connect(false).unwrap();
    r.fake.kill();
    r.ticks.as_ref().unwrap().send(()).unwrap();
    next_event(&r, |_| r.link.view().pairing.is_none());
    assert!(r.link.view().problem.unwrap().contains("not running"));

    // Pitwall says the key was delivered earlier: said plainly.
    let r = rig_with(
        FakeState {
            pair: PairMode::ApprovedWithoutKey,
            pending_polls: 0,
            ..FakeState::default()
        },
        true,
        42,
        1,
    );
    r.link.connect(false).unwrap();
    r.ticks.as_ref().unwrap().send(()).unwrap();
    next_event(&r, |_| r.link.view().pairing.is_none());
    assert!(r.link.view().problem.unwrap().contains("already delivered"));

    // Dropping the link ends the poll at once: the thread cannot upgrade its weak reference.
    let r = rig_with(
        FakeState {
            pair: PairMode::Pending,
            pending_polls: 0,
            ..FakeState::default()
        },
        true,
        42,
        1,
    );
    r.link.connect(false).unwrap();
    let Rig { link, ticks, .. } = r;
    drop(link);
    drop(ticks);
}

#[test]
fn a_link_that_has_not_seen_pitwall_says_what_is_missing() {
    // Running but the app holds no key.
    let r = rig(keyed());
    r.link.probe();
    assert_eq!(
        r.link.ready(Priority::Interactive).unwrap_err(),
        PitwallError::NeedsPairing
    );
    // Found only through the fallback port, with no data folder.
    // Not a released port: another process's Pitwall (on CI, a concurrent test's fake) could be handed that one.
    let foreign = hang_up_port();
    assert!(FakePitwall::try_start_on(foreign, keyed()).is_err());
    let nowhere = tempfile::tempdir().unwrap().path().join("gone");
    let link = Link::new(LinkConfig::new(
        app(),
        Locator::in_dir(&nowhere, Some(foreign), Arc::new(|_| false)),
        Arc::new(MemorySecrets::new("Debrief")),
    ));
    assert_eq!(link.probe().found, FoundKind::Absent);
    // A retry policy from the tuning reaches every call.
    let fake = FakePitwall::start(keyed());
    let dir = tempfile::tempdir().unwrap();
    fake.write_discovery(dir.path(), 42, 1);
    let secrets = Arc::new(MemorySecrets::new("Debrief"));
    KeyVault::new(secrets.clone()).set(KEY).unwrap();
    let mut cfg = LinkConfig::new(
        app(),
        Locator::in_dir(dir.path(), None, Arc::new(|p| p == 42)),
        secrets,
    );
    cfg.tuning.retry = Some(Policy::NONE);
    cfg.sleeper = Arc::new(Recorder::default());
    assert!(Link::new(cfg).probe().usable);
}

#[test]
fn routing_respects_the_apps_own_switch() {
    let r = rig(keyed());
    store_key(&r);
    r.link.probe();
    let session = Session::new(EngineChoice::Auto);
    assert!(matches!(
        run_or_fallback(&session, &r.link, "Ask", "Debrief", || 1, || 2),
        Routed::Ran(EngineKind::Pitwall, 1)
    ));
    session.set_disabled(true);
    let Routed::Unavailable(view) =
        run_or_fallback(&session, &r.link, "Ask", "Debrief", || 1, || 2)
    else {
        panic!("AI is off")
    };
    assert_eq!((view.reason, view.actions), (AiState::Disabled, vec![]));
    assert_eq!(session.state(&r.link), AiState::Disabled);
}

#[test]
fn a_cache_bypass_shows_in_the_provenance() {
    let fake = FakePitwall::start(FakeState {
        cache: Some("bypass".into()),
        ..keyed()
    });
    let reply = client(&fake).chat(&chat_req("x")).unwrap();
    assert_eq!(reply.provenance.cache, Some(CacheStatus::Bypass));
}

#[test]
fn a_held_answer_goes_on_when_it_is_released() {
    let fake = FakePitwall::start(FakeState {
        chat: ChatScript::HoldFirst,
        ..keyed()
    });
    let c = client(&fake).with_timeouts(
        Timeouts::QUICK,
        Timeouts {
            connect: Duration::from_secs(1),
            first_byte: Duration::from_secs(300),
            idle: Duration::from_secs(300),
        },
    );
    let h = std::thread::spawn(move || c.chat(&chat_req("x")));
    while fake.requests_to("/v1/chat/completions").is_empty() {
        std::thread::yield_now();
    }
    fake.release();
    assert_eq!(h.join().unwrap().unwrap().text, "late");
}

#[test]
fn the_snapshot_the_shared_component_draws_follows_the_session() {
    let r = rig_with(
        FakeState {
            pair: PairMode::Pending,
            pending_polls: 0,
            ..FakeState::default()
        },
        true,
        42,
        1,
    );
    let session = Session::new(EngineChoice::Auto);
    r.link.probe();
    let s = session.snapshot(&r.link, true);
    assert_eq!(
        (
            s.state.clone(),
            s.actions.clone(),
            s.built_in_available,
            s.built_in_for_session
        ),
        (AiState::NeedsPairing, vec![NextStep::Pair], true, false)
    );
    assert!(s.message.contains("Debrief is not connected"));
    assert_eq!(s.pairing, None);
    r.link.connect(false).unwrap();
    let s = session.snapshot(&r.link, false);
    assert_eq!(s.pairing.as_ref().map(|p| p.code.as_str()), Some("482913"));
    assert!(!s.built_in_available);
    session.use_built_in_for_session();
    assert!(session.snapshot(&r.link, true).built_in_for_session);
    let json = serde_json::to_value(&s).unwrap();
    assert_eq!(
        (
            json["state"]["kind"].as_str(),
            json["builtInAvailable"].as_bool(),
            json["actions"][0].as_str()
        ),
        (Some("needsPairing"), Some(false), Some("pair"))
    );
    assert_eq!(paddock_pitwall::status::COMMANDS[0], "pitwall_state");
    drop(r.ticks);
}

#[test]
fn an_approval_that_comes_after_the_request_expired_is_never_taken() {
    // Approval is ready on the server, but the request's life is over by the time the poll wakes: the app gives up first,
    // asks Pitwall nothing and stores no key.
    let r = rig_with(
        FakeState {
            pending_polls: 0,
            ..FakeState::default()
        },
        true,
        42,
        1,
    );
    r.link.connect(true).unwrap();
    r.clock.advance(Duration::from_millis(300_001));
    r.ticks.as_ref().unwrap().send(()).unwrap();
    next_event(&r, |_| r.link.view().pairing.is_none());
    assert!(r.link.view().problem.unwrap().contains("expired"));
    assert!(
        KeyVault::new(r.secrets.clone()).get().is_none(),
        "no key was taken"
    );
    assert!(
        r.fake.requests_to("/x/v1/pair/pr1-abc").is_empty(),
        "Pitwall was not even asked"
    );
    assert!(!r.link.view().paired);
    // A fresh request after the expiry works as a first one.
    let v = r.link.connect(true).unwrap();
    assert!(v.pairing.is_some());
    assert_eq!(r.fake.state().pairs, 2);
}

#[test]
fn a_polled_pairing_answers_pending_again_once_its_key_was_delivered() {
    let fake = FakePitwall::start(FakeState {
        pending_polls: 0,
        ..keyed()
    });
    let c = client(&fake);
    assert_eq!(
        c.poll_pairing("pr1-abc").unwrap(),
        PairPoll::Approved(Some("pw_fakekey".into()))
    );
    assert_eq!(c.poll_pairing("pr1-abc").unwrap(), PairPoll::Pending);
}

#[test]
fn every_keyed_call_refuses_a_client_without_a_key_the_same_way() {
    let fake = FakePitwall::start(keyed());
    let c = Client::new(fake.port, app());
    let no = PitwallError::NeedsPairing;
    assert_eq!(c.select(&ModelSel::Auto(Caps::chat(1024))).unwrap_err(), no);
    assert_eq!(c.models().unwrap_err(), no);
    assert_eq!(c.model_ids().unwrap_err(), no);
    assert_eq!(c.jobs().unwrap_err(), no);
    assert_eq!(c.cancel_job("1").unwrap_err(), no);
    assert_eq!(c.count_tokens("x").unwrap_err(), no);
    assert_eq!(c.truncate("x", 1, Keep::Start).unwrap_err(), no);
    assert_eq!(
        c.task(
            Task::Summarize,
            &serde_json::json!({"text": "x"}),
            &CallOptions::default()
        )
        .unwrap_err(),
        no
    );
    assert_eq!(
        c.embed(&EmbedRequest::new(vec!["x".into()])).unwrap_err(),
        no
    );
    assert_eq!(c.chat(&chat_req("x")).unwrap_err(), no);
    assert_eq!(
        c.ensure_capability(&ModelSel::Auto(Caps::chat(1024)), "chat")
            .unwrap_err(),
        no
    );
}

#[test]
fn a_killed_fake_refuses_connections_at_once() {
    // Pitwall quitting means the port is closed when `kill` returns, not shortly after: a request in between would be
    // accepted and dropped, which reads as "stopped answering" instead of "not running". `kill` joins the accept loop,
    // which owns the listener, so a few rounds are enough; each refused connection costs about two seconds on Windows.
    for _ in 0..3 {
        let fake = FakePitwall::start(FakeState::default());
        fake.kill();
        assert!(
            std::net::TcpStream::connect(("127.0.0.1", fake.port)).is_err(),
            "the port still accepts after kill"
        );
    }
}
