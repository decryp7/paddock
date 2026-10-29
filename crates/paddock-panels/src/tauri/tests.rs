//! The adapter on Tauri's mock runtime (`tauri::test`): no window ever reaches the desktop, no thread outlives its drag.
//! Ported from Debrief's `panel_windows.rs` and `drag_ghost` tests, plus the relay and the guard on a real invoke path.

use super::*;
use crate::ghost::geometry::{GhostSize, PhysRect, Screen};
use crate::ghost::request::GhostContent;
use crate::registry::MAX_POPOUTS;
use crate::sync::{BURST, MAX_MESSAGE_BYTES};
use crate::wire::{
    PANELS_EVENT, PanelError, PanelErrorCode, PopOutBounds, PopOutInfo, PopOutRequest, SyncPublish,
    SyncScope, UI_SYNC_EVENT,
};
use crate::{GhostRequest, Panel, PopOut};
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{Listener, Manager, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent};
use paddock_commands::tauri::Guard;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn app_with(panels: Option<Panels>) -> ::tauri::App<MockRuntime> {
    let guard = Guard::new(&crate::COMMANDS);
    let app = mock_builder()
        .invoke_handler(guard.wrap(invoke_handler()))
        .build(mock_context(noop_assets()))
        .unwrap();
    if let Some(p) = panels {
        app.manage(p);
    }
    window(&app, "main");
    app
}

/// A mouse button the test sets: tests never read the user's mouse (rule 7).
#[derive(Clone, Default)]
struct Button(Arc<Mutex<Option<bool>>>);

impl Button {
    fn set(&self, down: Option<bool>) {
        *self.0.lock().unwrap() = down;
    }
}

impl Pointer for Button {
    fn primary_button_down(&self) -> Option<bool> {
        *self.0.lock().unwrap()
    }
}

/// The test app, with a pointer that cannot tell (as on Linux) unless a test sets it through [`app_with_button`].
fn app() -> ::tauri::App<MockRuntime> {
    app_with_button().0
}

