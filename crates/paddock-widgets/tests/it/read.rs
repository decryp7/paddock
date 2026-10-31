//! Reading: every kind of bad input falls back or drops on its own, with one note.

use paddock_widgets::{Catalogue, NoteCode, Read, read};
use serde_json::{Value, json};

use crate::catalogue::{Cat, Gadget, Gadgets, defaults, disk, doc, ids, stored};

fn go(value: &Value) -> Read<Gadgets> {
    read::<Gadgets>(value)
}

fn clock(id: &str) -> Value {
    stored(id, 1, json!({ "kind": "clock" }))
}

fn alarms(id: &str) -> Value {
    stored(id, 1, json!({ "kind": "alarms", "rows": 2 }))
}

fn volume(id: &str) -> Value {
    stored(id, 2, json!({ "kind": "diskSpace", "volume": "all" }))
}

fn dropped<C: Catalogue>(read: &Read<C>) -> Option<u32> {
    read.notes.iter().find_map(|n| match n.code {
        NoteCode::Dropped { count } => Some(count),
        _ => None,
    })
}

fn fell_back(read: &Read<Gadgets>, code: NoteCode) {
    assert_eq!(read.layout.placements, defaults().placements);
    assert!(read.layout.presets.is_empty());
    assert_eq!(read.notes.len(), 1, "one note: {:?}", read.notes);
    assert_eq!(read.notes[0].code, code);
    let message = &read.notes[0].message;
    assert!(message.starts_with("The popup layout"), "{message}");
    assert!(message.contains("the defaults are shown"), "{message}");
    if !matches!(code, NoteCode::Newer { .. }) {
        assert!(
            message.ends_with("the old layout is kept until you change this one."),
            "{message}"
        );
    }
}

#[test]
fn missing_is_the_defaults_without_a_note() {
    let r = go(&Value::Null);
    assert_eq!(r.layout, defaults());
    assert!(r.notes.is_empty());
}

#[test]
fn a_valid_layout_is_read_as_it_is() {
    let r = go(&doc(json!([volume("disk"), clock("clock")])));
    assert_eq!(ids(&r.layout), ["disk", "clock"]);
    assert!(r.notes.is_empty());
    assert_eq!(r.layout.placements[0].widget, disk("all"));
}

#[test]
fn an_older_version_is_read() {
    let r = go(&json!({ "version": 1, "placements": [clock("clock")] }));
    assert_eq!(ids(&r.layout), ["clock"]);
    assert_eq!(r.layout.version, 2);
    assert!(r.notes.is_empty());
}

#[test]
fn damaged_input_gives_the_defaults_and_one_note() {
    for value in [
        json!([1, 2]),
        json!("text"),
        json!(7),
        json!(true),
        json!({}),
        json!({ "version": "2", "placements": [] }),
        json!({ "version": 0, "placements": [] }),
        json!({ "version": -1, "placements": [] }),
        json!({ "version": 2 }),
        json!({ "version": 2, "placements": "none" }),
        json!({ "version": 2, "placements": { "a": 1 } }),
    ] {
        let r = go(&value);
        fell_back(&r, NoteCode::Damaged);
        // Nothing is lost: the value is kept to be written back unchanged.
        assert_eq!(r.unread, Some(value));
    }
}

#[test]
fn a_newer_version_gives_the_defaults_keeps_the_value_and_one_note() {
    let newer = json!({ "version": 3, "placements": [clock("clock")] });
    let r = go(&newer);
    fell_back(&r, NoteCode::Newer { version: 3 });
    assert!(r.notes[0].message.contains("newer version (3)"));
    assert_eq!(r.unread, Some(newer));
    let huge = json!({ "version": 99999999999u64, "placements": 5 });
    let r = go(&huge);
    fell_back(&r, NoteCode::Newer { version: u32::MAX });
    assert_eq!(r.unread, Some(huge));
}

#[test]
fn an_oversized_layout_gives_the_defaults_and_one_note() {
    let big = doc(json!([stored(
        "clock",
        1,
        json!({ "kind": "clock", "pad": "x".repeat(5000) })
    )]));
    let r = go(&big);
    fell_back(&r, NoteCode::TooLarge);
    assert_eq!(r.unread, Some(big));
    let just_under = doc(json!([clock("clock")]));
    assert!(go(&just_under).notes.is_empty());
}

