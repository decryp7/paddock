//! Plugins that misbehave on purpose, for the host's tests. What it does depends on its own file name (the tests install
//! copies of it under different names): `garbled` answers every call with nonsense, `exit` ends the process on the first
//! call, `hang` never answers, `bare` is a plugin with no contributions, `greedy` is the fixture plugin that ignores the
//! budget of an MCP call, anything else (`forgetful`) is the fixture plugin that accepts its settings once and refuses
//! them in every process after the first (it leaves a file in its data folder).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_ipc::{CallCtx, Handler, Json, RemoteError, codes, decode_request, encode_reply};
use paddock_plugin_api::PLATFORM_PROTOCOL;
use paddock_plugin_api::PlatformRequest;
use paddock_plugin_api::serve::PluginHandler;
use paddock_plugin_test::fixture::{BarePlugin, FixturePlugin};
use paddock_plugin_test::process::Real;
use std::sync::Arc;

fn handler(name: &str) -> Arc<dyn Handler> {
    if name.contains("garbled") {
        Arc::new(|_: &CallCtx, _: &[u8]| encode_reply(&Json(serde_json::json!("nonsense"))))
    } else if name.contains("exit") {
        Arc::new(|_: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> { std::process::exit(5) })
    } else if name.contains("hang") {
        Arc::new(|_: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
            loop {
                std::thread::park();
            }
        })
    } else if name.contains("bare") {
        Arc::new(PluginHandler::new(Arc::new(BarePlugin), false))
    } else if name.contains("greedy") {
        let inner = PluginHandler::new(Arc::new(FixturePlugin::with_danger(Arc::new(Real))), false);
        Arc::new(move |ctx: &CallCtx, body: &[u8]| {
            if let Ok(PlatformRequest::McpTool { .. }) = decode_request::<PlatformRequest>(body) {
                let big = Json(serde_json::json!({ "text": "x".repeat(5000) }));
                return encode_reply(&Ok::<Json, paddock_plugin_api::Fail>(big));
            }
            inner.call(ctx, body)
        })
    } else {
        let inner = PluginHandler::new(Arc::new(FixturePlugin::with_danger(Arc::new(Real))), false);
        Arc::new(move |ctx: &CallCtx, body: &[u8]| {
            if let Ok(PlatformRequest::ApplySettings { .. }) =
                decode_request::<PlatformRequest>(body)
            {
                let marker = std::path::Path::new("applied-once");
                if marker.exists() {
                    return Err(RemoteError::new(
                        codes::INVALID,
                        "this process forgot how to take settings",
                    ));
                }
                std::fs::write(marker, "x").unwrap();
            }
            inner.call(ctx, body)
        })
    }
}

fn main() {
    let name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let result =
        paddock_ipc::child::serve("the app", PLATFORM_PROTOCOL, |_| handler(&name), |_, _| {});
    if let Err(e) = result {
        eprintln!("misbehaving: {e}");
        std::process::exit(2);
    }
}
