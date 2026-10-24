//! The layout commands on Tauri's mock runtime, behind the guard: no window reaches the desktop, files live in a
//! temporary folder.

use super::*;
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use paddock_commands::tauri::Guard;
use serde_json::{Value, json};

type App = ::tauri::App<MockRuntime>;

fn app(managed: bool) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let app = mock_builder()
        .invoke_handler(Guard::new(&crate::COMMANDS).wrap(invoke_handler()))
        .build(mock_context(noop_assets()))
        .unwrap();
    if managed {
        app.manage(Layouts::open(dir.path()));
    }
    (dir, app)
}

fn window(app: &App, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .build()
        .unwrap()
}

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

#[test]
fn saves_and_reads_a_layout_from_the_main_window_and_a_pop_out() {
    let (dir, app) = app(true);
    let main = window(&app, "main");
    assert_eq!(
        invoke(&main, "layout_get", json!({ "key": "main" })),
        Ok(Value::Null)
    );
    assert_eq!(
        invoke(
            &main,
            "layout_set",
            json!({ "key": "main", "value": { "version": 3 } })
        ),
        Ok(Value::Null)
    );
    let popout = window(&app, "p-main-1");
    assert_eq!(
        invoke(&popout, "layout_get", json!({ "key": "main" })),
        Ok(json!({ "version": 3 }))
    );
    assert_eq!(
        invoke(&main, "layout_set", json!({ "key": "main", "value": null })),
        Ok(Value::Null)
    );
    assert_eq!(
        invoke(&main, "layout_get", json!({ "key": "main" })),
        Ok(Value::Null)
    );
    assert!(dir.path().join(FILE_NAME).exists());
}

#[test]
fn refuses_bad_keys_and_values_in_the_one_envelope() {
    let (_dir, app) = app(true);
    let main = window(&app, "main");
    let err = invoke(&main, "layout_get", json!({ "key": "No Way" })).unwrap_err();
    assert_eq!(
        (err["code"].as_str(), err["field"].as_str()),
        (Some("invalid"), Some("key"))
    );
    let err = invoke(&main, "layout_set", json!({ "key": "main", "value": [1] })).unwrap_err();
    assert_eq!(
        (err["code"].as_str(), err["field"].as_str()),
        (Some("invalid"), Some("value"))
    );
    let err = invoke(
        &main,
        "layout_set",
        json!({ "key": "main", "value": { "x": "y".repeat(1024 * 1024) } }),
    )
    .unwrap_err();
    assert_eq!(err["code"].as_str(), Some("limit"));
}

#[test]
fn a_pop_out_cannot_write_and_dialogs_cannot_read() {
    let (_dir, app) = app(true);
    let popout = window(&app, "p-main-1");
    let err = invoke(&popout, "layout_set", json!({ "key": "main", "value": {} })).unwrap_err();
    assert_eq!(err["code"].as_str(), Some("denied"));
    let dialog = window(&app, "d-main-1");
    assert_eq!(
        invoke(&dialog, "layout_get", json!({ "key": "main" })).unwrap_err()["code"].as_str(),
        Some("denied")
    );
}

#[test]
fn says_what_is_missing_when_the_store_is_not_managed() {
    let (_dir, app) = app(false);
    let main = window(&app, "main");
    assert_eq!(
        invoke(&main, "layout_get", json!({ "key": "main" })).unwrap_err()["code"].as_str(),
        Some("internal")
    );
    assert_eq!(
        invoke(&main, "layout_set", json!({ "key": "main", "value": {} })).unwrap_err()["code"]
            .as_str(),
        Some("internal")
    );
}

#[test]
fn maps_every_error_and_logs_a_damaged_file() {
    assert_eq!(
        to_command_error(LayoutError::TooLarge).code.as_str(),
        "limit"
    );
    assert_eq!(to_command_error(LayoutError::Full).code.as_str(), "limit");
    assert_eq!(
        to_command_error(LayoutError::Write("disk full".into()))
            .code
            .as_str(),
        "io"
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(FILE_NAME), "nope").unwrap();
    let layouts = Layouts::open(dir.path());
    assert!(layouts.0.problem().is_some());
}
