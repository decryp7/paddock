//! The adapter on Tauri's mock runtime (`tauri::test`): no window ever reaches the desktop. Ported from Debrief's
//! `dialog_windows.rs` tests, plus the guard on a real invoke path with this crate's grants.

use super::*;
use crate::contract::Contracts;
use crate::window::{MAX_OPEN_PER_HOST, Parenting};
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{Listener, WebviewUrl};
use paddock_commands::tauri::Guard;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[::tauri::command]
async fn app_secret() -> String {
    "secret".into()
}

#[::tauri::command]
async fn app_status() -> String {
    "ok".into()
}

paddock_commands::command_table! {
    static TABLE, macro handler;
    Mutation [MAIN | DIALOG] limit 9 * paddock_commands::MIB => crate::tauri::dialog_open,
    Query [DIALOG] => crate::tauri::dialog_take_input,
    Mutation [DIALOG] limit 2 * paddock_commands::MIB => crate::tauri::dialog_close,
    Quiet [DIALOG] => crate::tauri::dialog_fit_height,
    Query [WINDOW] => app_status,
    Query [WINDOW] => app_secret,
}

/// Contracts for the ids these tests open.
fn contracts() -> Arc<Contracts> {
    let entry = |id: &str, kind: &str, modality: &str, required: bool, instanced: bool| {
        format!(
            r#""{id}": {{ "kind": "{kind}", "modality": "{modality}", "requiredChoice": {required}, "instanced": {instanced}, "commands": ["app_status"] }}"#
        )
    };
    let mut e = vec![
        entry("ai-setup", "form", "modeless", false, true),
        entry("settings", "tool", "modeless", false, false),
        entry("about", "viewer", "modeless", false, false),
        entry("close-prompt", "decision", "modal", true, false),
        entry("unsaved-changes", "decision", "modal", false, false),
        entry("confirm-x", "decision", "modal", false, false),
        entry("too-many", "form", "modeless", false, false),
    ];
    e.extend(
        (0..MAX_OPEN_PER_HOST).map(|i| entry(&format!("d{i}"), "form", "modeless", false, false)),
    );
    let json = format!(r#"{{ "version": 1, "dialogs": {{ {} }} }}"#, e.join(","));
    Arc::new(Contracts::parse(&json).expect("test contracts"))
}

/// Every log record of the guard, as (level, message, dialog, command).
static LOGGED: Mutex<Vec<(log::Level, String, String, String)>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        let kv = |k: &str| {
            record
                .key_values()
                .get(k.into())
                .map(|v| v.to_string())
                .unwrap_or_default()
        };
        LOGGED.lock().unwrap_or_else(|p| p.into_inner()).push((
            record.level(),
            record.args().to_string(),
            kv("Dialog"),
            kv("Command"),
        ));
    }
    fn flush(&self) {}
}

fn logged_for(dialog: &str) -> Vec<(log::Level, String, String, String)> {
    LOGGED
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .filter(|r| r.2 == dialog)
        .cloned()
        .collect()
}

fn app_with(dialogs: Option<Dialogs>) -> ::tauri::App<MockRuntime> {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Trace);
    let guard = Guard::new(&TABLE).dialog_grants(grant_of);
    let app = mock_builder()
        .invoke_handler(guard.wrap(handler!()))
        .build(mock_context(noop_assets()))
        .unwrap();
    if let Some(d) = dialogs {
        app.manage(d);
    }
    window(&app, "main");
    app
}

fn app() -> ::tauri::App<MockRuntime> {
    app_with(Some(Dialogs::new(contracts())))
}

fn window(app: &::tauri::App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    app.get_webview_window(label).unwrap_or_else(|| {
        WebviewWindowBuilder::new(app, label, WebviewUrl::default())
            .build()
            .unwrap()
    })
}

fn os_window(app: &::tauri::App<MockRuntime>, label: &str) -> Window<MockRuntime> {
    window(app, label).as_ref().window()
}

fn request(id: &str) -> DialogRequest {
    DialogRequest {
        dialog_id: id.into(),
        instance: None,
        title: "Set up the assistant".into(),
        input: Some(json!({ "modelId": "m1" })),
        width: Some(760.0),
        height: None,
        min_width: None,
        min_height: None,
        remembered: None,
    }
}

fn recorder(w: &WebviewWindow<MockRuntime>, event: &str) -> Arc<Mutex<Vec<String>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    w.listen(event, move |e| {
        s.lock().unwrap().push(e.payload().to_string())
    });
    seen
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

fn state(app: &::tauri::App<MockRuntime>) -> ::tauri::State<'_, Dialogs> {
    app.state::<Dialogs>()
}

