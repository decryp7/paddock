//! The generated TypeScript of the channels adapter, checked on every test run (a stale file fails CI): the wire types (ts-rs) and
//! the command names, in the `shell/channels` entry point of `@decryp7/paddock-ui`.

use crate::config::{
    ChannelConfig, MAX_CHANNELS, MAX_EVENTS, MAX_RECIPIENTS, Security, Settings, Transport,
};
use crate::control::{
    ChannelsError, ChannelsEvent, ChannelsView, ErrorCode, TestPhase, TestStarted,
};
use crate::message::Severity;
use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-channels --lib generated";

fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/channels/generated")
}

fn generated_files() -> Vec<(&'static str, String)> {
    let header = format!(
        "// Generated from the Rust wire types of paddock-channels (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        Severity::decl(),
        Security::decl(),
        Transport::decl(),
        ChannelConfig::decl(),
        Settings::decl(),
        ChannelsView::decl(),
        ErrorCode::decl(),
        ChannelsError::decl(),
        TestPhase::decl(),
        TestStarted::decl(),
        ChannelsEvent::decl(),
    ];
    let types: Vec<String> = decls.into_iter().map(|d| format!("export {d}\n")).collect();
    let consts = format!(
        "\n/** The kinds of transport a channel can have. */\nexport type TransportType = Transport['type'];\n\n\
/** The event every update (test progress and outcome) is sent on, to every window. */\nexport const CHANNELS_EVENT = '{}';\n\n\
/** The most channels one app keeps. */\nexport const MAX_CHANNELS = {MAX_CHANNELS};\n\
/** The most event names one channel lists. */\nexport const MAX_EVENTS = {MAX_EVENTS};\n\
/** The most recipients of an email channel. */\nexport const MAX_RECIPIENTS = {MAX_RECIPIENTS};\n\
/** A channel id: letters, digits, dot, underscore, hyphen; 1 to 64 characters. */\nexport const CHANNEL_ID_PATTERN = /^[A-Za-z0-9._-]{{1,64}}$/;\n\
/** An ntfy topic: letters, digits, underscore, hyphen; 1 to 64 characters. */\nexport const NTFY_TOPIC_PATTERN = /^[A-Za-z0-9_-]{{1,64}}$/;\n",
        crate::EVENT_NAME
    );
    vec![
        (
            "channels.ts",
            format!("{header}{}{consts}", types.join("\n")),
        ),
        (
            "channels-commands.ts",
            paddock_commands::ts::names_module(
                &crate::COMMANDS,
                "CHANNELS_COMMANDS",
                "ChannelsCommand",
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
fn the_table_has_the_channels_commands() {
    let names: Vec<&str> = crate::COMMANDS.specs().iter().map(|s| s.name()).collect();
    assert_eq!(
        names,
        [
            "channels_get",
            "channels_save",
            "channels_set_secret",
            "channels_clear_secret",
            "channels_test"
        ]
    );
}

/// The dialog contracts of this crate as TypeScript (ADR 0026 §3), beside its other generated files.
#[test]
fn generated_dialog_contracts_are_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/channels/generated/dialog-contracts.ts");
    let text =
        paddock_dialogs::ts::contracts_module("paddock-channels", &crate::DIALOGS, REGENERATE);
    testing::sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}
