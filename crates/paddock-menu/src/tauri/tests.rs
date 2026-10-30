//! The adapter on Tauri's mock runtime (`tauri::test`): no window or menu bar ever reaches the desktop. The native menu
//! bar is a recording fake ([`NativeBar`]); the Tauri menus themselves are built (not shown) where that is possible off
//! the main thread (Windows, Linux).

use super::*;
use crate::model::QUIT_COMMAND;
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use paddock_commands::tauri::Guard;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;

paddock_commands::command_table! {
    static TABLE, macro handler;
    Quiet [MAIN | POPOUT] limit paddock_commands::MIB => crate::tauri::menu_set,
}

/// What the fake menu bar was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Build(Vec<String>),
    States(Vec<String>),
}

#[derive(Clone, Default)]
struct FakeBar {
    calls: Arc<Mutex<Vec<Call>>>,
    /// `set_states` answers this (false: an item was not found, so the menu is built anew).
    states_found: Arc<Mutex<bool>>,
    fail_build: Arc<Mutex<bool>>,
}

impl FakeBar {
    fn new() -> Self {
        let bar = FakeBar::default();
        *lock(&bar.states_found) = true;
        bar
    }
    fn calls(&self) -> Vec<Call> {
        lock(&self.calls).clone()
    }
}

impl NativeBar<MockRuntime> for FakeBar {
    fn build(&self, _: &AppHandle<MockRuntime>, spec: &[NativeNode]) -> ::tauri::Result<()> {
        if *lock(&self.fail_build) {
            return Err(::tauri::Error::FailedToReceiveMessage);
        }
        lock(&self.calls).push(Call::Build(
            item_ids(spec).into_iter().map(str::to_string).collect(),
        ));
        Ok(())
    }
    fn set_states(&self, _: &AppHandle<MockRuntime>, changed: &[Item]) -> ::tauri::Result<bool> {
        lock(&self.calls).push(Call::States(changed.iter().map(|i| i.id.clone()).collect()));
        Ok(*lock(&self.states_found))
    }
}

fn app_with(menu: Option<NativeMenu<MockRuntime>>) -> ::tauri::App<MockRuntime> {
    let app = mock_builder()
        .invoke_handler(Guard::new(&TABLE).wrap(handler!()))
        .build(mock_context(noop_assets()))
        .expect("a mock app");
    if let Some(m) = menu {
        app.manage(m);
    }
    window(&app, "main");
    app
}

fn native_app(bar: &FakeBar) -> ::tauri::App<MockRuntime> {
    let app = app_with(Some(NativeMenu::with_bar("Demo", true, bar.clone())));
    install(app.handle());
    app
}

fn window(app: &::tauri::App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    app.get_webview_window(label).unwrap_or_else(|| {
        WebviewWindowBuilder::new(app, label, WebviewUrl::default())
            .build()
            .expect("a mock window")
    })
}

fn invoke(w: &WebviewWindow<MockRuntime>, body: Value) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: "menu_set".into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .expect("a url"),
        body: InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(w, request).map(|b| b.deserialize::<Value>().expect("json"))
}

fn item(id: &str, label: &str, checked: Option<bool>, enabled: bool) -> Value {
    json!({ "kind": "item", "id": id, "label": label, "keybinding": null, "checked": checked, "enabled": enabled })
}

fn wire(view_checked: bool, find_enabled: bool, find_label: &str) -> Value {
    json!({ "menu": { "menus": [
        { "id": "app", "label": "Demo", "items": [item("sd.about", "About Demo", None, true), item(QUIT_COMMAND, "Quit Demo", None, true)] },
        { "id": "edit", "label": "Edit", "items": [item("sd.find", find_label, None, find_enabled)] },
        { "id": "view", "label": "View", "items": [item("sd.sidebar", "Sidebar", Some(view_checked), true)] }
    ] } })
}

fn state(app: &::tauri::App<MockRuntime>) -> ::tauri::State<'_, NativeMenu<MockRuntime>> {
    app.state::<NativeMenu<MockRuntime>>()
}

#[test]
fn menu_set_is_async() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    paddock_commands::testing::assert_async(&TABLE, &[&src]);
}

