//! Serde round trips of the wire types, and ids from the catalogue.

use paddock_widgets::{
    Edit, Layout, PlacementState, Read, WidgetAction, WidgetState, apply, assign_ids, read,
};
use serde_json::json;

use crate::catalogue::{Gadget, Gadgets, defaults, disk, id, ids};

#[test]
fn a_layout_survives_serialise_and_read() {
    let mut layout = apply::<Gadgets>(
        &defaults(),
        Edit::SavePreset {
            name: "Mine".into(),
        },
    )
    .expect("the edit applies");
    layout = apply::<Gadgets>(
        &layout,
        Edit::Add {
            widget: disk("backup"),
            span: None,
            at: Some(1),
        },
    )
    .expect("the edit applies");
    let value = serde_json::to_value(&layout).expect("serialises");
    let Read {
        layout: back,
        notes,
    } = read::<Gadgets>(&value);
    assert!(notes.is_empty());
    assert_eq!(back, layout);
}

#[test]
fn the_undo_level_is_not_written() {
    let removed =
        apply::<Gadgets>(&defaults(), Edit::Remove { id: id("clock") }).expect("the edit applies");
    assert!(removed.can_undo());
    let value = serde_json::to_value(&removed).expect("serialises");
    assert!(value.get("previous").is_none());
    let back: Layout<Gadget> = read::<Gadgets>(&value).layout;
    assert!(!back.can_undo());
}

#[test]
fn every_edit_round_trips() {
    let edits: Vec<Edit<Gadget>> = vec![
        Edit::Add {
            widget: Gadget::Clock,
            span: Some(2),
            at: Some(0),
        },
        Edit::Add {
            widget: Gadget::Clock,
            span: None,
            at: None,
        },
        Edit::Remove { id: id("a") },
        Edit::Move { id: id("a"), to: 2 },
        Edit::Resize {
            id: id("a"),
            span: 2,
        },
        Edit::Replace {
            id: id("a"),
            widget: disk("x"),
        },
        Edit::ApplyPreset { id: id("a") },
        Edit::SavePreset { name: "n".into() },
        Edit::DeletePreset { id: id("a") },
        Edit::ResetToDefaults,
        Edit::Undo,
    ];
    for edit in edits {
        let text = serde_json::to_string(&edit).expect("serialises");
        assert_eq!(
            serde_json::from_str::<Edit<Gadget>>(&text).expect("deserialises"),
            edit
        );
    }
}

#[test]
fn an_edit_may_leave_out_its_optional_fields() {
    let edit: Edit<Gadget> =
        serde_json::from_value(json!({ "type": "add", "widget": { "kind": "clock" } }))
            .expect("deserialises");
    assert_eq!(
        edit,
        Edit::Add {
            widget: Gadget::Clock,
            span: None,
            at: None
        }
    );
    let named: Edit<Gadget> =
        serde_json::from_value(json!({ "type": "resetToDefaults" })).expect("deserialises");
    assert_eq!(named, Edit::ResetToDefaults);
}

#[test]
fn widget_states_have_the_agreed_shape() {
    let ready = WidgetState::Ready {
        value: 5u8,
        as_of: Some(1_700_000_000_000),
        stale: false,
    };
    assert_eq!(
        serde_json::to_value(&ready).expect("serialises"),
        json!({ "state": "ready", "value": 5, "asOf": 1_700_000_000_000i64, "stale": false })
    );
    let empty: WidgetState<u8> = WidgetState::Empty {
        title: "No alarms".into(),
        detail: "All clear".into(),
        action: Some(WidgetAction {
            id: "open".into(),
            label: "Open alarms".into(),
        }),
    };
    let error: WidgetState<u8> = WidgetState::Error {
        title: "Couldn't read".into(),
        cause: None,
        retry: true,
        action: None,
    };
    for state in [ready, empty, error] {
        let back: WidgetState<u8> =
            serde_json::from_value(serde_json::to_value(&state).expect("serialises"))
                .expect("deserialises");
        assert_eq!(back, state);
    }
    let placed = PlacementState {
        id: id("clock"),
        state: WidgetState::Ready {
            value: 1u8,
            as_of: None,
            stale: true,
        },
    };
    let back: PlacementState<u8> =
        serde_json::from_value(serde_json::to_value(&placed).expect("serialises"))
            .expect("deserialises");
    assert_eq!(back, placed);
}

#[test]
fn ids_from_the_kinds_are_unique() {
    let placed = assign_ids([
        (2, disk("a")),
        (2, disk("b")),
        (1, Gadget::Clock),
        (2, disk("c")),
    ]);
    assert_eq!(
        ids(&Layout::new::<Gadgets>(placed, vec![])),
        ["disk-space", "disk-space-2", "clock", "disk-space-3"]
    );
}
