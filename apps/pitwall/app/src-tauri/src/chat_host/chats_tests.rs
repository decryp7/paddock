//! The saved chats and "Don't save this chat" (ADR 0023 Decision 3, WP-3b): one file per conversation, damage and newer
//! files, the limits (refused in words, nothing deleted by itself), attachments by SHA-256, moving a chat either way, and a
//! chat that is not saved leaving no file, also through the assistant. Temp folders and a fake clock only (ER-14).

use super::chats::{ChatStores, ChatsError, PREFS_FILE};
use super::saved::{SavedChats, SavedLimits, CHATS_DIR};
use paddock_assistant::{
    Assistant, AssistantConfig, AssistantEvent, Attachment, ChatRequest, ChatResponse, Conversations, EndReason, Envelope, EventSink,
    LlmProvider, ProviderError, SendRequest, StopReason, StoredMessage, Tools,
};
use paddock_health::FakeClock;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn clock() -> FakeClock {
    FakeClock::at_wall_ms(1_700_000_000_000)
}

fn user(text: &str) -> StoredMessage {
    StoredMessage::user(text, vec![], 1)
}

fn answer(text: &str) -> StoredMessage {
    StoredMessage::assistant(text, vec![], Some(EndReason::Answered), 2)
}

/// Every file below `dir`, relative, sorted.
fn files(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = vec![];
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn small(dir: &Path, clock: &FakeClock, conversations: usize, file_bytes: u64, attachment_bytes: u64) -> SavedChats {
    SavedChats::open_with(
        dir.join(CHATS_DIR),
        Arc::new(clock.clone()),
        SavedLimits {
            conversations,
            file_bytes,
            attachment_bytes,
        },
    )
}

#[test]
fn each_saved_chat_is_one_owner_only_file_that_survives_a_restart() {
    let data = tempfile::tempdir().unwrap();
    let c = clock();
    let chats = ChatStores::new(data.path(), Arc::new(c.clone()));
    assert!(files(data.path()).is_empty(), "nothing is written before the first chat");
    let (a, title, saved) = chats.create(user("What uses the CPU?"));
    assert_eq!(saved, Ok(()));
    assert_eq!(title, "What uses the CPU?");
    c.advance(Duration::from_secs(1));
    assert_eq!(chats.append(&a, vec![answer("The build.")]), Some(Ok(())));
    c.advance(Duration::from_secs(1));
    let (b, _, _) = chats.create(user("And memory?"));
    assert_eq!(
        files(data.path()),
        [format!("chats/{a}.json"), format!("chats/{a}.json.bak"), format!("chats/{b}.json")]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(data.path().join(format!("chats/{a}.json")))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "only the user's account reads it");
    }
    let again = ChatStores::new(data.path(), Arc::new(c.clone()));
    let list = again.summaries();
    assert_eq!(
        list.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        [b.as_str(), a.as_str()],
        "newest first"
    );
    assert_eq!(list[1].message_count, 2);
    assert_eq!(again.get(&a).unwrap().messages[1].text, "The build.");
    assert_eq!(again.history(&a, 1000).len(), 2);
    assert_eq!(again.is_saved(&a), Some(true));
    assert!(again.take_problem().is_none());
    // Ids from the UI never name another file.
    assert_eq!(again.delete("../prefs"), Ok(false));
    assert!(again.get("../x").is_none());
    assert!(again.saved().attachment("../../etc/passwd").is_none());
}

