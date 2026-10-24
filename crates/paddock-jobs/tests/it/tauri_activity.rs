//! The activity journal's Tauri adapter on Tauri's mock runtime: entries as events on `paddock://activity`, the three
//! commands through the guarded invoke path, the app's registered undo, and the generated command names (plan X44).
#![cfg(feature = "tauri")]

use paddock_commands::testing::{WriteMode, sync_generated};
use paddock_commands::ts;
use paddock_jobs::FakeClock;
use paddock_jobs::journal::{Entry, Journal, NewEntry, UndoState};
use paddock_jobs::tauri_activity::{COMMANDS, EVENT, handler, manage, route};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
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

fn app() -> App<MockRuntime> {
    mock_builder()
        .invoke_handler(route(handler(), tauri::generate_handler![ping]))
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn window(app: &App<MockRuntime>) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
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

fn journal() -> Arc<Journal> {
    Arc::new(Journal::in_memory(
        50,
        Arc::new(FakeClock::at_wall_ms(1_792_656_000_000)),
    ))
}

fn code(e: Result<serde_json::Value, serde_json::Value>) -> String {
    e.unwrap_err()["code"].as_str().unwrap().to_string()
}

#[test]
fn entries_are_listed_and_pushed_as_events() {
    let app = app();
    let w = window(&app);
    let heard = Arc::new(Mutex::new(Vec::<Entry>::new()));
    let sink = heard.clone();
    app.listen_any(EVENT, move |e| {
        sink.lock()
            .unwrap()
            .push(serde_json::from_str(e.payload()).unwrap())
    });
    let j = journal();
    let a = manage(app.handle(), j.clone());
    assert!(
        Arc::ptr_eq(&a, &manage(app.handle(), journal())),
        "a second set-up keeps the first"
    );
    assert_eq!(EVENT, "paddock://activity");

    a.journal()
        .record(NewEntry::new("rule", "first").undoable());
    for n in 0..3 {
        j.record(NewEntry::new("system", format!("note {n}")));
    }
    let listed = invoke(&w, "activity_list", serde_json::json!({})).unwrap();
    let titles: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        titles,
        ["note 2", "note 1", "note 0", "first"],
        "newest first"
    );
    let two = invoke(&w, "activity_list", serde_json::json!({ "limit": 2 })).unwrap();
    assert_eq!(two.as_array().unwrap().len(), 2);
    assert_eq!(
        heard.lock().unwrap().len(),
        4,
        "one event per recorded entry"
    );
    assert_eq!(
        invoke(&w, "ping", serde_json::json!({})),
        Ok(serde_json::json!("pong")),
        "the app's own commands pass through"
    );
}

#[test]
fn undo_runs_the_apps_handler_then_marks_the_entry_and_tells_the_windows() {
    let app = app();
    let w = window(&app);
    let heard = Arc::new(Mutex::new(Vec::<Entry>::new()));
    let sink = heard.clone();
    app.listen_any(EVENT, move |e| {
        sink.lock()
            .unwrap()
            .push(serde_json::from_str(e.payload()).unwrap())
    });
    let a = manage(app.handle(), journal());
    let id = a
        .journal()
        .record(NewEntry::new("cleanup", "Moved 3 files").undoable())
        .id;

    // No handler registered yet.
    assert_eq!(
        code(invoke(&w, "activity_undo", serde_json::json!({ "id": id }))),
        "noUndoHandler"
    );

    let undone = Arc::new(Mutex::new(Vec::<u64>::new()));
    let log = undone.clone();
    let allow = Arc::new(Mutex::new(false));
    let flag = allow.clone();
    a.on_undo(move |e| {
        if *flag.lock().unwrap() {
            log.lock().unwrap().push(e.id);
            Ok(())
        } else {
            Err("The files were already deleted.".to_string())
        }
    });
    let refused = invoke(&w, "activity_undo", serde_json::json!({ "id": id })).unwrap_err();
    assert_eq!(
        (refused["code"].as_str(), refused["message"].as_str()),
        (Some("undoFailed"), Some("The files were already deleted."))
    );
    assert_eq!(
        a.journal().get(id).unwrap().undo,
        UndoState::Available,
        "a refused undo leaves the entry"
    );

    *allow.lock().unwrap() = true;
    let done = invoke(&w, "activity_undo", serde_json::json!({ "id": id })).unwrap();
    assert_eq!(done["undo"], "undone");
    assert_eq!(*undone.lock().unwrap(), [id]);
    assert_eq!(
        heard.lock().unwrap().last().unwrap().undo,
        UndoState::Undone
    );
    // Twice, and an unknown id.
    assert_eq!(
        code(invoke(&w, "activity_undo", serde_json::json!({ "id": id }))),
        "notAvailable"
    );
    assert_eq!(
        code(invoke(
            &w,
            "activity_undo",
            serde_json::json!({ "id": 999 })
        )),
        "notFound"
    );
    assert_eq!(undone.lock().unwrap().len(), 1, "the handler ran once");
}

