//! Every edit: success and each error, and each limit at and past its bound.

use paddock_widgets::{Counted, Edit, Layout, WidgetError, apply};

use crate::catalogue::{Cat, Gadget, Gadgets, defaults, disk, id, ids, pid};

fn run(layout: &Layout<Gadget>, edit: Edit<Gadget>) -> Layout<Gadget> {
    apply::<Gadgets>(layout, edit).expect("the edit applies")
}

fn fail(layout: &Layout<Gadget>, edit: Edit<Gadget>) -> WidgetError {
    apply::<Gadgets>(layout, edit).expect_err("the edit is refused")
}

fn add(widget: Gadget) -> Edit<Gadget> {
    Edit::Add {
        widget,
        span: None,
        at: None,
    }
}

fn empty() -> Layout<Gadget> {
    run(
        &run(
            &run(&defaults(), Edit::Remove { id: id("clock") }),
            Edit::Remove {
                id: id("disk-space"),
            },
        ),
        Edit::Remove { id: id("alarms") },
    )
}

#[test]
fn add_appends_with_the_default_span_and_a_derived_id() {
    let layout = run(&empty(), add(Gadget::Clock));
    assert_eq!(ids(&layout), ["clock"]);
    assert_eq!(layout.placements[0].span, 1);
    let layout = run(&layout, add(disk("all")));
    assert_eq!(ids(&layout), ["clock", "disk-space"]);
    assert_eq!(layout.placements[1].span, 2);
}

#[test]
fn add_at_a_position_and_past_the_end() {
    let base = run(
        &defaults(),
        Edit::Remove {
            id: id("disk-space"),
        },
    );
    let first = run(
        &base,
        Edit::Add {
            widget: disk("x"),
            span: None,
            at: Some(0),
        },
    );
    assert_eq!(ids(&first), ["disk-space", "clock", "alarms"]);
    let last = run(
        &base,
        Edit::Add {
            widget: disk("x"),
            span: None,
            at: Some(99),
        },
    );
    assert_eq!(ids(&last), ["clock", "alarms", "disk-space"]);
}

#[test]
fn a_second_instance_gets_a_numbered_id_and_a_third_is_refused() {
    let two = run(&defaults(), add(disk("backup")));
    assert_eq!(ids(&two), ["clock", "disk-space", "alarms", "disk-space-2"]);
    let smaller = run(&two, Edit::Remove { id: id("alarms") });
    assert_eq!(
        fail(&smaller, add(disk("third"))),
        WidgetError::KindLimit { max: 2 }
    );
}

#[test]
fn add_takes_a_span_the_kind_allows() {
    let base = run(&defaults(), Edit::Remove { id: id("clock") });
    let wide = run(
        &base,
        Edit::Add {
            widget: Gadget::Clock,
            span: Some(2),
            at: None,
        },
    );
    assert_eq!(wide.placements.last().map(|p| p.span), Some(2));
    assert_eq!(
        fail(
            &base,
            Edit::Add {
                widget: Gadget::Clock,
                span: Some(3),
                at: None
            }
        ),
        WidgetError::SpanNotAllowed {
            allowed: vec![1, 2]
        }
    );
}

#[test]
fn add_clamps_then_validates_the_options() {
    let base = empty();
    let layout = run(&base, add(Gadget::Alarms { rows: 9 }));
    assert_eq!(layout.placements[0].widget, Gadget::Alarms { rows: 3 });
    let trimmed = run(&base, add(disk("  all  ")));
    assert_eq!(trimmed.placements[0].widget, disk("all"));
    assert_eq!(
        fail(&base, add(disk("   "))),
        WidgetError::Invalid("Choose a volume of 1 to 16 characters.".into())
    );
}

#[test]
fn add_refuses_a_kind_outside_the_catalogue() {
    assert!(
        matches!(fail(&empty(), add(Gadget::Hidden)), WidgetError::Invalid(m) if m.contains("not available"))
    );
}

#[test]
fn the_surface_limit_is_checked_at_and_past_the_bound() {
    let four = run(&defaults(), add(disk("b")));
    assert_eq!(four.placements.len(), 4);
    assert_eq!(
        fail(&four, add(Gadget::Clock)),
        WidgetError::TooMany {
            counted: Counted::Placements,
            max: 4,
        }
    );
    let three = run(
        &four,
        Edit::Remove {
            id: id("disk-space-2"),
        },
    );
    assert_eq!(three.placements.len(), 3);
}

#[test]
fn a_kind_at_its_limit_is_refused() {
    assert_eq!(
        fail(&defaults(), add(Gadget::Clock)),
        WidgetError::KindLimit { max: 1 }
    );
}

#[test]
fn remove_and_its_errors() {
    let layout = run(
        &defaults(),
        Edit::Remove {
            id: id("disk-space"),
        },
    );
    assert_eq!(ids(&layout), ["clock", "alarms"]);
    assert_eq!(
        fail(&layout, Edit::Remove { id: id("nope") }),
        WidgetError::UnknownPlacement("nope".into())
    );
}

