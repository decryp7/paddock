//! The Tauri adapter on Tauri's mock runtime: frames emitted as events, `live_snapshot` through the guarded invoke path,
//! the routing in front of an app's handler, and the generated TypeScript.

use crate::common::{Change, Overview, event, memory};
use paddock_commands::testing::{WriteMode, assert_async, sync_generated};
use paddock_commands::{DialogGrant, ts};
use paddock_live::tauri::{
    COMMANDS, CommandError, RegisterError, handler, handler_with_grants, invoke_handler, register,
    route,
};
use paddock_live::{Channel, ChannelError, Frame, Hub, Link};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[tauri::command]
async fn ping() -> String {
    "pong".into()
}

struct Grant(&'static [&'static str]);

impl DialogGrant for Grant {
    fn dialog(&self) -> &str {
        "status-viewer"
    }
    fn allows(&self, command: &str) -> bool {
        self.0.contains(&command)
    }
}

fn app_with(
    live: impl Fn(tauri::ipc::Invoke<MockRuntime>) -> bool + Send + Sync + 'static,
) -> App<MockRuntime> {
    mock_builder()
        .invoke_handler(route(live, tauri::generate_handler![ping]))
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .build()
        .unwrap()
}

fn invoke(
    w: &WebviewWindow<MockRuntime>,
    cmd: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    let request = InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .unwrap(),
        body: InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(w, request).map(|b| b.deserialize::<serde_json::Value>().unwrap())
}

#[test]
fn frames_reach_the_webviews_in_order_on_the_channels_event() {
    let app = app_with(handler());
    let hub = Hub::new(Overview::default());
    let (tx, rx) = mpsc::channel();
    app.listen_any(Channel::new("overview").unwrap().event(), move |e| {
        let _ = tx.send(serde_json::from_str::<Frame<Overview, Change>>(e.payload()).unwrap());
    });
    let forwarder = register(app.handle(), "overview", &hub).unwrap();
    hub.publish(event("a"));
    hub.set_link(Link::Connected);
    hub.publish(Change::Bump);
    let first = rx.recv().unwrap();
    assert!(matches!(first, Frame::Snapshot { seq: 0, .. }));
    let mut seqs = Vec::new();
    // Every later frame is a delta or a link change; the deltas arrive in numeric order (some may merge or be replaced
    // by a snapshot if the forwarder lagged, so the assertion is about order, not count).
    hub.close();
    forwarder.join().unwrap();
    while let Ok(f) = rx.try_recv() {
        match f {
            Frame::Delta { seq, .. } | Frame::Snapshot { seq, .. } => seqs.push(seq),
            Frame::Link { .. } => {}
        }
    }
    assert!(seqs.windows(2).all(|w| w[0] <= w[1]), "{seqs:?}");
    assert_eq!(seqs.last().copied(), Some(2));
}

#[test]
fn live_snapshot_answers_the_whole_state_at_once() {
    let app = app_with(handler());
    let main = window(&app, "main");
    let hub = Hub::new(Overview::default());
    let _forwarder = register(app.handle(), "overview", &hub).unwrap();
    hub.publish(memory(9));
    hub.publish(event("hello"));
    let answer = invoke(
        &main,
        "live_snapshot",
        serde_json::json!({ "channel": "overview" }),
    )
    .unwrap();
    assert_eq!(answer["kind"], "snapshot");
    assert_eq!(answer["seq"], 2);
    assert_eq!(answer["state"]["memory"], 9);
    assert_eq!(answer["state"]["events"][0], "hello");
    assert_eq!(answer["link"]["state"], "connecting");
    hub.close();
}

#[test]
fn bad_unknown_and_unregistered_channels_are_refused_with_a_code() {
    let bare = app_with(handler());
    let main = window(&bare, "main");
    let e = invoke(
        &main,
        "live_snapshot",
        serde_json::json!({ "channel": "overview" }),
    )
    .unwrap_err();
    assert_eq!(e["code"], "notSetUp");
    assert_eq!(e["retryable"], false);

    let app = app_with(handler());
    let main = window(&app, "main");
    let hub = Hub::new(Overview::default());
    let _f = register(app.handle(), "overview", &hub).unwrap();
    assert_eq!(
        invoke(
            &main,
            "live_snapshot",
            serde_json::json!({ "channel": "Bad Name" })
        )
        .unwrap_err()["code"],
        "badChannel"
    );
    assert_eq!(
        invoke(
            &main,
            "live_snapshot",
            serde_json::json!({ "channel": "other" })
        )
        .unwrap_err()["code"],
        "unknownChannel"
    );
    hub.close();
    for e in [
        CommandError::NotSetUp,
        CommandError::BadChannel,
        CommandError::UnknownChannel,
        CommandError::Encode,
    ] {
        assert!(!e.to_string().is_empty() && !e.code().is_empty());
    }
}

