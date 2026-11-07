//! The conversation store as a port (ADR 0023 WP-2b): the file-backed store and an in-memory one behind `Conversations`, and
//! the assistant over either. A chat the user does not save never touches the disk (D3 note: no file is left even after a
//! crash). Temp folders and a fake clock only.

use crate::support::*;
use paddock_assistant::*;
use paddock_health::FakeClock;
use std::sync::Arc;

fn files_in(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0)
}

#[test]
fn an_in_memory_store_keeps_conversations_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let clock = FakeClock::at_wall_ms(1_700_000_000_000);
    let store = ConversationStore::in_memory(Arc::new(clock.clone()));
    let (id, title, saved) = store.create(StoredMessage::user("Hello there", vec![], 1));
    assert!(saved.is_ok());
    assert_eq!(title, "Hello there");
    assert_eq!(
        store.append(
            &id,
            vec![StoredMessage::assistant(
                "Hi",
                vec![],
                Some(EndReason::Answered),
                2
            )]
        ),
        Some(Ok(()))
    );
    assert_eq!(store.get(&id).unwrap().messages.len(), 2);
    assert_eq!(store.summaries().len(), 1);
    assert_eq!(store.history(&id, 1000).len(), 2);
    assert!(!store.is_saved());
    assert_eq!(store.delete(&id), Ok(true));
    assert_eq!(store.delete(&id), Ok(false));
    assert!(store.take_problem().is_none());
    // Bounded like the file: the oldest goes past the limit.
    let small = ConversationStore::in_memory_with(
        Arc::new(clock.clone()),
        StoreLimits {
            conversations: 2,
            file_bytes: MAX_FILE_BYTES,
        },
    );
    for i in 0..3 {
        clock.advance(std::time::Duration::from_secs(1));
        assert!(
            small
                .create(StoredMessage::user(format!("q{i}"), vec![], i))
                .2
                .is_ok()
        );
    }
    assert_eq!(
        small
            .summaries()
            .iter()
            .map(|s| s.title.clone())
            .collect::<Vec<_>>(),
        ["q2", "q1"]
    );
    assert_eq!(files_in(dir.path()), 0, "nothing on disk");
    let file = ConversationStore::open(dir.path().join("a.json"), Arc::new(clock));
    assert!(file.is_saved());
}

#[test]
fn the_assistant_runs_over_any_conversations_port_and_an_unsaved_chat_leaves_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let clock = FakeClock::at_wall_ms(1_700_000_000_000);
    let provider = Scripted::new(vec![Step::Reply(vec!["hi"], text("hi"))]);
    let store: Arc<dyn Conversations> =
        Arc::new(ConversationStore::in_memory(Arc::new(clock.clone())));
    let assistant = Assistant::with_conversations(
        provider,
        Tools::none(),
        store.clone(),
        Arc::new(clock),
        AssistantConfig::new("Demo", "sys"),
    );
    let (sink, rx) = sink();
    let ack = assistant.send(ask("hello"), sink).unwrap();
    let events = until_end(&rx);
    assert_eq!(done(&events).0, "hi");
    assert_eq!(
        assistant.load(&ack.conversation_id).unwrap().messages.len(),
        2
    );
    assert_eq!(assistant.conversations().len(), 1);
    assert!(Arc::ptr_eq(assistant.store(), &store));
    // The process could end here: there is nothing to clean up.
    assert_eq!(files_in(dir.path()), 0);
}
