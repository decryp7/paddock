//! The generated TypeScript of this crate's wire types (`ts-rs`) and command names, checked on every test run: a stale
//! file fails, and `PADDOCK_WRITE_TS=1` rewrites it.

use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-panels --lib generated";

/// Where the generated TypeScript goes: the `workbench/panels` entry point of `@decryp7/paddock-ui`.
fn path(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/workbench/panels/generated")
        .join(file)
}

fn types() -> String {
    let header = format!(
        "// Generated from the Rust wire types of paddock-panels (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        crate::PopOutBounds::decl(),
        crate::PopOutRequest::decl(),
        crate::PopOutInfo::decl(),
        crate::Look::decl(),
        crate::PanelsEvent::decl(),
        crate::SyncScope::decl(),
        crate::SyncPublish::decl(),
        crate::GhostRequest::decl(),
        crate::PanelErrorCode::decl(),
        crate::PanelError::decl(),
    ];
    let types: String = decls
        .into_iter()
        .map(|d| format!("export {d}\n\n"))
        .collect();
    format!(
        "{header}{types}\
/** The events of the panel layer: to the host window only (`panels`) and to the other windows of a host (`uiSync`). */
export const PANEL_EVENTS = {{
  panels: '{}',
  uiSync: '{}',
}} as const;

/** The most pop-outs one window can have, the largest forwarded UI message (bytes) and the longest instance key. */
export const PANEL_LIMITS = {{
  maxPopOuts: {},
  maxMessageBytes: {},
  maxKey: {},
}} as const;
",
        crate::PANELS_EVENT,
        crate::UI_SYNC_EVENT,
        crate::MAX_POPOUTS,
        crate::sync::MAX_MESSAGE_BYTES,
        crate::MAX_KEY,
    )
}

#[test]
fn generated_typescript_is_current() {
    let mode = WriteMode::from_env();
    testing::sync_generated(&path("panels.ts"), &types(), mode).unwrap();
    let names = paddock_commands::ts::names_module(
        &crate::COMMANDS,
        "PANEL_COMMANDS",
        "PanelCommand",
        REGENERATE,
    )
    .unwrap_or_default();
    testing::sync_generated(&path("panel-commands.ts"), &names, mode).unwrap();
}

#[test]
fn every_command_is_async() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    testing::assert_async(&crate::COMMANDS, &[root.as_path()]);
}
