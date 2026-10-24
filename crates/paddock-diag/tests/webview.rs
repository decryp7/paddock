//! The webview's log command writes through the process's installed log with the window it came from. Its own test binary:
//! the log installs once per process, and commands run on the async runtime's threads, not the test's.

#![cfg(feature = "tauri")]

use paddock_env::AppId;
use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;
use tauri::{WebviewUrl, WebviewWindowBuilder};

#[test]
fn webview_lines_reach_the_log_with_their_window_and_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let id = AppId::new("demo", "Demo").unwrap();
    let config = paddock_log::LogConfig::new(&id, "app", "1.0.0")
        .with_log_dir(dir.path())
        .with_stdout(false)
        .with_panic_hook(false);
    let handle = paddock_log::Logging::new(config).install().unwrap();
    let app = mock_builder()
        .invoke_handler(paddock_diag::tauri::invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let w = WebviewWindowBuilder::new(&app, "d-main-2", WebviewUrl::default())
        .build()
        .unwrap();
    let call = |body: Value| {
        let request = InvokeRequest {
            cmd: "log_from_webview".into(),
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
        get_ipc_response(&w, request).unwrap();
    };
    call(json!({ "level": "error", "message": "unhandled rejection in the page" }));
    call(json!({ "level": "5", "message": "x".repeat(100_000) }));
    handle.flush_repeats();
    let text = std::fs::read_to_string(handle.active_file().unwrap()).unwrap();
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let first = lines
        .iter()
        .find(|l| {
            l["@m"]
                .as_str()
                .is_some_and(|m| m.contains("unhandled rejection"))
        })
        .unwrap();
    assert_eq!(
        (first["Window"].as_str(), first["@l"].as_str()),
        (Some("d-main-2"), Some("Error"))
    );
    let long = lines
        .iter()
        .find(|l| l["@m"].as_str().is_some_and(|m| m.starts_with("xxx")))
        .unwrap();
    assert!(
        long["@m"].as_str().unwrap().chars().count() <= paddock_log::MAX_MESSAGE + 1,
        "a page cannot flood the log"
    );
}
