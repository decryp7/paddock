//! The guard on a real invoke path: a mock Tauri app (`tauri::test`) with the table's handler behind [`Guard`].

use super::*;
use crate::{DialogGrant, KIB};
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use std::sync::Mutex;

mod handlers {
    #[::tauri::command]
    pub async fn ping() -> String {
        "pong".into()
    }

    #[::tauri::command]
    pub async fn legal_text() -> String {
        "MIT".into()
    }

    #[::tauri::command]
    pub async fn echo(text: String) -> String {
        text
    }

    #[::tauri::command]
    pub async fn restart_app() -> String {
        "restarting".into()
    }
}

crate::command_table! {
    static TABLE, macro handler;
    Query [WINDOW] => handlers::ping,
    Query [WINDOW] => handlers::legal_text,
    Query [WINDOW] limit 2 * KIB => handlers::echo,
    Mutation [MAIN] => handlers::restart_app,
}

struct Grant(&'static str, &'static [&'static str]);

impl DialogGrant for Grant {
    fn dialog(&self) -> &str {
        self.0
    }
    fn allows(&self, command: &str) -> bool {
        self.1.contains(&command)
    }
}

/// The tests share the process-wide logger and [`LOGGED`], so they run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Every log record of the refusals, as (level, message, dialog).
static LOGGED: Mutex<Vec<(log::Level, String, String)>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        if record.target() != "paddock_commands" {
            return;
        }
        let dialog = record
            .key_values()
            .get("Dialog".into())
            .map(|v| v.to_string())
            .unwrap_or_default();
        LOGGED.lock().unwrap_or_else(|p| p.into_inner()).push((
            record.level(),
            record.args().to_string(),
            dialog,
        ));
    }
    fn flush(&self) {}
}

fn logged() -> Vec<(log::Level, String, String)> {
    std::mem::take(&mut *LOGGED.lock().unwrap_or_else(|p| p.into_inner()))
}

fn app(grants: bool) -> ::tauri::App<MockRuntime> {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Trace);
    let mut guard = Guard::new(&TABLE);
    if grants {
        guard = guard.dialog_grants(|_app, label| match label {
            "d-main-1" => Some(Grant("about", &["legal_text", "echo"])),
            _ => None,
        });
    }
    mock_builder()
        .invoke_handler(guard.wrap(handler!()))
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn window(app: &::tauri::App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .build()
        .unwrap()
}

fn invoke(
    w: &WebviewWindow<MockRuntime>,
    cmd: &str,
    body: InvokeBody,
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
        body,
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(w, request).map(|b| b.deserialize::<serde_json::Value>().unwrap())
}

fn call(w: &WebviewWindow<MockRuntime>, cmd: &str) -> Result<serde_json::Value, serde_json::Value> {
    invoke(w, cmd, InvokeBody::Json(serde_json::json!({})))
}

#[test]
fn the_guard_checks_every_call_before_the_handler() {
    let _serial = serial();
    let app = app(true);
    let main = window(&app, "main");
    let dialog = window(&app, "d-main-1");
    let unknown_dialog = window(&app, "d-main-2");
    let popout = window(&app, "p-main-1");
    let page = window(&app, "browser-main");
    logged();

    assert_eq!(call(&main, "ping"), Ok("pong".into()));
    assert_eq!(call(&main, "restart_app"), Ok("restarting".into()));
    assert_eq!(call(&popout, "ping"), Ok("pong".into()));
    assert_eq!(call(&dialog, "legal_text"), Ok("MIT".into()));
    assert!(logged().is_empty(), "allowed calls are not refusals");

    // A pop-out may not call a main-only command: a warning.
    let e = call(&popout, "restart_app").unwrap_err();
    assert_eq!(e["code"], "denied");
    assert_eq!(e["message"], "restart_app is not available in this window");
    assert_eq!(
        logged(),
        [(
            log::Level::Warn,
            "A command was refused: restart_app is not available in this window".into(),
            String::new()
        )]
    );

    // A dialog calling what its contract does not declare: an error naming the dialog and the command.
    let e = call(&dialog, "ping").unwrap_err();
    assert_eq!(
        (e["code"].as_str(), e["dialog"].as_str()),
        (Some("denied"), Some("about"))
    );
    let log = logged();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].0, log::Level::Error);
    assert!(
        log[0]
            .1
            .starts_with("The about dialog called ping, which its contract does not declare"),
        "{}",
        log[0].1
    );
    assert_eq!(log[0].2, "about");

    // A dialog window the dialog layer does not know, and a webview outside the scheme, may call nothing.
    assert_eq!(
        call(&unknown_dialog, "legal_text").unwrap_err()["code"],
        "denied"
    );
    assert_eq!(call(&page, "ping").unwrap_err()["code"], "denied");
    assert_eq!(logged().len(), 2);

    // Requests are bounded before their arguments are parsed, JSON and raw alike.
    let small = InvokeBody::Json(serde_json::json!({ "text": "x".repeat(100) }));
    assert_eq!(
        invoke(&main, "echo", small),
        Ok(serde_json::Value::from("x".repeat(100)))
    );
    let big = InvokeBody::Json(serde_json::json!({ "text": "x".repeat(3 * 1024) }));
    let e = invoke(&dialog, "echo", big).unwrap_err();
    assert_eq!(
        (e["code"].as_str(), e["dialog"].as_str()),
        (Some("limit"), Some("about"))
    );
    assert_eq!(
        logged()[0].0,
        log::Level::Warn,
        "too large is a warning, also from a dialog"
    );
    assert_eq!(
        invoke(&main, "echo", InvokeBody::Raw(vec![0; 3 * 1024])).unwrap_err()["code"],
        "limit"
    );
    logged();

    // An unknown command passes the guard and is answered by Tauri.
    assert!(call(&main, "no_such_command").is_err());
    assert!(logged().is_empty());
}

