//! The tool path of ADR 0023 WP-2a (Pitwall 1.1): the approval policy port, optional wall limits, a tool set per turn,
//! native tool calls, and call ids that stay unique over the rounds of a conversation. A scripted provider, a fake clock,
//! no sleeps.

use crate::support;
use paddock_assistant::*;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::*;

fn counter() -> Arc<AtomicUsize> {
    Arc::new(AtomicUsize::new(0))
}

/// Decision 8 of Pitwall's ADR 0023 in miniature: a read-only tool asks the first time and offers "Always allow"; a
/// remembered tool runs without asking; a tool that writes asks every call. Records every choice it is told about.
#[derive(Default)]
struct AskFirst {
    remembered: Mutex<HashMap<String, bool>>,
    told: Mutex<Vec<(String, ApprovalChoice)>>,
}

impl ApprovalPolicy for AskFirst {
    fn check(&self, call: &CallInfo<'_>) -> Verdict {
        if call.access.writes() {
            return Verdict::AskEveryTime;
        }
        if self
            .remembered
            .lock()
            .unwrap()
            .contains_key(&call.tool.name)
        {
            Verdict::Remembered
        } else {
            Verdict::AskOrRemember
        }
    }

    fn record(&self, call: &CallInfo<'_>, choice: ApprovalChoice) {
        self.told
            .lock()
            .unwrap()
            .push((call.call.name.clone(), choice));
        if choice == ApprovalChoice::AlwaysAllow {
            self.remembered
                .lock()
                .unwrap()
                .insert(call.tool.name.clone(), true);
        }
    }
}

fn approval_of(events: &[AssistantEvent], call_id: &str) -> Option<Approval> {
    events.iter().find_map(|e| match e {
        AssistantEvent::ToolFinished {
            call_id: id,
            approval,
            ..
        } if id == call_id => Some(*approval),
        _ => None,
    })?
}

#[test]
fn the_default_policy_keeps_todays_behaviour_read_only_runs_unasked_and_a_write_asks_without_always()
 {
    let runs = counter();
    let r = rig_with(
        vec![
            Step::Reply(
                vec![],
                calls(
                    "",
                    vec![
                        call("c1", "echo", json!({"word": "a"})),
                        call("c2", "note", json!({})),
                    ],
                ),
            ),
            Step::Reply(vec![], text("done")),
        ],
        vec![Arc::new(Echo(counter())), Arc::new(Note(runs.clone()))],
        |_| {},
    );
    let ack = r
        .send(SendRequest {
            allow_write: true,
            ..ask("go")
        })
        .unwrap();
    let seen = until(&r.rx, |e| {
        matches!(e, AssistantEvent::ApprovalNeeded { .. })
    });
    assert!(
        matches!(seen.last().unwrap(), AssistantEvent::ApprovalNeeded { call_id, remember: false, .. } if call_id == "c2"),
        "a write asks every time: no Always allow"
    );
    assert_eq!(approval_of(&seen, "c1"), Some(Approval::NotAsked));
    assert!(
        r.assistant
            .approve(&ack.conversation_id, "c2", ApprovalChoice::AlwaysAllow)
            == Err(AssistantError::NotOffered),
        "Always allow was not offered, so the engine refuses it"
    );
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    r.assistant
        .approve(&ack.conversation_id, "c2", ApprovalChoice::AllowOnce)
        .unwrap();
    let events = until_end(&r.rx);
    assert_eq!(approval_of(&events, "c2"), Some(Approval::Once));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    let stored = r.assistant.load(&ack.conversation_id).unwrap();
    let approvals: Vec<Option<Approval>> = stored
        .messages
        .iter()
        .filter(|m| m.role == Role::Tool)
        .map(|m| m.approval)
        .collect();
    assert_eq!(
        approvals,
        [Some(Approval::NotAsked), Some(Approval::Once)],
        "a reopened chat shows how each call was approved"
    );
}

