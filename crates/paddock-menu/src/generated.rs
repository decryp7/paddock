//! The generated TypeScript of this crate's wire types (`ts-rs`), checked on every test run: a stale file fails, and
//! `PADDOCK_WRITE_TS=1` rewrites it.

use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-menu --lib generated";

/// Where the menu's generated TypeScript goes: the `shell/menu` entry point of `@decryp7/paddock-ui`.
fn path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/menu/generated/menu.ts")
}

fn text() -> String {
    let header = format!(
        "// Generated from the Rust wire types of paddock-menu (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        crate::NativeMenuEntry::decl(),
        crate::NativeTopMenu::decl(),
        crate::NativeMenuModel::decl(),
        crate::MenuApplied::decl(),
        crate::MenuSetErrorCode::decl(),
        crate::MenuSetError::decl(),
    ];
    let types: String = decls
        .into_iter()
        .map(|d| format!("export {d}\n\n"))
        .collect();
    format!(
        "{header}{types}\
/** The command that sends a window's menus to the native menu bar. */
export const MENU_SET_COMMAND = '{}';

/** The event a native menu click arrives as (to the focused top-level window); the payload is the item id. */
export const MENU_EVENT = '{}';

/** The id of the shell's Quit command (last in the macOS app menu). */
export const QUIT_COMMAND = '{}';

/** The limits `menu_set` enforces. */
export const MENU_LIMITS = {{ menus: {}, entries: {}, depth: {}, id: {}, label: {} }} as const;
",
        crate::MENU_SET_COMMAND,
        crate::MENU_EVENT,
        crate::QUIT_COMMAND,
        crate::MAX_MENUS,
        crate::MAX_ENTRIES,
        crate::MAX_DEPTH,
        crate::MAX_ID,
        crate::MAX_LABEL,
    )
}

#[test]
fn generated_typescript_is_current() {
    let synced = testing::sync_generated(&path(), &text(), WriteMode::from_env());
    synced.unwrap_or_else(|e| panic!("{e}"));
}
