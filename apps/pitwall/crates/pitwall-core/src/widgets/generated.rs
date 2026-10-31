//! The generated TypeScript of Pitwall's widget wire types (`ts-rs`), checked on every test run: a stale file fails and
//! `PADDOCK_WRITE_TS=1` rewrites it (ADR 0026 section 3, family ADR 0035 section 3). The generic layout and state types
//! are the shared package's (`@decryp7/paddock-ui/widgets`), not repeated here.

use super::*;
use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p pitwall-core --lib widgets::generated";

fn path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/generated/widgets.ts")
}

fn types() -> String {
    let header = format!("// Generated from Pitwall's widget wire types (ts-rs). Do not edit: {REGENERATE}.\n\n");
    let decls = [
        // Pitwall's catalogue.
        PitwallSurface::decl(),
        PopoverWidget::decl(),
        LoadingRows::decl(),
        // What the popup's widgets show.
        ServerState::decl(),
        EngineHealth::decl(),
        MeterTone::decl(),
        MemoryLevel::decl(),
        VramReading::decl(),
        ModelRowState::decl(),
        ModelRow::decl(),
        DownloadRowState::decl(),
        DownloadRow::decl(),
        ErrorRow::decl(),
        PopoverStatus::decl(),
        ServingToggle::decl(),
        PopoverHeader::decl(),
        Page::decl(),
        PopoverValue::decl(),
        WidgetActionId::decl(),
    ];
    let body: String = decls.into_iter().map(|d| format!("export {d}\n\n")).collect();
    format!("{header}{}", body.trim_end().to_string() + "\n")
}

#[test]
fn generated_typescript_is_current() {
    testing::sync_generated(&path(), &types(), WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}