#[test]
fn a_policy_asks_the_first_time_remembers_always_allow_and_asks_an_action_every_call() {
    let echoes = counter();
    let notes = counter();
    let policy = Arc::new(AskFirst::default());
    let r = rig_cfg(
        vec![
            Step::Reply(
                vec![],
                calls("", vec![call("c1", "echo", json!({"word": "a"}))]),
            ),
            Step::Reply(
                vec![],
                calls(
                    "",
                    vec![
                        call("c2", "echo", json!({"word": "b"})),
                        call("c3", "note", json!({})),
                    ],
                ),
            ),
            Step::Reply(vec![], calls("", vec![call("c4", "note", json!({}))])),
            Step::Reply(vec![], text("done")),
        ],
        vec![
            Arc::new(Echo(echoes.clone())),
            Arc::new(Note(notes.clone())),
        ],
        |cfg| cfg.policy = policy.clone(),
    );
    let ack = r
        .send(SendRequest {
            allow_write: true,
            ..ask("go")
        })
        .unwrap();
    let id = ack.conversation_id;

    // First call of a read-only tool: asked, with Always allow offered; nothing runs before the click.
    let seen = until(&r.rx, |e| {
        matches!(e, AssistantEvent::ApprovalNeeded { .. })
    });
    assert!(
        matches!(seen.last().unwrap(), AssistantEvent::ApprovalNeeded { call_id, remember: true, destructive: false, .. } if call_id == "c1")
    );
    assert_eq!(echoes.load(Ordering::SeqCst), 0);
    r.assistant
        .approve(&id, "c1", ApprovalChoice::AlwaysAllow)
        .unwrap();

    // Second call of it: remembered, runs unasked. The action asks, without Always allow.
    let seen = until(&r.rx, |e| {
        matches!(e, AssistantEvent::ApprovalNeeded { .. })
    });
    assert_eq!(approval_of(&seen, "c1"), Some(Approval::Always));
    assert_eq!(approval_of(&seen, "c2"), Some(Approval::Remembered));
    assert_eq!(echoes.load(Ordering::SeqCst), 2);
    assert!(
        matches!(seen.last().unwrap(), AssistantEvent::ApprovalNeeded { call_id, remember: false, .. } if call_id == "c3")
    );
    r.assistant
        .approve(&id, "c3", ApprovalChoice::AllowOnce)
        .unwrap();

    // The action's next call asks again (D2); Deny tells the model and the turn goes on.
    let seen = until(&r.rx, |e| {
        matches!(e, AssistantEvent::ApprovalNeeded { .. })
    });
    assert_eq!(approval_of(&seen, "c3"), Some(Approval::Once));
    assert!(
        matches!(seen.last().unwrap(), AssistantEvent::ApprovalNeeded { call_id, .. } if call_id == "c4")
    );
    r.assistant
        .approve(&id, "c4", ApprovalChoice::Deny)
        .unwrap();
    let events = until_end(&r.rx);
    assert_eq!(done(&events).0, "done");
    assert!(events.iter().any(|e| matches!(
        e,
        AssistantEvent::ToolFinished {
            call_id,
            outcome: Some(ToolOutcome::Declined),
            approval: None,
            ..
        } if call_id == "c4"
    )));
    assert_eq!(notes.load(Ordering::SeqCst), 1);
    assert!(
        r.provider.seen.lock().unwrap()[3]
            .messages
            .last()
            .unwrap()
            .content
            .contains("declined")
    );
    assert_eq!(
        *policy.told.lock().unwrap(),
        [
            ("echo".to_string(), ApprovalChoice::AlwaysAllow),
            ("note".to_string(), ApprovalChoice::AllowOnce),
            ("note".to_string(), ApprovalChoice::Deny),
        ],
        "the policy hears every choice the user made, and nothing for calls it let run"
    );
}

#[test]
fn a_call_the_policy_never_saw_is_refused_before_the_policy_malformed_unknown_and_bad_arguments() {
    let policy = Arc::new(AskFirst::default());
    let r = rig_cfg(
        vec![
            Step::Reply(
                vec![],
                calls(
                    "",
                    vec![
                        call("c1", "", json!({"_invalid_json": "{"})),
                        call("c2", "nope", json!({})),
                        call("c3", "echo", json!({})),
                    ],
                ),
            ),
            Step::Reply(vec![], text("ok")),
        ],
        vec![Arc::new(Echo(counter()))],
        |cfg| cfg.policy = policy.clone(),
    );
    let (_, events) = r.turn(ask("go"));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, AssistantEvent::ApprovalNeeded { .. })),
        "no card for a call that cannot run"
    );
    for id in ["c1", "c2", "c3"] {
        assert_eq!(approval_of(&events, id), None);
    }
    assert!(policy.told.lock().unwrap().is_empty());
}

