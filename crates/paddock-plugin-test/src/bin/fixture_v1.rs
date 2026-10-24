//! The frozen plugin of protocol 1, for tests (ADR 0003 phase 4): it connects claiming to be built for platform protocol 1.1,
//! so a host must refuse it with a message that says to update the plugin. Never changes.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_ipc::{NoCalls, ProtocolVersion};
use std::sync::Arc;

fn main() {
    let result = paddock_ipc::child::serve("the app", ProtocolVersion::new(1, 1), |_| Arc::new(NoCalls), |_, _| {});
    if let Err(e) = result {
        eprintln!("fixture v1: {e}");
        std::process::exit(2);
    }
}
