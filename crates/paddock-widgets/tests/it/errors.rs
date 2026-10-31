//! The messages say what happened and what to do; the wire shape of errors and notes.

use paddock_widgets::{Note, NoteCode, PlacementId, WidgetError};
use serde_json::json;

#[test]
fn every_message_says_what_to_do() {
    let cases = [
        (
            WidgetError::BadId,
            "Use 1 to 64 characters of a-z, 0-9 and hyphens for an id.",
        ),
        (
            WidgetError::DuplicateId("a".into()),
            "Two items share the id \"a\"; ids must be unique.",
        ),
        (
            WidgetError::UnknownPlacement("a".into()),
            "There is no widget \"a\" in this layout; reload the layout and try again.",
        ),
        (
            WidgetError::TooMany {
                what: "widgets".into(),
                max: 8,
            },
            "At most 8 widgets fit; remove one first.",
        ),
        (
            WidgetError::KindLimit {
                kind: "Clock".into(),
                max: 1,
            },
            "At most 1 of \"Clock\" fit; remove one first.",
        ),
        (
            WidgetError::SpanNotAllowed {
                kind: "Clock".into(),
                spans: vec![1, 2],
            },
            "\"Clock\" can be 1 or 2 column(s) wide; choose one of those.",
        ),
        (
            WidgetError::Invalid("Choose a volume.".into()),
            "Choose a volume.",
        ),
        (
            WidgetError::NotAPreset("a".into()),
            "There is no layout \"a\"; choose another.",
        ),
        (
            WidgetError::BuiltinPreset,
            "Built-in layouts can't be deleted; save your own layout instead.",
        ),
        (WidgetError::NothingToUndo, "There is nothing to undo."),
    ];
    for (error, message) in cases {
        assert_eq!(error.to_string(), message);
    }
}

#[test]
fn errors_and_notes_have_a_wire_shape() {
    assert_eq!(
        serde_json::to_value(WidgetError::NothingToUndo).expect("serialises"),
        json!("nothingToUndo")
    );
    assert_eq!(
        serde_json::to_value(WidgetError::TooMany {
            what: "widgets".into(),
            max: 8
        })
        .expect("serialises"),
        json!({ "tooMany": { "what": "widgets", "max": 8 } })
    );
    let note = Note {
        code: NoteCode::Dropped { count: 2 },
        message: "m".into(),
    };
    assert_eq!(
        serde_json::to_value(note).expect("serialises"),
        json!({ "code": "dropped", "count": 2, "message": "m" })
    );
    let note = Note {
        code: NoteCode::Newer,
        message: "m".into(),
    };
    assert_eq!(
        serde_json::to_value(note).expect("serialises"),
        json!({ "code": "newer", "message": "m" })
    );
}

#[test]
fn ids_are_parsed_once() {
    for good in ["a", "disk-space-2", "0", &"a".repeat(64)] {
        assert_eq!(PlacementId::new(good).expect("valid").as_str(), good);
        assert_eq!(
            PlacementId::try_from(good.to_string())
                .expect("valid")
                .to_string(),
            good
        );
    }
    for bad in ["", "A", "a b", "é", "a_b", &"a".repeat(65)] {
        assert_eq!(PlacementId::new(bad), Err(WidgetError::BadId));
        assert!(serde_json::from_value::<PlacementId>(json!(bad)).is_err());
    }
    assert_eq!(String::from(PlacementId::new("x").expect("valid")), "x");
}
