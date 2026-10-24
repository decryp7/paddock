//! Conversation persistence: bounded, versioned, damaged files fall back to defaults with one problem, newer files are
//! left untouched.

use crate::support;
use paddock_assistant::*;
use paddock_health::FakeClock;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use support::*;

fn open(path: &Path) -> (ConversationStore, FakeClock) {
    let clock = FakeClock::at_wall_ms(1_700_000_000_000);
    (
        ConversationStore::open(path, Arc::new(clock.clone())),
        clock,
    )
}

fn user(t: &str) -> StoredMessage {
    StoredMessage::user(t, vec![], 1)
}

#[test]
fn conversations_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.json");
    let (store, _) = open(&path);
    let (id, title, saved) = store.create(user("  A   question\nover lines "));
    assert_eq!((title.as_str(), saved), ("A question over lines", Ok(())));
    store
        .append(
            &id,
            vec![StoredMessage::assistant(
                "An answer",
                vec![],
                Some(EndReason::Answered),
                2,
            )],
        )
        .unwrap()
        .unwrap();
    assert!(store.append("missing", vec![]).is_none());
    drop(store);
    let (store, _) = open(&path);
    assert_eq!(store.take_problem(), None);
    assert_eq!(store.get(&id).unwrap().messages.len(), 2);
    assert!(store.get("missing").is_none());
    assert_eq!(store.delete("missing"), Ok(false));
}

#[test]
fn a_corrupt_file_gives_defaults_one_problem_and_is_kept_aside() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.json");
    std::fs::write(&path, "{ this is not json").unwrap();
    let (store, _) = open(&path);
    assert!(store.summaries().is_empty());
    let problem = store.take_problem().expect("one problem");
    assert!(problem.starts_with("The saved conversations:"), "{problem}");
    assert_eq!(store.take_problem(), None, "told once");
    assert!(
        std::fs::read_dir(dir.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("corrupt"))
    );
    // The store works and saves again.
    let (_, _, saved) = store.create(user("fresh"));
    assert_eq!(saved, Ok(()));
}

#[test]
fn a_newer_file_is_left_untouched_and_the_assistant_still_answers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("assistant.json");
    let newer = json!({"version": 99, "conversations": [], "future": true}).to_string();
    std::fs::write(&path, &newer).unwrap();
    let r = rig_in(
        dir,
        vec![Step::Reply(vec![], text("still here"))],
        vec![],
        |_| {},
    );
    assert!(
        r.assistant
            .store()
            .take_problem()
            .unwrap()
            .contains("newer version")
    );
    let (id, events) = r.turn(ask("hello"));
    assert!(events.iter().any(|e| matches!(e, AssistantEvent::Notice { message } if message.contains("could not be saved"))));
    assert_eq!(done(&events).0, "still here");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        newer,
        "the newer file is never touched"
    );
    assert_eq!(
        r.assistant.load(&id).unwrap().messages.len(),
        2,
        "the conversation lives on in memory"
    );
    assert!(matches!(
        r.assistant.delete(&id),
        Err(AssistantError::Failed(_))
    ));
}

