//! How full the model's context is (ADR 0023 WP-4b, the composer's meter): the engine measures it, the pane only shows it. After
//! a turn the count is the engine's own (the last model call's prompt plus answer); before any turn of this session it is an
//! estimate over the system prompt, the tools offered and the history, and says so. The level (ok, close to the limit, full)
//! is decided here, never in the UI. A scripted provider whose window is 8192 tokens; no sleeps.

use crate::support::*;
use paddock_assistant::*;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

fn reply(input: u64, output: u64) -> ChatResponse {
    ChatResponse {
        usage: Some(Usage {
            input_tokens: input,
            output_tokens: output,
        }),
        ..text("an answer")
    }
}

#[test]
fn a_new_chat_is_an_estimate_of_the_system_prompt_and_the_tools() {
    let bare = rig(vec![]);
    let none = bare.assistant.context(None);
    assert!(!none.exact);
    assert_eq!(none.limit_tokens, 8192);
    // "You are the demo assistant." is 27 characters: about 7 tokens (rounded up).
    assert_eq!(none.used_tokens, 7);
    assert_eq!(none.level, ContextLevel::Ok);

    let tooled = rig_with(
        vec![],
        vec![Arc::new(Echo(Arc::new(AtomicUsize::new(0))))],
        |_| {},
    );
    let with_tools = tooled.assistant.context(None);
    assert!(
        with_tools.used_tokens > none.used_tokens,
        "the tools' definitions take room on every turn: {with_tools:?}"
    );
}

#[test]
fn after_a_turn_the_count_is_the_engines_own_for_the_last_model_call() {
    let r = rig_with(
        vec![
            Step::Reply(
                vec![],
                calls(
                    "",
                    vec![call("c1", "echo", serde_json::json!({"word": "hi"}))],
                ),
            ),
            // The last call of the turn: its prompt and its answer are what the next question carries.
            Step::Reply(vec![], reply(6400, 200)),
        ],
        vec![Arc::new(Echo(Arc::new(AtomicUsize::new(0))))],
        |_| {},
    );
    let (id, events) = r.turn(ask("How busy is this computer?"));
    done(&events);
    let used = r.assistant.context(Some(&id));
    assert_eq!(
        used,
        ContextUse {
            used_tokens: 6600,
            limit_tokens: 8192,
            exact: true,
            level: ContextLevel::High,
        }
    );
}

#[test]
fn a_chat_reopened_without_a_turn_is_estimated_from_its_history() {
    let r = rig(vec![Step::Reply(
        vec![],
        ChatResponse {
            usage: None,
            ..text(&"x".repeat(400))
        },
    )]);
    let (id, events) = r.turn(ask("A question"));
    done(&events);
    // No count from the engine: the estimate covers the system prompt, the question and the 400-character answer.
    let used = r.assistant.context(Some(&id));
    assert!(!used.exact);
    assert!((100..140).contains(&used.used_tokens), "{used:?}");
    // A conversation that does not exist is measured as a new one.
    assert_eq!(r.assistant.context(Some("nope")), r.assistant.context(None));
}

#[test]
fn deleting_a_chat_forgets_its_count() {
    let r = rig(vec![Step::Reply(vec![], reply(5000, 10))]);
    let (id, events) = r.turn(ask("A question"));
    done(&events);
    assert!(r.assistant.context(Some(&id)).exact);
    r.assistant.delete(&id).unwrap();
    assert!(!r.assistant.context(Some(&id)).exact);
}

#[test]
fn the_level_is_decided_by_the_engine() {
    assert_eq!(context_level(0, 0), ContextLevel::Unknown);
    assert_eq!(context_level(10_000, 0), ContextLevel::Unknown);
    assert_eq!(context_level(0, 8192), ContextLevel::Ok);
    assert_eq!(context_level(6553, 8192), ContextLevel::Ok);
    // 80 % and more: close to the limit.
    assert_eq!(context_level(6554, 8192), ContextLevel::High);
    assert_eq!(context_level(8191, 8192), ContextLevel::High);
    // At the limit the next question cannot fit: full.
    assert_eq!(context_level(8192, 8192), ContextLevel::Full);
    assert_eq!(context_level(u64::MAX, 8192), ContextLevel::Full);
    assert_eq!(context_level(u64::MAX, u64::MAX), ContextLevel::Full);
}

#[test]
fn estimates_round_up_and_never_overflow() {
    assert_eq!(estimate_tokens(0), 0);
    assert_eq!(estimate_tokens(1), 1);
    assert_eq!(estimate_tokens(4), 1);
    assert_eq!(estimate_tokens(5), 2);
    assert_eq!(estimate_tokens(usize::MAX), (usize::MAX as u64).div_ceil(4));
}
