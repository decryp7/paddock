//! The generated TypeScript of this crate's wire types (`ts-rs`) and command names, checked on every test run: a stale
//! file fails, and `PADDOCK_WRITE_TS=1` rewrites it.

use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-window --lib generated";

/// Where the shell's generated TypeScript goes: the `shell` entry point of `@decryp7/paddock-ui`.
fn path(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/generated")
        .join(file)
}

fn types() -> String {
    let header = format!(
        "// Generated from the Rust wire types of paddock-window (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        crate::WindowInfo::decl(),
        crate::OpenAsk::decl(),
        crate::WindowErrorCode::decl(),
        crate::WindowError::decl(),
        crate::DeepLink::decl(),
        crate::OpenRequest::decl(),
        crate::OnClose::decl(),
        crate::CloseChoice::decl(),
        crate::OpenChoice::decl(),
    ];
    let types: String = decls
        .into_iter()
        .map(|d| format!("export {d}\n\n"))
        .collect();
    format!(
        "{header}{types}\
/** The events of the window layer: to every window (`windows`) and to one window (the others). */
export const WINDOW_EVENTS = {{
  windows: '{}',
  closeRequested: '{}',
  openAsk: '{}',
  open: '{}',
}} as const;
",
        crate::WINDOWS_EVENT,
        crate::CLOSE_REQUESTED_EVENT,
        crate::OPEN_ASK_EVENT,
        crate::OPEN_EVENT,
    )
}

#[test]
fn generated_typescript_is_current() {
    let mode = WriteMode::from_env();
    testing::sync_generated(&path("window.ts"), &types(), mode).unwrap_or_else(|e| panic!("{e}"));
    let names = paddock_commands::ts::names_module(
        &crate::COMMANDS,
        "WINDOW_COMMANDS",
        "WindowCommand",
        REGENERATE,
    )
    .unwrap_or_default();
    testing::sync_generated(&path("window-commands.ts"), &names, mode)
        .unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn every_command_is_async() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    testing::assert_async(&crate::COMMANDS, &[root.as_path()]);
}
