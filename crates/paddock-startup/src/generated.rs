//! The generated TypeScript of the start-up contract, checked on every test run (a stale file fails CI): the wire types
//! (ts-rs) and the command names, in the `shell/startup` entry point of `@decryp7/paddock-ui`.

use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-startup --lib generated";

fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/startup/generated")
}

fn generated_files() -> Vec<(&'static str, String)> {
    let header = format!(
        "// Generated from the Rust wire types of paddock-startup (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        crate::Recovery::decl(),
        crate::ReadyReply::decl(),
        crate::Mode::decl(),
        crate::SessionInfo::decl(),
        crate::CrashInfo::decl(),
        crate::Offers::decl(),
        crate::UncleanShutdown::decl(),
        crate::StartReport::decl(),
        crate::RestorePlan::decl(),
        crate::BootErrorKind::decl(),
        PingEvent::decl(),
    ];
    let types: Vec<String> = decls.into_iter().map(|d| format!("export {d}\n")).collect();
    let consts = format!(
        "\n/** The event a ready window gets as a liveness ping; it answers with `window_pong` and the same `seq`. */\nexport const PING_EVENT = '{}';\n",
        crate::PING_EVENT
    );
    vec![
        (
            "startup.ts",
            format!("{header}{}{consts}", types.join("\n")),
        ),
        (
            "startup-commands.ts",
            paddock_commands::ts::names_module(
                &crate::COMMANDS,
                "STARTUP_COMMANDS",
                "StartupCommand",
                REGENERATE,
            )
            .unwrap_or_default(),
        ),
    ]
}

/// The payload of [`crate::PING_EVENT`] (the adapter emits `{ seq }`).
#[derive(TS)]
#[allow(dead_code)]
struct PingEvent {
    /// The ping's number; `window_pong` answers with it.
    #[ts(type = "number")]
    seq: u64,
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
fn the_table_has_the_contract_commands() {
    let names: Vec<&str> = crate::COMMANDS.specs().iter().map(|s| s.name()).collect();
    assert_eq!(
        names,
        [
            "window_ready",
            "window_boot_error",
            "ui_checkpoint",
            "recovery_status",
            "recovery_restore",
            "recovery_start_fresh",
            "recovery_safe_mode",
            "app_relaunch",
            "window_pong"
        ]
    );
    assert_eq!(
        crate::COMMANDS.spec("ui_checkpoint").map(|s| s.limit()),
        Some(16 * paddock_commands::MIB)
    );
    assert!(
        !crate::COMMANDS.opens_to_dialogs("ui_checkpoint"),
        "a dialog has no UI state to recover"
    );
    assert!(crate::COMMANDS.opens_to_dialogs("window_ready"));
    for c in [
        "recovery_status",
        "recovery_restore",
        "recovery_start_fresh",
        "recovery_safe_mode",
    ] {
        assert!(
            !crate::COMMANDS.opens_to_dialogs(c),
            "{c}: the notice belongs to the app's own windows"
        );
    }
}