#[test]
fn a_channel_registers_once_and_its_name_is_checked() {
    let app = app_with(handler());
    let hub = Hub::new(Overview::default());
    let _f = register(app.handle(), "overview", &hub).unwrap();
    assert_eq!(
        register(app.handle(), "overview", &Hub::new(Overview::default())).unwrap_err(),
        RegisterError::Duplicate
    );
    assert_eq!(
        register(app.handle(), "No Good", &hub).unwrap_err(),
        RegisterError::Channel(ChannelError::Start)
    );
    for e in [
        RegisterError::Duplicate,
        RegisterError::Thread,
        RegisterError::Channel(ChannelError::Length),
    ] {
        assert!(!e.to_string().is_empty());
    }
    hub.close();
}

#[test]
fn dialogs_call_live_snapshot_only_through_their_contract() {
    let plain = app_with(handler());
    let hub = Hub::new(Overview::default());
    let _f = register(plain.handle(), "overview", &hub).unwrap();
    let dialog = window(&plain, "d-main-1");
    assert_eq!(
        invoke(
            &dialog,
            "live_snapshot",
            serde_json::json!({ "channel": "overview" })
        )
        .unwrap_err()["code"],
        "denied"
    );

    let granted = app_with(handler_with_grants(|_app, label| {
        (label == "d-main-1").then_some(Grant(&["live_snapshot"]))
    }));
    let hub2 = Hub::new(Overview::default());
    let _f2 = register(granted.handle(), "overview", &hub2).unwrap();
    let dialog = window(&granted, "d-main-1");
    assert!(
        invoke(
            &dialog,
            "live_snapshot",
            serde_json::json!({ "channel": "overview" })
        )
        .is_ok()
    );
    // Anything not in the table goes to the app's own handler.
    let main = window(&granted, "main");
    assert_eq!(
        invoke(&main, "ping", serde_json::json!({})),
        Ok("pong".into())
    );
    hub.close();
    hub2.close();
}

#[test]
fn the_unguarded_handler_serves_the_command_too() {
    let app = app_with(invoke_handler());
    let main = window(&app, "main");
    let hub = Hub::new(Overview::default());
    let _f = register(app.handle(), "overview", &hub).unwrap();
    assert!(
        invoke(
            &main,
            "live_snapshot",
            serde_json::json!({ "channel": "overview" })
        )
        .is_ok()
    );
    hub.close();
}

#[test]
fn every_command_is_async_and_in_the_table() {
    assert_async(
        &COMMANDS,
        &[&Path::new(env!("CARGO_MANIFEST_DIR")).join("src")],
    );
    assert_eq!(
        COMMANDS.kind_of("live_snapshot"),
        paddock_commands::Kind::Query
    );
}

const REGENERATE: &str =
    "PADDOCK_WRITE_TS=1 cargo test -p paddock-live --features tauri --test it generated";

fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/projects/paddock-ui/live/generated")
}

/// A whole conversation as Rust sent it: the frames of every shape a window folds, and the state Rust ended with. The UI
/// spec folds the frames with its own `apply` and must reach the same state (the two folds stay equal).
fn conversation() -> serde_json::Value {
    let hub = Hub::new(Overview::default());
    let sub = hub.subscribe();
    hub.reset_with(Overview::default(), Link::Connected);
    hub.publish(event("first"));
    hub.publish(memory(100));
    hub.publish(memory(200));
    hub.publish(Change::Bump);
    hub.publish(event("second"));
    hub.set_link(Link::Reconnecting {
        attempt: 1,
        retry_in_ms: 1000,
        reason: "The connection was closed".into(),
    });
    let mut frames: Vec<serde_json::Value> = sub
        .drain()
        .iter()
        .map(|f| serde_json::to_value(f).unwrap())
        .collect();
    hub.set_link(Link::Down {
        reason: "The agent is not installed".into(),
    });
    frames.extend(sub.drain().iter().map(|f| serde_json::to_value(f).unwrap()));
    hub.set_link(Link::Stopped);
    frames.extend(sub.drain().iter().map(|f| serde_json::to_value(f).unwrap()));
    serde_json::json!({
        "frames": frames,
        "finalSeq": hub.seq(),
        "finalState": hub.state(),
        "finalLink": hub.link(),
    })
}

#[test]
fn generated_typescript_is_current() {
    let names = ts::names_module(&COMMANDS, "LIVE_COMMANDS", "LiveCommand", REGENERATE).unwrap();
    let conversation = serde_json::to_string_pretty(&conversation()).unwrap();
    let fixtures = format!(
        "// Generated from paddock-live's wire model by its tests (Rust round trip). Do not edit: {REGENERATE}.\n\n\
         /** A conversation as Rust sent it (snapshot, merged and plain deltas, link changes) with the state, sequence and link\n \
         * Rust ended with. The state is the test overview `{{ memory, events, count }}`. */\n\
         export const LIVE_CONVERSATION: {{ frames: readonly unknown[]; finalSeq: number; finalState: unknown; finalLink: unknown }} = {conversation};\n"
    );
    let mode = WriteMode::from_env();
    let stale: Vec<String> = [("live-commands.ts", names), ("live-fixtures.ts", fixtures)]
        .into_iter()
        .filter_map(|(name, text)| sync_generated(&generated_dir().join(name), &text, mode).err())
        .map(|e| e.to_string())
        .collect();
    assert!(stale.is_empty(), "{}", stale.join("\n"));
}