/// A start builds the menu bar once with the standard menus and once with the UI's (the UI sends it once the app is
/// ready, coalesced: `NativeMenuBridge`); the same model again builds nothing, and a changed check mark or enabled state
/// is updated in place.
#[test]
fn a_start_builds_the_menu_bar_once_per_real_change() {
    let bar = FakeBar::new();
    let app = native_app(&bar);
    let main = window(&app, "main");
    assert_eq!(state(&app).builds(), 1, "install shows the standard menus");
    assert_eq!(
        invoke(&main, wire(true, true, "Find")),
        Ok(json!("rebuilt"))
    );
    assert_eq!(state(&app).builds(), 2);
    for _ in 0..3 {
        assert_eq!(
            invoke(&main, wire(true, true, "Find")),
            Ok(json!("unchanged"))
        );
    }
    assert_eq!(
        state(&app).builds(),
        2,
        "the same model again builds nothing"
    );
    assert_eq!(
        invoke(&main, wire(false, false, "Find")),
        Ok(json!("states"))
    );
    assert_eq!(
        state(&app).builds(),
        2,
        "a check mark and an enabled state are updated in place"
    );
    assert_eq!(
        invoke(&main, wire(false, false, "Find in models")),
        Ok(json!("rebuilt"))
    );
    assert_eq!(state(&app).builds(), 3);
    assert_eq!(
        bar.calls(),
        [
            Call::Build(vec![]),
            Call::Build(vec![
                "sd.about".into(),
                QUIT_COMMAND.into(),
                "sd.find".into(),
                "sd.sidebar".into(),
            ]),
            Call::States(vec!["sd.find".into(), "sd.sidebar".into()]),
            Call::Build(vec![
                "sd.about".into(),
                QUIT_COMMAND.into(),
                "sd.find".into(),
                "sd.sidebar".into(),
            ]),
        ]
    );
    assert_eq!(
        state(&app).shown_ids(),
        ["sd.about", QUIT_COMMAND, "sd.find", "sd.sidebar"]
    );
}

#[test]
fn a_state_update_that_cannot_find_its_item_rebuilds() {
    let bar = FakeBar::new();
    let app = native_app(&bar);
    let main = window(&app, "main");
    invoke(&main, wire(true, true, "Find")).expect("shown");
    *lock(&bar.states_found) = false;
    assert_eq!(
        invoke(&main, wire(false, true, "Find")),
        Ok(json!("states"))
    );
    assert_eq!(
        state(&app).builds(),
        3,
        "the item was not found: built anew"
    );
}

#[test]
fn an_invalid_model_is_refused_before_anything_is_shown() {
    let bar = FakeBar::new();
    let app = native_app(&bar);
    let dup = json!({ "menu": { "menus": [{ "id": "f", "label": "F", "items": [item("a", "x", None, true), item("a", "y", None, true)] }] } });
    let err = invoke(&window(&app, "main"), dup).expect_err("refused");
    assert_eq!(err["code"], "invalid");
    assert!(
        err["message"]
            .as_str()
            .is_some_and(|m| m.contains("used twice")),
        "{err}"
    );
    assert_eq!(state(&app).builds(), 1);
    let unknown = json!({ "menu": { "menus": [], "script": "x" } });
    assert!(
        invoke(&window(&app, "main"), unknown).is_err(),
        "the contract is exact"
    );
}

#[test]
fn without_a_native_menu_bar_the_model_is_validated_and_not_shown() {
    let bar = FakeBar::new();
    let app = app_with(Some(NativeMenu::with_bar("Demo", false, bar.clone())));
    install(app.handle());
    assert!(!state(&app).is_native());
    let main = window(&app, "main");
    assert_eq!(invoke(&main, wire(true, true, "Find")), Ok(json!("inApp")));
    let bad = json!({ "menu": { "menus": [{ "id": "", "label": "F", "items": [] }] } });
    assert_eq!(invoke(&main, bad).expect_err("refused")["code"], "invalid");
    assert_eq!(state(&app).builds(), 0);
    assert!(bar.calls().is_empty());
}

#[test]
fn the_menu_bar_shows_the_focused_windows_menus() {
    let bar = FakeBar::new();
    let app = native_app(&bar);
    let main = window(&app, "main");
    let other = window(&app, "w-2");
    on_window_event(&other.as_ref().window(), &WindowEvent::Focused(true));
    assert_eq!(state(&app).target().as_deref(), Some("w-2"));
    assert_eq!(
        invoke(&main, wire(true, true, "Find")),
        Ok(json!("notFocused"))
    );
    assert_eq!(
        invoke(&other, wire(true, true, "Find")),
        Ok(json!("rebuilt"))
    );
    // A pop-out speaks for its host.
    let popout = window(&app, "p-w-2-1");
    assert_eq!(
        invoke(&popout, wire(false, true, "Find")),
        Ok(json!("states"))
    );
    // Losing the focus changes nothing; the target is gone once its window is.
    on_window_event(&other.as_ref().window(), &WindowEvent::Focused(false));
    assert_eq!(state(&app).target().as_deref(), Some("w-2"));
    on_window_event(&other.as_ref().window(), &WindowEvent::Destroyed);
    assert_eq!(state(&app).target(), None);
    assert_eq!(
        invoke(&main, wire(true, true, "Find")),
        Ok(json!("states")),
        "no target: the caller's menus are shown"
    );
}

