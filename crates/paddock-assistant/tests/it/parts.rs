//! Message parts (ADR 0023 WP-2b): a question carries references to attachments (an image, an audio clip, a text file) kept by
//! the app by SHA-256; the engine checks them, keeps them with the message and gives them to the model's request. The bytes
//! never pass through the assistant. A scripted provider; no sleeps.

use crate::support::*;
use paddock_assistant::*;

fn sha(c: char) -> String {
    std::iter::repeat_n(c, 64).collect()
}

fn image() -> Attachment {
    Attachment::Image {
        sha256: sha('a'),
        mime: "image/png".into(),
        width: 640,
        height: 480,
    }
}

#[test]
fn attachments_are_kept_with_the_question_and_reach_the_model() {
    let r = rig(vec![Step::Reply(vec![], text("a cat"))]);
    let clip = Attachment::Audio {
        sha256: sha('b'),
        seconds: 12,
    };
    let file = Attachment::File {
        sha256: sha('c'),
        name: "notes.txt".into(),
    };
    let (id, events) = r.turn(SendRequest {
        attachments: vec![image(), clip.clone(), file.clone()],
        ..ask("What is this?")
    });
    assert_eq!(done(&events).0, "a cat");
    let stored = r.assistant.load(&id).unwrap();
    assert_eq!(
        stored.messages[0].attachments,
        vec![image(), clip.clone(), file.clone()]
    );
    let seen = r.provider.seen.lock().unwrap();
    let asked = seen[0].messages.last().unwrap();
    assert_eq!(asked.attachments, vec![image(), clip, file]);
    assert_eq!(asked.content, "What is this?");
}

#[test]
fn bad_attachments_are_refused_before_the_turn() {
    let r = rig(vec![]);
    let too_many = vec![image(); MAX_ATTACHMENTS + 1];
    let bad_sha = Attachment::Image {
        sha256: "xyz".into(),
        mime: "image/png".into(),
        width: 1,
        height: 1,
    };
    let bad_mime = Attachment::Image {
        sha256: sha('a'),
        mime: "image/svg+xml".into(),
        width: 1,
        height: 1,
    };
    let bad_name = Attachment::File {
        sha256: sha('a'),
        name: "a/../b".into(),
    };
    for (list, words) in [
        (too_many, "at most"),
        (vec![bad_sha], "not a file the app keeps"),
        (vec![bad_mime], "PNG, JPEG, WebP or GIF"),
        (vec![bad_name], "file name"),
    ] {
        match r.send(SendRequest {
            attachments: list,
            ..ask("q")
        }) {
            Err(AssistantError::Invalid(m)) => assert!(m.contains(words), "{m}"),
            other => panic!("{other:?}"),
        }
    }
    // An old request without the field still parses.
    let req: SendRequest = serde_json::from_value(serde_json::json!({"text": "q"})).unwrap();
    assert!(req.attachments.is_empty());
}
