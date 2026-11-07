//! The adapter on Tauri's mock runtime: the commands behind the shell's guard, events to every window. No window reaches the
//! desktop, files live in a temp folder, waits are channel waits.

#[path = "it/support.rs"]
mod support;
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use paddock_assistant::tauri::{COMMANDS, EVENT, handler, invoke_handler, manage, route};
use paddock_assistant::*;
use paddock_commands::tauri::Guard;
use serde_json::{Value, json};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use support::*;

type App = ::tauri::App<MockRuntime>;

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

fn app_with(steps: Vec<Step>, tools: Vec<Arc<dyn AssistantTool>>) -> (App, Rig) {
    let rig = rig_with(steps, tools, |_| {});
    let app = mock_builder()
        .invoke_handler(route(handler(), |_invoke| false))
        .build(mock_context(noop_assets()))
        .unwrap();
    manage(app.handle(), rig.assistant.clone());
    (app, rig)
}

#[test]
fn the_table_lists_the_commands_and_each_has_a_handler() {
    let names: Vec<&str> = COMMANDS.specs().iter().map(|s| s.name()).collect();
    for n in [
        "assistant_send",
        "assistant_stop",
        "assistant_approve_tool",
        "assistant_conversations",
        "assistant_load",
        "assistant_delete",
        "assistant_info",
    ] {
        assert!(names.contains(&n), "{n}");
    }
    let _ = invoke_handler::<MockRuntime>();
}

