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
        // The id the next create would make: create must step past it.
        id: format!("{}2", first.strip_suffix('1').unwrap()),
        ..from.get(&id).unwrap()
    };
    let taken_id = taken.id.clone();
    mem.adopt(taken).unwrap();
    let (second, _, _) = mem.create(StoredMessage::user("b", vec![], 2));
    assert_ne!(second, first);
    assert_ne!(second, taken_id);
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

#[test]
fn a_store_that_refuses_at_its_bounds_never_drops_a_conversation_and_says_why() {
    let clock = FakeClock::at_wall_ms(1_700_000_000_000);
    let store = ConversationStore::in_memory_with(
        Arc::new(clock.clone()),
        StoreLimits {
            conversations: 2,
            file_bytes: 4000,
        },
    )
    .with_overflow(Overflow::Refuse);
    let (a, _, _) = store.create(StoredMessage::user("one", vec![], 1));
    let (b, _, _) = store.create(StoredMessage::user("two", vec![], 2));
    assert_eq!(store.admits(Some(&a), true), Ok(()));
    // A third conversation: refused in words, and nothing is dropped, even if created anyway.
    let why = store.admits(None, false).unwrap_err();
    assert!(why.0.contains("at most 2"), "{why}");
    let (c, _, kept) = store.create(StoredMessage::user("three", vec![], 3));
    assert!(kept.is_err());
    assert!(store.get(&c).is_none());
    assert!(store.get(&a).is_some() && store.get(&b).is_some());
    // Past the size bound: the answer is kept (it was shown), nothing else goes, and the next question is refused.
    assert!(
        store
            .append(
                &a,
                vec![StoredMessage::assistant(
                    "x".repeat(5000),
                    vec![],
                    Some(EndReason::Answered),
                    4
                )]
            )
            .is_some()
    );
    assert_eq!(store.get(&a).unwrap().messages.len(), 2);
    assert!(store.get(&b).is_some());
    assert!(store.admits(Some(&b), false).is_err());
    let moved = from_memory(&clock, "moved");
    assert!(store.adopt(moved).is_err(), "taking one in is refused too");
    // A store that evicts (the default) admits everything, as before.
    assert_eq!(
        ConversationStore::in_memory(Arc::new(clock)).admits(None, true),
        Ok(())
    );
}

fn from_memory(clock: &FakeClock, text: &str) -> Conversation {
    let s = ConversationStore::in_memory(Arc::new(clock.clone()));
    let (id, _, _) = s.create(StoredMessage::user(text, vec![], 1));
    s.get(&id).unwrap()
}

/// A port that admits nothing.
struct Full(ConversationStore);

impl Conversations for Full {
    fn summaries(&self) -> Vec<ConversationSummary> {
        self.0.summaries()
    }
    fn get(&self, id: &str) -> Option<Conversation> {
        self.0.get(id)
    }
    fn create(&self, first: StoredMessage) -> (String, String, Result<(), SaveError>) {
        self.0.create(first)
    }
    fn append(&self, id: &str, messages: Vec<StoredMessage>) -> Option<Result<(), SaveError>> {
        self.0.append(id, messages)
    }
    fn delete(&self, id: &str) -> Result<bool, SaveError> {
        self.0.delete(id)
    }
    fn history(&self, id: &str, history_chars: usize) -> Vec<ChatMessage> {
        self.0.history(id, history_chars)
    }
    fn admits(&self, _: Option<&str>, save: bool) -> Result<(), SaveError> {
        Err(SaveError(format!("full (save {save})")))
    }
}

#[test]
fn a_question_the_store_cannot_take_is_refused_in_its_words_and_no_turn_starts() {
    let clock = FakeClock::at_wall_ms(1_700_000_000_000);
    let store = Arc::new(Full(ConversationStore::in_memory(Arc::new(clock.clone()))));
    let assistant = Assistant::with_conversations(
        Scripted::new(vec![]),
        Tools::none(),
        store.clone(),
        Arc::new(clock),
        AssistantConfig::new("Demo", "sys"),
    );
    let (sink, rx) = sink();
    let err = assistant
        .send(
            SendRequest {
                save: false,
                ..ask("hello")
            },
            sink,
        )
        .unwrap_err();
    assert_eq!(err, AssistantError::Invalid("full (save false)".into()));
    assert!(store.summaries().is_empty(), "the question is not kept");
    assert!(rx.try_recv().is_err(), "no turn started");
}

#[test]
fn a_refusing_store_never_shortens_a_conversation_and_refuses_one_too_long() {
    let clock = FakeClock::at_wall_ms(1_700_000_000_000);
    let store = ConversationStore::in_memory_with(
        Arc::new(clock.clone()),
        StoreLimits {
            conversations: 10,
            file_bytes: 64 << 20,
        },
    )
    .with_overflow(Overflow::Refuse);
    let (id, _, _) = store.create(StoredMessage::user("long", vec![], 1));
    let answers: Vec<StoredMessage> = (0..MAX_MESSAGES)
        .map(|i| StoredMessage::assistant(format!("a{i}"), vec![], None, 2))
        .collect();
    assert_eq!(store.append(&id, answers), Some(Ok(())));
    assert_eq!(
        store.get(&id).unwrap().messages.len(),
        MAX_MESSAGES + 1,
        "nothing dropped"
    );
    let why = store.admits(Some(&id), true).unwrap_err();
    assert!(
        why.0.contains(&format!("at most {MAX_MESSAGES} messages")),
        "{why}"
    );
    // Taking in a conversation longer than that is refused, not cut.
    let mut long = store.get(&id).unwrap();
    long.id = "elsewhere".into();
    assert!(store.adopt(long).unwrap_err().0.contains("at most"));
    // The port's default has no problem to tell.
    let port = Full(ConversationStore::in_memory(Arc::new(clock)));
    assert!(port.take_problem().is_none());
}
