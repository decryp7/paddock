//! The tray popup's layout and values through the engine facade (family ADR 0035): edits are saved and survive a restart,
//! a refused edit or an unwritable file changes nothing, the settings page's save never touches the layout, and the values
//! follow the watches. Everything lives in a temporary folder; nothing leaves the machine.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use paddock_widgets::Edit;
use spotter_core::time::Timestamp;
use spotter_core::widgets::{PopoverValue, PopoverWidget};
use spotter_engine::clock::ManualClock;
use spotter_engine::notifier::RecordingNotifier;
use spotter_engine::secrets::MemorySecrets;
use spotter_engine::{Change, Engine, EngineConfig, EngineError, Services};
use std::path::Path;
use std::sync::Arc;

const T0: i64 = 1_800_000_000_000;

fn open(dir: &Path) -> Engine {
    let mut svc = Services::new(Arc::new(MemorySecrets::new()));
    svc.notifier = Arc::new(RecordingNotifier::default());
    svc.clock = Arc::new(ManualClock::new(Timestamp(T0)));
    Engine::open(EngineConfig::new(dir, "test"), svc).unwrap()
}

fn kinds(engine: &Engine) -> Vec<PopoverWidget> {
    engine.popover_view().layout.placements.iter().map(|p| p.widget).collect()
}

fn id_of(engine: &Engine, kind: PopoverWidget) -> paddock_widgets::PlacementId {
    engine
        .popover_view()
        .layout
        .placements
        .iter()
        .find(|p| p.widget == kind)
        .unwrap()
        .id
        .clone()
}

#[test]
fn a_fresh_engine_shows_the_default_layout() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(dir.path());
    let view = engine.popover_view();
    assert!(view.editable && !view.can_undo);
    assert_eq!(kinds(&engine)[0], PopoverWidget::RecentChanges);
    assert_eq!(view.catalogue.kinds.len(), 6);
}

#[test]
fn an_edit_is_saved_announced_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(dir.path());
    let mut rx = engine.subscribe();
    let id = id_of(&engine, PopoverWidget::Failures);
    let view = engine.edit_popover(Edit::Remove { id }).unwrap();
    assert!(view.can_undo);
    assert!(!kinds(&engine).contains(&PopoverWidget::Failures));
    assert!(matches!(rx.try_recv(), Ok(Change::Settings)));
    drop(engine);
    let again = open(dir.path());
    assert!(!kinds(&again).contains(&PopoverWidget::Failures), "the layout is in spotter.toml");
}

#[test]
fn a_refused_edit_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(dir.path());
    let before = kinds(&engine);
    let err = engine
        .edit_popover(Edit::Add {
            widget: PopoverWidget::Failures,
            span: None,
            at: None,
        })
        .unwrap_err();
    assert_eq!(err.code(), "badRequest");
    assert_eq!(kinds(&engine), before);
}

#[test]
fn an_unwritable_file_refuses_the_edit_and_keeps_the_layout() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(dir.path());
    let p = engine.config_path();
    let _ = std::fs::remove_file(&p);
    std::fs::create_dir_all(p.join("blocker")).unwrap();
    let before = kinds(&engine);
    let id = id_of(&engine, PopoverWidget::Failures);
    assert!(matches!(engine.edit_popover(Edit::Remove { id }), Err(EngineError::Internal(_))));
    assert_eq!(kinds(&engine), before);
}

#[test]
fn the_settings_pages_save_never_changes_the_layout() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(dir.path());
    let id = id_of(&engine, PopoverWidget::Failures);
    engine.edit_popover(Edit::Remove { id }).unwrap();
    let mut from_the_page = engine.settings();
    from_the_page.popover = Default::default();
    from_the_page.notifications = false;
    let saved = engine.save_settings(from_the_page).unwrap();
    assert!(!saved.notifications);
    assert!(!kinds(&engine).contains(&PopoverWidget::Failures), "the page's stale copy of the layout was ignored");
}

#[test]
fn a_damaged_layout_in_the_file_gives_the_defaults_and_is_kept_until_the_first_edit() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(dir.path());
    let path = engine.config_path();
    drop(engine);
    std::fs::write(&path, "schema = 3\n[settings.popover]\nversion = 0\n").unwrap();
    let engine = open(dir.path());
    let view = engine.popover_view();
    assert!(view.editable);
    assert_eq!(view.notes.len(), 1, "one note says the layout could not be read");
    assert_eq!(kinds(&engine)[0], PopoverWidget::RecentChanges);
    // Kept until the person changes it: the first edit replaces it.
    let id = id_of(&engine, PopoverWidget::Failures);
    engine.edit_popover(Edit::Remove { id }).unwrap();
    assert!(std::fs::read_to_string(&path).unwrap().contains("version = 1"));
}

#[test]
fn values_follow_the_layout() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(dir.path());
    let values = engine.popover_values();
    assert_eq!(values.placements.len(), engine.popover_view().layout.placements.len());
    let id = id_of(&engine, PopoverWidget::Failures);
    engine.edit_popover(Edit::Remove { id }).unwrap();
    assert_eq!(engine.popover_values().placements.len(), values.placements.len() - 1);
    // No watches: every placed widget is an empty state that starts something, none is an error.
    assert!(engine
        .popover_values()
        .placements
        .iter()
        .all(|p| !matches!(p.state, paddock_widgets::WidgetState::Error { .. })));
    let _ = PopoverValue::Failures { rows: vec![], more: 0 };
}
