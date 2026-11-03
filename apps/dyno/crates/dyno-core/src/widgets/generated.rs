//! The generated TypeScript of Dyno's widget wire types (`ts-rs`), checked on every test run: a stale file fails and
//! `PADDOCK_WRITE_TS=1` rewrites it (ADR 0026 §3, family ADR 0035 §3). The file also carries the shared crate's generic
//! types the Dyno types are built on, so the app compiles on its own until it imports them from `@decryp7/paddock-ui`.

use super::popover_values::{
    AlarmRow, AppRow, BackupValue, BackupVerdict, CounterRow, IssueValue, Level, Navigate, Page, PopoverNoticeKind, PopoverSurfaceValues,
    PopoverValue, SettingsSection, VolumeRow, WidgetActionId,
};
use super::*;
use paddock_commands::testing::{self, WriteMode};
use paddock_widgets as pw;
use std::path::{Path, PathBuf};
use ts_rs::{Dummy, TS};

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p dyno-core --lib widgets::generated";

fn path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/generated/widgets.ts")
}

fn types() -> String {
    let header = format!("// Generated from Dyno's widget wire types and paddock-widgets (ts-rs). Do not edit: {REGENERATE}.\n\n");
    let decls = [
        // paddock-widgets (the generic layout and state envelope).
        pw::PlacementId::decl(),
        pw::PresetId::decl(),
        pw::Counted::decl(),
        pw::WidgetError::decl(),
        pw::NoteCode::decl(),
        pw::Note::decl(),
        pw::Refresh::decl(),
        pw::Span::decl(),
        pw::Cells::decl(),
        pw::StepsInfo::decl(),
        pw::KindSize::decl(),
        pw::Block::decl(),
        pw::Placed::decl(),
        pw::Arrangement::decl(),
        pw::PlacementMoves::decl(),
        pw::PreviewRequest::decl(),
        pw::Rect::decl(),
        pw::Dir::decl(),
        pw::GridGesture::decl(),
        pw::Moves::decl(),
        pw::Placement::<Dummy, Dummy>::decl(),
        pw::Preset::<Dummy, Dummy>::decl(),
        pw::Layout::<Dummy, Dummy>::decl(),
        pw::Edit::<Dummy>::decl(),
        pw::SurfaceInfo::<Dummy>::decl(),
        pw::KindInfo::<Dummy>::decl(),
        pw::CatalogueInfo::<Dummy, Dummy>::decl(),
        pw::LayoutView::<Dummy, Dummy, Dummy, Dummy>::decl(),
        pw::WidgetAction::<Dummy>::decl(),
        pw::Progress::decl(),
        pw::WidgetState::<Dummy, Dummy>::decl(),
        pw::PlacementState::<Dummy, Dummy>::decl(),
        pw::Tone::decl(),
        pw::SurfaceNotice::<Dummy>::decl(),
        pw::SurfaceValues::<Dummy, Dummy>::decl(),
        // Dyno's catalogues.
        DynoSurface::decl(),
        Metric::decl(),
        VolumeChoice::decl(),
        PopoverWidget::decl(),
        PopoverKind::decl(),
        Thresholds::decl(),
        WidgetOptions::decl(),
        DashboardKind::decl(),
        DashboardWidget::decl(),
        MenuBarItem::decl(),
        MenuBarKind::decl(),
        // What the popup's widgets show.
        crate::widgets::dashboard::ValueTone::decl(),
        crate::counters::CounterUnit::decl(),
        Page::decl(),
        SettingsSection::decl(),
        Navigate::decl(),
        WidgetActionId::decl(),
        Level::decl(),
        AlarmRow::decl(),
        VolumeRow::decl(),
        CounterRow::decl(),
        AppRow::decl(),
        BackupVerdict::decl(),
        BackupValue::decl(),
        IssueValue::decl(),
        PopoverValue::decl(),
        // What the choosers offer and what a reading says.
        MetricChoice::decl(),
        DashboardKindChoices::decl(),
        PopoverKindChoices::decl(),
        MenuBarKindChoices::decl(),
        WidgetChoices::decl(),
        MetricReading::decl(),
        ReadingQuery::decl(),
        PopoverNoticeKind::decl(),
        PopoverSurfaceValues::decl(),
        // The wire of the link.
        SurfaceView::decl(),
        SurfaceEdit::decl(),
    ];
    let body: String = decls.into_iter().map(|d| format!("export {d}\n\n")).collect();
    format!("{header}{}", body.trim_end().to_string() + "\n")
}

#[test]
fn generated_typescript_is_current() {
    testing::sync_generated(&path(), &types(), WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}