fn app_with_button() -> (::tauri::App<MockRuntime>, Button) {
    let button = Button::default();
    let app = app_with(Some(Panels::builder().pointer(button.clone()).build()));
    (app, button)
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

fn recorder(w: &WebviewWindow<MockRuntime>, event: &str) -> Arc<Mutex<Vec<Value>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    w.listen(event, move |e| {
        s.lock()
            .unwrap()
            .push(serde_json::from_str(e.payload()).unwrap_or(Value::Null))
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

fn request(panel: &str) -> PopOutRequest {
    PopOutRequest {
        panel: panel.into(),
        title: Some("Inspector".into()),
        bounds: Some(PopOutBounds {
            x: 10.0,
            y: 20.0,
            width: 500.0,
            height: 300.0,
            scale: Some(1.0),
        }),
        keep_on_top: None,
    }
}

fn state(app: &::tauri::App<MockRuntime>) -> ::tauri::State<'_, Panels> {
    app.state::<Panels>()
}

// ---- opening, listing, focusing -------------------------------------------------------------------------------------

#[test]
fn popping_out_opens_a_window_of_the_host_group_and_tells_the_host() {
    let app = app();
    let h = app.handle().clone();
    let events = recorder(&window(&app, "main"), PANELS_EVENT);
    let info = open(&h, "main", request("pn.code#src/a b.rs")).unwrap();
    assert_eq!(info.label, "p-main-1");
    assert_eq!(
        (info.host.as_str(), info.panel.as_str(), info.title.as_str()),
        ("main", "pn.code#src/a b.rs", "Inspector")
    );
    assert!(
        h.get_webview_window("p-main-1").is_some(),
        "a real (mock) window"
    );
    assert_eq!(
        events.lock().unwrap().as_slice(),
        [json!({ "kind": "changed" })]
    );
    // Popping out the same panel again focuses the existing window.
    assert_eq!(
        open(&h, "main", request("pn.code#src/a b.rs"))
            .unwrap()
            .label,
        "p-main-1"
    );
    assert_eq!(events.lock().unwrap().len(), 1);
    // Another instance of the view is another window; a title is made when missing.
    let mut bare = request("pn.code#other");
    bare.title = None;
    bare.bounds = None;
    let second = open(&h, "main", bare).unwrap();
    assert_eq!(
        (second.label.as_str(), second.title.as_str()),
        ("p-main-2", "pn.code#other")
    );
    // The pop-out sees its group's list, so does the host.
    for from in ["main", "p-main-1"] {
        assert_eq!(list(&h, from).len(), 2, "{from}");
    }
    assert!(list(&h, "w-2").is_empty());
    assert!(focus(&h, "main", "pn.code#other"));
    assert!(!focus(&h, "main", "pn.nothing") && !focus(&h, "main", "not a panel"));
    assert!(
        !focus(&h, "w-2", "pn.code#other"),
        "another group's pop-out is not reachable"
    );
}

#[test]
fn the_window_title_names_the_host() {
    assert_eq!(popout::window_title("Log", ""), "Log");
    assert_eq!(
        popout::window_title("Log", "Incident 7"),
        "Log - Incident 7"
    );
}

#[test]
fn bad_requests_are_refused_before_anything_opens() {
    let app = app();
    let h = app.handle().clone();
    let code = |r: Result<PopOutInfo, PanelError>| r.unwrap_err().code;
    assert_eq!(
        code(open(&h, "main", request("../etc"))),
        PanelErrorCode::Invalid
    );
    assert_eq!(
        code(open(&h, "main", request("pn.code#"))),
        PanelErrorCode::Invalid
    );
    assert_eq!(
        code(open(
            &h,
            "main",
            request(&format!("pn.code#{}", "k".repeat(2000)))
        )),
        PanelErrorCode::Invalid
    );
    // A pop-out, a dialog and an unknown webview cannot host pop-outs.
    for caller in ["p-main-1", "d-main-1", "tray", "other"] {
        assert_eq!(
            code(open(&h, caller, request("pn.code"))),
            PanelErrorCode::NotAllowed,
            "{caller}"
        );
    }
    assert_eq!(
        code(open(&h, "w-9", request("pn.code"))),
        PanelErrorCode::Closed
    );
    assert!(h.get_webview_window("p-main-1").is_none());
    // Without the layer set up.
    let bare = app_with(None);
    assert_eq!(
        code(open(bare.handle(), "main", request("pn.code"))),
        PanelErrorCode::NotReady
    );
    assert!(list(bare.handle(), "main").is_empty());
}

#[test]
fn at_most_twelve_pop_outs_per_host() {
    let app = app();
    let h = app.handle().clone();
    for i in 0..MAX_POPOUTS {
        open(&h, "main", request(&format!("pn.view{i}"))).unwrap();
    }
    let e = open(&h, "main", request("pn.one-too-many")).unwrap_err();
    assert_eq!(e.code, PanelErrorCode::TooMany);
    assert!(e.message.contains("At most 12"), "{e}");
    assert_eq!(state(&app).popouts().of_host("main").len(), MAX_POPOUTS);
    assert!(
        h.get_webview_window("p-main-13").is_none(),
        "the refused one has no window"
    );
    // Another window has a budget of its own; closing one frees a slot.
    window(&app, "w-2");
    assert!(open(&h, "w-2", request("pn.view0")).is_ok());
    on_window_event(&os_window(&app, "p-main-3"), &WindowEvent::Destroyed);
    assert!(open(&h, "main", request("pn.one-too-many")).is_ok());
}

#[test]
fn a_window_that_cannot_be_built_leaves_nothing_behind() {
    let app = app();
    let h = app.handle().clone();
    // A label already taken by another webview: the build fails and the record is dropped again.
    window(&app, "p-main-1");
    let e = open(&h, "main", request("pn.code")).unwrap_err();
    assert_eq!(e.code, PanelErrorCode::Failed);
    assert!(state(&app).popouts().of_host("main").is_empty());
}

#[test]
fn the_apps_hooks_shape_every_pop_out() {
    let app = app();
    let built = Arc::new(Mutex::new(Vec::new()));
    let seen = built.clone();
    let shaped = Arc::new(Mutex::new(0));
    let count = shaped.clone();
    app.manage(
        WindowHooks::<MockRuntime>::new()
            .builder(move |b| {
                *count.lock().unwrap() += 1;
                b
            })
            .built(move |w| seen.lock().unwrap().push(w.label().to_string())),
    );
    open(app.handle(), "main", request("pn.code")).unwrap();
    assert_eq!(built.lock().unwrap().as_slice(), ["p-main-1"]);
    assert_eq!(*shaped.lock().unwrap(), 1);
}

// ---- closing, docking ---------------------------------------------------------------------------------------------

#[test]
fn closing_a_pop_out_docks_the_panel_back_into_the_host() {
    let app = app();
    let h = app.handle().clone();
    let events = recorder(&window(&app, "main"), PANELS_EVENT);
    open(&h, "main", request("pn.code")).unwrap();
    events.lock().unwrap().clear();
    on_window_event(&os_window(&app, "p-main-1"), &WindowEvent::Destroyed);
    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            json!({ "kind": "returned", "panel": "pn.code", "label": "p-main-1" }),
            json!({ "kind": "changed" })
        ]
    );
    assert!(state(&app).popouts().of_host("main").is_empty());
    // A second Destroyed (or an unknown label) does nothing.
    on_window_event(&os_window(&app, "p-main-1"), &WindowEvent::Destroyed);
    assert_eq!(events.lock().unwrap().len(), 2);
    // While quitting nothing is docked back (the layout keeps the pop-out for next time).
    open(&h, "main", request("pn.log")).unwrap();
    events.lock().unwrap().clear();
    state(&app).set_exiting(true);
    on_window_event(&os_window(&app, "p-main-2"), &WindowEvent::Destroyed);
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn closing_the_host_closes_its_pop_outs_without_docking() {
    let app = app();
    let h = app.handle().clone();
    window(&app, "w-2");
    open(&h, "main", request("pn.code")).unwrap();
    open(&h, "main", request("pn.log")).unwrap();
    open(&h, "w-2", request("pn.code")).unwrap();
    let events = recorder(&window(&app, "w-2"), PANELS_EVENT);
    // The host window goes: the event handler closes its pop-outs; the others keep theirs.
    on_window_event(&os_window(&app, "w-2"), &WindowEvent::Destroyed);
    assert!(
        state(&app).popouts().of_host("w-2").is_empty(),
        "the host's pop-outs are gone"
    );
    assert_eq!(
        state(&app).popouts().of_host("main").len(),
        2,
        "other hosts keep theirs"
    );
    assert!(
        events.lock().unwrap().is_empty(),
        "nothing is docked back into a closing host"
    );
    assert_eq!(close_all_of(&h, "nobody"), 0);
    assert_eq!(close_all_of(app_with(None).handle(), "main"), 0);
}

#[test]
fn a_pop_out_docks_itself_and_a_host_docks_one_by_panel() {
    let app = app();
    let h = app.handle().clone();
    window(&app, "w-2");
    open(&h, "main", request("pn.code")).unwrap();
    open(&h, "main", request("pn.log")).unwrap();
    open(&h, "w-2", request("pn.only-there")).unwrap();
    assert!(
        dock_back(&h, "p-main-1", None).is_ok(),
        "a pop-out docks itself"
    );
    assert!(
        dock_back(&h, "main", Some("pn.log")).is_ok(),
        "a host names the panel"
    );
    let code = |r: Result<(), PanelError>| r.unwrap_err().code;
    assert_eq!(
        code(dock_back(&h, "main", None)),
        PanelErrorCode::Invalid,
        "a host must name one"
    );
    assert_eq!(
        code(dock_back(&h, "main", Some("???"))),
        PanelErrorCode::Invalid
    );
    assert_eq!(
        code(dock_back(&h, "main", Some("pn.nothing"))),
        PanelErrorCode::Closed
    );
    assert_eq!(
        code(dock_back(&h, "main", Some("pn.only-there"))),
        PanelErrorCode::Closed,
        "the other host's pop-out is not reachable"
    );
    assert_eq!(
        code(dock_back(&h, "p-main-9", None)),
        PanelErrorCode::Closed,
        "already closed"
    );
    assert_eq!(
        code(dock_back(app_with(None).handle(), "main", None)),
        PanelErrorCode::NotReady
    );
}

#[test]
fn hiding_the_host_hides_its_pop_outs_with_it() {
    let app = app();
    let h = app.handle().clone();
    open(&h, "main", request("pn.code")).unwrap();
    set_visible(&h, "main", false);
    set_visible(&h, "main", true);
    set_visible(&h, "nobody", false);
    set_visible(app_with(None).handle(), "main", false);
}

#[test]
fn moving_and_resizing_a_pop_out_tells_its_host() {
    let app = app();
    let h = app.handle().clone();
    open(&h, "main", request("pn.code")).unwrap();
    let events = recorder(&window(&app, "main"), PANELS_EVENT);
    let popout = os_window(&app, "p-main-1");
    on_window_event(&popout, &WindowEvent::Moved(Default::default()));
    let seen = events.lock().unwrap().clone();
    assert_eq!(seen[0], json!({ "kind": "changed" }));
    assert_eq!(
        (seen[1]["kind"].as_str(), seen[1]["panel"].as_str()),
        (Some("moved"), Some("pn.code"))
    );
    assert!(seen[1]["x"].is_number() && seen[1]["y"].is_number());
    on_window_event(&popout, &WindowEvent::Resized(Default::default()));
    assert_eq!(
        events.lock().unwrap().len(),
        3,
        "a resize is a change, not a move"
    );
    // An unknown pop-out and other windows' moves are ignored.
    super::events::dragged(&h, "main", "p-main-9");
    on_window_event(
        &os_window(&app, "main"),
        &WindowEvent::Moved(Default::default()),
    );
    assert_eq!(events.lock().unwrap().len(), 3);
    assert_eq!(
        super::events::to_logical(200.0, 100.0, 2.0),
        Some((100.0, 50.0))
    );
    assert_eq!(super::events::to_logical(f64::NAN, 1.0, 1.0), None);
    assert_eq!(super::events::to_logical(1.0, 1.0, 0.0), None);
}

#[test]
fn keeping_on_top_rebuilds_the_pop_out_without_docking_it_back() {
    let app = app();
    let h = app.handle().clone();
    let events = recorder(&window(&app, "main"), PANELS_EVENT);
    let first = open(&h, "main", request("pn.code")).unwrap();
    events.lock().unwrap().clear();
    set_keep_on_top(&h, &first.label, true).unwrap();
    let listed = list(&h, "main");
    assert_eq!(listed.len(), 1, "replaced, not duplicated");
    assert!(
        listed[0].keep_on_top && listed[0].panel == "pn.code" && listed[0].label != first.label
    );
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .all(|e| e["kind"] != "returned"),
        "the panel stays popped out: nothing is docked back"
    );
    // Already the way it is: nothing is rebuilt.
    set_keep_on_top(&h, &listed[0].label, true).unwrap();
    assert_eq!(list(&h, "main")[0].label, listed[0].label);
    // Opened owned from the start; only a pop-out may ask; a gone one cannot.
    let mut owned = request("pn.log");
    owned.keep_on_top = Some(true);
    assert!(open(&h, "main", owned).unwrap().keep_on_top);
    assert_eq!(
        set_keep_on_top(&h, "main", true).unwrap_err().code,
        PanelErrorCode::NotAllowed
    );
    assert_eq!(
        set_keep_on_top(&h, "p-main-99", true).unwrap_err().code,
        PanelErrorCode::Closed
    );
    let ghost_record = state(&app).popouts().reserve_replacing(PopOut {
        host: "main".into(),
        panel: Panel::parse("pn.zombie").unwrap(),
        title: "z".into(),
        keep_on_top: false,
        last: None,
    });
    assert_eq!(
        set_keep_on_top(&h, &ghost_record, true).unwrap_err().code,
        PanelErrorCode::Closed,
        "record without a window"
    );
    // The host window is gone while its pop-out lives on.
    let orphan = state(&app).popouts().reserve_replacing(PopOut {
        host: "w-5".into(),
        panel: Panel::parse("pn.orphan").unwrap(),
        title: "o".into(),
        keep_on_top: false,
        last: None,
    });
    window(&app, &orphan);
    assert_eq!(
        set_keep_on_top(&h, &orphan, true).unwrap_err().code,
        PanelErrorCode::Closed
    );
}

