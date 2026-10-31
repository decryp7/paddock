//! Serde round trips of the wire types, and ids from the catalogue.

use paddock_widgets::{
    Edit, Layout, PlacementState, Progress, Read, SurfaceNotice, SurfaceValues, Tone, WidgetAction,
    WidgetState, apply, assign_ids, read,
};
use serde_json::json;

use crate::catalogue::{Gadget, Gadgets, defaults, disk, id, ids, pid};

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
        unread,
    } = read::<Gadgets>(&value);
    assert!(unread.is_none());
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
        Edit::ApplyPreset { id: pid("a") },
        Edit::SavePreset { name: "n".into() },
        Edit::DeletePreset { id: pid("a") },
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
        serde_json::from_value(json!({ "add": { "widget": { "kind": "clock" } } }))
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
        serde_json::from_value(json!("resetToDefaults")).expect("deserialises");
    assert_eq!(named, Edit::ResetToDefaults);
    let preset: Edit<Gadget> =
        serde_json::from_value(json!({ "applyPreset": { "id": "compact" } }))
            .expect("deserialises");
    assert_eq!(preset, Edit::ApplyPreset { id: pid("compact") });
    assert!(
        serde_json::from_value::<Edit<Gadget>>(json!({ "applyPreset": { "id": "No" } })).is_err()
    );
}

#[test]
fn widget_states_have_the_agreed_shape() {
    type State = WidgetState<u8, String>;
    let action = || WidgetAction {
        action: "open".to_string(),
        label: "Open alarms".into(),
    };
    let ready: State = WidgetState::Ready {
        value: 5,
        as_of: 1_700_000_000_000,
        stale_since: None,
    };
    assert_eq!(
        serde_json::to_value(&ready).expect("serialises"),
        json!({ "state": "ready", "value": 5, "asOf": 1_700_000_000_000i64, "staleSince": null })
    );
    let states: Vec<State> = vec![
        ready,
        WidgetState::Ready {
            value: 5,
            as_of: 1,
            stale_since: Some(2),
        },
        WidgetState::Working {
            title: "Checking backups".into(),
            phase: "Reading volumes".into(),
            progress: Progress::Count {
                done: 3,
                unit: "volumes".into(),
            },
            last: Some(4),
            cancel: Some(action()),
        },
        WidgetState::Working {
            title: "t".into(),
            phase: "p".into(),
            progress: Progress::Fraction {
                done: 1,
                total: 4,
                unit: "files".into(),
            },
            last: None,
            cancel: None,
        },
        WidgetState::Working {
            title: "Checking".into(),
            phase: "Time Machine".into(),
            progress: Progress::Unknown,
            last: None,
            cancel: None,
        },
        WidgetState::Empty {
            title: "No alarms".into(),
            detail: "All clear".into(),
            next: Some(action()),
            calm: true,
        },
        WidgetState::Error {
            title: "Couldn't read".into(),
            cause: None,
            next: None,
        },
    ];
    for state in states {
        let back: State = serde_json::from_value(serde_json::to_value(&state).expect("serialises"))
            .expect("deserialises");
        assert_eq!(back, state);
    }
    let placed: PlacementState<u8, String> = PlacementState {
        id: id("clock"),
        state: WidgetState::Ready {
            value: 1,
            as_of: 0,
            stale_since: Some(1),
        },
    };
    let back: PlacementState<u8, String> =
        serde_json::from_value(serde_json::to_value(&placed).expect("serialises"))
            .expect("deserialises");
    assert_eq!(back, placed);
}

#[test]
fn surface_values_carry_a_notice_and_the_placements() {
    let values: SurfaceValues<u8, String> = SurfaceValues {
        notice: Some(SurfaceNotice {
            tone: Tone::Danger,
            title: "Agent stopped".into(),
            detail: "Values are not updating.".into(),
            action: Some(WidgetAction {
                action: "restart".into(),
                label: "Restart agent".into(),
            }),
        }),
        placements: vec![PlacementState {
            id: id("clock"),
            state: WidgetState::Empty {
                title: "t".into(),
                detail: "d".into(),
                next: None,
                calm: false,
            },
        }],
    };
    let value = serde_json::to_value(&values).expect("serialises");
    assert_eq!(value["notice"]["tone"], "danger");
    assert_eq!(value["placements"][0]["state"]["state"], "empty");
    let back: SurfaceValues<u8, String> = serde_json::from_value(value).expect("deserialises");
    assert_eq!(back, values);
    for tone in [Tone::Info, Tone::Warning] {
        let back: Tone = serde_json::from_value(serde_json::to_value(tone).expect("serialises"))
            .expect("deserialises");
        assert_eq!(back, tone);
    }
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
        ids(&Layout::new(2, placed, vec![])),
        ["disk-space", "disk-space-2", "clock", "disk-space-3"]
    );
}

#[test]
fn an_empty_state_is_not_calm_unless_the_engine_says_so() {
    let old = serde_json::json!({"state": "empty", "title": "t", "detail": "d", "next": null});
    let state: WidgetState<u8, String> =
        serde_json::from_value(old).expect("an empty state without calm still reads");
    assert!(matches!(state, WidgetState::Empty { calm: false, .. }));
    let calm = serde_json::to_value(WidgetState::<u8, String>::Empty {
        title: "t".into(),
        detail: "d".into(),
        next: None,
        calm: true,
    })
    .expect("serialises");
    assert_eq!(calm["calm"], true);
}

#[test]
fn unknown_progress_is_a_bare_word_on_the_wire() {
    assert_eq!(
        serde_json::to_value(Progress::Unknown).expect("serialises"),
        "unknown"
    );
    let back: Progress =
        serde_json::from_value(serde_json::json!("unknown")).expect("deserialises");
    assert_eq!(back, Progress::Unknown);
}
