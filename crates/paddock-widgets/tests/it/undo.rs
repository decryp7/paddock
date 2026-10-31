//! One level of undo for the destructive edits.

use paddock_widgets::{Edit, Layout, WidgetError, apply};

use crate::catalogue::{Gadget, Gadgets, defaults, disk, id, ids};

fn run(layout: &Layout<Gadget>, edit: Edit<Gadget>) -> Layout<Gadget> {
    apply::<Gadgets>(layout, edit).expect("the edit applies")
}

#[test]
fn a_fresh_layout_has_nothing_to_undo() {
    assert!(!defaults().can_undo());
    assert_eq!(
        apply::<Gadgets>(&defaults(), Edit::Undo),
        Err(WidgetError::NothingToUndo)
    );
}

#[test]
fn undo_after_remove() {
    let removed = run(&defaults(), Edit::Remove { id: id("clock") });
    assert!(removed.can_undo());
    assert_eq!(run(&removed, Edit::Undo).placements, defaults().placements);
}

#[test]
fn undo_after_a_preset() {
    let applied = run(&defaults(), Edit::ApplyPreset { id: id("compact") });
    assert!(applied.can_undo());
    assert_eq!(run(&applied, Edit::Undo).placements, defaults().placements);
}

#[test]
fn undo_after_reset() {
    let changed = run(&defaults(), Edit::Remove { id: id("clock") });
    let reset = run(&changed, Edit::ResetToDefaults);
    assert_eq!(run(&reset, Edit::Undo).placements, changed.placements);
}

#[test]
fn undo_is_one_level() {
    let undone = run(
        &run(&defaults(), Edit::Remove { id: id("clock") }),
        Edit::Undo,
    );
    assert!(!undone.can_undo());
    assert_eq!(
        apply::<Gadgets>(&undone, Edit::Undo),
        Err(WidgetError::NothingToUndo)
    );
}

#[test]
fn a_placement_edit_clears_the_level() {
    let removed = run(&defaults(), Edit::Remove { id: id("clock") });
    for edit in [
        Edit::Move {
            id: id("alarms"),
            to: 0,
        },
        Edit::Resize {
            id: id("disk-space"),
            span: 2,
        },
        Edit::Replace {
            id: id("disk-space"),
            widget: disk("x"),
        },
        Edit::Add {
            widget: Gadget::Clock,
            span: None,
            at: None,
        },
    ] {
        assert!(!run(&removed, edit).can_undo());
    }
}

#[test]
fn presets_edits_keep_the_level() {
    let removed = run(&defaults(), Edit::Remove { id: id("clock") });
    let saved = run(
        &removed,
        Edit::SavePreset {
            name: "Mine".into(),
        },
    );
    assert!(saved.can_undo());
    let deleted = run(&saved, Edit::DeletePreset { id: id("mine") });
    assert!(deleted.can_undo());
    assert_eq!(
        ids(&run(&deleted, Edit::Undo)),
        ["clock", "disk-space", "alarms"]
    );
}

#[test]
fn undo_does_not_touch_saved_presets() {
    let removed = run(
        &run(
            &defaults(),
            Edit::SavePreset {
                name: "Mine".into(),
            },
        ),
        Edit::Remove { id: id("clock") },
    );
    assert_eq!(run(&removed, Edit::Undo).presets.len(), 1);
}