#[test]
fn a_pop_out_remembers_where_it_was_for_the_next_one_of_the_view() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("places.json");
    let app = app_with(Some(
        Panels::builder()
            .route("popout.html")
            .bounds_file(&file)
            .build(),
    ));
    let h = app.handle().clone();
    let info = open(&h, "main", request("pn.code#a")).unwrap();
    let place = crate::place::remember(300.0, 200.0, 640.0, 480.0, 1.0);
    state(&app).popouts().set_last(&info.label, place);
    on_window_event(&os_window(&app, &info.label), &WindowEvent::Destroyed);
    assert_eq!(
        state(&app).memory.get("pn.code"),
        place,
        "per view, not per instance"
    );
    // It survives a restart of the layer.
    assert_eq!(Memory::in_file(&file).get("pn.code"), place);
    assert_eq!(state(&app).route(), "popout.html");
    // A host closing remembers too.
    window(&app, "w-2");
    let other = open(&h, "w-2", request("pn.log")).unwrap();
    state(&app).popouts().set_last(&other.label, place);
    close_all_of(&h, "w-2");
    assert_eq!(state(&app).memory.get("pn.log"), place);
    // The mock window reports a place of its own.
    let again = open(
        &h,
        "main",
        PopOutRequest {
            bounds: None,
            ..request("pn.code")
        },
    )
    .unwrap();
    assert!(again.bounds.is_some());
}