#[test]
fn a_damaged_file_uses_its_backup_or_is_kept_aside_and_a_newer_one_is_left_as_it_is() {
    let data = tempfile::tempdir().unwrap();
    let c = clock();
    let chats = ChatStores::new(data.path(), Arc::new(c.clone()));
    let (good, _, _) = chats.create(user("kept"));
    chats.append(&good, vec![answer("twice")]);
    let (lost, _, _) = chats.create(user("lost"));
    let dir = data.path().join(CHATS_DIR);
    // The newest copy of one is damaged (its backup is good); the other has no backup at all.
    std::fs::write(dir.join(format!("{good}.json")), b"{\"version\":1,\"conver").unwrap();
    std::fs::write(dir.join(format!("{lost}.json")), b"not json").unwrap();
    let newer = "{\"version\": 9, \"conversation\": {\"future\": true}}";
    std::fs::write(dir.join("s1-9.json"), newer).unwrap();

    let reopened = ChatStores::new(data.path(), Arc::new(c.clone()));
    let ids: Vec<String> = reopened.summaries().into_iter().map(|s| s.id).collect();
    assert_eq!(
        ids,
        std::slice::from_ref(&good),
        "the damaged one without a backup and the newer one are not listed"
    );
    assert_eq!(reopened.get(&good).unwrap().messages.len(), 1, "the last good copy");
    let problem = reopened.take_problem().unwrap();
    assert!(
        problem.contains("2 chats could not be read") || problem.contains("could not be read"),
        "{problem}"
    );
    assert!(problem.contains("newer Pitwall"), "{problem}");
    assert!(reopened.take_problem().is_none(), "told once");
    assert_eq!(
        std::fs::read_to_string(dir.join("s1-9.json")).unwrap(),
        newer,
        "a newer file is never touched"
    );
    assert!(
        files(data.path())
            .iter()
            .any(|f| f.starts_with(&format!("chats/{lost}.json.corrupt-"))),
        "the damaged file is kept aside: {:?}",
        files(data.path())
    );
    // A new chat never takes the newer file's name, and the newer file stays as it is.
    let (fresh, _, saved) = reopened.create(user("new"));
    assert_eq!(saved, Ok(()));
    assert_ne!(fresh, "s1-9");
    assert_eq!(std::fs::read_to_string(dir.join("s1-9.json")).unwrap(), newer);
}

#[test]
fn at_a_limit_the_new_item_is_refused_in_words_kept_for_the_session_and_nothing_is_deleted() {
    let data = tempfile::tempdir().unwrap();
    let c = clock();
    let chats = ChatStores::with(small(data.path(), &c, 2, 4096, 64), Arc::new(c.clone()));
    let (a, _, _) = chats.create(user("one"));
    let (b, _, _) = chats.create(user("two"));
    let before = files(data.path());
    // The third chat: refused with words, kept until Pitwall closes, and both saved chats are still there.
    let (third, _, saved) = chats.create(user("three"));
    let words = saved.unwrap_err().to_string();
    assert!(words.contains("at most 2 saved chats") && words.contains("Delete chats"), "{words}");
    assert_eq!(chats.get(&third).unwrap().messages.len(), 1);
    assert_eq!(chats.summaries().len(), 3);
    assert_eq!(files(data.path()), before, "nothing deleted, nothing written");
    assert_eq!(chats.saved().is_on_disk(&third), Some(false));
    // Once the user deletes one, the kept chat is saved with its next answer.
    assert_eq!(chats.delete(&a), Ok(true));
    assert_eq!(chats.append(&third, vec![answer("saved now")]), Some(Ok(())));
    assert_eq!(chats.saved().is_on_disk(&third), Some(true));

    // A chat past the file size: its newest messages are kept for the session, the file keeps the last that fitted.
    let file = data.path().join(CHATS_DIR).join(format!("{b}.json"));
    let fitted = std::fs::read(&file).unwrap();
    let words = chats.append(&b, vec![answer(&"x".repeat(5000))]).unwrap().unwrap_err().to_string();
    assert!(words.contains("larger than") && words.contains("Start a new chat"), "{words}");
    assert_eq!(std::fs::read(&file).unwrap(), fitted);
    assert_eq!(chats.get(&b).unwrap().messages.len(), 2, "the session still has it");

    // Attachments: the same bytes are kept once; past the limit the new one is refused in words.
    let sha = chats.put_attachment(true, &[7u8; 40]).unwrap();
    assert_eq!(chats.put_attachment(true, &[7u8; 40]).unwrap(), sha);
    assert_eq!(chats.attachment(&sha).unwrap(), vec![7u8; 40]);
    let words = chats.put_attachment(true, &[8u8; 40]).unwrap_err().to_string();
    assert!(words.contains("attachments") && words.contains("Delete chats"), "{words}");
    assert!(chats.attachment(&sha).is_some(), "the kept one stays");
}

