//! `Stored`: a settings field that never fails its file.

use paddock_widgets::{Edit, NoteCode, Stored, apply};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::catalogue::{Gadgets, defaults, doc, id, ids, stored};

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
struct Settings {
    #[serde(default)]
    theme: String,
    #[serde(default)]
    popover: Stored<Gadgets>,
}

#[test]
fn a_missing_field_is_the_defaults() {
    let s: Settings = serde_json::from_value(json!({ "theme": "dark" })).expect("settings load");
    assert_eq!(s.popover.layout(), &defaults());
    assert!(s.popover.notes().is_empty());
    assert_eq!(
        s,
        Settings {
            theme: "dark".into(),
            ..Settings::default()
        }
    );
}

#[test]
fn a_damaged_field_never_fails_the_file() {
    for value in [
        json!(5),
        json!("x"),
        json!([1]),
        json!({ "version": 9, "placements": [] }),
    ] {
        let s: Settings = serde_json::from_value(json!({ "theme": "dark", "popover": value }))
            .expect("settings load");
        assert_eq!(s.theme, "dark");
        assert_eq!(s.popover.layout().placements, defaults().placements);
        assert_eq!(s.popover.notes().len(), 1);
    }
}

#[test]
fn it_reads_from_text_and_writes_only_the_layout() {
    let text = json!({ "popover": doc(json!([stored("clock", 1, json!({ "kind": "clock" }))])) })
        .to_string();
    let s: Settings = serde_json::from_str(&text).expect("settings load");
    assert_eq!(ids(s.popover.layout()), ["clock"]);
    let out = serde_json::to_value(&s).expect("settings save");
    assert_eq!(out["popover"]["version"], 2);
    assert!(out["popover"].get("notes").is_none());
    assert!(out["popover"].get("previous").is_none());
    let back: Settings = serde_json::from_value(out).expect("settings load again");
    assert_eq!(back.popover.layout(), s.popover.layout());
}

#[test]
fn notes_are_shown_once() {
    let mut s: Stored<Gadgets> = serde_json::from_value(json!(3)).expect("loads");
    assert_eq!(s.notes().len(), 1);
    let view = s.view();
    assert_eq!(view.notes.len(), 1);
    assert_eq!(view.notes[0].code, NoteCode::Damaged);
    let taken = s.take_notes();
    assert_eq!(taken.len(), 1);
    assert!(s.take_notes().is_empty());
    assert!(s.view().notes.is_empty());
}

#[test]
fn an_edited_layout_is_stored_without_notes() {
    let edited =
        apply::<Gadgets>(&defaults(), Edit::Remove { id: id("clock") }).expect("the edit applies");
    let s = Stored::<Gadgets>::new(edited.clone());
    assert!(s.notes().is_empty());
    assert!(s.view().can_undo);
    assert_eq!(s.layout(), &edited);
    let copy = s.clone();
    assert_eq!(copy, s);
    assert!(format!("{copy:?}").contains("Stored"));
}