#[test]
fn displays_are_read_as_work_areas_with_their_scales() {
    let monitor = |x: i32, scale: f64| -> ::tauri::Monitor {
        let size = ::tauri::PhysicalSize::new(1920, 1080);
        let position = ::tauri::PhysicalPosition::new(x, 0);
        tauri_runtime::monitor::Monitor {
            name: None,
            size,
            position,
            work_area: ::tauri::PhysicalRect {
                position,
                size: ::tauri::PhysicalSize::new(1920, 1040),
            },
            scale_factor: scale,
        }
        .into()
    };
    let m = monitors_from(&[monitor(-1920, 1.5), monitor(0, 1.0)]);
    assert_eq!(
        (m[0].work.x, m[0].work.height, m[0].scale),
        (-1920.0, 1040.0, 1.5)
    );
    assert_eq!(m.len(), 2);
    assert!(monitors_from(&[]).is_empty());
}

// ---- the relay ----------------------------------------------------------------------------------------------------

fn sync(scope: SyncScope, message: Value) -> SyncPublish {
    SyncPublish { scope, message }
}

#[test]
fn ui_state_reaches_only_the_windows_of_the_senders_host() {
    let app = app();
    let h = app.handle().clone();
    window(&app, "w-2");
    open(&h, "main", request("pn.code")).unwrap();
    open(&h, "main", request("pn.log")).unwrap();
    open(&h, "w-2", request("pn.code")).unwrap();
    let dialog = window(&app, "d-main-5");
    let other_dialog = window(&app, "d-w-2-6");
    let heard: Vec<_> = ["main", "p-main-1", "p-main-2", "w-2", "p-w-2-3"]
        .iter()
        .map(|l| (*l, recorder(&window(&app, l), UI_SYNC_EVENT)))
        .collect();
    let heard_dialog = recorder(&dialog, UI_SYNC_EVENT);
    let heard_other_dialog = recorder(&other_dialog, UI_SYNC_EVENT);
    let count = |l: &str| {
        heard
            .iter()
            .find(|(n, _)| *n == l)
            .unwrap()
            .1
            .lock()
            .unwrap()
            .len()
    };

    let msg = json!({ "t": "state", "origin": "o1", "seq": 1, "host": false, "key": "query", "value": "x" });
    assert_eq!(
        publish(&h, "p-main-1", sync(SyncScope::Full, msg.clone())).unwrap(),
        2
    );
    assert_eq!(
        (count("main"), count("p-main-2"), count("p-main-1")),
        (1, 1, 0),
        "the others, not the sender"
    );
    assert_eq!(
        (count("w-2"), count("p-w-2-3")),
        (0, 0),
        "a pop-out of another host never receives the state"
    );
    assert!(
        heard_dialog.lock().unwrap().is_empty(),
        "a dialog gets display state only"
    );
    assert_eq!(heard[0].1.lock().unwrap()[0], msg);

    // Display scope also reaches the dialogs of that host, still not another host's.
    assert_eq!(
        publish(
            &h,
            "main",
            sync(SyncScope::Display, json!({ "t": "state" }))
        )
        .unwrap(),
        3
    );
    assert_eq!(heard_dialog.lock().unwrap().len(), 1);
    assert!(heard_other_dialog.lock().unwrap().is_empty());
    assert_eq!(count("w-2") + count("p-w-2-3"), 0);

    // A dialog's message reaches its host and the host's pop-outs.
    assert_eq!(
        publish(
            &h,
            "d-main-5",
            sync(SyncScope::Display, json!({ "t": "hello" }))
        )
        .unwrap(),
        3
    );
}

