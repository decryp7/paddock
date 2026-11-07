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
        crate::Span::decl(),
        crate::Cells::decl(),
        crate::StepsInfo::decl(),
        crate::KindSize::decl(),
        crate::Block::decl(),
        crate::Placed::decl(),
        crate::Arrangement::decl(),
        crate::PlacementMoves::decl(),
        crate::PreviewRequest::decl(),
        crate::PreviewReply::<Dummy>::decl(),
        crate::Outcome::decl(),
        crate::Rect::decl(),
        crate::Dir::decl(),
        crate::GridGesture::decl(),
        crate::Moves::decl(),
        crate::Placement::<Dummy, Dummy>::decl(),
        crate::Preset::<Dummy, Dummy>::decl(),
        crate::Layout::<Dummy, Dummy>::decl(),
        crate::Edit::<Dummy>::decl(),
        crate::SurfaceInfo::<Dummy>::decl(),
        crate::KindInfo::<Dummy>::decl(),
        crate::CatalogueInfo::<Dummy, Dummy>::decl(),
        crate::LayoutView::<Dummy, Dummy, Dummy, Dummy>::decl(),
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

/// Golden cases of the grid engine for the UI's mock backend (ADR 0044 §6, D6): written by this test from the real
/// engine, so the mock is never the reference. `PADDOCK_WRITE_TS=1` rewrites the file.
fn grid_cases() -> String {
    use crate::grid::{Dir, GridGesture, Item, Rect, Size, SizeRule, Steps, arrange};
    use crate::id::PlacementId;
    use serde_json::{Value, json};

    const RULE: SizeRule = SizeRule {
        widths: Steps::Range { min: 2, max: 12 },
        heights: Steps::Range { min: 2, max: 14 },
        default: Size { w: 3, h: 3 },
    };
    let id = |s: &str| PlacementId::try_from(s.to_string()).expect("a valid id");
    let item = |n: &str, x: u8, y: u16, w: u8, h: u8| Item {
        id: id(n),
        rect: Rect::new(x, y, w, h),
        rule: RULE,
    };
    let tiles = || {
        vec![
            item("a", 0, 0, 6, 4),
            item("b", 6, 0, 6, 4),
            item("c", 0, 4, 6, 4),
            item("d", 6, 4, 6, 4),
        ]
    };
    let rects = |items: &[Item]| -> Value {
        items
            .iter()
            .map(|i| json!({ "id": i.id, "rect": i.rect }))
            .collect()
    };
    let cases: Vec<(&str, Vec<Item>, GridGesture)> = vec![
        (
            "drop on a same-size tile swaps",
            tiles(),
            GridGesture::Place {
                id: id("a"),
                rect: Rect::new(6, 0, 6, 4),
            },
        ),
        (
            "drop across two tiles pushes them down",
            tiles(),
            GridGesture::Place {
                id: id("a"),
                rect: Rect::new(3, 4, 6, 4),
            },
        ),
        (
            "drop below everything rises",
            tiles(),
            GridGesture::Place {
                id: id("a"),
                rect: Rect::new(0, 30, 6, 4),
            },
        ),
        (
            "resize taller pushes neighbours",
            tiles(),
            GridGesture::Place {
                id: id("a"),
                rect: Rect::new(0, 0, 6, 8),
            },
        ),
        (
            "resize is clamped to the rule",
            tiles(),
            GridGesture::Place {
                id: id("a"),
                rect: Rect::new(0, 0, 40, 40),
            },
        ),
        (
            "nudge right swaps a same-size neighbour",
            tiles(),
            GridGesture::Nudge {
                id: id("a"),
                dir: Dir::Right,
            },
        ),
        (
            "nudge down passes the widget below",
            tiles(),
            GridGesture::Nudge {
                id: id("a"),
                dir: Dir::Down,
            },
        ),
        (
            "nudge at the edge is blocked",
            tiles(),
            GridGesture::Nudge {
                id: id("a"),
                dir: Dir::Left,
            },
        ),
        (
            "grow wider by one step",
            vec![item("a", 0, 0, 4, 3)],
            GridGesture::Grow {
                id: id("a"),
                dir: Dir::Right,
            },
        ),
        (
            "grow at the grid edge is blocked",
            vec![item("a", 8, 0, 4, 3)],
            GridGesture::Grow {
                id: id("a"),
                dir: Dir::Right,
            },
        ),
        (
            "home moves to the first place",
            tiles(),
            GridGesture::ToFirst { id: id("d") },
        ),
        (
            "end moves after the last one",
            tiles(),
            GridGesture::ToLast { id: id("a") },
        ),
    ];
    let out: Vec<Value> = cases
        .into_iter()
        .map(|(name, items, gesture)| {
            let result = match arrange(&items, 12, &gesture) {
                Ok(next) => json!({ "ok": rects(&next) }),
                Err(e) => json!({ "error": e.to_string() }),
            };
            json!({
                "name": name,
                "columns": 12,
                "widths": { "min": 2, "max": 12 },
                "heights": { "min": 2, "max": 14 },
                "items": rects(&items),
                "gesture": gesture,
                "result": result,
            })
        })
        .collect();
    serde_json::to_string_pretty(&out).expect("cases serialise") + "\n"
}

#[test]
fn grid_cases_are_current() {
    testing::sync_generated(
        &path("grid-cases.json"),
        &grid_cases(),
        WriteMode::from_env(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
}