#[test]
fn opening_twice_focuses_the_one_window_and_hands_the_input_to_it_only() {
    let app = app();
    let h = app.handle().clone();
    let first = open(&h, "main", request("ai-setup")).unwrap();
    assert_eq!((first.label.as_str(), first.existing), ("d-main-1", false));
    assert!(h.get_webview_window("d-main-1").is_some());
    let mut other = request("ai-setup");
    other.instance = Some("abc1".into());
    assert!(
        !open(&h, "main", other.clone()).unwrap().existing,
        "another input is another window"
    );
    assert!(open(&h, "main", other).unwrap().existing);
    let again = open(&h, "main", request("ai-setup")).unwrap();
    assert_eq!((again.label.as_str(), again.existing), ("d-main-1", true));
    // A dialog can open another one for the same host; a pop-out opens them for its host too.
    let nested = open(&h, "d-main-1", request("confirm-x")).unwrap();
    assert_eq!(nested.label, "d-main-3");
    assert_eq!(state(&app).get("d-main-3").unwrap().parent, "d-main-1");
    let start = take_input(&h, "d-main-1").unwrap();
    assert_eq!(start.input, Some(json!({ "modelId": "m1" })));
    assert_eq!(
        (start.kind, start.modality, start.title.as_str()),
        (
            crate::Kind::Form,
            Modality::Modeless,
            "Set up the assistant"
        )
    );
    assert_eq!(
        take_input(&h, "main").unwrap_err().code,
        DialogErrorCode::NotADialog
    );
    let e = open(&h, "w-9", request("about")).unwrap_err();
    assert_eq!(e.code, DialogErrorCode::Closed);
    window(&app, "p-main-1");
    let from_popout = open(&h, "p-main-1", request("about")).unwrap();
    assert_eq!(state(&app).get(&from_popout.label).unwrap().host, "main");
}

#[test]
fn only_dialogs_with_a_contract_open_and_the_contract_decides_modality() {
    let app = app();
    let h = app.handle().clone();
    let e = open(&h, "main", request("not-a-dialog")).unwrap_err();
    assert!(
        e.code == DialogErrorCode::Unknown && e.message.contains("no dialog"),
        "{e}"
    );
    let mut keyed = request("about");
    keyed.instance = Some("k1".into());
    let e = open(&h, "main", keyed).unwrap_err();
    assert!(
        e.message.contains("instance"),
        "only an instanced dialog takes a key"
    );
    assert_eq!(
        open(&h, "main", request("../x")).unwrap_err().code,
        DialogErrorCode::Invalid
    );
    assert_eq!(
        open(&h, "browser-main", request("about")).unwrap_err().code,
        DialogErrorCode::NotADialog,
        "a webview outside the label scheme opens nothing"
    );
    // The close prompt is a modal required choice by contract, whatever the webview asks.
    open(&h, "main", request("close-prompt")).unwrap();
    let entry = state(&app).get("d-main-1").unwrap();
    assert!(entry.modality == Modality::Modal && entry.required_choice);
    // The command guard asks which dialog a window shows: the layer's own record, never the webview's word.
    let grant = grant_of(&h, "d-main-1").expect("an open dialog has a grant");
    assert_eq!(
        paddock_commands::DialogGrant::dialog(&grant),
        "close-prompt"
    );
    assert!(grant_of(&h, "d-main-9").is_none() && grant_of(&h, "main").is_none());
}

#[test]
fn without_the_managed_state_nothing_opens_and_every_call_degrades() {
    let app = app_with(None);
    let h = app.handle().clone();
    let e = open(&h, "main", request("about")).unwrap_err();
    assert!(e.code == DialogErrorCode::NotReady && e.retryable);
    assert!(grant_of(&h, "d-main-1").is_none());
    assert!(close_requested(&h, "d-main-1"));
    page_unresponsive(&h, "d-main-1", true);
    on_destroyed(&h, "d-main-1");
    assert_eq!(close_all_of(&h, "main"), 0);
    set_visible(&h, "main", false);
    moved(&os_window(&app, "main"));
    assert_eq!(
        take_input(&h, "d-main-1").unwrap_err().code,
        DialogErrorCode::NotReady
    );
}

#[test]
fn at_most_eight_dialogs_per_host() {
    let app = app();
    let h = app.handle().clone();
    for i in 0..MAX_OPEN_PER_HOST {
        open(&h, "main", request(&format!("d{i}"))).unwrap();
    }
    let e = open(&h, "main", request("too-many")).unwrap_err();
    assert!(e.code == DialogErrorCode::Limit && e.message.contains("At most 8"));
}

