//! The sample plugin as a process, for tests: `paddock-plugin-fixture [--protocol <major.minor>]`. With `--protocol` it claims
//! to be built for that platform protocol (to test the refusal of an older or newer plugin).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_ipc::ProtocolVersion;
use paddock_plugin_api::PLATFORM_PROTOCOL;
use paddock_plugin_api::serve::{ELEVATED_ROLE, PluginHandler};
use paddock_plugin_test::fixture::FixturePlugin;
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let protocol = args
        .iter()
        .position(|a| a == "--protocol")
        .and_then(|i| args.get(i + 1))
        .map_or(PLATFORM_PROTOCOL, |v| v.parse::<ProtocolVersion>().expect("--protocol major.minor"));
    let result = paddock_ipc::child::serve(
        "the app",
        protocol,
        |boot| Arc::new(PluginHandler::new(Arc::new(FixturePlugin::new()), boot.role == ELEVATED_ROLE)),
        |_, _| {},
    );
    if let Err(e) = result {
        eprintln!("fixture: {e}");
        std::process::exit(2);
    }
}
