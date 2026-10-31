//! The messages say what happened and what to do; the wire shape of errors and notes.

use paddock_widgets::{BadId, Counted, Note, NoteCode, PlacementId, PresetId, WidgetError};
use serde_json::json;

#[test]
fn every_message_says_what_to_do() {
    let cases = [
        (
            WidgetError::UnknownPlacement("a".into()),
            "No widget has the id \"a\".",
        ),
        (
            WidgetError::UnknownPreset("a".into()),
            "No preset has the id \"a\".",
        ),
        (
            WidgetError::DuplicateId("a".into()),
            "Two items share the id \"a\"; ids must be unique.",
        ),
        (
            WidgetError::TooMany {
                counted: Counted::Placements,
                max: 8,
            },
            "At most 8 widgets fit; remove one first.",
        ),
        (
            WidgetError::TooMany {
                counted: Counted::UserPresets,
                max: 2,
            },
            "At most 2 saved presets fit; remove one first.",
        ),
        (
            WidgetError::TooMany {
                counted: Counted::PresetPlacements,
                max: 3,
            },
            "At most 3 widgets in a saved preset fit; remove one first.",
        ),
        (
            WidgetError::KindLimit { max: 1 },
            "At most 1 of this widget fit; remove one first.",
        ),
        (
            WidgetError::SpanNotAllowed {
                allowed: vec![1, 2],
            },
            "This widget can be 1 or 2 columns wide.",
        ),
        (
            WidgetError::TooFew { min: 1 },
            "At least 1 widgets are needed here.",
        ),
        (
            WidgetError::Invalid("Choose a volume.".into()),
            "Choose a volume.",
        ),
        (
            WidgetError::BuiltinPreset,
            "Built-in presets cannot be changed or deleted.",
        ),
        (WidgetError::NothingToUndo, "There is nothing to undo."),
        (
            WidgetError::NewerLayout,
            "This layout was saved by a newer version of the app and is kept unchanged; update the app to edit it.",
        ),
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
            counted: Counted::UserPresets,
            max: 8
        })
        .expect("serialises"),
        json!({ "tooMany": { "counted": "userPresets", "max": 8 } })
    );
    let note = Note {
        code: NoteCode::Dropped { count: 2 },
        message: "m".into(),
    };
    assert_eq!(
        serde_json::to_value(note).expect("serialises"),
        json!({ "code": { "dropped": { "count": 2 } }, "message": "m" })
    );
    let note = Note {
        code: NoteCode::Newer { version: 3 },
        message: "m".into(),
    };
    assert_eq!(
        serde_json::to_value(note).expect("serialises"),
        json!({ "code": { "newer": { "version": 3 } }, "message": "m" })
    );
    assert_eq!(
        serde_json::to_value(NoteCode::TooLarge).expect("serialises"),
        json!("tooLarge")
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
        assert_eq!(PresetId::new(good).expect("valid").as_str(), good);
    }
    for bad in ["", "A", "a b", "é", "a_b", &"a".repeat(65)] {
        assert_eq!(PlacementId::new(bad), Err(BadId));
        assert_eq!(PresetId::try_from(bad.to_string()), Err(BadId));
        assert!(serde_json::from_value::<PlacementId>(json!(bad)).is_err());
        assert!(serde_json::from_value::<PresetId>(json!(bad)).is_err());
    }
    assert_eq!(String::from(PlacementId::new("x").expect("valid")), "x");
    assert_eq!(String::from(PresetId::new("y").expect("valid")), "y");
    assert_eq!(
        BadId.to_string(),
        "An id is 1 to 64 characters of a-z, 0-9 and -."
    );
}