#[test]
fn a_window_that_cannot_be_built_leaves_no_record() {
    let app = app();
    let h = app.handle().clone();
    // A webview already holds the label the next dialog would get.
    window(&app, "d-main-1");
    let e = open(&h, "main", request("about")).unwrap_err();
    assert!(
        e.code == DialogErrorCode::Failed && e.message.starts_with("could not open"),
        "{e}"
    );
    assert!(state(&app).of_host("main").is_empty());
}

#[test]
fn a_result_goes_to_the_opener_and_closing_reports_where_the_window_was() {
    let app = app();
    let h = app.handle().clone();
    let main = window(&app, "main");
    let results = recorder(&main, RESULT_EVENT);
    let closed = recorder(&main, CLOSED_EVENT);
    open(&h, "main", request("ai-setup")).unwrap();
    let dialog = os_window(&app, "d-main-1");
    on_window_event(&dialog, &WindowEvent::Moved(PhysicalPosition::new(5, 5)));
    assert!(state(&app).get("d-main-1").unwrap().bounds.is_some());
    close(&h, &dialog, Some(json!(true))).unwrap();
    on_window_event(&dialog, &WindowEvent::Destroyed);
    let r: Value = serde_json::from_str(&results.lock().unwrap()[0]).unwrap();
    assert_eq!(
        r,
        json!({ "label": "d-main-1", "dialogId": "ai-setup", "result": true })
    );
    let c: Value = serde_json::from_str(&closed.lock().unwrap()[0]).unwrap();
    assert_eq!(c["dialogId"], "ai-setup");
    assert!(state(&app).of_host("main").is_empty());
    // The closed event carries the remembered place as an offset from the parent, never absolute bounds (the mock
    // window has no size, so there is nothing to remember).
    assert!(
        c.get("bounds").is_none() && c["remembered"].is_null(),
        "{c}"
    );
    // A dialog dismissed (no result) only reports closing; one destroyed twice reports once.
    open(&h, "main", request("ai-setup")).unwrap();
    let d2 = os_window(&app, "d-main-2");
    close(&h, &d2, None).unwrap();
    on_window_event(&d2, &WindowEvent::Destroyed);
    on_window_event(&d2, &WindowEvent::Destroyed);
    assert_eq!(results.lock().unwrap().len(), 1);
    assert_eq!(closed.lock().unwrap().len(), 2);
}

#[test]
fn a_modal_disables_its_parent_natively_except_on_macos_and_the_last_one_re_enables_it() {
    for os in ["windows", "linux", "macos"] {
        let app = app();
        let h = app.handle().clone();
        let closed = recorder(&window(&app, "main"), CLOSED_EVENT);
        let a = open_on(&h, "main", request("unsaved-changes"), os).unwrap();
        let b = open_on(&h, "main", request("confirm-x"), os).unwrap();
        assert_eq!(state(&app).modal_children_of("main"), 2);
        on_destroyed_on(&h, &a.label, os);
        assert_eq!(state(&app).modal_children_of("main"), 1);
        on_destroyed_on(&h, &b.label, os);
        assert_eq!(state(&app).modal_children_of("main"), 0);
        assert_eq!(closed.lock().unwrap().len(), 2, "{os}");
        assert_eq!(
            Parenting::of(Modality::Modeless, os) == Parenting::Owned,
            os != "macos"
        );
    }
}

#[test]
fn while_the_app_exits_closing_dialogs_report_nothing_and_a_gone_parent_is_skipped() {
    let app = app();
    let h = app.handle().clone();
    let closed = recorder(&window(&app, "main"), CLOSED_EVENT);
    open(&h, "main", request("about")).unwrap();
    state(&app).set_exiting(true);
    on_destroyed(&h, "d-main-1");
    assert!(closed.lock().unwrap().is_empty() && state(&app).get("d-main-1").is_none());
    state(&app).set_exiting(false);
    // The opener is gone before its dialog (its window never reached the manager here).
    let about = state(&app).contracts().get("about").unwrap().clone();
    let orphan = state(&app)
        .reserve(OpenDialog::new("main", "w-7", "about", &about))
        .unwrap();
    set_visible(&h, "main", false);
    on_destroyed(&h, &orphan);
    assert!(closed.lock().unwrap().is_empty() && state(&app).get(&orphan).is_none());
}

