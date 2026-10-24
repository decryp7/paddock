//! The adapter on Tauri's mock runtime: no window reaches the desktop, waiting is event-driven (bounded), never a sleep.

use super::*;
use crate::control::{ErrorCode, MemorySettings};
use crate::{FakeMail, FakePoster};
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use paddock_jobs::time::SystemClock;
use paddock_secrets::MemorySecrets;
use serde_json::{Value, json};
use std::sync::mpsc;
use std::time::Duration;

type App = ::tauri::App<MockRuntime>;

fn invoke(w: &WebviewWindow<MockRuntime>, cmd: &str, body: Value) -> Result<Value, Value> {
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
    get_ipc_response(w, request).map(|b| b.deserialize::<Value>().unwrap())
}

fn app() -> (App, WebviewWindow<MockRuntime>, Arc<ChannelsControl>) {
    let app = mock_builder()
        .invoke_handler(invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
        .build()
        .unwrap();
    let control = install(app.handle(), |emit| {
        ChannelsControl::new(
            "Pitcrew",
            Arc::new(MemorySettings::default()),
            Arc::new(MemorySecrets::new("Pitcrew")),
            Arc::new(FakePoster::new()),
            Arc::new(FakeMail::new()),
            Arc::new(SystemClock),
            emit,
        )
    });
    (app, window, control)
}

fn channel(id: &str) -> Value {
    json!({"id": id, "name": "Ops", "enabled": true, "events": ["diskLow"], "minSeverity": "info",
           "transport": {"type": "webhook", "url": "https://hooks.example.org/in", "auth": true}})
}

#[test]
fn the_commands_save_show_and_forget_secrets() {
    let (_app, w, _control) = app();
    let v = invoke(&w, "channels_get", json!({})).unwrap();
    assert_eq!(v["settings"]["channels"], json!([]));
    invoke(
        &w,
        "channels_set_secret",
        json!({"id": "w", "value": "tok"}),
    )
    .unwrap();
    let saved = invoke(
        &w,
        "channels_save",
        json!({"settings": {"channels": [channel("w")]}}),
    )
    .unwrap();
    assert_eq!(saved["secretSet"]["w"], true);
    assert_eq!(invoke(&w, "channels_get", json!({})).unwrap(), saved);
    invoke(&w, "channels_clear_secret", json!({"id": "w"})).unwrap();
    assert_eq!(
        invoke(&w, "channels_get", json!({})).unwrap()["secretSet"]["w"],
        false
    );
}

#[test]
fn a_refused_command_carries_the_code_and_the_message() {
    let (_app, w, _control) = app();
    let mut bad = channel("w");
    bad["transport"]["url"] = json!("nonsense");
    let e = invoke(
        &w,
        "channels_save",
        json!({"settings": {"channels": [bad]}}),
    )
    .unwrap_err();
    assert_eq!(e["code"], json!(ErrorCode::Invalid));
    assert!(e["message"].as_str().unwrap().contains("https://"));
    let e = invoke(&w, "channels_set_secret", json!({"id": "w", "value": ""})).unwrap_err();
    assert_eq!(e["retryable"], false);
}

#[test]
fn a_test_returns_at_once_and_the_outcome_arrives_on_the_event() {
    let (app, w, _control) = app();
    let (tx, rx) = mpsc::channel();
    app.listen(EVENT, move |e| {
        let _ = tx.send(serde_json::from_str::<Value>(e.payload()).unwrap());
    });
    invoke(
        &w,
        "channels_set_secret",
        json!({"id": "w", "value": "tok"}),
    )
    .unwrap();
    let started = invoke(&w, "channels_test", json!({"channel": channel("w")})).unwrap();
    let event = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("the outcome");
    assert_eq!(
        (
            event["kind"].as_str(),
            event["testId"].clone(),
            event["ok"].as_bool()
        ),
        (Some("test"), started["testId"].clone(), Some(true))
    );
    assert_eq!(event["channelId"], "w");
}

#[test]
fn install_twice_returns_the_managed_control_and_the_table_matches_the_handler() {
    let (app, _w, first) = app();
    let again = install(app.handle(), |_| unreachable!("already managed"));
    assert!(Arc::ptr_eq(&first, &again));
    let table = &crate::COMMANDS;
    for c in [
        "channels_get",
        "channels_save",
        "channels_set_secret",
        "channels_clear_secret",
        "channels_test",
    ] {
        assert!(table.spec(c).is_some(), "{c}");
    }
    assert_eq!(failed().code, ErrorCode::Failed);
}