/// The model: answers "ok".
struct Ok1;

impl LlmProvider for Ok1 {
    fn id(&self) -> &str {
        "ok"
    }
    fn display_name(&self) -> String {
        "Ok".into()
    }
    fn context_length(&self) -> usize {
        8192
    }
    fn chat(&self, _: &ChatRequest, _: &mut dyn FnMut(&str)) -> Result<ChatResponse, ProviderError> {
        Ok(ChatResponse {
            text: "ok".into(),
            tool_calls: vec![],
            stop_reason: StopReason::EndTurn,
            usage: None,
        })
    }
}

struct Sink(Mutex<Sender<Envelope>>);

impl EventSink for Sink {
    fn emit(&self, e: &Envelope) {
        let _ = self.0.lock().unwrap().send(e.clone());
    }
}

fn ask(text: &str, save: bool) -> SendRequest {
    SendRequest {
        conversation_id: None,
        text: text.into(),
        context: vec![],
        allow_write: false,
        attachments: vec![],
        save,
    }
}

#[test]
fn a_chat_marked_dont_save_leaves_no_file_through_the_assistant_and_the_default_follows_settings() {
    let data = tempfile::tempdir().unwrap();
    let c = clock();
    let chats = Arc::new(ChatStores::new(data.path(), Arc::new(c.clone())));
    let assistant = Assistant::with_conversations(
        Arc::new(Ok1),
        Tools::none(),
        chats.clone(),
        Arc::new(c.clone()),
        AssistantConfig::new("Pitwall", "s"),
    );
    let (tx, rx) = channel();
    let sink: Arc<dyn EventSink> = Arc::new(Sink(Mutex::new(tx)));
    let ended = |rx: &std::sync::mpsc::Receiver<Envelope>| loop {
        let e = rx.recv_timeout(Duration::from_secs(30)).expect("the turn ended");
        if matches!(e.event, AssistantEvent::Done { .. } | AssistantEvent::Error { .. }) {
            return e;
        }
    };
    let ack = assistant.send(ask("secret question", false), sink.clone()).unwrap();
    ended(&rx);
    let ack2 = assistant
        .send(
            SendRequest {
                conversation_id: Some(ack.conversation_id.clone()),
                ..ask("and more", true)
            },
            sink.clone(),
        )
        .unwrap();
    assert_eq!(ack2.conversation_id, ack.conversation_id);
    ended(&rx);
    // An attachment of a chat that is not saved stays in memory too.
    let sha = chats.put_attachment(false, b"private bytes").unwrap();
    assert_eq!(chats.attachment(&sha).unwrap(), b"private bytes");
    assert_eq!(assistant.load(&ack.conversation_id).unwrap().messages.len(), 4);
    assert_eq!(chats.is_saved(&ack.conversation_id), Some(false));
    assert_eq!(
        files(data.path()),
        Vec::<String>::new(),
        "a chat that is not saved leaves no file, nothing to clean up after a crash"
    );
    assert_eq!(chats.storage().unsaved, 1);

    // "Save new chats" off: a question that does not say goes to memory; the setting is the one file.
    assert!(chats.save_new());
    chats.set_save_new(false).unwrap();
    assert!(
        !ChatStores::new(data.path(), Arc::new(c.clone())).save_new(),
        "the setting survives a restart"
    );
    let (id, _, _) = chats.create(user("unsaved by default"));
    assert_eq!(chats.is_saved(&id), Some(false));
    assert_eq!(files(data.path()), [format!("{CHATS_DIR}/{PREFS_FILE}")]);
    assistant.shutdown();
}