#[test]
fn oversized_malformed_and_too_frequent_messages_are_refused() {
    let app = app();
    let h = app.handle().clone();
    open(&h, "main", request("pn.code")).unwrap();
    let heard = recorder(&window(&app, "main"), UI_SYNC_EVENT);
    let big = json!({ "t": "state", "value": "x".repeat(MAX_MESSAGE_BYTES + 1) });
    let e = publish(&h, "p-main-1", sync(SyncScope::Full, big)).unwrap_err();
    assert_eq!(e.code, PanelErrorCode::TooLarge);
    let e = publish(&h, "p-main-1", sync(SyncScope::Full, json!([1]))).unwrap_err();
    assert_eq!(e.code, PanelErrorCode::Invalid);
    assert!(heard.lock().unwrap().is_empty(), "nothing was delivered");
    // A flood is cut off; another window is unaffected.
    let codes: Vec<_> = (0..BURST + 100)
        .map(|_| {
            publish(
                &h,
                "p-main-1",
                sync(SyncScope::Full, json!({ "t": "state" })),
            )
        })
        .collect();
    assert!(
        codes
            .iter()
            .any(|r| matches!(r, Err(e) if e.code == PanelErrorCode::RateLimited))
    );
    assert!(codes.iter().take(BURST as usize).all(Result::is_ok));
    assert!(publish(&h, "main", sync(SyncScope::Full, json!({ "t": "state" }))).is_ok());
    // A sender outside the scheme reaches nobody.
    assert_eq!(
        publish(&h, "elsewhere", sync(SyncScope::Full, json!({}))).unwrap(),
        0
    );
    assert_eq!(
        publish(
            app_with(None).handle(),
            "main",
            sync(SyncScope::Full, json!({}))
        )
        .unwrap_err()
        .code,
        PanelErrorCode::NotReady
    );
}

// ---- the guard on a real invoke path ------------------------------------------------------------------------------

#[test]
fn commands_are_guarded_by_window_class_and_size() {
    let app = app();
    let h = app.handle().clone();
    let main = window(&app, "main");
    let popped = invoke(
        &main,
        "panel_pop_out",
        json!({ "request": { "panel": "pn.code", "title": "T" } }),
    )
    .unwrap();
    assert_eq!(
        (popped["label"].as_str(), popped["panel"].as_str()),
        (Some("p-main-1"), Some("pn.code"))
    );
    let guest = window(&app, "p-main-1");
    // A pop-out cannot pop out further, drag-ghost, or open dialogs; the host-only commands are refused by class.
    for (cmd, body) in [
        ("panel_pop_out", json!({ "request": { "panel": "pn.log" } })),
        ("drag_ghost_end", json!({})),
        ("drag_ghost_show", json!({ "visible": true })),
    ] {
        assert!(invoke(&guest, cmd, body).is_err(), "{cmd}");
    }
    assert_eq!(
        invoke(&guest, "panels_list", json!({})).unwrap()[0]["panel"],
        "pn.code"
    );
    assert_eq!(
        invoke(&main, "panel_focus", json!({ "panel": "pn.code" })).unwrap(),
        json!(true)
    );
    assert!(invoke(&guest, "panel_keep_on_top", json!({ "on": false })).is_ok());
    assert!(
        invoke(&main, "panel_keep_on_top", json!({ "on": true })).is_err(),
        "host windows cannot"
    );
    // The size limit applies before the command runs.
    let huge = json!({ "sync": { "scope": "full", "message": { "v": "x".repeat(700 * 1024) } } });
    assert!(invoke(&guest, "ui_sync_publish", huge).is_err());
    let heard = recorder(&main, UI_SYNC_EVENT);
    assert_eq!(
        invoke(
            &guest,
            "ui_sync_publish",
            json!({ "sync": { "scope": "full", "message": { "t": "bye" } } })
        )
        .unwrap(),
        json!(1)
    );
    assert_eq!(heard.lock().unwrap().len(), 1);
    assert!(invoke(&guest, "panel_dock_back", json!({})).is_ok());
    // The test app's pointer cannot tell (the command answers from the layer's pointer, never the user's mouse).
    assert_eq!(
        invoke(&main, "pointer_button_down", json!({})).unwrap(),
        Value::Null
    );
    // The drag ghost window can call nothing and no label of the scheme is its own.
    for spec in crate::COMMANDS.specs() {
        assert!(
            paddock_commands::check(
                &crate::COMMANDS,
                spec.name(),
                crate::ghost::GHOST_LABEL,
                None,
                10
            )
            .is_err(),
            "{}",
            spec.name()
        );
    }
    drop(h);
}

