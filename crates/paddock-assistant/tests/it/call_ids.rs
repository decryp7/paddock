//! Tool-call ids stay unique over the rounds of a turn and over the conversation (ADR 0023 WP-2a): the prompted protocol
//! numbers calls per answer (`call_1` again in every round) and a native engine may send none, while the cards, the approval
//! gate and the results are keyed by the id. A scripted provider, no sleeps.

use crate::support;
use paddock_assistant::*;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use support::*;

fn counter() -> Arc<AtomicUsize> {
    Arc::new(AtomicUsize::new(0))
}

#[test]
fn call_ids_stay_unique_over_rounds_so_each_card_and_result_is_its_own() {
    let r = rig_with(
        vec![
            // The prompted protocol numbers calls per answer, so every round starts again at call_1; a native engine may
            // send no id at all.
            Step::Reply(
                vec![],
                calls("", vec![call("call_1", "echo", json!({"word": "a"}))]),
            ),
            Step::Reply(
                vec![],
                calls(
                    "",
                    vec![
                        call("call_1", "echo", json!({"word": "b"})),
                        call("", "echo", json!({"word": "c"})),
                    ],
                ),
            ),
            Step::Reply(vec![], text("three echoes")),
        ],
        vec![Arc::new(Echo(counter()))],
        |b| b.max_tool_calls = 10,
    );
    let (_, events) = r.turn(ask("go"));
    let started: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            AssistantEvent::ToolStarted { call_id, .. } => Some(call_id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(started.len(), 3);
    let mut unique = started.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 3, "ids {started:?}");
    assert_eq!(started[0], "call_1", "an id that is free is kept");
    // What the model reads next pairs each result with the call it answers, under the new ids.
    let seen = r.provider.seen.lock().unwrap();
    let last = &seen[2].messages;
    let asked: Vec<&str> = last
        .iter()
        .flat_map(|m| m.tool_calls.iter().map(|c| c.id.as_str()))
        .collect();
    let answered: Vec<&str> = last
        .iter()
        .filter_map(|m| m.tool_call_id.as_deref())
        .collect();
    assert_eq!(asked, answered);
    assert_eq!(
        asked,
        started.iter().map(String::as_str).collect::<Vec<_>>()
    );
}

#[test]
fn ids_are_unique_against_the_conversation_history_too() {
    let r = rig_with(
        vec![
            Step::Reply(
                vec![],
                calls("", vec![call("call_1", "echo", json!({"word": "a"}))]),
            ),
            Step::Reply(vec![], text("one")),
            Step::Reply(
                vec![],
                calls("", vec![call("call_1", "echo", json!({"word": "b"}))]),
            ),
            Step::Reply(vec![], text("two")),
        ],
        vec![Arc::new(Echo(counter()))],
        |_| {},
    );
    let (id, _) = r.turn(ask("first"));
    let (_, events) = r.turn(SendRequest {
        conversation_id: Some(id.clone()),
        ..ask("second")
    });
    let second = events
        .iter()
        .find_map(|e| match e {
            AssistantEvent::ToolStarted { call_id, .. } => Some(call_id.clone()),
            _ => None,
        })
        .unwrap();
    assert_ne!(second, "call_1");
    let stored = r.assistant.load(&id).unwrap();
    let ids: Vec<&str> = stored
        .messages
        .iter()
        .filter_map(|m| m.tool_call_id.as_deref())
        .collect();
    assert_eq!(ids, ["call_1", second.as_str()]);
}
