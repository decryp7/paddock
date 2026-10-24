//! The generated TypeScript of the wire types, checked on every run (a stale file fails CI): the `shell/pitwall` entry point
//! of `@decryp7/paddock-ui` uses it.

use crate::{AiSnapshot, AiState, Caps, NeedsAiView, NextStep, PairingView, status};
use paddock_commands::testing::{WriteMode, sync_generated};
use std::path::Path;
use ts_rs::TS;

const REGENERATE: &str =
    "PADDOCK_WRITE_TS=1 cargo test -p paddock-pitwall --all-features --lib generated";

fn pitwall_ts() -> String {
    let quoted: Vec<String> = status::COMMANDS.iter().map(|c| format!("'{c}'")).collect();
    let commands = format!("[{}]", quoted.join(", "));
    let mut out = format!(
        "// Generated from the Rust wire types of paddock-pitwall (ts-rs). Do not edit: {REGENERATE}.\n\n\
         /** Every window hears it when the app's Pitwall snapshot changed; the payload is an `AiSnapshot`. */\n\
         export const PITWALL_EVENT = '{}';\n\n\
         /** The commands the app registers for the shared component (snapshot, connect, cancel pairing, use the built-in engine). */\n\
         export const PITWALL_COMMANDS = {} as const;\n\n\
         export type PitwallCommand = (typeof PITWALL_COMMANDS)[number];\n\n",
        status::EVENT,
        commands,
    );
    for d in [
        NextStep::decl(),
        Caps::decl(),
        AiState::decl(),
        NeedsAiView::decl(),
        PairingView::decl(),
        AiSnapshot::decl(),
    ] {
        out.push_str(&format!("export {d}\n\n"));
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

#[test]
fn generated_typescript_is_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/pitwall/generated/pitwall.ts");
    sync_generated(&path, &pitwall_ts(), WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}
