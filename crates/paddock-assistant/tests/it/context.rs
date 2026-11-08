//! How full the model's context is (ADR 0023 WP-4b, the composer's meter): the engine measures it, the pane only shows it. After
//! a turn the count is the engine's own (the last model call's prompt plus answer); before any turn of this session it is an
//! estimate over the system prompt, the tools offered and the history, and says so. The level (ok, close to the limit, full)
//! is decided here, never in the UI. The room for the conversation is the model's window less a reserve for the answer
//! (`Budgets::max_answer_tokens`); attachments count through the app's hook; a turn that cannot fit is refused in words before
//! anything is sent or kept (user decision 2026-11-08: nothing is dropped silently). A scripted provider whose window is 8192
//! tokens and the default 1024-token answer reserve, so 7168 tokens of room; no sleeps.

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
    let none = bare.assistant.context(None, &[]);
    assert!(!none.exact);
    assert_eq!((none.limit_tokens, none.reserve_tokens), (7168, 1024));
    // "You are the demo assistant." is 27 characters: about 7 tokens (rounded up).
    assert_eq!(none.used_tokens, 7);
    assert_eq!(none.level, ContextLevel::Ok);

    let tooled = rig_with(
        vec![],
        vec![Arc::new(Echo(Arc::new(AtomicUsize::new(0))))],
        |_| {},
    );
    let with_tools = tooled.assistant.context(None, &[]);
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
    let used = r.assistant.context(Some(&id), &[]);
    assert_eq!(
        used,
        ContextUse {
            used_tokens: 6600,
            limit_tokens: 7168,
            reserve_tokens: 1024,
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
    let used = r.assistant.context(Some(&id), &[]);
    assert!(!used.exact);
    assert!((100..140).contains(&used.used_tokens), "{used:?}");
    // A conversation that does not exist is measured as a new one.
    assert_eq!(
        r.assistant.context(Some("nope"), &[]),
        r.assistant.context(None, &[])
    );
}

#[test]
fn deleting_a_chat_forgets_its_count() {
    let r = rig(vec![Step::Reply(vec![], reply(5000, 10))]);
    let (id, events) = r.turn(ask("A question"));
    done(&events);
    assert!(r.assistant.context(Some(&id), &[]).exact);
    r.assistant.delete(&id).unwrap();
    assert!(!r.assistant.context(Some(&id), &[]).exact);
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

fn file(sha: char, name: &str) -> Attachment {
    Attachment::File {
        sha256: std::iter::repeat_n(sha, 64).collect(),
        name: name.into(),
    }
}

/// The app knows its files' sizes: a 40 KiB text file is about 10k tokens, `b` files are 400 bytes.
fn sized(cfg: &mut AssistantConfig) {
    cfg.attachment_tokens = Arc::new(|a: &Attachment| match a {
        Attachment::File { sha256, .. } if sha256.starts_with('a') => estimate_tokens(40 * 1024),
        Attachment::File { .. } => estimate_tokens(400),
        _ => 0,
    });
}

#[test]
fn files_in_the_history_and_files_about_to_be_sent_count() {
    let r = rig_cfg(
        vec![Step::Reply(
            vec![],
            ChatResponse {
                usage: None,
                ..text("ok")
            },
        )],
        vec![],
        |c| {
            sized(c);
            c.budgets.max_answer_tokens = 512;
        },
    );
    // A small file first, so the turn fits; its estimate is in the conversation's count afterwards.
    let (id, events) = r.turn(SendRequest {
        attachments: vec![file('b', "small.txt")],
        ..ask("Read this")
    });
    done(&events);
    let with_small = r.assistant.context(Some(&id), &[]);
    assert!(with_small.used_tokens >= 100, "{with_small:?}");
    // A file waiting in the composer moves the meter before it is sent: 40 KiB is at least 10k tokens.
    let pending = r.assistant.context(Some(&id), &[file('a', "big.log")]);
    assert!(
        pending.used_tokens >= with_small.used_tokens + 10_000,
        "{pending:?}"
    );
    assert_eq!(pending.level, ContextLevel::Full);
}

#[test]
fn a_conversation_holding_a_40_kib_file_estimates_at_least_10k_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let clock = paddock_health::FakeClock::at_wall_ms(1);
    let store = Arc::new(ConversationStore::open(
        dir.path().join("a.json"),
        Arc::new(clock.clone()),
    ));
    let (id, _, _) = store.create(
        StoredMessage::user("Summarise", vec![], 1).with_attachments(vec![file('a', "big.log")]),
    );
    let mut cfg = AssistantConfig::new("Demo", "S");
    sized(&mut cfg);
    let assistant = Assistant::new(
        Scripted::new(vec![]),
        Tools::none(),
        store,
        Arc::new(clock),
        cfg,
    );
    let used = assistant.context(Some(&id), &[]);
    assert!(!used.exact);
    assert!(used.used_tokens >= 10_000, "{used:?}");
}

#[test]
fn a_turn_that_cannot_fit_is_refused_in_words_before_anything_is_sent_or_kept() {
    let r = rig_cfg(vec![], vec![], sized);
    let refused = r
        .send(SendRequest {
            attachments: vec![file('a', "big.log")],
            ..ask("Summarise this log")
        })
        .unwrap_err();
    let words = refused.to_string();
    assert!(
        words.contains("too long for the model") && words.contains("Remove a file"),
        "{words}"
    );
    assert!(words.contains("7168"), "says how much fits: {words}");
    assert!(
        r.provider.seen.lock().unwrap().is_empty(),
        "nothing reached the model"
    );
    assert!(r.assistant.conversations().is_empty(), "nothing was kept");
}

#[test]
fn a_follow_up_that_would_overflow_the_window_is_refused_too() {
    let r = rig_cfg(vec![Step::Reply(vec![], reply(7100, 100))], vec![], |_| {});
    let (id, events) = r.turn(ask("First"));
    done(&events);
    assert_eq!(
        r.assistant.context(Some(&id), &[]).level,
        ContextLevel::Full
    );
    let words = r
        .send(SendRequest {
            conversation_id: Some(id.clone()),
            ..ask("And then?")
        })
        .unwrap_err()
        .to_string();
    assert!(
        words.contains("This chat is too long for the model"),
        "{words}"
    );
    assert_eq!(
        r.assistant.load(&id).unwrap().messages.len(),
        2,
        "the refused question is not kept"
    );
}

#[test]
fn the_whole_chat_reaches_the_model_when_the_app_reads_all_of_it() {
    let long = "x".repeat(5000);
    let steps = (0..4).map(|_| Step::Reply(vec![], text("ok"))).collect();
    let r = rig_cfg(steps, vec![], |c| c.history_chars = usize::MAX);
    let (id, events) = r.turn(ask(&long));
    done(&events);
    for q in ["two", "three", "four"] {
        let (_, events) = r.turn(SendRequest {
            conversation_id: Some(id.clone()),
            ..ask(q)
        });
        done(&events);
    }
    let seen = r.provider.seen.lock().unwrap();
    assert_eq!(
        seen[3].messages.len(),
        7,
        "every message so far, the first 5000 characters included"
    );
    assert_eq!(seen[3].messages[0].content, long);
}
