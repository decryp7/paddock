//! "Don't save this chat" through the port (ADR 0023 Decision 3, D3 note; WP-3b): a question says whether its new
//! conversation is saved, the port hears it, and the pieces an app's own store needs are the family's (the title, the bounds
//! of a message, what the model reads, taking a conversation in with its id). Temp folders and a fake clock only.

use crate::support::*;
use paddock_assistant::*;
use paddock_health::FakeClock;
use std::sync::{Arc, Mutex};

/// A port that records what `create_saved` was told, over an in-memory store.
struct Recording {
    inner: ConversationStore,
    told: Mutex<Vec<bool>>,
}

impl Conversations for Recording {
    fn summaries(&self) -> Vec<ConversationSummary> {
        self.inner.summaries()
    }
    fn get(&self, id: &str) -> Option<Conversation> {
        self.inner.get(id)
    }
    fn create(&self, first: StoredMessage) -> (String, String, Result<(), SaveError>) {
        self.create_saved(first, true)
    }
    fn create_saved(
        &self,
        first: StoredMessage,
        save: bool,
    ) -> (String, String, Result<(), SaveError>) {
        self.told.lock().unwrap().push(save);
        self.inner.create(first)
    }
    fn append(&self, id: &str, messages: Vec<StoredMessage>) -> Option<Result<(), SaveError>> {
        self.inner.append(id, messages)
    }
    fn delete(&self, id: &str) -> Result<bool, SaveError> {
        self.inner.delete(id)
    }
    fn history(&self, id: &str, history_chars: usize) -> Vec<ChatMessage> {
        self.inner.history(id, history_chars)
    }
}

#[test]
fn a_question_says_whether_its_new_conversation_is_saved_and_the_port_hears_it() {
    let clock = FakeClock::at_wall_ms(1_700_000_000_000);
    let store = Arc::new(Recording {
        inner: ConversationStore::in_memory(Arc::new(clock.clone())),
        told: Mutex::default(),
    });
    let provider = Scripted::new(vec![
        Step::Reply(vec!["a"], text("a")),
        Step::Reply(vec!["b"], text("b")),
        Step::Reply(vec!["c"], text("c")),
    ]);
    let assistant = Assistant::with_conversations(
        provider,
        Tools::none(),
        store.clone(),
        Arc::new(clock),
        AssistantConfig::new("Demo", "sys"),
    );
    let (sink, rx) = sink();
    // Saved unless the question says otherwise.
    assistant.send(ask("one"), sink.clone()).unwrap();
    until_end(&rx);
    let ack = assistant
        .send(
            SendRequest {
                save: false,
                ..ask("two")
            },
            sink.clone(),
        )
        .unwrap();
    until_end(&rx);
    // A follow-up continues its conversation: nothing is created, whatever it says.
    assistant
        .send(
            SendRequest {
                conversation_id: Some(ack.conversation_id),
                save: true,
                ..ask("three")
            },
            sink,
        )
        .unwrap();
    until_end(&rx);
    assert_eq!(*store.told.lock().unwrap(), [true, false]);
}

#[test]
fn save_is_optional_on_the_wire_and_true_when_left_out() {
    let plain: SendRequest = serde_json::from_str(r#"{"text":"hi"}"#).unwrap();
    assert!(plain.save);
    let unsaved: SendRequest = serde_json::from_str(r#"{"text":"hi","save":false}"#).unwrap();
    assert!(!unsaved.save);
    assert!(ask("hi").save);
}

#[test]
fn a_store_that_cannot_keep_a_conversation_unsaved_creates_it_as_usual() {
    let dir = tempfile::tempdir().unwrap();
    let clock = FakeClock::at_wall_ms(1_700_000_000_000);
    let file = ConversationStore::open(dir.path().join("a.json"), Arc::new(clock));
    let port: &dyn Conversations = &file;
    let (id, title, saved) =
        port.create_saved(StoredMessage::user("Kept anyway", vec![], 1), false);
    assert!(saved.is_ok());
    assert_eq!(title, "Kept anyway");
    assert!(port.get(&id).is_some());
}

#[test]
fn a_store_takes_a_conversation_in_with_its_id_once() {
    let clock = FakeClock::at_wall_ms(1_700_000_000_000);
    let from = ConversationStore::in_memory(Arc::new(clock.clone()));
    let (id, _, _) = from.create(StoredMessage::user("Move me", vec![], 1));
    from.append(
        &id,
        vec![StoredMessage::assistant(
            "Moved",
            vec![],
            Some(EndReason::Answered),
            2,
        )],
    );
    let moving = from.get(&id).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let to = ConversationStore::open(dir.path().join("b.json"), Arc::new(clock.clone()));
    assert_eq!(to.adopt(moving.clone()), Ok(()));
    assert_eq!(to.get(&id).as_ref(), Some(&moving));
    // Saved with it: a restart finds it.
    let again = ConversationStore::open(dir.path().join("b.json"), Arc::new(clock.clone()));
    assert_eq!(again.get(&id).as_ref(), Some(&moving));
    // An id the store holds already is refused, in words, and nothing changes.
    let err = to.adopt(moving.clone()).unwrap_err();
    assert!(err.0.contains("already"), "{err}");
    assert_eq!(to.summaries().len(), 1);
    // A conversation without a message is refused too.
    let empty = Conversation {
        id: "x".into(),
        messages: vec![],
        ..moving
    };
    assert!(to.adopt(empty).is_err());
    // A later create never reuses an adopted id.
    let mem = ConversationStore::in_memory(Arc::new(clock));
    let (first, _, _) = mem.create(StoredMessage::user("a", vec![], 1));
    let taken = Conversation {
        id: format!("{}-x", first),
        ..from.get(&id).unwrap()
    };
    mem.adopt(taken).unwrap();
    let (second, _, _) = mem.create(StoredMessage::user("b", vec![], 2));
    assert_ne!(second, first);
    assert_eq!(mem.summaries().len(), 3);
}

#[test]
fn the_title_the_bounds_and_the_history_are_the_familys_for_any_store() {
    assert_eq!(
        conversation_title("  What   uses\nthe CPU?  "),
        "What uses the CPU?"
    );
    assert_eq!(
        conversation_title(&"x".repeat(200)).chars().count(),
        MAX_TITLE_CHARS
    );
    let long = StoredMessage::user("y".repeat(MAX_MESSAGE_CHARS + 10), vec![], 1).bounded();
    assert_eq!(long.text.chars().count(), MAX_MESSAGE_CHARS);
    // What the model reads is the same whichever store holds the conversation.
    let clock = FakeClock::at_wall_ms(1_700_000_000_000);
    let store = ConversationStore::in_memory(Arc::new(clock));
    let (id, _, _) = store.create(StoredMessage::user("first question", vec![], 1));
    store.append(
        &id,
        vec![
            StoredMessage::assistant("first answer", vec![], Some(EndReason::Answered), 2),
            StoredMessage::user("second question", vec![], 3),
            StoredMessage::assistant("", vec![], Some(EndReason::Failed), 4),
        ],
    );
    let c = store.get(&id).unwrap();
    for chars in [0, 20, 40, 1000] {
        assert_eq!(history_of(&c, chars), store.history(&id, chars), "{chars}");
    }
}