#[test]
fn remove_cannot_go_below_the_fewest() {
    type Strict = Cat<1, 2>;
    let one = apply::<Strict>(
        &apply::<Strict>(
            &Layout::defaults::<Strict>(),
            Edit::Remove { id: id("clock") },
        )
        .unwrap(),
        Edit::Remove {
            id: id("disk-space"),
        },
    )
    .unwrap();
    assert_eq!(one.placements.len(), 1);
    let error = apply::<Strict>(&one, Edit::Remove { id: id("alarms") }).unwrap_err();
    assert_eq!(error, WidgetError::TooFew { min: 1 });
}

#[test]
fn move_to_the_start_the_end_and_past_the_end() {
    let start = run(
        &defaults(),
        Edit::Move {
            id: id("alarms"),
            to: 0,
        },
    );
    assert_eq!(ids(&start), ["alarms", "clock", "disk-space"]);
    let end = run(
        &defaults(),
        Edit::Move {
            id: id("clock"),
            to: 2,
        },
    );
    assert_eq!(ids(&end), ["disk-space", "alarms", "clock"]);
    let past = run(
        &defaults(),
        Edit::Move {
            id: id("clock"),
            to: 999,
        },
    );
    assert_eq!(ids(&past), ["disk-space", "alarms", "clock"]);
    assert_eq!(
        fail(
            &defaults(),
            Edit::Move {
                id: id("nope"),
                to: 0
            }
        ),
        WidgetError::UnknownPlacement("nope".into())
    );
}

#[test]
fn resize_and_its_errors() {
    let wide = run(
        &defaults(),
        Edit::Resize {
            id: id("clock"),
            span: 2,
        },
    );
    assert_eq!(wide.placements[0].span, 2);
    assert_eq!(
        fail(
            &defaults(),
            Edit::Resize {
                id: id("clock"),
                span: 3
            }
        ),
        WidgetError::SpanNotAllowed {
            allowed: vec![1, 2]
        }
    );
    assert_eq!(
        fail(
            &defaults(),
            Edit::Resize {
                id: id("nope"),
                span: 1
            }
        ),
        WidgetError::UnknownPlacement("nope".into())
    );
}

#[test]
fn replace_changes_options_not_the_kind() {
    let layout = run(
        &defaults(),
        Edit::Replace {
            id: id("disk-space"),
            widget: disk("  system "),
        },
    );
    assert_eq!(layout.placements[1].widget, disk("system"));
    assert!(matches!(
        fail(&defaults(), Edit::Replace { id: id("clock"), widget: disk("x") }),
        WidgetError::Invalid(m) if m.contains("keeps its kind")
    ));
    assert_eq!(
        fail(
            &defaults(),
            Edit::Replace {
                id: id("disk-space"),
                widget: disk("")
            }
        ),
        WidgetError::Invalid("Choose a volume of 1 to 16 characters.".into())
    );
    assert_eq!(
        fail(
            &defaults(),
            Edit::Replace {
                id: id("nope"),
                widget: disk("x")
            }
        ),
        WidgetError::UnknownPlacement("nope".into())
    );
}

#[test]
fn apply_a_builtin_preset_and_a_saved_one() {
    let compact = run(&defaults(), Edit::ApplyPreset { id: pid("compact") });
    assert_eq!(ids(&compact), ["clock"]);
    assert_eq!(compact.placements[0].span, 2);
    let saved = run(
        &defaults(),
        Edit::SavePreset {
            name: "Mine".into(),
        },
    );
    let emptied = run(&saved, Edit::Remove { id: id("clock") });
    let again = run(&emptied, Edit::ApplyPreset { id: pid("mine") });
    assert_eq!(again.placements, defaults().placements);
}

#[test]
fn apply_refuses_an_unknown_preset_and_an_invalid_saved_one() {
    assert_eq!(
        fail(&defaults(), Edit::ApplyPreset { id: pid("nope") }),
        WidgetError::UnknownPreset("nope".into())
    );
    let mut bad = run(&defaults(), Edit::SavePreset { name: "Bad".into() });
    bad.presets[0].placements =
        paddock_widgets::assign_ids([(1, Gadget::Clock), (1, Gadget::Clock)]);
    assert_eq!(
        fail(&bad, Edit::ApplyPreset { id: pid("bad") }),
        WidgetError::KindLimit { max: 1 }
    );
}