// ---- the drag ghost -----------------------------------------------------------------------------------------------

const SIZE: GhostSize = GhostSize {
    width: 400.0,
    height: 300.0,
    grab_x: 100.0,
    grab_y: 10.0,
};

fn ghost_request() -> GhostRequest {
    GhostRequest {
        title: "Inspector".into(),
        icon: None,
        snapshot: None,
        dark: None,
        width: 400.0,
        height: 300.0,
        grab_x: 100.0,
        grab_y: 10.0,
    }
}

fn screens() -> Vec<Screen> {
    let r = |w, h| PhysRect {
        x: 0,
        y: 0,
        width: w,
        height: h,
    };
    vec![Screen {
        bounds: r(1920, 1080),
        work: r(1920, 1040),
        scale: 1.0,
    }]
}

fn live(app: &::tauri::App<MockRuntime>) -> bool {
    state(app).ghost().active()
}

#[test]
fn commands_drive_one_windows_session() {
    let app = app();
    let h = app.handle().clone();
    let ghost = || ghost_request().parse().unwrap();
    // Without displays (a platform that cannot tell) there is no ghost.
    assert!(!ghost::start(&h, "main", ghost(), Vec::new()).unwrap());
    assert!(h.get_webview_window(crate::ghost::GHOST_LABEL).is_none());
    assert_eq!(end_for(&h, "main"), None);
    assert!(ghost::start(&h, "main", ghost(), screens()).unwrap());
    assert!(
        h.get_webview_window(crate::ghost::GHOST_LABEL).is_some(),
        "created hidden"
    );
    // The mock cursor is at 0,0: the ghost sits in the corner of the work area.
    ghost::show_for(&h, "main", true);
    ghost::show_for(&h, "main", false);
    assert_eq!(end_for(&h, "main"), None, "back inside: no position");
    assert!(
        ghost::start(&h, "main", ghost(), screens()).unwrap(),
        "reused"
    );
    ghost::show_for(&h, "main", true);
    let at = end_for(&h, "main").unwrap();
    assert_eq!(
        (at.x, at.y, at.width, at.height, at.scale),
        (0.0, 0.0, 400.0, 300.0, Some(1.0))
    );
    // Another window's calls do nothing to this window's drag.
    assert!(ghost::start(&h, "main", ghost(), screens()).unwrap());
    ghost::show_for(&h, "w-2", true);
    assert_eq!(end_for(&h, "w-2"), None);
    assert!(live(&app));
    // The dragging window goes: its drag ends; with another window left the ghost stays for later.
    window(&app, "w-2");
    on_window_event(&os_window(&app, "main"), &WindowEvent::Destroyed);
    assert!(!live(&app));
    assert!(h.get_webview_window(crate::ghost::GHOST_LABEL).is_some());
    // The last application window goes: the hidden ghost goes too, so it never keeps the app running.
    on_window_event(&os_window(&app, "w-2"), &WindowEvent::Destroyed);
    os_window(&app, "w-2").destroy().unwrap();
    on_window_event(&os_window(&app, "w-2"), &WindowEvent::Destroyed);
}

#[test]
fn a_malformed_request_is_refused_before_anything_happens() {
    let app = app();
    let h = app.handle().clone();
    let bad = GhostRequest {
        snapshot: Some("javascript:alert(1)".into()),
        ..ghost_request()
    };
    assert_eq!(
        ghost::begin(&h, "main", bad).unwrap_err().code,
        PanelErrorCode::Invalid
    );
    assert!(h.get_webview_window(crate::ghost::GHOST_LABEL).is_none());
    // Valid: starts (or answers false where the platform cannot place windows); end it either way.
    let started = ghost::begin(&h, "main", ghost_request()).unwrap();
    assert_eq!(
        started,
        positioning_supported_here() && !ghost::screens(&h, "main").is_empty()
    );
    end_for(&h, "main");
    assert_eq!(
        ghost::begin(app_with(None).handle(), "main", ghost_request())
            .unwrap_err()
            .code,
        PanelErrorCode::NotReady
    );
}

/// Regression: the tests read the user's real mouse button (`NSEvent::pressedMouseButtons`, `GetAsyncKeyState`). A blur
/// kept the session while the user happened to hold the button (`the_session_ends_with_the_drag_so_the_ghost_never_stays`
/// failed at "the window lost focus"), and `pointer_button_down` answered with the user's button. The test app's
/// pointer answers instead, and the OS is never asked.
#[test]
fn the_test_app_never_reads_the_users_mouse() {
    let app = app();
    let h = app.handle().clone();
    assert!(ghost::start(&h, "main", ghost_request().parse().unwrap(), screens()).unwrap());
    ghost::show_for(&h, "main", true);
    on_window_event(&os_window(&app, "main"), &WindowEvent::Focused(false));
    let _ = invoke(&window(&app, "main"), "pointer_button_down", json!({}));
    end_for(&h, "main");
    assert_eq!(
        super::native::OS_READS.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a test read the user's mouse button"
    );
}