#[test]
fn without_wall_limits_neither_the_turn_nor_an_approval_ends_on_the_clock() {
    let runs = counter();
    let r = rig_cfg(
        vec![
            Step::Reply(vec![], calls("", vec![call("c2", "note", json!({}))])),
            Step::Reply(vec![], text("finally")),
        ],
        vec![Arc::new(Note(runs.clone()))],
        |cfg| {
            cfg.budgets.max_wall = None;
            cfg.budgets.approval_wait = None;
        },
    );
    let ack = r
        .send(SendRequest {
            allow_write: true,
            ..ask("go")
        })
        .unwrap();
    until(&r.rx, |e| {
        matches!(e, AssistantEvent::ApprovalNeeded { .. })
    });
    // A user at lunch: hours pass on the turn's clock while the card waits.
    r.clock.advance(Duration::from_secs(4 * 3600));
    assert_eq!(
        r.assistant
            .approve(&ack.conversation_id, "c2", ApprovalChoice::AllowOnce),
        Ok(()),
        "still waiting after four hours"
    );
    let events = until_end(&r.rx);
    let (text, end, _) = done(&events);
    assert_eq!((text.as_str(), end), ("finally", EndReason::Answered));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[test]
fn without_a_wall_limit_a_slow_tool_does_not_end_the_turn() {
    let dir = tempfile::tempdir().unwrap();
    let fake = paddock_health::FakeClock::at_wall_ms(1_700_000_000_000);
    let provider = Scripted::new(vec![
        Step::Reply(
            vec![],
            calls(
                "",
                vec![call("c1", "slow", json!({})), call("c2", "slow", json!({}))],
            ),
        ),
        Step::Reply(vec![], text("slow but done")),
    ]);
    let store = Arc::new(ConversationStore::open(
        dir.path().join("a.json"),
        Arc::new(fake.clone()),
    ));
    let mut cfg = AssistantConfig::new("Demo", "sys");
    cfg.budgets.max_wall = None;
    let tools = Tools::new(vec![Arc::new(Slow(fake.clone()))]).unwrap();
    let assistant = Assistant::new(provider, tools, store, Arc::new(fake), cfg);
    let (sink, rx) = sink();
    assistant.send(ask("go"), sink).unwrap();
    let (text, end, _) = done(&until_end(&rx));
    assert_eq!((text.as_str(), end), ("slow but done", EndReason::Answered));
}

#[test]
fn the_tool_set_can_change_between_turns_and_a_running_turn_keeps_its_own() {
    let echoes = counter();
    let erased = counter();
    let swapped: Arc<Mutex<Option<Arc<Assistant>>>> = Arc::default();
    let hook_target = swapped.clone();
    let erase_runs = erased.clone();
    let r = rig_with(
        vec![
            // While turn 1 runs, the app's connections change: the turn still finishes on the tools it started with.
            Step::Hook(
                Box::new(move || {
                    let a = hook_target.lock().unwrap().clone().unwrap();
                    a.set_tools(Tools::new(vec![Arc::new(Erase(erase_runs.clone()))]).unwrap());
                }),
                calls("", vec![call("c1", "echo", json!({"word": "a"}))]),
            ),
            Step::Reply(vec![], text("one")),
            Step::Reply(vec![], text("two")),
        ],
        vec![Arc::new(Echo(echoes.clone()))],
        |_| {},
    );
    *swapped.lock().unwrap() = Some(r.assistant.clone());
    let (id, events) = r.turn(ask("first"));
    assert_eq!(done(&events).0, "one");
    assert_eq!(echoes.load(Ordering::SeqCst), 1, "the old tool still ran");
    assert_eq!(r.provider.tool_names(0), ["echo"]);
    assert_eq!(r.provider.tool_names(1), ["echo"]);

    // The next turn and `info` see the new set.
    assert_eq!(
        r.assistant
            .info()
            .tools
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        ["erase"]
    );
    assert!(r.assistant.info().has_write);
    let (_, events) = r.turn(SendRequest {
        conversation_id: Some(id),
        allow_write: true,
        ..ask("second")
    });
    assert_eq!(done(&events).0, "two");
    assert_eq!(r.provider.tool_names(2), ["erase"]);

    // No tools at all: the model is offered none.
    r.assistant.set_tools(Tools::none());
    assert!(r.assistant.info().tools.is_empty());
}

#[test]
fn a_native_tool_call_is_parsed_strictly_and_a_bad_one_is_reported_not_run() {
    let ok = ToolCall::native("abc", "echo", r#"{"word":"a"}"#);
    assert_eq!(ok, call("abc", "echo", json!({"word": "a"})));
    assert!(!ok.is_malformed());
    assert_eq!(
        ToolCall::native("abc", "echo", "  "),
        call("abc", "echo", json!({})),
        "no arguments is an empty object"
    );
    for raw in [r#"{"word": "a""#, "not json", r#"{"a":1} trailing"#] {
        let bad = ToolCall::native("abc", "echo", raw);
        assert!(bad.is_malformed(), "{raw}");
        assert_eq!(bad.input["_invalid_json"], raw);
        assert_eq!(bad.id, "abc");
    }
    assert!(
        ToolCall::native("abc", "", "{}").is_malformed(),
        "a call without a name"
    );
    // A huge argument string is not parsed and is cut in the marker.
    let huge = format!("{{\"word\":\"{}\"}}", "x".repeat(protocol::MAX_CALL_LEN));
    let bad = ToolCall::native("abc", "echo", &huge);
    assert!(bad.is_malformed());
    assert!(bad.input["_invalid_json"].as_str().unwrap().chars().count() <= 500);
    // A JSON value that is not an object is kept, and the loop's argument check refuses it.
    assert_eq!(ToolCall::native("abc", "echo", "[1]").input, json!([1]));

    // Through the loop: the bad native call never runs and never reaches an approval.
    let runs = counter();
    let r = rig_with(
        vec![
            Step::Reply(
                vec![],
                calls("", vec![ToolCall::native("n1", "echo", "{oops")]),
            ),
            Step::Reply(vec![], text("sorry")),
        ],
        vec![Arc::new(Echo(runs.clone()))],
        |_| {},
    );
    let (_, events) = r.turn(ask("go"));
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AssistantEvent::Notice { .. }))
    );
    assert_eq!(done(&events).0, "sorry");
}

