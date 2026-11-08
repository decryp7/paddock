//! Any JSON gives a valid layout and no panic; any run of edits keeps the layout valid.

use paddock_widgets::{Edit, PlacementId, PresetId, apply, read};
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::catalogue::{Gadget, Gadgets, defaults};

fn any_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        ".{0,12}".prop_map(Value::from),
    ];
    leaf.prop_recursive(4, 48, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::hash_map("[a-z]{1,10}", inner, 0..6)
                .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

/// Documents that look like layouts: the right keys, entries that are mostly almost right.
fn layoutish() -> impl Strategy<Value = Value> {
    let widget = prop_oneof![
        Just(json!({ "kind": "clock" })),
        "[a-z ]{0,6}".prop_map(|v| json!({ "kind": "diskSpace", "volume": v })),
        any::<u8>().prop_map(|r| json!({ "kind": "alarms", "rows": r })),
        Just(json!({ "kind": "hidden" })),
        any_json(),
    ];
    let entry = (
        prop_oneof![
            Just("a".to_string()),
            Just("b".to_string()),
            "[a-zA-Z0-9-]{0,70}"
        ],
        0u8..5,
        widget,
    )
        .prop_map(|(id, span, widget)| json!({ "id": id, "span": span, "widget": widget }));
    let preset = (
        prop_oneof![Just("compact".to_string()), "[a-z0-9-]{0,6}"],
        ".{0,8}",
        prop::collection::vec(entry.clone(), 0..8),
    )
        .prop_map(
            |(id, name, placements)| json!({ "id": id, "name": name, "placements": placements }),
        );
    (0u64..4, prop::collection::vec(entry, 0..24), prop::collection::vec(preset, 0..8))
        .prop_map(|(version, placements, presets)| json!({ "version": version, "placements": placements, "presets": presets }))
}

fn arbitrary_edit() -> impl Strategy<Value = Edit<Gadget>> {
    let pid = prop_oneof![
        Just("clock"),
        Just("disk-space"),
        Just("disk-space-2"),
        Just("alarms"),
        Just("compact"),
        Just("mine")
    ]
    .prop_map(|s| PlacementId::new(s).expect("a valid id"));
    let widget = prop_oneof![
        Just(Gadget::Clock),
        "[a-z ]{0,20}".prop_map(|volume| Gadget::DiskSpace { volume }),
        any::<u8>().prop_map(|rows| Gadget::Alarms { rows }),
        Just(Gadget::Hidden),
    ];
    prop_oneof![
        (
            widget.clone(),
            prop::option::of(0u8..4),
            prop::option::of(0u16..8)
        )
            .prop_map(|(widget, span, at)| Edit::Add { widget, span, at }),
        pid.clone().prop_map(|id| Edit::Remove { id }),
        (pid.clone(), 0u16..8).prop_map(|(id, to)| Edit::Move { id, to }),
        (pid.clone(), 0u8..4).prop_map(|(id, span)| Edit::Resize { id, span }),
        (pid.clone(), widget.clone()).prop_map(|(id, widget)| Edit::Replace { id, widget }),
        (pid.clone(), widget).prop_map(|(id, widget)| Edit::ChangeKind { id, widget }),
        prop_oneof![Just("compact"), Just("mine"), Just("clock")].prop_map(|s| Edit::ApplyPreset {
            id: PresetId::new(s).expect("a valid id")
        }),
        ".{0,80}".prop_map(|name| Edit::SavePreset { name }),
        prop_oneof![Just("compact"), Just("mine"), Just("clock")].prop_map(|s| {
            Edit::DeletePreset {
                id: PresetId::new(s).expect("a valid id"),
            }
        }),
        Just(Edit::ResetToDefaults),
        Just(Edit::Undo),
    ]
}

proptest! {
    #[test]
    fn any_json_reads_to_a_valid_layout(value in any_json()) {
        let r = read::<Gadgets>(&value);
        prop_assert_eq!(r.layout.validate::<Gadgets>(), Ok(()));
        prop_assert!(r.notes.len() <= 1);
    }

    #[test]
    fn layout_like_json_reads_to_a_valid_layout(value in layoutish()) {
        let r = read::<Gadgets>(&value);
        prop_assert_eq!(r.layout.validate::<Gadgets>(), Ok(()));
        prop_assert!(r.notes.len() <= 1);
        prop_assert!(r.layout.placements.len() <= 4);
        // Reading what was read changes nothing and says nothing.
        let again = read::<Gadgets>(&serde_json::to_value(&r.layout).expect("serialises"));
        prop_assert_eq!(&again.layout, &r.layout);
        prop_assert!(again.notes.is_empty());
    }

    #[test]
    fn edits_never_break_the_layout(edits in prop::collection::vec(arbitrary_edit(), 0..40)) {
        let mut layout = defaults();
        for edit in edits {
            if let Ok(next) = apply::<Gadgets>(&layout, edit) {
                prop_assert_eq!(next.validate::<Gadgets>(), Ok(()));
                layout = next;
            }
        }
    }
}

/// Documents the reader sees, for a surface that needs at least one widget: presets may hold few or no valid entries.
fn needy_documents() -> impl Strategy<Value = Value> {
    let entry = prop_oneof![
        Just(json!({ "id": "a", "span": 1, "widget": { "kind": "clock" } })),
        Just(json!({ "id": "b", "span": 2, "widget": { "kind": "diskSpace", "volume": "all" } })),
        Just(json!({ "id": "c", "span": 1, "widget": { "kind": "alarms", "rows": 1 } })),
        Just(json!({ "id": "bad", "span": 7, "widget": { "kind": "clock" } })),
        Just(json!(3)),
    ];
    let preset = (
        prop_oneof![Just("mine"), Just("p2"), Just("compact")],
        prop::collection::vec(entry.clone(), 0..5),
    )
        .prop_map(|(id, placements)| json!({ "id": id, "name": id, "placements": placements }));
    (prop::collection::vec(entry, 0..5), prop::collection::vec(preset, 0..4))
        .prop_map(|(placements, presets)| json!({ "version": 2, "placements": placements, "presets": presets }))
}

proptest! {
    #[test]
    fn a_surface_that_needs_a_widget_never_goes_below_its_fewest(
        document in needy_documents(),
        edits in prop::collection::vec(arbitrary_edit(), 0..30),
    ) {
        type Needy = crate::catalogue::Cat<1, 2>;
        let mut layout = read::<Needy>(&document).layout;
        prop_assert_eq!(layout.validate::<Needy>(), Ok(()));
        for edit in edits {
            if let Ok(next) = apply::<Needy>(&layout, edit) {
                prop_assert!(!next.placements.is_empty());
                prop_assert_eq!(next.validate::<Needy>(), Ok(()));
                layout = next;
            }
        }
    }
}