#[test]
fn a_bad_entry_is_dropped_on_its_own_with_a_count() {
    let entries = json!([
        clock("clock"),
        stored("x", 1, json!({ "kind": "nope" })),
        json!("not a placement"),
        json!({ "id": "UPPER", "span": 1, "widget": { "kind": "alarms", "rows": 1 } }),
        stored("dup", 1, json!({ "kind": "alarms", "rows": 1 })),
        stored("dup", 2, json!({ "kind": "diskSpace", "volume": "a" })),
        stored("wide", 3, json!({ "kind": "alarms", "rows": 1 })),
        stored("hidden", 1, json!({ "kind": "hidden" })),
        stored("blank", 2, json!({ "kind": "diskSpace", "volume": "" })),
    ]);
    let r = go(&doc(entries));
    assert_eq!(ids(&r.layout), ["clock", "dup"]);
    assert_eq!(dropped(&r), Some(7));
    assert_eq!(r.notes.len(), 1);
    assert_eq!(
        r.notes[0].message,
        "7 widgets in the saved popup layout were not valid and were removed."
    );
}

#[test]
fn a_duplicate_id_and_a_kind_over_its_limit_are_dropped() {
    let r = go(&doc(json!([
        clock("a"),
        clock("a"),
        clock("b"),
        alarms("c")
    ])));
    assert_eq!(ids(&r.layout), ["a", "c"]);
    assert_eq!(dropped(&r), Some(2));
}

#[test]
fn placements_over_the_surface_limit_are_dropped() {
    let entries = json!([
        clock("a"),
        volume("b"),
        volume("c"),
        alarms("d"),
        volume("e")
    ]);
    let r = go(&doc(entries));
    assert_eq!(ids(&r.layout), ["a", "b", "c", "d"]);
    assert_eq!(dropped(&r), Some(1));
}

#[test]
fn only_twice_the_limit_is_looked_at_the_rest_is_dropped_unread() {
    let entries: Vec<Value> = (0..40).map(|i| volume(&format!("v{i}"))).collect();
    let r = go(&doc(Value::Array(entries)));
    assert_eq!(r.layout.placements.len(), 2);
    assert_eq!(dropped(&r), Some(38));
}

#[test]
fn ranges_are_clamped_on_read() {
    let r = go(&doc(json!([stored(
        "a",
        1,
        json!({ "kind": "alarms", "rows": 200 })
    )])));
    assert_eq!(r.layout.placements[0].widget, Gadget::Alarms { rows: 3 });
}

#[test]
fn when_every_entry_is_dropped_the_defaults_placements_are_used() {
    let r = go(&doc(json!([
        stored("x", 1, json!({ "kind": "nope" })),
        json!(5)
    ])));
    assert_eq!(r.layout.placements, defaults().placements);
    assert_eq!(dropped(&r), Some(2));
}

#[test]
fn an_empty_layout_is_allowed_when_the_surface_allows_none() {
    let r = go(&doc(json!([])));
    assert!(r.layout.placements.is_empty());
    assert!(r.notes.is_empty());
}

#[test]
fn an_empty_layout_gets_the_defaults_when_the_surface_needs_one() {
    let r = read::<Cat<1, 2>>(&doc(json!([])));
    assert_eq!(r.layout.placements, defaults().placements);
    assert!(r.notes.is_empty());
}

#[test]
fn presets_are_read_cleaned_and_kept_apart_from_builtin_ones() {
    let r = go(&json!({
        "version": 2,
        "placements": [clock("clock")],
        "presets": [
            { "id": "mine", "name": "  \u{7}Mine  ", "placements": [clock("c"), volume("d")] },
            { "id": "builtin-claim", "name": "x", "builtin": true, "placements": [] },
        ],
    }));
    assert_eq!(r.layout.presets.len(), 1);
    assert_eq!(r.layout.presets[0].name, "Mine");
    assert!(!r.layout.presets[0].builtin);
    assert_eq!(r.layout.presets[0].placements.len(), 2);
    assert!(r.notes.is_empty());
}

#[test]
fn a_bad_preset_is_dropped_on_its_own() {
    let r = read::<Cat<0, 6>>(&json!({
        "version": 2,
        "placements": [clock("clock")],
        "presets": [
            { "id": "compact", "name": "clashes with built-in", "placements": [] },
            { "id": "ok", "name": "ok", "placements": [] },
            { "id": "ok", "name": "twin", "placements": [] },
            { "id": "BAD", "name": "bad id", "placements": [] },
            { "id": "blank", "name": " \u{7} ", "placements": [] },
            { "id": "shape", "name": "no placements" },
            "text",
            { "id": "third", "name": "third", "placements": [] },
        ],
    }));
    let names: Vec<&str> = r.layout.presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["ok", "third"]);
    assert_eq!(dropped(&r), Some(6));
}

#[test]
fn a_preset_drops_its_own_bad_placements_and_limits() {
    let r = go(&json!({
        "version": 2,
        "placements": [clock("clock")],
        "presets": [{ "id": "p", "name": "p", "placements": [clock("a"), clock("b"), json!(1), volume("c"), volume("d"), alarms("e")] }],
    }));
    let ids: Vec<String> = r.layout.presets[0]
        .placements
        .iter()
        .map(|p| p.id.to_string())
        .collect();
    assert_eq!(ids, ["a", "c", "d"]);
    assert_eq!(dropped(&r), Some(3));
}