#[test]
fn a_chat_moves_to_memory_deleting_its_saved_copy_or_back_to_disk() {
    let data = tempfile::tempdir().unwrap();
    let c = clock();
    let chats = ChatStores::new(data.path(), Arc::new(c.clone()));
    let only = chats.put_attachment(true, b"only this chat").unwrap();
    let shared = chats.put_attachment(true, b"two chats").unwrap();
    let file = |sha: &str| Attachment::File {
        sha256: sha.into(),
        name: "a.txt".into(),
    };
    let (id, _, _) = chats.create(user("move me").with_attachments(vec![file(&only), file(&shared)]));
    chats.append(&id, vec![answer("ok")]);
    let (other, _, _) = chats.create(user("keeps one").with_attachments(vec![file(&shared)]));

    // "Don't save" on (the user said yes to deleting the saved copy): the file and the attachment only it used go.
    assert_eq!(chats.set_saved(&id, false), Ok(false));
    let left = files(data.path());
    assert!(!left.iter().any(|f| f.contains(&id)), "{left:?}");
    assert!(!left.iter().any(|f| f.ends_with(&only)), "{left:?}");
    assert!(left.iter().any(|f| f.ends_with(&shared)), "another chat still uses it: {left:?}");
    assert_eq!(chats.get(&id).unwrap().messages.len(), 2, "the chat goes on in memory");
    assert_eq!(chats.attachment(&only).unwrap(), b"only this chat");
    assert_eq!(chats.is_saved(&id), Some(false));
    assert_eq!(chats.set_saved(&id, false), Ok(false), "nothing to do twice");

    // Saved again: its file and its attachment come back.
    assert_eq!(chats.set_saved(&id, true), Ok(true));
    assert_eq!(chats.is_saved(&id), Some(true));
    let back = files(data.path());
    assert!(back.contains(&format!("chats/{id}.json")), "{back:?}");
    assert!(back.iter().any(|f| f.ends_with(&only)), "{back:?}");
    assert_eq!(ChatStores::new(data.path(), Arc::new(c)).get(&id).unwrap().messages.len(), 2);
    assert_eq!(chats.set_saved("nope", true), Err(ChatsError::NotFound));
    assert_eq!(chats.delete(&other), Ok(true));
}

#[test]
fn delete_all_removes_every_chat_file_and_attachment_but_keeps_the_setting() {
    let data = tempfile::tempdir().unwrap();
    let c = clock();
    let chats = ChatStores::new(data.path(), Arc::new(c.clone()));
    chats.set_save_new(true).unwrap();
    chats.put_attachment(true, b"photo").unwrap();
    let _ = chats.create(user("one"));
    let _ = chats.create_saved(user("two"), false);
    let dir = data.path().join(CHATS_DIR);
    std::fs::write(dir.join("s1-9.json"), "{\"version\": 9}").unwrap();
    std::fs::write(dir.join("s1-8.json.corrupt-1"), "damaged").unwrap();
    let outside: PathBuf = data.path().join("settings.toml");
    std::fs::write(&outside, "kept").unwrap();
    assert_eq!(chats.delete_all(), Ok(2));
    assert!(chats.summaries().is_empty());
    assert_eq!(
        files(data.path()),
        [format!("{CHATS_DIR}/{PREFS_FILE}"), "settings.toml".to_string()]
    );
    assert_eq!(chats.storage().chats, 0);
    assert_eq!(chats.storage().attachment_bytes, 0);
}