/// A blur while the button is held is the drag leaving the window (the session goes on); with the button up it ends the
/// drag. The command answers the same source.
#[test]
fn a_blur_ends_the_session_only_with_the_button_up() {
    let (app, button) = app_with_button();
    let h = app.handle().clone();
    let main = window(&app, "main");
    for down in [Some(true), Some(false)] {
        button.set(down);
        assert_eq!(
            invoke(&main, "pointer_button_down", json!({})).unwrap(),
            json!(down)
        );
        assert!(ghost::start(&h, "main", ghost_request().parse().unwrap(), screens()).unwrap());
        on_window_event(&os_window(&app, "main"), &WindowEvent::Focused(false));
        assert_eq!(live(&app), down == Some(true), "button down: {down:?}");
        end_for(&h, "main");
    }
}

#[test]
fn the_session_ends_with_the_drag_so_the_ghost_never_stays() {
    let app = app();
    let h = app.handle().clone();
    window(&app, "w-2");
    let start =
        || assert!(ghost::start(&h, "main", ghost_request().parse().unwrap(), screens()).unwrap());
    // Ends with the drag.
    start();
    ghost::show_for(&h, "main", true);
    assert!(live(&app));
    end_for(&h, "main");
    assert!(!live(&app), "the drag ended");
    // Ends when the dragging window loses focus (button up).
    start();
    ghost::show_for(&h, "main", true);
    on_window_event(&os_window(&app, "main"), &WindowEvent::Focused(false));
    assert!(!live(&app), "the window lost focus");
    // Another window's blur does not end it; a window that gains focus does not either.
    start();
    on_window_event(&os_window(&app, "main"), &WindowEvent::Focused(true));
    assert!(live(&app));
    end_for(&h, "main");
    // The ghost's own window going away resets the page state.
    state(&app).ghost().set_loaded(true);
    on_window_event(
        &os_window(&app, crate::ghost::GHOST_LABEL),
        &WindowEvent::Destroyed,
    );
    assert!(!state(&app).ghost().loaded());
}

#[test]
fn the_tracker_follows_the_cursor_and_gives_up_when_the_drag_is_over() {
    let app = app();
    let h = app.handle().clone();
    let state_of = || state(&app);
    let id = {
        assert!(ghost::start(&h, "main", ghost_request().parse().unwrap(), screens()).unwrap());
        state_of()
            .ghost()
            .begin(
                "main",
                GhostContent {
                    title: "T".into(),
                    icon: None,
                    snapshot: None,
                    dark: false,
                },
                SIZE,
                screens(),
            )
            .0
    };
    // Hidden: nothing to move; keeps following.
    assert!(ghost::step(&h, id, 0, Duration::ZERO));
    ghost::show_for(&h, "main", true);
    assert!(
        ghost::step(&h, id, 0, Duration::ZERO),
        "shown, cursor unchanged"
    );
    // The button was up for too long without an end: hide and stop.
    assert!(!ghost::step(
        &h,
        id,
        crate::ghost::RELEASED_TICKS,
        Duration::ZERO
    ));
    // The end still answers where the ghost was after a lost release.
    assert!(end_for(&h, "main").is_some());
    // The session is gone: the tracker stops. So it does for a drag that never ends, and without the layer.
    assert!(!ghost::step(&h, id, 0, Duration::ZERO));
    let id = state_of()
        .ghost()
        .begin(
            "main",
            GhostContent {
                title: "T".into(),
                icon: None,
                snapshot: None,
                dark: false,
            },
            SIZE,
            screens(),
        )
        .0;
    assert!(!ghost::step(
        &h,
        id,
        0,
        crate::ghost::MAX_DRAG + Duration::from_secs(1)
    ));
    assert!(!live(&app));
    assert!(!ghost::step(app_with(None).handle(), id, 0, Duration::ZERO));
    // A cursor on the mock display moves the shown ghost.
    let id = state_of()
        .ghost()
        .begin(
            "main",
            GhostContent {
                title: "T".into(),
                icon: None,
                snapshot: None,
                dark: false,
            },
            SIZE,
            screens(),
        )
        .0;
    state_of()
        .ghost()
        .set_visible("main", true, Some((500.0, 500.0)));
    assert!(ghost::step(&h, id, 0, Duration::ZERO));
    end_for(&h, "main");
}

#[test]
fn drag_commands_work_through_the_invoke_path() {
    let app = app();
    let main = window(&app, "main");
    let h = app.handle().clone();
    let started = invoke(
        &main,
        "drag_ghost_start",
        json!({ "request": ghost_request_json() }),
    )
    .unwrap();
    assert_eq!(started, json!(false), "the mock reports no displays");
    assert_eq!(
        invoke(&main, "drag_ghost_show", json!({ "visible": true })).unwrap(),
        Value::Null
    );
    assert_eq!(
        invoke(&main, "drag_ghost_end", json!({})).unwrap(),
        Value::Null
    );
    assert!(!live(&app));
    drop(h);
}