#[test]
fn only_twice_the_preset_limit_is_looked_at() {
    let presets: Vec<Value> = (0..9)
        .map(|i| json!({ "id": format!("p{i}"), "name": "p", "placements": [] }))
        .collect();
    let r = go(&json!({ "version": 2, "placements": [clock("clock")], "presets": presets }));
    assert_eq!(r.layout.presets.len(), 2);
    assert_eq!(dropped(&r), Some(7));
}

#[test]
fn presets_of_a_surface_without_any_are_dropped() {
    let r = read::<Cat<0, 0>>(&json!({
        "version": 2,
        "placements": [clock("clock")],
        "presets": [{ "id": "p", "name": "p", "placements": [] }],
    }));
    assert!(r.layout.presets.is_empty());
    assert_eq!(r.notes.len(), 1);
}

#[test]
fn presets_that_are_not_a_list_are_dropped_with_a_count() {
    let r = go(&json!({ "version": 2, "placements": [clock("clock")], "presets": 5 }));
    assert!(r.layout.presets.is_empty());
    assert_eq!(dropped(&r), Some(1));
    let none = go(&json!({ "version": 2, "placements": [clock("clock")], "presets": null }));
    assert!(none.notes.is_empty());
}

#[test]
fn the_read_layout_is_always_valid() {
    let r = go(&doc(json!([
        clock("a"),
        volume("b"),
        json!(null),
        volume("b")
    ])));
    assert_eq!(r.layout.validate::<Gadgets>(), Ok(()));
}

#[test]
fn one_dropped_widget_is_singular() {
    let r = go(&doc(json!([clock("a"), json!(1)])));
    assert_eq!(
        r.notes[0].message,
        "1 widget in the saved popup layout was not valid and was removed."
    );
}

#[test]
fn a_valid_placement_after_the_scanned_entries_is_not_kept() {
    // The surface holds 4, so 8 entries are looked at: the first, then 7 invalid ones, then a valid one that is unread.
    let mut entries = vec![clock("first")];
    entries.extend((0..7).map(|_| json!(1)));
    entries.push(alarms("late"));
    let r = go(&doc(Value::Array(entries)));
    assert_eq!(ids(&r.layout), ["first"]);
    assert_eq!(dropped(&r), Some(8));
}

#[test]
fn a_valid_preset_after_the_scanned_presets_is_not_kept() {
    // Two saved presets fit, so 4 entries are looked at.
    let good = |id: &str| json!({ "id": id, "name": id, "placements": [] });
    let presets = json!([good("first"), 1, 2, 3, good("late")]);
    let r = go(&json!({ "version": 2, "placements": [clock("clock")], "presets": presets }));
    let names: Vec<&str> = r.layout.presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["first"]);
    assert_eq!(dropped(&r), Some(4));
}

#[test]
fn a_valid_placement_after_the_scanned_preset_entries_is_not_kept() {
    // A saved preset holds 3, so 6 entries are looked at.
    let mut entries = vec![clock("first")];
    entries.extend((0..5).map(|_| json!(1)));
    entries.push(alarms("late"));
    let presets = json!([{ "id": "p", "name": "p", "placements": entries }]);
    let r = go(&json!({ "version": 2, "placements": [clock("clock")], "presets": presets }));
    let kept: Vec<String> = r.layout.presets[0]
        .placements
        .iter()
        .map(|p| p.id.to_string())
        .collect();
    assert_eq!(kept, ["first"]);
    assert_eq!(dropped(&r), Some(6));
}

#[test]
fn the_size_limit_is_inclusive() {
    let of_len = |len: usize| {
        let base = json!({ "version": 2, "placements": [], "pad": "" });
        let pad = len - base.to_string().len();
        json!({ "version": 2, "placements": [], "pad": "x".repeat(pad) })
    };
    let exact = of_len(4096);
    assert_eq!(exact.to_string().len(), 4096);
    assert!(go(&exact).notes.is_empty());
    let over = of_len(4097);
    assert_eq!(go(&over).notes[0].code, NoteCode::TooLarge);
}

#[test]
fn a_saved_preset_that_keeps_too_few_widgets_is_dropped() {
    let r = read::<Cat<1, 2>>(&json!({
        "version": 2,
        "placements": [clock("clock")],
        "presets": [
            { "id": "empty", "name": "empty", "placements": [] },
            { "id": "bad", "name": "bad", "placements": [json!(1)] },
            { "id": "ok", "name": "ok", "placements": [clock("c")] },
        ],
    }));
    let names: Vec<&str> = r.layout.presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["ok"]);
    assert_eq!(dropped(&r), Some(3));
}