/// Coordinator follow-up: chats that are not saved are never dropped to make room. Past their bound a new one (or a question
/// in a full set) is refused in words, through the assistant too, and every chat is still there.
#[test]
fn unsaved_chats_past_their_bound_are_refused_in_words_never_dropped() {
    use paddock_assistant::{AssistantError, StoreLimits};
    let data = tempfile::tempdir().unwrap();
    let c = clock();
    let chats = Arc::new(ChatStores::with_unsaved_limits(
        SavedChats::open(data.path().join(CHATS_DIR), Arc::new(c.clone())),
        Arc::new(c.clone()),
        StoreLimits {
            conversations: 2,
            file_bytes: 1 << 20,
        },
    ));
    let assistant = Assistant::with_conversations(
        Arc::new(Ok1),
        Tools::none(),
        chats.clone(),
        Arc::new(c.clone()),
        AssistantConfig::new("Pitwall", "s"),
    );
    let (tx, rx) = channel();
    let sink: Arc<dyn EventSink> = Arc::new(Sink(Mutex::new(tx)));
    let mut ids = vec![];
    for q in ["first", "second"] {
        ids.push(assistant.send(ask(q, false), sink.clone()).unwrap().conversation_id);
        loop {
            let e = rx.recv_timeout(Duration::from_secs(30)).expect("the turn ended");
            if matches!(e.event, AssistantEvent::Done { .. } | AssistantEvent::Error { .. }) {
                break;
            }
        }
        c.advance(Duration::from_secs(1));
    }
    let Err(AssistantError::Invalid(words)) = assistant.send(ask("third", false), sink.clone()) else {
        panic!("a third unsaved chat must be refused");
    };
    assert!(words == "Pitwall keeps at most 2 unsaved chats (1 MiB) and never drops one to make room. Save or delete an unsaved chat, then ask again.", "{words}");
    for id in &ids {
        assert_eq!(chats.get(id).unwrap().messages.len(), 2, "nothing was dropped");
    }
    assert_eq!(chats.summaries().len(), 2);
    // Moving a saved chat in is refused the same way; a saved new chat is still fine.
    let (saved, _, ok) = chats.create(user("saved one"));
    assert_eq!(ok, Ok(()));
    assert!(matches!(chats.set_saved(&saved, false), Err(ChatsError::Save(w)) if w.contains("at most 2 unsaved chats")));
    assert_eq!(chats.is_saved(&saved), Some(true), "it stays saved");
    // A long unsaved chat says what is long about it, not that there are too many chats.
    let roomy = ChatStores::new(data.path(), Arc::new(c.clone()));
    let (long, _, _) = roomy.create_saved(user("long"), false);
    let answers = (0..paddock_assistant::MAX_MESSAGES).map(|_| answer("a")).collect();
    assert_eq!(roomy.append(&long, answers), Some(Ok(())));
    let words = roomy.admits(Some(&long), false).unwrap_err().0;
    assert!(words.starts_with("This chat holds as many messages as a chat can (400)"), "{words}");
    assistant.shutdown();
}

/// ADR 0023 WP-4b: a file attached in the composer is staged in memory (the chat may still become "Don't save", or never be
/// sent); it reaches the disk only with the question of a saved chat, and never for a chat that is not saved.
#[test]
fn an_attached_file_is_staged_in_memory_and_saved_only_with_a_saved_chats_question() {
    let data = tempfile::tempdir().unwrap();
    let c = clock();
    let chats = ChatStores::new(data.path(), Arc::new(c.clone()));
    let file = |sha: &str, name: &str| Attachment::File {
        sha256: sha.into(),
        name: name.into(),
    };
    let staged = chats.stage_attachment(b"staged notes").unwrap();
    assert_eq!(chats.attachment(&staged).unwrap(), b"staged notes");
    assert_eq!(files(data.path()), Vec::<String>::new(), "staging writes nothing");

    // A chat that is not saved: still nothing on disk.
    let (unsaved, _, kept) = chats.create_saved(user("private").with_attachments(vec![file(&staged, "notes.txt")]), false);
    kept.unwrap();
    assert_eq!(files(data.path()), Vec::<String>::new());

    // A saved chat's first question and a later one: the staged files are written beside it.
    let first = chats.stage_attachment(b"first file").unwrap();
    let (id, _, kept) = chats.create_saved(user("saved").with_attachments(vec![file(&first, "a.txt")]), true);
    kept.unwrap();
    let later = chats.stage_attachment(b"later file").unwrap();
    chats
        .append(&id, vec![answer("ok"), user("more").with_attachments(vec![file(&later, "b.txt")])])
        .unwrap()
        .unwrap();
    let on_disk = files(data.path());
    assert!(on_disk.iter().any(|f| f.ends_with(&first)), "{on_disk:?}");
    assert!(on_disk.iter().any(|f| f.ends_with(&later)), "{on_disk:?}");
    assert!(
        !on_disk.iter().any(|f| f.ends_with(&staged)),
        "the unsaved chat's file stays in memory: {on_disk:?}"
    );
    let again = ChatStores::new(data.path(), Arc::new(c));
    assert_eq!(again.attachment(&first).unwrap(), b"first file", "it survives a restart");
    assert_eq!(chats.is_saved(&unsaved), Some(false));
}