fn ghost_request_json() -> Value {
    json!({ "title": "Inspector", "width": 400.0, "height": 300.0, "grabX": 100.0, "grabY": 10.0 })
}

#[test]
fn the_ghost_page_gets_its_content_whenever_it_loads() {
    let app = app();
    let h = app.handle().clone();
    // Nothing to hand over before a drag; the page loading then only marks it ready.
    ghost::push_content(&h);
    ghost::page_loaded(&h);
    assert!(state(&app).ghost().loaded());
    // With a drag started on a loaded page the content is handed over at once, and again after a reload.
    assert!(ghost::start(&h, "main", ghost_request().parse().unwrap(), screens()).unwrap());
    assert!(state(&app).ghost().content().is_some());
    ghost::page_loaded(&h);
    end_for(&h, "main");
    // Without the layer, or without the ghost window, nothing happens.
    ghost::page_loaded(app_with(None).handle());
    ghost::show(&h);
    ghost::hide(&h);
    os_window(&app, crate::ghost::GHOST_LABEL)
        .destroy()
        .unwrap();
    ghost::push_content(&h);
}

#[test]
fn a_ghost_without_its_window_or_layer_is_ignored() {
    let app = app();
    let h = app.handle().clone();
    // A session whose ghost window is gone: showing it places nothing and breaks nothing.
    state(&app).ghost().begin(
        "main",
        GhostContent {
            title: "T".into(),
            icon: None,
            snapshot: None,
            dark: false,
        },
        SIZE,
        screens(),
    );
    ghost::show_for(&h, "main", true);
    end_for(&h, "main");
    ghost::show_for(app_with(None).handle(), "main", true);
    assert_eq!(end_for(app_with(None).handle(), "main"), None);
}

#[test]
fn displays_are_read_with_their_bounds_work_areas_and_scales() {
    let size = ::tauri::PhysicalSize::new(3840, 2160);
    let position = ::tauri::PhysicalPosition::new(1920, 0);
    let monitor: ::tauri::Monitor = tauri_runtime::monitor::Monitor {
        name: Some("4K".into()),
        size,
        position,
        work_area: ::tauri::PhysicalRect {
            position,
            size: ::tauri::PhysicalSize::new(3840, 2080),
        },
        scale_factor: 2.0,
    }
    .into();
    let s = ghost::screens_from(&[monitor]);
    assert_eq!(
        (
            s[0].bounds.x,
            s[0].bounds.width,
            s[0].work.height,
            s[0].scale
        ),
        (1920, 3840, 2080, 2.0)
    );
    assert!(ghost::screens_from(&[]).is_empty());
    assert!(ghost::screens(app().handle(), "nobody").is_empty());
}

#[test]
fn on_a_platform_that_places_windows_the_exact_physical_place_is_set() {
    let app = app();
    let h = app.handle().clone();
    for (os, panel) in [("windows", "pn.w"), ("linux", "pn.l"), ("macos", "pn.m")] {
        assert!(
            popout::open_on(&h, "main", request(panel), os).is_ok(),
            "{os}"
        );
    }
}

#[test]
fn a_pop_out_that_cannot_be_rebuilt_stays_as_it_was() {
    let app = app();
    let h = app.handle().clone();
    let first = open(&h, "main", request("pn.code")).unwrap();
    // The label the rebuilt window would get is taken by another webview.
    window(&app, "p-main-2");
    let e = set_keep_on_top(&h, &first.label, true).unwrap_err();
    assert_eq!(e.code, PanelErrorCode::Failed);
    let listed = list(&h, "main");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].label.as_str(), listed[0].keep_on_top),
        (first.label.as_str(), false)
    );
}

#[test]
fn windows_closing_without_the_layer_or_a_record_do_nothing() {
    let bare = app_with(None);
    on_window_event(&os_window(&bare, "main"), &WindowEvent::Destroyed);
    on_window_event(&os_window(&bare, "main"), &WindowEvent::Focused(false));
    let app = app();
    let h = app.handle().clone();
    window(&app, "p-main-9");
    on_window_event(&os_window(&app, "p-main-9"), &WindowEvent::Destroyed);
    on_window_event(
        &os_window(&app, "p-main-9"),
        &WindowEvent::Moved(Default::default()),
    );
    // The last application window going takes the hidden ghost with it.
    ghost::start(&h, "main", ghost_request().parse().unwrap(), screens()).unwrap();
    super::events::end_ghost_without_windows(&h, "main");
    // (The mock runtime keeps a destroyed window listed, so its going is not observable here.)
    end_for(&h, "main");
}

#[test]
fn where_windows_cannot_be_placed_there_is_no_ghost() {
    let app = app();
    let h = app.handle().clone();
    assert!(!ghost::begin_on(&h, "main", ghost_request().parse().unwrap(), false).unwrap());
    assert!(h.get_webview_window(crate::ghost::GHOST_LABEL).is_none());
    // A popped-out window going away without the layer is ignored.
    let bare = app_with(None);
    window(&bare, "p-main-1");
    on_window_event(&os_window(&bare, "p-main-1"), &WindowEvent::Destroyed);
}
