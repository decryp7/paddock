//! What the chooser gets: the catalogue as data, with the layout.

use paddock_widgets::{Edit, Refresh, apply, catalogue_info, view};
use serde_json::json;

use crate::catalogue::{GadgetKind, Gadgets, defaults, id};

#[test]
fn the_catalogue_is_data() {
    let info = catalogue_info::<Gadgets>();
    assert_eq!(info.surface.id, "popover");
    assert_eq!(
        (
            info.surface.columns,
            info.surface.max_placements,
            info.surface.min_placements
        ),
        (2, 4, 0)
    );
    assert_eq!(
        (
            info.surface.max_user_presets,
            info.surface.max_preset_placements
        ),
        (2, 3)
    );
    let titles: Vec<&str> = info.kinds.iter().map(|k| k.title.as_str()).collect();
    assert_eq!(titles, ["Clock", "Disk space", "Active alarms"]);
    assert_eq!(info.kinds[0].kind, GadgetKind::Clock);
    assert_eq!(info.kinds[0].spans, [1, 2]);
    assert_eq!(info.kinds[1].max_instances, 2);
    assert_eq!(info.kinds[1].refresh, Refresh::Every { secs: 60 });
    assert_eq!(info.kinds[2].icon, "bell");
}

#[test]
fn the_view_carries_the_layout_presets_notes_and_undo() {
    let removed =
        apply::<Gadgets>(&defaults(), Edit::Remove { id: id("clock") }).expect("the edit applies");
    let v = view::<Gadgets>(removed, vec![]);
    assert!(v.can_undo);
    assert_eq!(v.builtin_presets.len(), 1);
    assert!(v.builtin_presets[0].builtin);
    let fresh = view::<Gadgets>(defaults(), vec![]);
    assert!(!fresh.can_undo);
    let value = serde_json::to_value(&fresh).expect("serialises");
    assert_eq!(value["catalogue"]["kinds"][1]["maxInstances"], 2);
    assert_eq!(
        value["catalogue"]["kinds"][2]["refresh"],
        json!({ "type": "onChange" })
    );
    assert_eq!(value["canUndo"], false);
    assert_eq!(value["builtinPresets"][0]["id"], "compact");
}