#[test]
fn without_a_grant_lookup_dialogs_may_call_nothing() {
    let _serial = serial();
    let app = app(false);
    let dialog = window(&app, "d-main-1");
    let e = call(&dialog, "legal_text").unwrap_err();
    assert_eq!(
        e["message"],
        "legal_text is not available in an unknown dialog"
    );
    let main = window(&app, "main");
    assert_eq!(call(&main, "legal_text"), Ok("MIT".into()));
}

#[test]
fn request_sizes_are_measured_as_serialised_and_stop_past_the_cap() {
    let _serial = serial();
    let big = serde_json::json!({ "ui": "x".repeat(1000), "n": [1, 2, true, null] });
    let size = json_size(&big, usize::MAX);
    assert!((1000..1100).contains(&size), "{size}");
    let many = serde_json::Value::Array(vec![serde_json::Value::from("a".repeat(100)); 100]);
    assert!(json_size(&many, 500) < 1000, "stops past the cap");
    let wide: serde_json::Map<String, serde_json::Value> = (0..100)
        .map(|i| (format!("k{i}"), "a".repeat(100).into()))
        .collect();
    assert!(
        json_size(&serde_json::Value::Object(wide), 500) < 1000,
        "objects too"
    );
    assert_eq!(body_size(&InvokeBody::Raw(vec![0; 7]), 10), 7);
    assert_eq!(body_size(&InvokeBody::Json(serde_json::Value::Null), 10), 5);
}

mod other {
    #[::tauri::command]
    pub async fn status() -> String {
        "fine".into()
    }
}

// One leaf table per module (each emits its own `dialog_callable` markers).
#[macro_use]
mod other_table {
    crate::command_table! {
        pub(super) static OTHER, macro other_handler;
        Query [MAIN] => crate::tauri::tests::other::status,
    }
}
use other_table::OTHER;

crate::command_table! {
    static BOTH = [TABLE, OTHER];
}

#[test]
fn routes_send_each_command_to_the_handler_of_its_table_behind_one_guard() {
    let _serial = serial();
    let routes = Routes::new()
        .route(&TABLE, handler!())
        .route(&OTHER, other_handler!());
    assert_eq!(routes.table_of("status").map(|t| t.specs().len()), Some(1));
    assert_eq!(routes.table_of("ping").map(|t| t.specs().len()), Some(4));
    assert!(routes.table_of("nope").is_none());
    let _ = log::set_logger(&Capture);
    let app = mock_builder()
        .invoke_handler(Guard::new(&BOTH).wrap(routes.handler()))
        .build(mock_context(noop_assets()))
        .unwrap();
    let main = window(&app, "main");
    assert_eq!(call(&main, "ping"), Ok("pong".into()));
    assert_eq!(call(&main, "status"), Ok("fine".into()));
    // The composed table's guard applies the part's classes: `status` is for main windows only.
    let popout = window(&app, "p-main-1");
    assert_eq!(call(&popout, "status").unwrap_err()["code"], "denied");
    // A command no table lists is not handled: Tauri says it does not exist.
    let e = call(&main, "nope").unwrap_err();
    assert!(e.to_string().contains("nope"), "{e}");
    let _ = logged();
}

#[test]
fn routes_without_any_table_handle_nothing() {
    let _serial = serial();
    let app = mock_builder()
        .invoke_handler(Routes::<MockRuntime>::default().handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let main = window(&app, "main");
    assert!(call(&main, "ping").is_err());
}