#[test]
fn save_a_preset_with_a_cleaned_name_and_a_unique_id() {
    let one = run(
        &defaults(),
        Edit::SavePreset {
            name: " \u{7}My  layout! ".into(),
        },
    );
    assert_eq!(one.presets[0].name, "My  layout!");
    assert_eq!(one.presets[0].id.as_str(), "my-layout");
    assert!(!one.presets[0].builtin);
    assert_eq!(one.presets[0].placements, defaults().placements);
    let clash = run(
        &defaults(),
        Edit::SavePreset {
            name: "Compact".into(),
        },
    );
    assert_eq!(clash.presets[0].id.as_str(), "compact-2");
    let twice = run(
        &clash,
        Edit::SavePreset {
            name: "Compact".into(),
        },
    );
    assert_eq!(twice.presets[1].id.as_str(), "compact-3");
    let symbols = run(&defaults(), Edit::SavePreset { name: "!!!".into() });
    assert_eq!(symbols.presets[0].id.as_str(), "layout");
    let long = run(
        &defaults(),
        Edit::SavePreset {
            name: "x".repeat(100),
        },
    );
    assert_eq!(long.presets[0].name.chars().count(), 64);
}

#[test]
fn save_a_preset_refusals() {
    let msg = WidgetError::Invalid("Give the preset a name of 1 to 64 characters.".into());
    assert_eq!(
        fail(
            &defaults(),
            Edit::SavePreset {
                name: "  \u{7} ".into()
            }
        ),
        msg
    );
    let two = run(
        &run(&defaults(), Edit::SavePreset { name: "a".into() }),
        Edit::SavePreset { name: "b".into() },
    );
    assert_eq!(
        fail(&two, Edit::SavePreset { name: "c".into() }),
        WidgetError::TooMany {
            counted: Counted::UserPresets,
            max: 2,
        }
    );
    let four = run(&defaults(), add(disk("b")));
    assert_eq!(
        fail(&four, Edit::SavePreset { name: "big".into() }),
        WidgetError::TooMany {
            counted: Counted::PresetPlacements,
            max: 3,
        }
    );
}

#[test]
fn delete_a_preset_and_its_errors() {
    let saved = run(
        &defaults(),
        Edit::SavePreset {
            name: "Mine".into(),
        },
    );
    assert!(
        run(&saved, Edit::DeletePreset { id: pid("mine") })
            .presets
            .is_empty()
    );
    assert_eq!(
        fail(&saved, Edit::DeletePreset { id: pid("compact") }),
        WidgetError::BuiltinPreset
    );
    assert_eq!(
        fail(&saved, Edit::DeletePreset { id: pid("nope") }),
        WidgetError::UnknownPreset("nope".into())
    );
}

#[test]
fn reset_restores_the_defaults() {
    let changed = run(&defaults(), Edit::Remove { id: id("clock") });
    assert_eq!(
        run(&changed, Edit::ResetToDefaults).placements,
        defaults().placements
    );
}

#[test]
fn an_edit_leaves_its_input_untouched() {
    let before = defaults();
    let _ = run(&before, Edit::Remove { id: id("clock") });
    assert_eq!(before, defaults());
}

#[test]
fn validate_names_the_first_problem() {
    assert_eq!(defaults().validate::<Gadgets>(), Ok(()));
    let mut too_many = defaults();
    too_many.presets = (0..3)
        .map(|i| paddock_widgets::Preset {
            id: pid(&format!("p{i}")),
            name: "p".into(),
            builtin: false,
            placements: vec![],
        })
        .collect();
    assert_eq!(
        too_many.validate::<Gadgets>(),
        Err(WidgetError::TooMany {
            counted: Counted::UserPresets,
            max: 2,
        })
    );
    let mut clash = defaults();
    clash.presets = vec![paddock_widgets::Preset {
        id: pid("compact"),
        name: "c".into(),
        builtin: false,
        placements: vec![],
    }];
    assert_eq!(
        clash.validate::<Gadgets>(),
        Err(WidgetError::DuplicateId("compact".into()))
    );
    let mut twin = defaults();
    let p = paddock_widgets::Preset {
        id: pid("p"),
        name: "p".into(),
        builtin: false,
        placements: vec![],
    };
    twin.presets = vec![p.clone(), p];
    assert_eq!(
        twin.validate::<Gadgets>(),
        Err(WidgetError::DuplicateId("p".into()))
    );
    let mut big = defaults();
    big.presets = vec![paddock_widgets::Preset {
        id: pid("p"),
        name: "p".into(),
        builtin: false,
        placements: run(&defaults(), add(disk("b"))).placements,
    }];
    assert_eq!(
        big.validate::<Gadgets>(),
        Err(WidgetError::TooMany {
            counted: Counted::PresetPlacements,
            max: 3,
        })
    );
    type Strict = Cat<1, 2>;
    let none = Layout::new::<Strict>(vec![], vec![]);
    assert!(none.validate::<Strict>().is_err());
}

#[test]
fn validate_checks_the_placements_too() {
    let mut twin = defaults();
    twin.placements.push(twin.placements[0].clone());
    assert_eq!(
        twin.validate::<Gadgets>(),
        Err(WidgetError::DuplicateId("clock".into()))
    );
}
