//! The generated TypeScript of the MCP adapter, checked on every test run (a stale file fails CI): the wire types (ts-rs) and the
//! command names, in the `shell/mcp` entry point of `@decryp7/paddock-ui`.

use crate::audit::Outcome;
use crate::control::{
    AuditRow, ErrorCode, IssuedKey, KeyInfo, McpError, McpEvent, McpSettings, McpView, ToolInfo,
};
use crate::listener::ListenerView;
use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-mcp --lib generated";

fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/projects/paddock-ui/shell/mcp/generated")
}

fn generated_files() -> Vec<(&'static str, String)> {
    let header = format!(
        "// Generated from the Rust wire types of paddock-mcp (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        Outcome::decl(),
        McpSettings::decl(),
        ToolInfo::decl(),
        KeyInfo::decl(),
        ListenerView::decl(),
        McpView::decl(),
        IssuedKey::decl(),
        AuditRow::decl(),
        ErrorCode::decl(),
        McpError::decl(),
        McpEvent::decl(),
    ];
    let types: Vec<String> = decls.into_iter().map(|d| format!("export {d}\n")).collect();
    let consts = format!(
        "\n/** The event every audit change is sent on, to every window. */\nexport const MCP_EVENT = '{}';\n\n\
/** The most client keys one app keeps. */\nexport const MAX_KEYS = {};\n",
        crate::EVENT_NAME,
        crate::MAX_KEYS
    );
    vec![
        ("mcp.ts", format!("{header}{}{consts}", types.join("\n"))),
        (
            "mcp-commands.ts",
            paddock_commands::ts::names_module(
                &crate::COMMANDS,
                "MCP_COMMANDS",
                "McpCommand",
                REGENERATE,
            )
            .unwrap_or_default(),
        ),
    ]
}

#[test]
fn generated_typescript_is_current() {
    let mode = WriteMode::from_env();
    let stale: Vec<String> = generated_files()
        .into_iter()
        .filter_map(|(name, text)| {
            testing::sync_generated(&generated_dir().join(name), &text, mode).err()
        })
        .map(|e| e.to_string())
        .collect();
    assert!(stale.is_empty(), "{}", stale.join("\n"));
}

#[test]
fn the_table_has_the_mcp_commands() {
    let names: Vec<&str> = crate::COMMANDS.specs().iter().map(|s| s.name()).collect();
    assert_eq!(
        names,
        [
            "mcp_get",
            "mcp_save",
            "mcp_issue_key",
            "mcp_revoke_key",
            "mcp_audit",
            "mcp_audit_clear"
        ]
    );
}

/// The dialog contracts of this crate as TypeScript (ADR 0026 §3), beside its other generated files.
#[test]
fn generated_dialog_contracts_are_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/mcp/generated/dialog-contracts.ts");
    let text = paddock_dialogs::ts::contracts_module("paddock-mcp", &crate::DIALOGS, REGENERATE);
    testing::sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}