#[test]
fn clicks_go_to_the_target_window_only() {
    let bar = FakeBar::new();
    let app = native_app(&bar);
    let main = window(&app, "main");
    let other = window(&app, "w-2");
    let seen = |w: &WebviewWindow<MockRuntime>| {
        let got = Arc::new(Mutex::new(Vec::<String>::new()));
        let g = got.clone();
        w.listen(MENU_EVENT, move |e| lock(&g).push(e.payload().to_string()));
        got
    };
    let (on_main, on_other) = (seen(&main), seen(&other));
    invoke(&main, wire(true, true, "Find")).expect("shown");
    assert_eq!(
        menu_chosen(app.handle(), "sd.find"),
        None,
        "no window had the focus yet"
    );
    on_window_event(&main.as_ref().window(), &WindowEvent::Focused(true));
    assert_eq!(
        menu_chosen(app.handle(), "sd.find").as_deref(),
        Some("main")
    );
    assert_eq!(
        menu_chosen(app.handle(), "tray.quit"),
        None,
        "not one of the shown items"
    );
    on_window_event(
        &window(&app, "d-w-2-1").as_ref().window(),
        &WindowEvent::Focused(true),
    );
    assert_eq!(
        menu_chosen(app.handle(), "sd.sidebar").as_deref(),
        Some("w-2"),
        "a dialog's host runs it"
    );
    assert_eq!(*lock(&on_main), ["\"sd.find\""]);
    assert_eq!(*lock(&on_other), ["\"sd.sidebar\""]);
}

#[test]
fn without_install_the_command_says_so_and_clicks_and_events_are_ignored() {
    let app = app_with(None);
    let main = window(&app, "main");
    let err = invoke(&main, wire(true, true, "Find")).expect_err("not set up");
    assert_eq!(err["code"], "native");
    assert!(
        err["message"]
            .as_str()
            .is_some_and(|m| m.contains("install"))
    );
    assert_eq!(menu_chosen(app.handle(), "sd.find"), None);
    on_window_event(&main.as_ref().window(), &WindowEvent::Focused(true));
}

#[test]
fn a_failed_build_is_reported_as_a_native_error_and_retried_next_time() {
    let bar = FakeBar::new();
    *lock(&bar.fail_build) = true;
    let app = native_app(&bar);
    assert_eq!(state(&app).builds(), 0, "install logs the failure");
    let main = window(&app, "main");
    let err = invoke(&main, wire(true, true, "Find")).expect_err("failed");
    assert_eq!(err["code"], "native");
    assert!(
        err["message"]
            .as_str()
            .is_some_and(|m| m.starts_with("the native menu bar could not be updated"))
    );
    *lock(&bar.fail_build) = false;
    assert_eq!(
        invoke(&main, wire(true, true, "Find")),
        Ok(json!("rebuilt"))
    );
}

#[test]
fn install_manages_the_platforms_menu_state_when_the_app_has_none() {
    let app = app_with(None);
    // No native menu bar is built here: on macOS that needs the main thread and would show the app's menus.
    app.manage(NativeMenu::<MockRuntime>::with_bar(
        "Demo",
        false,
        FakeBar::new(),
    ));
    install(app.handle());
    let fresh = NativeMenu::<MockRuntime>::new("Demo");
    assert_eq!(fresh.is_native(), cfg!(target_os = "macos"));
    assert_eq!(fresh.builds(), 0);
    assert!(fresh.shown_ids().is_empty());
}

#[test]
fn the_tauri_bar_without_a_menu_asks_for_a_rebuild() {
    let app = app_with(None);
    let found =
        NativeBar::<MockRuntime>::set_states(&TauriBar, app.handle(), &[]).expect("no error");
    assert!(!found);
}