#[test]
fn oversized_results_are_refused_and_only_dialogs_may_close() {
    let app = app();
    let h = app.handle().clone();
    open(&h, "main", request("ai-setup")).unwrap();
    let big = Some(Value::String(
        "x".repeat(crate::window::MAX_RESULT_BYTES + 1),
    ));
    let e = close(&h, &os_window(&app, "d-main-1"), big).unwrap_err();
    assert_eq!(e.code, DialogErrorCode::Limit);
    let e = close(&h, &os_window(&app, "main"), None).unwrap_err();
    assert_eq!(e.code, DialogErrorCode::NotADialog);
}

#[test]
fn a_close_request_goes_to_the_dialog_first() {
    let app = app();
    let h = app.handle().clone();
    open(&h, "main", request("settings")).unwrap();
    let asked = recorder(&window(&app, "d-main-1"), CLOSE_REQUESTED_EVENT);
    assert!(!close_requested(&h, "d-main-1"), "the dialog decides");
    assert_eq!(asked.lock().unwrap().len(), 1);
    assert!(
        close_requested(&h, "d-main-9"),
        "unknown windows just close"
    );
}

#[test]
fn a_required_choice_ignores_close_requests_unless_its_page_hangs() {
    let app = app();
    let h = app.handle().clone();
    open(&h, "main", request("close-prompt")).unwrap();
    let asked = recorder(&window(&app, "d-main-1"), CLOSE_REQUESTED_EVENT);
    assert!(!close_requested(&h, "d-main-1"));
    assert!(
        asked.lock().unwrap().is_empty(),
        "nothing to cancel, so the dialog is not even asked"
    );
    // The window watchdog decided the page does not answer: nothing could ever answer the choice.
    page_unresponsive(&h, "d-main-1", true);
    assert!(close_requested(&h, "d-main-1"), "the OS close ends it");
    page_unresponsive(&h, "d-main-1", false);
    assert!(
        !close_requested(&h, "d-main-1"),
        "answering again: required again"
    );
}

#[test]
fn closing_the_host_closes_its_dialogs_and_hiding_it_hides_them() {
    let app = app();
    let h = app.handle().clone();
    open(&h, "main", request("about")).unwrap();
    open(&h, "main", request("settings")).unwrap();
    set_visible(&h, "main", false);
    set_visible(&h, "main", true);
    // A pop-out's end does not touch the host's dialogs; the host's end closes them.
    on_window_event(&os_window(&app, "p-main-1"), &WindowEvent::Destroyed);
    assert_eq!(state(&app).of_host("main").len(), 2);
    on_window_event(&os_window(&app, "main"), &WindowEvent::Destroyed);
    assert!(state(&app).of_host("main").is_empty());
    assert_eq!(close_all_of(&h, "main"), 0);
    // Events of other windows and kinds are not the dialog layer's.
    on_window_event(&os_window(&app, "w-2"), &WindowEvent::Focused(true));
}

#[test]
fn a_refit_sizes_the_window_by_kind_on_its_own_monitor() {
    let app = app();
    let h = app.handle().clone();
    open(&h, "main", request("about")).unwrap();
    let dialog = os_window(&app, "d-main-1");
    // The mock window measures 0x0 at (0, 0); a monitor whose work area starts below it moves it down into place.
    let m = Monitor {
        work: Rect {
            x: 0.0,
            y: 25.0,
            width: 2560.0,
            height: 1415.0,
        },
        scale: 1.0,
    };
    for os in ["windows", "macos"] {
        assert_eq!(fit_height(&h, &dialog, 700.0, Some(m), os), Ok(()));
    }
    assert_eq!(fit_height(&h, &dialog, 700.0, None, "linux"), Ok(()));
    let e = fit_height(&h, &dialog, f64::NAN, None, "linux").unwrap_err();
    assert_eq!(e.code, DialogErrorCode::Invalid);
    let e = fit_height(&h, &os_window(&app, "main"), 300.0, None, "linux").unwrap_err();
    assert_eq!(e.code, DialogErrorCode::NotADialog);
}

#[test]
fn the_monitor_is_the_current_one_or_the_one_the_window_overlaps_most() {
    let monitor = |name: Option<&str>, x: i32, scale: f64| -> ::tauri::Monitor {
        let size = ::tauri::PhysicalSize::new(1920, 1080);
        let position = PhysicalPosition::new(x, 0);
        tauri_runtime::monitor::Monitor {
            name: name.map(str::to_string),
            size,
            position,
            work_area: ::tauri::PhysicalRect { position, size },
            scale_factor: scale,
        }
        .into()
    };
    let left = || {
        vec![
            monitor(Some("left"), -1920, 1.5),
            monitor(Some("primary"), 0, 1.0),
        ]
    };
    let on_left = Rect {
        x: -1500.0,
        y: 100.0,
        width: 800.0,
        height: 600.0,
    };
    let (m, name) = pick_monitor(None, left, Some(on_left)).unwrap();
    assert_eq!((name.as_str(), m.work.x, m.scale), ("left", -1920.0, 1.5));
    let (_, name) = pick_monitor(Some(monitor(Some("current"), 0, 2.0)), Vec::new, None).unwrap();
    assert_eq!(name, "current");
    assert!(
        pick_monitor(None, left, None).is_none(),
        "no window rectangle, no guess"
    );
    assert!(pick_monitor(None, Vec::new, Some(on_left)).is_none());
    assert_eq!(
        pick_monitor(Some(monitor(None, 0, 1.0)), Vec::new, None)
            .unwrap()
            .1,
        ""
    );
    assert!(
        monitor_of_window(&os_window(&app(), "main")).is_none(),
        "the mock reports none"
    );
}

