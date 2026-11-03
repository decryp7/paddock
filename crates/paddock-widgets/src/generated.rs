//! The generated TypeScript of this crate's wire types (`ts-rs`), checked on every test run: a stale file fails, and
//! `PADDOCK_WRITE_TS=1` rewrites it. Generic types are declared over `ts_rs::Dummy`, so the file keeps their parameters.

use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::{Dummy, TS};

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-widgets --lib generated";

/// Where the widgets entry point's generated TypeScript goes: `@decryp7/paddock-ui/widgets`.
fn path(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/widgets/generated")
        .join(file)
}

fn types() -> String {
    let header = format!(
        "// Generated from the Rust wire types of paddock-widgets (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        crate::PlacementId::decl(),
        crate::PresetId::decl(),
        crate::Counted::decl(),
        crate::WidgetError::decl(),
        crate::NoteCode::decl(),
        crate::Note::decl(),
        crate::Refresh::decl(),
        crate::Rect::decl(),
        crate::Dir::decl(),
        crate::GridGesture::decl(),
        crate::Moves::decl(),
        crate::Placement::<Dummy>::decl(),
        crate::Preset::<Dummy>::decl(),
        crate::Layout::<Dummy>::decl(),
        crate::Edit::<Dummy>::decl(),
        crate::SurfaceInfo::<Dummy>::decl(),
        crate::KindInfo::<Dummy>::decl(),
        crate::CatalogueInfo::<Dummy, Dummy>::decl(),
        crate::LayoutView::<Dummy, Dummy, Dummy>::decl(),
        crate::WidgetAction::<Dummy>::decl(),
        crate::Progress::decl(),
        crate::WidgetState::<Dummy, Dummy>::decl(),
        crate::PlacementState::<Dummy, Dummy>::decl(),
        crate::Tone::decl(),
        crate::SurfaceNotice::<Dummy>::decl(),
        crate::SurfaceValues::<Dummy, Dummy>::decl(),
    ];
    let body: String = decls
        .into_iter()
        .map(|d| format!("export {d}\n\n"))
        .collect();
    format!("{header}{}", body.trim_end().to_string() + "\n")
}

#[test]
fn generated_typescript_is_current() {
    testing::sync_generated(&path("widgets.ts"), &types(), WriteMode::from_env())
        .unwrap_or_else(|e| panic!("{e}"));
}