#[test]
fn a_handler_that_panics_is_a_failure_not_a_crash() {
    let app = app();
    let w = window(&app);
    let a = manage(app.handle(), journal());
    let id = a.journal().record(NewEntry::new("rule", "x").undoable()).id;
    a.on_undo(|_| panic!("bug in the app"));
    let e = invoke(&w, "activity_undo", serde_json::json!({ "id": id })).unwrap_err();
    assert_eq!(e["code"], "undoFailed");
    assert_eq!(a.journal().get(id).unwrap().undo, UndoState::Available);
}

#[test]
fn mark_moves_an_entry_without_running_the_handler() {
    let app = app();
    let w = window(&app);
    let a = manage(app.handle(), journal());
    let id = a
        .journal()
        .record(NewEntry::new("cleanup", "x").undoable())
        .id;
    let ran = Arc::new(Mutex::new(false));
    let flag = ran.clone();
    a.on_undo(move |_| {
        *flag.lock().unwrap() = true;
        Ok(())
    });
    let marked = invoke(
        &w,
        "activity_mark",
        serde_json::json!({ "id": id, "to": "expired" }),
    )
    .unwrap();
    assert_eq!(marked["undo"], "expired");
    assert!(!*ran.lock().unwrap());
    assert_eq!(
        code(invoke(
            &w,
            "activity_mark",
            serde_json::json!({ "id": id, "to": "undone" })
        )),
        "notAvailable"
    );
    assert_eq!(
        code(invoke(
            &w,
            "activity_mark",
            serde_json::json!({ "id": id, "to": "available" })
        )),
        "badTarget"
    );
    assert_eq!(
        code(invoke(
            &w,
            "activity_mark",
            serde_json::json!({ "id": 99, "to": "undone" })
        )),
        "notFound"
    );
}

#[test]
fn the_commands_say_so_when_the_app_forgot_to_set_up_the_log() {
    let app = app();
    let w = window(&app);
    for (cmd, body) in [
        ("activity_list", serde_json::json!({})),
        ("activity_undo", serde_json::json!({ "id": 1 })),
        (
            "activity_mark",
            serde_json::json!({ "id": 1, "to": "undone" }),
        ),
    ] {
        let e = invoke(&w, cmd, body).unwrap_err();
        assert_eq!(e["code"], "notSetUp", "{cmd}");
        assert_eq!(e["message"], "The activity log is not set up in this app");
        assert_eq!(e["retryable"], false);
    }
}

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-jobs --features tauri --test it tauri_activity::generated";

fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/projects/paddock-ui/shell/undo/generated")
}

#[test]
fn generated_typescript_is_current() {
    let names = ts::names_module(
        &COMMANDS,
        "ACTIVITY_COMMANDS",
        "ActivityCommandName",
        REGENERATE,
    )
    .unwrap();
    let message = sync_generated(
        &generated_dir().join("activity-commands.ts"),
        &names,
        WriteMode::from_env(),
    )
    .err();
    assert_eq!(message.map(|e| e.to_string()), None);
}

#[test]
fn error_texts_are_for_the_user() {
    use paddock_jobs::journal::JournalError;
    use paddock_jobs::tauri_activity::CommandError;
    assert_eq!(
        CommandError::NoUndoHandler.to_string(),
        "This app cannot undo actions"
    );
    assert_eq!(
        CommandError::from(JournalError::NotFound(4)).to_string(),
        "activity entry 4 is no longer in the log"
    );
}

#[test]
fn the_unguarded_handler_serves_the_same_commands() {
    let app = mock_builder()
        .invoke_handler(paddock_jobs::tauri_activity::invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let w = window(&app);
    manage(app.handle(), journal())
        .journal()
        .record(NewEntry::new("system", "Started"));
    let listed = invoke(&w, "activity_list", serde_json::json!({})).unwrap();
    assert_eq!(listed[0]["title"], "Started");
}