#[test]
fn placements_are_logged_with_the_monitor_they_chose() {
    let m = Monitor {
        work: Rect {
            x: -1920.0,
            y: 0.0,
            width: 1920.0,
            height: 1050.0,
        },
        scale: 1.5,
    };
    assert_eq!(
        describe(Some(&(m, "DELL U2720Q".into()))),
        (
            "DELL U2720Q".to_string(),
            "-1920,0 1920x1050 @1.5".to_string()
        )
    );
    assert_eq!(describe(None).0, "unknown");
}

#[test]
fn the_apps_window_hooks_run_on_every_dialog_window() {
    let app = app();
    let h = app.handle().clone();
    let built = Arc::new(AtomicUsize::new(0));
    let configured = Arc::new(AtomicUsize::new(0));
    let (b, c) = (built.clone(), configured.clone());
    app.manage(
        WindowHooks::<MockRuntime>::new()
            .builder(move |builder| {
                c.fetch_add(1, Ordering::SeqCst);
                builder.focused(true)
            })
            .built(move |_w| {
                b.fetch_add(1, Ordering::SeqCst);
            }),
    );
    open(&h, "main", request("about")).unwrap();
    open(&h, "main", request("settings")).unwrap();
    assert_eq!(
        (
            configured.load(Ordering::SeqCst),
            built.load(Ordering::SeqCst)
        ),
        (2, 2)
    );
}

/// The commands on the invoke path behind the guard, and the acceptance of plan S5: a dialog calling a command its
/// contract does not declare is refused and logged as an error naming the dialog and the command.
#[test]
fn the_guard_holds_each_dialog_to_its_contract_on_the_invoke_path() {
    let app = app();
    let main = window(&app, "main");
    let info = invoke(
        &main,
        "dialog_open",
        json!({ "request": { "dialogId": "settings", "title": "Settings" } }),
    )
    .unwrap();
    assert_eq!(
        info,
        json!({ "label": "d-main-1", "dialogId": "settings", "existing": false })
    );
    let dialog = window(&app, "d-main-1");
    let start = invoke(&dialog, "dialog_take_input", json!({})).unwrap();
    assert_eq!(
        (start["dialogId"].as_str(), start["kind"].as_str()),
        (Some("settings"), Some("tool"))
    );
    assert_eq!(invoke(&dialog, "app_status", json!({})), Ok(json!("ok")));
    assert_eq!(
        invoke(&dialog, "dialog_fit_height", json!({ "content": 400.0 })),
        Ok(Value::Null)
    );
    assert!(
        logged_for("settings")
            .iter()
            .all(|r| r.0 != log::Level::Error),
        "allowed calls are not refusals"
    );

    let e = invoke(&dialog, "app_secret", json!({})).unwrap_err();
    assert_eq!(
        (e["code"].as_str(), e["dialog"].as_str()),
        (Some("denied"), Some("settings"))
    );
    let errors: Vec<_> = logged_for("settings")
        .into_iter()
        .filter(|r| r.0 == log::Level::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].3, "app_secret");
    assert!(
        errors[0].1.starts_with(
            "The settings dialog called app_secret, which its contract does not declare"
        ),
        "{}",
        errors[0].1
    );
    // Malformed arguments are refused before the handler.
    assert!(invoke(&main, "dialog_open", json!({})).is_err());
    assert!(invoke(&dialog, "dialog_fit_height", json!({ "content": "tall" })).is_err());
    // A dialog's own command errors come back typed.
    let e = invoke(
        &main,
        "dialog_open",
        json!({ "request": { "dialogId": "nope", "title": "x" } }),
    )
    .unwrap_err();
    assert_eq!(e["code"], "unknown");
    assert_eq!(
        invoke(&dialog, "dialog_close", json!({ "result": { "ok": true } })),
        Ok(Value::Null)
    );
}