/// The Tauri menus of a spec, built and updated in place (not shown). macOS creates native menus on the main thread
/// only, which a test is not on; there the real app covers this, and the Windows and Linux runners run it.
#[cfg(not(target_os = "macos"))]
#[test]
fn the_tauri_menu_of_a_spec_has_every_item_and_updates_states_in_place() {
    use crate::native::native_spec;
    let app = app_with(None);
    let model: NativeMenuModel =
        serde_json::from_value(wire(true, true, "Find")["menu"].clone()).expect("wire");
    let mut model = parse(model).expect("valid");
    model.menus[2]
        .items
        .push(crate::model::Entry::Header("Engines".into()));
    model.menus[2].items.push(crate::model::Entry::Submenu(
        "More".into(),
        vec![crate::model::Entry::Separator],
    ));
    let spec = native_spec(&model, "Demo");
    let menu = build_menu(app.handle(), &spec).expect("built");
    assert_eq!(
        menu.items().expect("items").len(),
        4,
        "Demo, Edit, View, Window"
    );
    let sidebar = Item {
        id: "sd.sidebar".into(),
        label: "Sidebar".into(),
        accelerator: None,
        checked: Some(false),
        enabled: false,
    };
    let find = Item {
        id: "sd.find".into(),
        label: "Find".into(),
        accelerator: None,
        checked: None,
        enabled: false,
    };
    assert!(set_states_in(&menu, &[sidebar.clone(), find.clone()]).expect("updated"));
    match find_item(&menu, "sd.sidebar") {
        Some(MenuItemKind::Check(c)) => {
            assert!(!c.is_checked().expect("checked"));
            assert!(!c.is_enabled().expect("enabled"));
        }
        _ => panic!("a check item"),
    }
    assert!(
        !set_states_in(
            &menu,
            &[Item {
                id: "sd.gone".into(),
                ..find.clone()
            }]
        )
        .expect("no error")
    );
    assert!(
        !set_states_in(
            &menu,
            &[Item {
                checked: Some(true),
                ..find
            }]
        )
        .expect("no error"),
        "a plain item cannot take a check mark"
    );
}

/// The Tauri bar on an app without windows (each window would take the menu natively): built, then updated in place.
/// Not on macOS, for the reason above; `install` there would build the native menu bar.
#[cfg(not(target_os = "macos"))]
#[test]
fn the_tauri_bar_sets_the_app_menu_and_install_manages_the_platform_state() {
    use crate::native::native_spec;
    let app = mock_builder()
        .build(mock_context(noop_assets()))
        .expect("a mock app");
    install(app.handle());
    assert!(
        !state(&app).is_native(),
        "no native menu bar on this platform"
    );
    let model: NativeMenuModel =
        serde_json::from_value(wire(true, true, "Find")["menu"].clone()).expect("wire");
    let spec = native_spec(&parse(model).expect("valid"), "Demo");
    NativeBar::<MockRuntime>::build(&TauriBar, app.handle(), &spec).expect("set");
    assert!(app.menu().is_some());
    let sidebar = Item {
        id: "sd.sidebar".into(),
        label: "Sidebar".into(),
        accelerator: None,
        checked: Some(false),
        enabled: true,
    };
    assert!(
        NativeBar::<MockRuntime>::set_states(&TauriBar, app.handle(), &[sidebar]).expect("updated")
    );
}

#[cfg(not(target_os = "macos"))]
fn find_item(menu: &Menu<MockRuntime>, id: &str) -> Option<MenuItemKind<MockRuntime>> {
    find(menu.items().ok()?, id).ok()?
}

#[test]
fn quit_without_a_target_window_goes_to_the_no_window_handler_only() {
    let bar = FakeBar::new();
    let app = native_app(&bar);
    let main = window(&app, "main");
    invoke(&main, wire(true, true, "Find")).expect("shown");
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    // No handler yet: the click is dropped.
    assert_eq!(menu_chosen(app.handle(), QUIT_COMMAND), None);
    let seen = calls.clone();
    state(&app).on_no_window(move |_, id| lock(&seen).push(id.to_string()));
    assert_eq!(menu_chosen(app.handle(), QUIT_COMMAND), None);
    assert_eq!(*lock(&calls), [QUIT_COMMAND]);
    // Other items, and ids that are not shown, never reach it.
    assert_eq!(menu_chosen(app.handle(), "sd.find"), None);
    assert_eq!(menu_chosen(app.handle(), "tray.quit"), None);
    assert_eq!(lock(&calls).len(), 1);
    // A target window gets the click itself.
    on_window_event(&main.as_ref().window(), &WindowEvent::Focused(true));
    assert_eq!(
        menu_chosen(app.handle(), QUIT_COMMAND).as_deref(),
        Some("main")
    );
    assert_eq!(lock(&calls).len(), 1);
    // The last window destroyed: the handler again.
    on_window_event(&main.as_ref().window(), &WindowEvent::Destroyed);
    assert_eq!(menu_chosen(app.handle(), QUIT_COMMAND), None);
    assert_eq!(lock(&calls).len(), 2);
}
