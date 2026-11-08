//! Undo and redo: twenty steps for every placement edit.

use paddock_widgets::{Edit, Layout, WidgetError, apply};

use crate::catalogue::{Gadget, Gadgets, defaults, disk, id, ids, pid};

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
    let applied = run(&defaults(), Edit::ApplyPreset { id: pid("compact") });
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
fn undo_goes_back_step_by_step_and_redo_forward() {
    let one = run(&defaults(), Edit::Remove { id: id("clock") });
    let two = run(&one, Edit::Remove { id: id("alarms") });
    let undone = run(&two, Edit::Undo);
    assert_eq!(undone.placements, one.placements);
    assert!(undone.can_undo() && undone.can_redo());
    let again = run(&undone, Edit::Undo);
    assert_eq!(again.placements, defaults().placements);
    assert!(!again.can_undo());
    assert_eq!(
        apply::<Gadgets>(&again, Edit::Undo),
        Err(WidgetError::NothingToUndo)
    );
    let redone = run(&run(&again, Edit::Redo), Edit::Redo);
    assert_eq!(redone.placements, two.placements);
    assert!(!redone.can_redo());
    assert_eq!(
        apply::<Gadgets>(&redone, Edit::Redo),
        Err(WidgetError::NothingToRedo)
    );
}

#[test]
fn every_placement_edit_is_a_step_and_clears_redo() {
    let removed = run(&defaults(), Edit::Remove { id: id("clock") });
    let undone = run(&removed, Edit::Undo);
    assert!(undone.can_redo());
    for edit in [
        Edit::Move {
            id: id("alarms"),
            to: 0,
        },
        Edit::Resize {
            id: id("clock"),
            span: 2,
        },
        Edit::Replace {
            id: id("disk-space"),
            widget: disk("x"),
        },
        Edit::Add {
            widget: disk("y"),
            span: None,
            at: None,
        },
        Edit::Remove { id: id("alarms") },
    ] {
        let next = run(&undone, edit);
        assert!(next.can_undo(), "a step");
        assert!(!next.can_redo(), "a new edit clears redo");
    }
}

#[test]
fn an_edit_that_changes_nothing_is_not_a_step() {
    let same = run(
        &defaults(),
        Edit::Resize {
            id: id("disk-space"),
            span: 2,
        },
    );
    assert!(!same.can_undo());
}

#[test]
fn at_most_twenty_steps_are_kept() {
    let mut layout = defaults();
    for n in 0..30 {
        let edit = Edit::Replace {
            id: id("disk-space"),
            widget: disk(&format!("v{n}")),
        };
        layout = run(&layout, edit);
    }
    let mut steps = 0;
    while layout.can_undo() {
        layout = run(&layout, Edit::Undo);
        steps += 1;
    }
    assert_eq!(steps, paddock_widgets::UNDO_DEPTH);
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
    let deleted = run(&saved, Edit::DeletePreset { id: pid("mine") });
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