#[test]
fn a_question_returns_at_once_and_the_answer_arrives_as_events_in_every_window() {
    let (app, rig) = app_with(
        vec![Step::Reply(vec!["Hi", " there"], text("Hi there"))],
        vec![],
    );
    let (main, second) = (window(&app, "main"), window(&app, "w-2"));
    let heard: Vec<Arc<Mutex<Vec<Value>>>> =
        [&main, &second].iter().map(|_| Arc::default()).collect();
    let (end_tx, end_rx) = mpsc::channel();
    for (w, h) in [&main, &second].into_iter().zip(&heard) {
        let (h, end_tx) = (h.clone(), end_tx.clone());
        w.listen(EVENT, move |e| {
            let v: Value = serde_json::from_str(e.payload()).unwrap();
            let last = v["type"] == "done";
            h.lock().unwrap().push(v);
            if last {
                let _ = end_tx.send(());
            }
        });
    }
    let ack = invoke(
        &main,
        "assistant_send",
        json!({"request": {"text": "Say hi"}}),
    )
    .unwrap();
    let id = ack["conversationId"].as_str().unwrap().to_string();
    end_rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    end_rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    for h in &heard {
        let types: Vec<String> = h
            .lock()
            .unwrap()
            .iter()
            .map(|v| v["type"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(types, ["started", "delta", "delta", "done"]);
        assert_eq!(h.lock().unwrap()[0]["conversationId"], id);
    }
    // The turn is over (the final event is sent after the conversation is saved and free again).
    let list = invoke(&second, "assistant_conversations", json!({})).unwrap();
    assert_eq!(list[0]["id"], id);
    assert_eq!(list[0]["messageCount"], 2);
    let loaded = invoke(&second, "assistant_load", json!({"conversationId": id})).unwrap();
    assert_eq!(loaded["messages"][1]["text"], "Hi there");
    let info = invoke(&main, "assistant_info", json!({})).unwrap();
    assert_eq!(
        (info["provider"].as_str(), info["hasWrite"].as_bool()),
        (Some("Scripted"), Some(false))
    );
    assert_eq!(
        invoke(&main, "assistant_stop", json!({"conversationId": id})).unwrap(),
        json!(false)
    );
    invoke(&main, "assistant_delete", json!({"conversationId": id})).unwrap();
    assert_eq!(
        invoke(&main, "assistant_conversations", json!({})).unwrap(),
        json!([])
    );
    rig.assistant.shutdown();
}

#[test]
fn refusals_are_command_errors_with_the_right_code() {
    let (app, rig) = app_with(vec![Step::Block], vec![]);
    let w = window(&app, "main");
    let code = |r: Result<Value, Value>| r.unwrap_err()["code"].as_str().unwrap().to_string();
    assert_eq!(
        code(invoke(
            &w,
            "assistant_send",
            json!({"request": {"text": " "}})
        )),
        "invalid"
    );
    assert_eq!(
        code(invoke(
            &w,
            "assistant_send",
            json!({"request": {"text": "q", "conversationId": "nope"}})
        )),
        "notFound"
    );
    assert_eq!(
        code(invoke(
            &w,
            "assistant_load",
            json!({"conversationId": "nope"})
        )),
        "notFound"
    );
    assert_eq!(
        code(invoke(
            &w,
            "assistant_delete",
            json!({"conversationId": "nope"})
        )),
        "notFound"
    );
    assert_eq!(
        code(invoke(
            &w,
            "assistant_approve_tool",
            json!({"conversationId": "x", "callId": "c", "choice": "allowOnce"})
        )),
        "notFound"
    );
    let ack = invoke(&w, "assistant_send", json!({"request": {"text": "first"}})).unwrap();
    let id = ack["conversationId"].as_str().unwrap();
    assert_eq!(
        code(invoke(
            &w,
            "assistant_send",
            json!({"request": {"text": "again", "conversationId": id}})
        )),
        "busy"
    );
    assert_eq!(
        invoke(&w, "assistant_stop", json!({"conversationId": id})).unwrap(),
        json!(true)
    );
    rig.assistant.shutdown();
}

#[test]
fn the_guard_keeps_dialogs_out_and_an_app_without_an_assistant_says_so() {
    let (app, rig) = app_with(vec![], vec![]);
    let dialog = window(&app, "d-main-1");
    assert!(invoke(&dialog, "assistant_conversations", json!({})).is_err());
    rig.assistant.shutdown();

    let bare = mock_builder()
        .invoke_handler(Guard::new(&COMMANDS).wrap(invoke_handler()))
        .build(mock_context(noop_assets()))
        .unwrap();
    let main = window(&bare, "main");
    let e = invoke(&main, "assistant_info", json!({})).unwrap_err();
    assert_eq!(e["code"], "unavailable");
    // A second `manage` keeps the first assistant.
    let first = rig_with(vec![], vec![], |_| {});
    let second = rig_with(vec![], vec![], |_| {});
    let kept = manage(bare.handle(), first.assistant.clone());
    assert!(Arc::ptr_eq(
        &kept,
        &manage(bare.handle(), second.assistant.clone())
    ));
}

#[test]
fn a_write_waits_for_the_approval_command() {
    let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (app, rig) = app_with(
        vec![
            Step::Reply(vec![], calls("", vec![call("c1", "erase", json!({}))])),
            Step::Reply(vec![], text("erased")),
        ],
        vec![Arc::new(Erase(runs.clone()))],
    );
    let w = window(&app, "main");
    let (tx, rx) = mpsc::channel();
    w.listen(EVENT, move |e| {
        let v: Value = serde_json::from_str(e.payload()).unwrap();
        let _ = tx.send(v);
    });
    let ack = invoke(
        &w,
        "assistant_send",
        json!({"request": {"text": "erase", "allowWrite": true}}),
    )
    .unwrap();
    loop {
        if rx.recv_timeout(std::time::Duration::from_secs(30)).unwrap()["type"] == "approvalNeeded"
        {
            break;
        }
    }
    // The default policy asks a write every time: Always allow is an invalid choice, and the call still waits.
    let refused = invoke(
        &w,
        "assistant_approve_tool",
        json!({"conversationId": ack["conversationId"], "callId": "c1", "choice": "alwaysAllow"}),
    )
    .unwrap_err();
    assert_eq!(refused["code"], "invalid", "{refused}");
    invoke(
        &w,
        "assistant_approve_tool",
        json!({"conversationId": ack["conversationId"], "callId": "c1", "choice": "allowOnce"}),
    )
    .unwrap();
    loop {
        if rx.recv_timeout(std::time::Duration::from_secs(30)).unwrap()["type"] == "done" {
            break;
        }
    }
    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1);
    rig.assistant.shutdown();
}

#[test]
fn a_conversation_that_cannot_be_deleted_is_a_failed_command() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("assistant.json"),
        json!({"version": 99}).to_string(),
    )
    .unwrap();
    let rig = rig_in(dir, vec![Step::Reply(vec![], text("a"))], vec![], |_| {});
    let app = mock_builder()
        .invoke_handler(route(handler(), |_invoke| false))
        .build(mock_context(noop_assets()))
        .unwrap();
    manage(app.handle(), rig.assistant.clone());
    let w = window(&app, "main");
    let (tx, rx) = mpsc::channel();
    w.listen(EVENT, move |e| {
        let v: Value = serde_json::from_str(e.payload()).unwrap();
        if v["type"] == "done" {
            let _ = tx.send(());
        }
    });
    let ack = invoke(&w, "assistant_send", json!({"request": {"text": "q"}})).unwrap();
    rx.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    let e = invoke(
        &w,
        "assistant_delete",
        json!({"conversationId": ack["conversationId"]}),
    )
    .unwrap_err();
    assert_eq!(e["code"], "failed");
}

#[test]
fn commands_outside_the_table_go_to_the_apps_handler() {
    let seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = seen.clone();
    let app = mock_builder()
        .invoke_handler(route(handler(), move |_invoke| {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            false
        }))
        .build(mock_context(noop_assets()))
        .unwrap();
    let w = window(&app, "main");
    let _ = invoke(&w, "app_other", json!({}));
    assert!(seen.load(std::sync::atomic::Ordering::SeqCst));
}