#[test]
fn texts_data_and_message_counts_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = open(&dir.path().join("a.json"));
    let long = "é".repeat(MAX_MESSAGE_CHARS + 50);
    let (id, _, _) = store.create(user(&long));
    let big = json!({"blob": "x".repeat(MAX_DATA_BYTES + 1)});
    let mut batch = vec![
        StoredMessage::tool("c", "t", "r", true, "s".into(), Some(big), 2),
        StoredMessage::tool("d", "t", "r", true, "s".into(), Some(json!({"ok": 1})), 2),
    ];
    batch.extend(
        (0..MAX_MESSAGES + 10).map(|i| StoredMessage::assistant(format!("m{i}"), vec![], None, 3)),
    );
    store.append(&id, batch).unwrap().unwrap();
    let c = store.get(&id).unwrap();
    assert_eq!(
        c.messages.len(),
        0,
        "a conversation starts with a user message, and every user message was dropped"
    );
    // With a user message in the window the rest stays.
    store
        .append(
            &id,
            vec![
                user("q"),
                StoredMessage::tool(
                    "c",
                    "t",
                    "r",
                    true,
                    "s".into(),
                    Some(json!({"blob": "x".repeat(MAX_DATA_BYTES + 1)})),
                    2,
                ),
            ],
        )
        .unwrap()
        .unwrap();
    let c = store.get(&id).unwrap();
    assert_eq!(c.messages[0].text, "q");
    assert_eq!(
        c.messages[1].data, None,
        "oversized data is dropped, the text stays"
    );
    let (id2, _, _) = store.create(user(&long));
    assert_eq!(
        store.get(&id2).unwrap().messages[0].text.chars().count(),
        MAX_MESSAGE_CHARS
    );
    let (id3, _, _) = store.create(StoredMessage::user(
        "ctx",
        (0..12)
            .map(|i| ContextItem {
                id: format!("i{i}"),
                label: "l".into(),
                text: "y".repeat(MAX_CONTEXT_CHARS + 9),
            })
            .collect(),
        1,
    ));
    let ctx = &store.get(&id3).unwrap().messages[0].context;
    assert_eq!(
        (ctx.len(), ctx[0].text.chars().count()),
        (MAX_CONTEXT_ITEMS, MAX_CONTEXT_CHARS)
    );
}

#[test]
fn the_number_of_conversations_and_the_file_size_are_bounded_oldest_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.json");
    let (store, clock) = open(&path);
    let mut first = String::new();
    for i in 0..MAX_CONVERSATIONS + 5 {
        clock.advance(std::time::Duration::from_secs(1));
        let (id, _, _) = store.create(user(&format!("q{i}")));
        if i == 0 {
            first = id;
        }
    }
    assert_eq!(store.summaries().len(), MAX_CONVERSATIONS);
    assert!(store.get(&first).is_none(), "the oldest went first");
    // Big conversations push the file over its size bound: the oldest go, the newest stays.
    let (store, clock) = open(&dir.path().join("b.json"));
    let mut ids = vec![];
    for _ in 0..6 {
        clock.advance(std::time::Duration::from_secs(1));
        let (id, _, _) = store.create(user("seed"));
        let big: Vec<StoredMessage> = (0..60)
            .map(|_| StoredMessage::assistant("w".repeat(MAX_MESSAGE_CHARS), vec![], None, 2))
            .collect();
        store.append(&id, big).unwrap().unwrap();
        ids.push(id);
    }
    assert!(std::fs::metadata(dir.path().join("b.json")).unwrap().len() <= MAX_FILE_BYTES);
    assert!(store.get(ids.last().unwrap()).is_some());
    assert!(store.get(&ids[0]).is_none());
}

#[test]
fn history_keeps_the_newest_messages_that_fit_from_a_user_message_on() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = open(&dir.path().join("a.json"));
    assert!(store.history("none", 100).is_empty());
    let (id, _, _) = store.create(user("first question"));
    let call = ToolCall {
        id: "c1".into(),
        name: "t".into(),
        input: json!({}),
    };
    store
        .append(
            &id,
            vec![
                StoredMessage::assistant("a1", vec![call], None, 2),
                StoredMessage::tool("c1", "t", "result one", true, "s".into(), None, 3),
                StoredMessage::assistant("answer one", vec![], Some(EndReason::Answered), 4),
                user("second question that is long"),
            ],
        )
        .unwrap()
        .unwrap();
    let all = store.history(&id, 10_000);
    assert_eq!(all.len(), 5);
    assert_eq!(all[2].role, Role::Tool);
    assert_eq!(all[2].tool_call_id.as_deref(), Some("c1"));
    // Only the last user message fits: it is still kept.
    let tight = store.history(&id, 5);
    assert_eq!(
        tight.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(),
        ["second question that is long"]
    );
    // Room for "answer one" and the question, but a window starting at an assistant message starts at the next user message.
    let mid = store.history(
        &id,
        "answer one".len() + "second question that is long".len(),
    );
    assert_eq!(mid.len(), 1);
}