#[test]
fn the_config_prints_without_its_policy_and_the_refusal_reads_well() {
    let cfg = AssistantConfig::new("Demo", "sys");
    let printed = format!("{cfg:?}");
    assert!(
        printed.contains("Demo") && printed.contains("history_chars"),
        "{printed}"
    );
    assert_eq!(
        AssistantError::NotOffered.to_string(),
        "This tool asks every time it is used, so it cannot be always allowed."
    );
}

/// A tool with a person-facing title (an MCP tool's `title`) shows it on its card; one without shows its name.
struct Titled;

impl AssistantTool for Titled {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "dyno_top_processes".into(),
            description: "d".into(),
            input_schema: json!({"type": "object"}),
        }
    }
    fn title(&self) -> Option<String> {
        Some("Top processes".into())
    }
    fn call(&self, _: &serde_json::Value, _: &Cancel) -> ToolOutput {
        ToolOutput::ok("1. build")
    }
}

#[test]
fn a_tools_title_reaches_its_card_and_the_tool_list() {
    let r = rig_with(
        vec![
            Step::Reply(
                vec![],
                calls("", vec![call("c1", "dyno_top_processes", json!({}))]),
            ),
            Step::Reply(vec![], text("ok")),
        ],
        vec![Arc::new(Titled), Arc::new(Echo(counter()))],
        |_| {},
    );
    let (_, events) = r.turn(ask("go"));
    assert!(events.iter().any(|e| matches!(
        e,
        AssistantEvent::ToolStarted { title: Some(t), .. } if t == "Top processes"
    )));
    let infos = r.assistant.info().tools;
    assert_eq!(
        infos
            .iter()
            .find(|t| t.name == "dyno_top_processes")
            .unwrap()
            .title
            .as_deref(),
        Some("Top processes")
    );
    assert_eq!(infos.iter().find(|t| t.name == "echo").unwrap().title, None);
}
