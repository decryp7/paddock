//! The generated TypeScript of the packs adapter, checked on every test run (a stale file fails CI): the wire types
//! (ts-rs), the event name and the command names, in the `shell/packs` entry point of `@decryp7/paddock-ui`.

use crate::view::{KindView, PackEntry, PacksView};
use crate::{Health, Origin, Problem};
use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-packs --lib generated";

fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/projects/paddock-ui/shell/packs/generated")
}

fn generated_files() -> Vec<(&'static str, String)> {
    let header = format!(
        "// Generated from the Rust wire types of paddock-packs (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        Origin::decl(),
        Problem::decl(),
        Health::decl(),
        PackEntry::decl(),
        KindView::decl(),
        PacksView::decl(),
    ];
    let types: Vec<String> = decls.into_iter().map(|d| format!("export {d}\n")).collect();
    let consts = format!(
        "\n/** The event every change of a pack kind is sent on, to every window (a `PacksView`). */\nexport const PACKS_EVENT = '{}';\n",
        crate::EVENT_NAME
    );
    vec![
        ("packs.ts", format!("{header}{}{consts}", types.join("\n"))),
        (
            "packs-commands.ts",
            paddock_commands::ts::names_module(
                &crate::COMMANDS,
                "PACKS_COMMANDS",
                "PacksCommand",
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
fn the_table_has_the_packs_commands() {
    let names: Vec<&str> = crate::COMMANDS.specs().iter().map(|s| s.name()).collect();
    assert_eq!(names, ["packs_status", "packs_reload"]);
    assert!(
        crate::COMMANDS.opens_to_dialogs("packs_reload"),
        "the Settings dialog may reload"
    );
    assert_eq!(
        crate::COMMANDS.kind_of("packs_reload"),
        paddock_commands::Kind::Mutation
    );
}
