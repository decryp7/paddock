//! The adapter on Tauri's mock runtime (`tauri::test`): no window, tray icon or single-instance lock reaches the
//! desktop, Quit and the process exit are hooks, and the tray is a fake backend. Ported from Debrief's
//! `window_manager.rs`, `instance.rs`, `tray.rs` and `lifecycle.rs` tests, plus the guard on a real invoke path.

use super::manager::{bounds_now, place_for};
use super::popover::{effects, work_rect};
use super::*;
use crate::bounds::SavedBounds;
use crate::close::{Background, OnClose};
use crate::launch::{LaunchArgs, OpenFilesIn, OpenRequest, parse_args};
use crate::popover::{Material, Rect as IconRect, Style};
use crate::tray::{TrayView, disabled, item};
use crate::wire::{CLOSE_REQUESTED_EVENT, OPEN_ASK_EVENT, OPEN_EVENT, WINDOWS_EVENT};
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{
    AppHandle, Listener, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};
use paddock_commands::tauri::Guard;
use paddock_dialogs::placement::{Monitor, Rect};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type App = ::tauri::App<MockRuntime>;
type Handle = AppHandle<MockRuntime>;
type Calls = Arc<Mutex<Vec<String>>>;

fn calls() -> Calls {
    Arc::default()
}

fn taken(c: &Calls) -> Vec<String> {
    std::mem::take(&mut *c.lock().unwrap())
}

/// A builder whose Quit and process exit are recorded in `quits`.
fn windows(config: WindowConfig, quits: &Calls) -> WindowsBuilder<MockRuntime> {
    let (q, e) = (quits.clone(), quits.clone());
    Windows::builder(config)
        .exit_app(move |_, code| q.lock().unwrap().push(format!("exit {code}")))
        .end_process(move |code| e.lock().unwrap().push(format!("end {code}")))
}

fn app_with(windows: Windows<MockRuntime>) -> App {
    let app = mock_builder()
        .invoke_handler(Guard::new(&crate::COMMANDS).wrap(invoke_handler()))
        .build(mock_context(noop_assets()))
        .unwrap();
    app.manage(windows);
    app
}

fn multi() -> WindowConfig {
    WindowConfig::new("Debrief")
        .multi_window(32)
        .schemes(&["Debrief"])
}

fn window(app: &App, label: &str) -> WebviewWindow<MockRuntime> {
    app.get_webview_window(label).unwrap_or_else(|| {
        WebviewWindowBuilder::new(app, label, WebviewUrl::default())
            .build()
            .unwrap()
    })
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

fn heard(w: &WebviewWindow<MockRuntime>, event: &'static str) -> Calls {
    let seen = calls();
    let s = seen.clone();
    w.listen(event, move |e| {
        s.lock().unwrap().push(e.payload().to_string())
    });
    seen
}

fn src_dir() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn paths(p: &[String]) -> OpenRequest {
    OpenRequest {
        paths: p.to_vec(),
        links: vec![],
    }
}

fn state(app: &App) -> ::tauri::State<'_, Windows<MockRuntime>> {
    app.state::<Windows<MockRuntime>>()
}

/// A fake tray backend: records what it draws, fails on request.
#[derive(Clone, Default)]
struct FakeTray {
    drawn: Calls,
    fail_create: bool,
    fail_update: bool,
}

impl TrayBackend<MockRuntime> for FakeTray {
    fn create(&self, _: &Handle, view: &TrayView) -> Result<(), String> {
        if self.fail_create {
            return Err("no tray here".into());
        }
        self.drawn
            .lock()
            .unwrap()
            .push(format!("create {}", view.tooltip));
        Ok(())
    }

    fn update(&self, _: &Handle, view: &TrayView) -> Result<(), String> {
        self.drawn
            .lock()
            .unwrap()
            .push(format!("update {}", view.tooltip));
        if self.fail_update {
            return Err("gone".into());
        }
        Ok(())
    }
}

/// A tray whose tooltip is `tip` (an empty tip makes the menu invalid).
fn tray(fake: &FakeTray, tip: &Arc<Mutex<String>>, actions: &Calls) -> Tray<MockRuntime> {
    let (tip, actions) = (tip.clone(), actions.clone());
    Tray::new(fake.clone(), move |_app: &Handle| {
        let t = tip.lock().unwrap().clone();
        let menu = if t.is_empty() {
            vec![item("dup", "a"), item("dup", "b")]
        } else {
            vec![
                item(crate::tray::SHOW_ID, "Show"),
                disabled("Idle"),
                item("app.stop", "Stop"),
            ]
        };
        TrayView::new(&t, menu, "idle")
    })
    .on_action(move |_, id| actions.lock().unwrap().push(id.to_string()))
}

fn with_tray(app: &App) -> FakeTray {
    let fake = FakeTray::default();
    assert!(create_tray(
        app.handle(),
        tray(&fake, &Arc::new(Mutex::new("App".into())), &calls())
    ));
    fake
}

#[test]
fn the_main_window_is_built_at_start_with_the_launch_files_waiting() {
    let app = app_with(windows(multi(), &calls()).build());
    let h = app.handle();
    let launch = parse_args(
        [
            src_dir(),
            "/does/not/exist".into(),
            "debrief://open?x=1".into(),
        ],
        None,
        &["debrief"],
    );
    start(h, &launch).unwrap();
    let main = app.get_webview_window("main").expect("main built");
    let pending = invoke(&main, "window_take_pending", json!({})).unwrap();
    assert_eq!(pending["paths"], json!([src_dir()]));
    assert_eq!(pending["links"][0]["host"], "open");
    assert_eq!(
        invoke(&main, "window_take_pending", json!({})).unwrap(),
        json!({ "paths": [], "links": [] }),
        "taken once"
    );
    // An existing main window is kept.
    assert_eq!(create_main(h).unwrap().label(), "main");
    assert_eq!(target(h).as_deref(), Some("main"));
}

#[test]
fn a_login_start_builds_no_window_only_when_a_tray_can_bring_it_back() {
    let hidden = LaunchArgs {
        hidden: true,
        ..LaunchArgs::default()
    };
    let app = app_with(windows(WindowConfig::new("Pitwall"), &calls()).build());
    with_tray(&app);
    start(app.handle(), &hidden).unwrap();
    assert!(app.get_webview_window("main").is_none());
    let app = app_with(windows(WindowConfig::new("Pitwall"), &calls()).build());
    start(app.handle(), &hidden).unwrap();
    assert!(
        app.get_webview_window("main").is_some(),
        "no tray: the window is the way in"
    );
}

#[test]
fn without_the_state_everything_says_so_and_nothing_breaks() {
    let app = mock_builder()
        .invoke_handler(Guard::new(&crate::COMMANDS).wrap(invoke_handler()))
        .build(mock_context(noop_assets()))
        .unwrap();
    let h = app.handle();
    let main = window(&app, "main");
    for (cmd, body) in [
        ("window_take_pending", json!({})),
        ("app_quit", json!({})),
        ("window_set_title", json!({ "title": "x" })),
        ("window_new", json!({})),
    ] {
        let e = invoke(&main, cmd, body).unwrap_err();
        assert_eq!(e["code"], "notReady", "{cmd}");
    }
    assert_eq!(
        start(h, &LaunchArgs::default()).unwrap_err().code,
        crate::WindowErrorCode::NotReady
    );
    assert!(target(h).is_none());
    assert_eq!(list(h).len(), 1);
    assert!(!exit_requested(h, None));
    exit(h);
    arm_exit_watchdog(h, 0);
    quit(h);
    destroyed(h, "main");
    second_instance(h, vec!["app".into()], String::new());
    open_from_outside(h, OpenRequest::default(), false);
    queue_open(h, "main", paths(&["/x".into()]));
    assert!(!close_requested(h, &main));
    assert!(!tray_available(h));
    refresh_tray(h);
    let icon = IconRect {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
    };
    assert_eq!(
        toggle_popover(h, None, icon).unwrap_err().code,
        crate::WindowErrorCode::NotReady
    );
    assert!(fit_popover(h, "tray", 100.0).is_err());
}

#[test]
fn windows_open_list_reveal_and_carry_their_titles() {
    let app = app_with(windows(multi(), &calls()).build());
    let h = app.handle();
    let main = create_main(h).unwrap();
    let changes = heard(&main, WINDOWS_EVENT);
    let label = open_new(h, paths(&["/x".into()])).unwrap();
    assert_eq!(label, "w-2");
    let info = invoke(&main, "window_new", json!({})).unwrap();
    assert_eq!(
        info,
        json!({ "label": "w-3", "title": "Debrief", "focused": false, "visible": true })
    );
    let labels: Vec<String> = invoke(&main, "windows_list", json!({}))
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["label"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(labels, ["main", "w-2", "w-3"]);
    assert!(
        taken(&changes).len() >= 2,
        "every window hears about new windows"
    );
    assert_eq!(
        target(h).as_deref(),
        Some("w-3"),
        "the newest window has the focus"
    );
    invoke(&main, "window_focus", json!({ "label": "w-2" })).unwrap();
    assert_eq!(target(h).as_deref(), Some("w-2"));
    let gone = invoke(&main, "window_focus", json!({ "label": "w-99" })).unwrap_err();
    assert_eq!(gone["code"], "closed");
    let dialog = invoke(&main, "window_focus", json!({ "label": "d-main-1" })).unwrap_err();
    assert_eq!(dialog["code"], "notAllowed");
    // Titles reach the window list; the same title twice changes nothing.
    let w2 = window(&app, "w-2");
    invoke(
        &w2,
        "window_set_title",
        json!({ "title": "  Incident\n — Debrief " }),
    )
    .unwrap();
    invoke(
        &w2,
        "window_set_title",
        json!({ "title": "Incident — Debrief" }),
    )
    .unwrap();
    assert!(
        list(h)
            .iter()
            .any(|w| w.label == "w-2" && w.title == "Incident — Debrief")
    );
    let w2_pending = invoke(&w2, "window_take_pending", json!({})).unwrap();
    assert_eq!(w2_pending["paths"], json!(["/x"]));
    // A destroyed window is forgotten.
    destroyed(h, "w-2");
    destroyed(h, "d-main-1");
    assert_eq!(target(h).as_deref(), Some("main"), "falls back to main");
}

#[test]
fn a_single_window_app_refuses_new_windows_and_a_full_app_refuses_more() {
    let app = app_with(windows(WindowConfig::new("Pitwall"), &calls()).build());
    let main = create_main(app.handle()).unwrap();
    assert_eq!(
        invoke(&main, "window_new", json!({})).unwrap_err()["code"],
        "notAllowed"
    );
    let app = app_with(windows(WindowConfig::new("Debrief").multi_window(2), &calls()).build());
    let h = app.handle();
    create_main(h).unwrap();
    assert_eq!(open_new(h, OpenRequest::default()).unwrap(), "w-2");
    assert_eq!(
        open_new(h, OpenRequest::default()).unwrap_err().code,
        crate::WindowErrorCode::TooMany
    );
    // A label already taken by another window: the platform refuses, the bookkeeping is undone.
    let app = app_with(windows(multi(), &calls()).build());
    window(&app, "w-2");
    let e = open_new(app.handle(), paths(&["/x".into()])).unwrap_err();
    assert_eq!(e.code, crate::WindowErrorCode::Failed);
    assert!(state(&app).take_pending("w-2").is_empty());
}

#[test]
fn closing_follows_the_setting() {
    let quits = calls();
    let prefs = Arc::new(FixedPrefs::default());
    // No tray: Ask means quit.
    let app = app_with(
        windows(WindowConfig::new("Debrief"), &quits)
            .prefs(prefs.clone())
            .build(),
    );
    let main = create_main(app.handle()).unwrap();
    assert!(close_requested(app.handle(), &main));
    assert_eq!(taken(&quits), ["exit 0"]);
    // With a tray: the window stays and its UI is asked.
    let app = app_with(
        windows(WindowConfig::new("Debrief"), &quits)
            .prefs(prefs.clone())
            .build(),
    );
    let h = app.handle();
    with_tray(&app);
    let main = create_main(h).unwrap();
    let asked = heard(&main, CLOSE_REQUESTED_EVENT);
    assert!(close_requested(h, &main), "asked: the window stays");
    assert_eq!(taken(&asked).len(), 1);
    // "Keep running in the tray", remembered: the window closes (destroyed), the app keeps running.
    invoke(
        &main,
        "window_resolve_close",
        json!({ "choice": "tray", "remember": true }),
    )
    .unwrap();
    assert_eq!(prefs.on_close(), OnClose::Tray);
    assert!(
        !close_requested(h, &main),
        "now closing just closes, without asking"
    );
    assert!(taken(&asked).is_empty());
    assert!(
        exit_requested(h, None),
        "the last window closed: keep running"
    );
    // With another visible window, closing just closes.
    let other = window(&app, "w-2");
    assert!(!close_requested(h, &other));
    // "Quit" quits.
    invoke(
        &main,
        "window_resolve_close",
        json!({ "choice": "quit", "remember": false }),
    )
    .unwrap();
    assert_eq!(taken(&quits), ["exit 0"]);
    // While quitting, every close goes through and nothing keeps running.
    state(&app).exit.begin_quit();
    assert!(!close_requested(h, &main));
    assert!(!exit_requested(h, None));
    invoke(&main, "window_close", json!({})).unwrap();
}

#[test]
fn closing_again_while_asked_quits_and_hiding_keeps_the_window() {
    let quits = calls();
    let app = app_with(
        windows(
            WindowConfig::new("Debrief").background(Background::Hide),
            &quits,
        )
        .build(),
    );
    let h = app.handle();
    with_tray(&app);
    let main = create_main(h).unwrap();
    assert!(close_requested(h, &main));
    assert!(close_requested(h, &main), "the UI may be stuck: quit");
    assert_eq!(taken(&quits), ["exit 0"]);
    // Hide: the close is prevented, the window and its family are hidden.
    let prefs = Arc::new(FixedPrefs::new(OnClose::Tray, OpenFilesIn::Ask));
    let app = app_with(
        windows(
            WindowConfig::new("Debrief").background(Background::Hide),
            &quits,
        )
        .prefs(prefs)
        .build(),
    );
    let h = app.handle();
    with_tray(&app);
    let main = create_main(h).unwrap();
    window(&app, "p-main-1");
    assert!(close_requested(h, &main));
    invoke(
        &main,
        "window_resolve_close",
        json!({ "choice": "tray", "remember": false }),
    )
    .unwrap();
    reveal(h, "main").unwrap();
    assert!(taken(&quits).is_empty());
    // "Tray" without a tray quits.
    let app = app_with(windows(WindowConfig::new("Debrief"), &quits).build());
    let main = create_main(app.handle()).unwrap();
    invoke(
        &main,
        "window_resolve_close",
        json!({ "choice": "tray", "remember": false }),
    )
    .unwrap();
    assert_eq!(taken(&quits), ["exit 0"]);
}

/// Settings whose "remember" fails: logged, the window closes anyway.
struct BrokenPrefs;

impl Prefs for BrokenPrefs {
    fn on_close(&self) -> OnClose {
        OnClose::Ask
    }
    fn remember_on_close(&self, _: OnClose) -> Result<(), String> {
        Err("disk full".into())
    }
}

#[test]
fn a_failing_remember_does_not_keep_the_window_open() {
    let quits = calls();
    let app = app_with(
        windows(WindowConfig::new("Debrief"), &quits)
            .prefs(Arc::new(BrokenPrefs))
            .build(),
    );
    let main = create_main(app.handle()).unwrap();
    assert_eq!(BrokenPrefs.open_files_in(), OpenFilesIn::Ask);
    invoke(
        &main,
        "window_resolve_close",
        json!({ "choice": "quit", "remember": true }),
    )
    .unwrap();
    assert_eq!(taken(&quits), ["exit 0"]);
    invoke(&main, "app_quit", json!({})).unwrap();
    assert_eq!(taken(&quits), ["exit 0"]);
}

#[test]
fn a_second_launch_opens_in_the_running_instance() {
    let app = app_with(windows(multi(), &calls()).build());
    let h = app.handle();
    let main = create_main(h).unwrap();
    let opened = heard(&main, OPEN_EVENT);
    // The empty main window takes the files; links come along.
    second_instance(
        h,
        vec![
            "app".into(),
            src_dir(),
            "/does/not/exist".into(),
            "debrief://x".into(),
        ],
        String::new(),
    );
    assert_eq!(taken(&opened).len(), 1);
    let p = state(&app).take_pending("main");
    assert_eq!((p.paths.len(), p.links.len()), (1, 1));
    // Relative to the caller's folder, in a new window.
    second_instance(
        h,
        vec!["app".into(), "--new-window".into(), "src".into()],
        env!("CARGO_MANIFEST_DIR").into(),
    );
    assert_eq!(state(&app).take_pending("w-2").paths, [src_dir()]);
    // No paths: the window is just shown; a second log-in start changes nothing.
    second_instance(h, vec!["app".into()], String::new());
    second_instance(h, vec!["app".into(), "--hidden".into()], String::new());
    assert_eq!(list(h).len(), 2);
}

#[test]
fn files_for_a_busy_window_are_asked_about_and_the_answer_applies_once() {
    let app = app_with(windows(multi(), &calls()).pristine(|_, _| false).build());
    let h = app.handle();
    let main = create_main(h).unwrap();
    let asks = heard(&main, OPEN_ASK_EVENT);
    let links = parse_args(["debrief://y".to_string()], None, &["debrief"]).links;
    open_from_outside(
        h,
        OpenRequest {
            paths: vec![src_dir()],
            links,
        },
        false,
    );
    let payload: Value = serde_json::from_str(&taken(&asks)[0]).unwrap();
    assert_eq!(payload, json!({ "count": 1, "names": ["src"] }));
    assert_eq!(
        state(&app).take_pending("main").links.len(),
        1,
        "links open at once"
    );
    invoke(&main, "window_resolve_open", json!({ "choice": "here" })).unwrap();
    assert_eq!(state(&app).take_pending("main").paths, [src_dir()]);
    // Nothing waiting: nothing happens.
    invoke(
        &main,
        "window_resolve_open",
        json!({ "choice": "newWindow" }),
    )
    .unwrap();
    assert_eq!(list(h).len(), 1);
    // "New window".
    open_from_outside(h, paths(&[src_dir()]), false);
    invoke(
        &main,
        "window_resolve_open",
        json!({ "choice": "newWindow" }),
    )
    .unwrap();
    assert_eq!(state(&app).take_pending("w-2").paths, [src_dir()]);
    // Cancel drops them; a later answer opens nothing.
    let w2 = window(&app, "w-2");
    open_from_outside(h, paths(&[src_dir()]), false);
    invoke(&w2, "window_resolve_open", json!({ "choice": "cancel" })).unwrap();
    invoke(&w2, "window_resolve_open", json!({ "choice": "here" })).unwrap();
    assert!(state(&app).take_pending("w-2").is_empty());
    // A pop-out cannot answer for its host (the guard), nor through the function.
    let pop = window(&app, "p-main-1");
    let denied = invoke(&pop, "window_resolve_open", json!({ "choice": "here" })).unwrap_err();
    assert_eq!(denied["code"], "denied");
    let direct = ::tauri::async_runtime::block_on(window_resolve_open(
        h.clone(),
        pop,
        crate::OpenChoice::Here,
    ));
    assert_eq!(direct.unwrap_err().code, crate::WindowErrorCode::NotAllowed);
}

#[test]
fn files_go_where_the_setting_says() {
    let prefs = Arc::new(FixedPrefs::new(OnClose::Ask, OpenFilesIn::CurrentWindow));
    let app = app_with(
        windows(multi(), &calls())
            .prefs(prefs)
            .pristine(|_, _| false)
            .build(),
    );
    let h = app.handle();
    create_main(h).unwrap();
    open_from_outside(h, paths(&[src_dir()]), false);
    assert_eq!(state(&app).take_pending("main").paths.len(), 1);
    // The new window is refused (one at most): logged, nothing breaks.
    let prefs = Arc::new(FixedPrefs::new(OnClose::Ask, OpenFilesIn::NewWindow));
    let app = app_with(
        windows(WindowConfig::new("Debrief").multi_window(1), &calls())
            .prefs(prefs)
            .pristine(|_, _| false)
            .build(),
    );
    let h = app.handle();
    create_main(h).unwrap();
    open_from_outside(h, paths(&[src_dir()]), false);
    assert_eq!(list(h).len(), 1);
    // "New window" when the app is full: an error for the answer.
    let app = app_with(
        windows(WindowConfig::new("Debrief").multi_window(1), &calls())
            .pristine(|_, _| false)
            .build(),
    );
    let h = app.handle();
    let main = create_main(h).unwrap();
    open_from_outside(h, paths(&[src_dir()]), false);
    let full = invoke(
        &main,
        "window_resolve_open",
        json!({ "choice": "newWindow" }),
    )
    .unwrap_err();
    assert_eq!(full["code"], "tooMany");
}

#[test]
fn a_tray_app_without_a_window_rebuilds_it_for_a_second_launch_the_dock_and_opened_links() {
    let app =
        app_with(windows(WindowConfig::new("Pitwall").schemes(&["pitwall"]), &calls()).build());
    let h = app.handle();
    second_instance(
        h,
        vec!["pitwall-app".into(), "pitwall://models".into()],
        String::new(),
    );
    assert!(
        app.get_webview_window("main").is_some(),
        "rebuilt from the configuration"
    );
    assert_eq!(state(&app).take_pending("main").links[0].host, "models");
    // macOS: files and links opened with the app, the Dock icon.
    let urls = vec![
        url::Url::from_directory_path(src_dir()).unwrap(),
        url::Url::parse("pitwall://apps?id=1").unwrap(),
        url::Url::parse("https://example.com").unwrap(),
    ];
    opened(h, &urls);
    let p = state(&app).take_pending("main");
    assert_eq!((p.paths.len(), p.links.len()), (1, 1));
    reopen(h);
    assert_eq!(list(h).len(), 1);
}

#[test]
fn an_exit_request_keeps_a_tray_app_running_and_a_stalled_quit_is_ended() {
    let quits = calls();
    let steps = calls();
    let s = steps.clone();
    let config = WindowConfig::new("Pitwall")
        .exit_stall_limit(Duration::from_millis(50))
        .step_limit(Duration::from_millis(200));
    let (release, stuck) = std::sync::mpsc::channel::<()>();
    let stuck = Mutex::new(stuck);
    let app = app_with(
        windows(config, &quits)
            .teardown("jobs", move |_| s.lock().unwrap().push("jobs".into()))
            .teardown("stuck", move |_| {
                let _ = stuck.lock().unwrap().recv_timeout(Duration::from_secs(5));
            })
            .build(),
    );
    let h = app.handle();
    with_tray(&app);
    create_main(h).unwrap();
    assert!(exit_requested(h, None), "the last window closed");
    let started = std::time::Instant::now();
    assert!(!exit_requested(h, Some(3)), "an explicit exit");
    assert!(state(&app).quitting());
    // The windows never close: the watchdog tears down and ends the process with the code, once.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !quits.lock().unwrap().iter().any(|c| c == "end 3") {
        assert!(
            std::time::Instant::now() < deadline,
            "the watchdog ended the exit"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "bounded by the step limits"
    );
    assert_eq!(taken(&steps), ["jobs"]);
    drop(release);
    arm_exit_watchdog(h, 4);
    exit(h);
    assert!(taken(&steps).is_empty(), "the teardown runs once");
}

#[test]
fn the_normal_exit_tears_down_and_the_watchdog_then_does_nothing() {
    let quits = calls();
    let config = WindowConfig::new("Debrief").exit_stall_limit(Duration::from_millis(30));
    let app = app_with(windows(config, &quits).build());
    let h = app.handle();
    assert!(!exit_requested(h, Some(0)));
    on_run_event(h, &RunEvent::Exit);
    on_run_event(h, &RunEvent::Ready);
    let deadline = std::time::Instant::now() + Duration::from_millis(300);
    while std::time::Instant::now() < deadline {
        assert!(
            taken(&quits).is_empty(),
            "the normal exit owned the teardown"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn macos_opened_files_reach_the_window() {
    let app = app_with(windows(WindowConfig::new("Pitwall"), &calls()).build());
    let h = app.handle();
    reopen(h);
    assert!(app.get_webview_window("main").is_some());
    let urls = vec![url::Url::from_directory_path(src_dir()).unwrap()];
    on_run_event(h, &RunEvent::Opened { urls });
    assert_eq!(state(&app).take_pending("main").paths.len(), 1);
}

#[test]
fn bounds_are_remembered_saved_and_restored() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("window-state.json");
    let saved = SavedBounds {
        x: 100.0,
        y: 120.0,
        width: 1000.0,
        height: 700.0,
        scale: 2.0,
        maximized: true,
        fullscreen: false,
    };
    {
        let app = app_with(windows(multi(), &calls()).bounds_file(&file).build());
        let ws = state(&app);
        ws.remember("main", saved);
        ws.remember("bad key", saved);
        ws.remember(
            "window",
            SavedBounds {
                width: -1.0,
                ..saved
            },
        );
        ws.persist_bounds().unwrap();
    }
    let back = crate::bounds::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(back.keys().collect::<Vec<_>>(), ["main"]);
    // A new run reads them and the main window opens there (mock windows see no monitors: as remembered).
    let app = app_with(windows(multi(), &calls()).bounds_file(&file).build());
    assert_eq!(state(&app).saved("main"), Some(saved));
    let main = create_main(app.handle()).unwrap();
    // Moving and resizing are recorded (the mock reports a zero size, which is not remembered).
    let os = main.as_ref().window();
    on_window_event(
        &os,
        &WindowEvent::Moved(::tauri::PhysicalPosition::new(1, 2)),
    );
    on_window_event(&os, &WindowEvent::Resized(::tauri::PhysicalSize::new(1, 2)));
    assert_eq!(state(&app).saved("main"), Some(saved));
    exit(app.handle());
    // Damaged, newer and unreadable files: no memory.
    for (name, text) in [("damaged.json", "{"), ("newer.json", r#"{ "version": 9 }"#)] {
        let f = dir.path().join(name);
        std::fs::write(&f, text).unwrap();
        let app = app_with(windows(multi(), &calls()).bounds_file(&f).build());
        assert_eq!(state(&app).saved("main"), None, "{name}");
    }
    let app = app_with(windows(multi(), &calls()).bounds_file(dir.path()).build());
    assert_eq!(state(&app).saved("main"), None, "a folder");
    state(&app).remember("main", saved);
    assert!(state(&app).persist_bounds().is_err());
    exit(app.handle());
    // Without a file, nothing is written.
    let app = app_with(windows(multi(), &calls()).build());
    assert!(state(&app).persist_bounds().is_ok());
}

#[test]
fn a_window_place_cascades_restores_and_tracks_maximized() {
    let config = multi();
    let desk = [Monitor {
        work: Rect {
            x: 0.0,
            y: 0.0,
            width: 2560.0,
            height: 1400.0,
        },
        scale: 1.0,
    }];
    let parent = Rect {
        x: 100.0,
        y: 100.0,
        width: 1300.0,
        height: 900.0,
    };
    let r = place_for("w-2", None, Some(parent), &desk, &config).unwrap();
    assert_eq!((r.x, r.y, r.width), (132.0, 132.0, 1280.0));
    let saved = SavedBounds {
        x: 9000.0,
        y: 10.0,
        width: 800.0,
        height: 600.0,
        scale: 1.0,
        maximized: false,
        fullscreen: false,
    };
    let r = place_for("main", Some(saved), Some(parent), &desk, &config).unwrap();
    assert_eq!(
        r.x,
        2560.0 - 800.0,
        "main never cascades; it comes back on screen"
    );
    assert_eq!(place_for("w-2", None, None, &desk, &config), None);
    // Maximized or full screen: the last normal place is kept with the flag.
    let now = bounds_now((1.0, 2.0), (300.0, 200.0), 2.0, false, false, None);
    assert_eq!(now.sanitized().map(|b| b.scale), Some(2.0));
    let zero = bounds_now((1.0, 2.0), (0.0, 0.0), 1.0, false, false, Some(saved));
    assert_eq!(zero.sanitized(), None, "nonsense is not remembered");
    let max = bounds_now((0.0, 0.0), (2560.0, 1400.0), 1.0, true, false, Some(saved));
    assert_eq!((max.x, max.width, max.maximized), (9000.0, 800.0, true));
    let first = bounds_now((0.0, 0.0), (1.0, 1.0), 1.0, false, true, None);
    assert!(
        first.fullscreen && first.width == 1.0,
        "full screen at once: its size for now"
    );
    // Applying a place with both flags on a (mock) window.
    let app = app_with(windows(multi(), &calls()).build());
    let main = create_main(app.handle()).unwrap();
    let both = SavedBounds {
        maximized: true,
        fullscreen: true,
        ..saved
    };
    super::manager::apply_place(&main, &crate::bounds::restore(&both, &[], (1.0, 1.0)));
}

#[test]
fn window_events_track_focus_and_destroyed_windows() {
    let freed = calls();
    let f = freed.clone();
    let app = app_with(
        windows(multi(), &calls())
            .on_destroyed(move |_, label| f.lock().unwrap().push(label.to_string()))
            .window_builder(|b| b.title("hooked"))
            .build(),
    );
    let h = app.handle();
    create_main(h).unwrap();
    open_new(h, OpenRequest::default()).unwrap();
    // A pop-out stands for its host.
    let pop = window(&app, "p-main-1").as_ref().window();
    on_window_event(&pop, &WindowEvent::Focused(true));
    assert_eq!(target(h).as_deref(), Some("main"));
    on_window_event(&pop, &WindowEvent::Focused(false));
    on_window_event(
        &pop,
        &WindowEvent::Moved(::tauri::PhysicalPosition::new(1, 2)),
    );
    on_window_event(&pop, &WindowEvent::Destroyed);
    let w2 = window(&app, "w-2").as_ref().window();
    on_window_event(&w2, &WindowEvent::Destroyed);
    assert_eq!(taken(&freed), ["w-2"]);
    // Quitting: destroyed windows do not announce a change.
    state(&app).exit.begin_quit();
    destroyed(h, "main");
    assert_eq!(taken(&freed), ["main"]);
}

#[test]
fn dialogs_get_the_same_feel_and_hooks() {
    let contracts = paddock_dialogs::Contracts::parse(
        r#"{ "version": 1, "dialogs": { "about": { "kind": "viewer", "modality": "modeless" } } }"#,
    )
    .unwrap();
    let app = app_with(
        windows(multi(), &calls())
            .window_builder(|b| b.title("hooked"))
            .build(),
    );
    let h = app.handle();
    app.manage(paddock_dialogs::Dialogs::new(Arc::new(contracts.clone())));
    app.manage(Windows::dialog_hooks(h));
    create_main(h).unwrap();
    let request: paddock_dialogs::DialogRequest =
        serde_json::from_value(json!({ "dialogId": "about", "title": "About" })).unwrap();
    let info = paddock_dialogs::tauri::open(h, "main", request.clone()).unwrap();
    assert!(app.get_webview_window(&info.label).is_some());
    // Without the window state the hooks still apply the feel.
    let bare = mock_builder().build(mock_context(noop_assets())).unwrap();
    bare.manage(paddock_dialogs::Dialogs::new(Arc::new(contracts)));
    bare.manage(Windows::<MockRuntime>::dialog_hooks(bare.handle()));
    window(&bare, "main");
    assert!(paddock_dialogs::tauri::open(bare.handle(), "main", request).is_ok());
}

#[test]
fn the_tray_draws_its_model_only_when_it_changed_and_runs_its_items() {
    let quits = calls();
    let app = app_with(windows(multi(), &quits).build());
    let h = app.handle();
    let fake = FakeTray::default();
    let tip = Arc::new(Mutex::new("App".to_string()));
    let actions = calls();
    assert!(create_tray(h, tray(&fake, &tip, &actions)));
    assert!(tray_available(h));
    assert_eq!(taken(&fake.drawn), ["create App"]);
    refresh_tray(h);
    assert!(
        taken(&fake.drawn).is_empty(),
        "the same view is not drawn again"
    );
    *tip.lock().unwrap() = "App — 1 job".into();
    refresh_tray(h);
    assert_eq!(taken(&fake.drawn), ["update App — 1 job"]);
    // An invalid model is not drawn (logged).
    tip.lock().unwrap().clear();
    refresh_tray(h);
    assert!(taken(&fake.drawn).is_empty());
    // The shell's items and the app's.
    menu_chosen(h, crate::tray::SHOW_ID);
    assert!(
        app.get_webview_window("main").is_some(),
        "Show builds the main window"
    );
    menu_chosen(h, crate::tray::NEW_WINDOW_ID);
    menu_chosen(h, "pd.window:w-2");
    menu_chosen(h, "pd.window:w-99");
    menu_chosen(h, "app.stop");
    assert_eq!(taken(&actions), ["app.stop"]);
    menu_chosen(h, crate::tray::QUIT_ID);
    assert_eq!(taken(&quits), ["exit 0"]);
    // A tray without a handler ignores the app's items.
    let app = app_with(windows(multi(), &calls()).build());
    let bare = Tray::new(FakeTray::default(), |_: &Handle| {
        TrayView::new("x", vec![], "idle")
    });
    assert!(create_tray(app.handle(), bare));
    menu_chosen(app.handle(), "app.stop");
}

#[test]
fn without_a_tray_the_app_runs_without_one() {
    let app = app_with(windows(multi(), &calls()).build());
    let failing = FakeTray {
        fail_create: true,
        ..FakeTray::default()
    };
    let tip = Arc::new(Mutex::new("App".to_string()));
    assert!(!create_tray(app.handle(), tray(&failing, &tip, &calls())));
    assert!(!tray_available(app.handle()));
    refresh_tray(app.handle());
    // An invalid first model: no icon either.
    let app = app_with(windows(multi(), &calls()).build());
    let fake = FakeTray::default();
    let empty = Arc::new(Mutex::new(String::new()));
    assert!(!create_tray(app.handle(), tray(&fake, &empty, &calls())));
    assert!(taken(&fake.drawn).is_empty());
    // A failing redraw is logged.
    let app = app_with(windows(multi(), &calls()).build());
    let flaky = FakeTray {
        fail_update: true,
        ..FakeTray::default()
    };
    let tip = Arc::new(Mutex::new("a".to_string()));
    assert!(create_tray(app.handle(), tray(&flaky, &tip, &calls())));
    *tip.lock().unwrap() = "b".into();
    refresh_tray(app.handle());
    assert_eq!(taken(&flaky.drawn), ["create a", "update b"]);
}

fn icon() -> IconRect {
    IconRect {
        x: 1000.0,
        y: 0.0,
        w: 44.0,
        h: 44.0,
    }
}

fn popover_app() -> App {
    let app = app_with(windows(WindowConfig::new("Dyno"), &calls()).build());
    app.manage(
        Popovers::new()
            // No material: the platform's effects need a real window handle (the mock's would crash).
            .style(Style {
                os: "macos",
                material: Material::None,
            })
            .popover(
                None,
                PopoverSpec::new("index.html#/tray", "Dyno").width(400.0),
            )
            .popover(Some("cpu"), PopoverSpec::new("index.html#/tray/cpu", "CPU"))
            .popover(Some("BAD"), PopoverSpec::new("x", "x")),
    );
    app
}

#[test]
fn the_popover_toggles_fits_its_content_and_closes_on_blur_and_escape() {
    let app = popover_app();
    let h = app.handle();
    assert_eq!(toggle_popover(h, Some("cpu"), icon()), Ok(true));
    let cpu = app.get_webview_window("tray-cpu").expect("built");
    assert!(app.state::<Popovers>().is_open("tray-cpu"));
    // The page reports its height; the guard lets only popovers ask.
    let fit = |h: f64| invoke(&cpu, "tray_fit_height", json!({ "height": h }));
    assert_eq!(fit(300.0).unwrap(), json!(300.0));
    assert_eq!(fit(5.0).unwrap(), json!(120.0));
    let main = create_main(h).unwrap();
    let denied = invoke(&main, "tray_fit_height", json!({ "height": 300.0 })).unwrap_err();
    assert_eq!(denied["code"], "denied");
    let denied = invoke(&cpu, "window_set_title", json!({ "title": "x" })).unwrap_err();
    assert_eq!(denied["code"], "denied");
    assert!(
        invoke(&cpu, "windows_list", json!({})).is_ok(),
        "named TRAY"
    );
    assert_eq!(
        fit_popover(h, "main", 1.0).unwrap_err().code,
        crate::WindowErrorCode::NotAllowed
    );
    assert_eq!(
        fit_popover(h, "tray-gone", 1.0).unwrap_err().code,
        crate::WindowErrorCode::Closed
    );
    // A click on the icon toggles it closed; the click right after does not reopen it.
    assert_eq!(toggle_popover(h, Some("cpu"), icon()), Ok(false));
    assert!(!app.state::<Popovers>().is_open("tray-cpu"));
    assert_eq!(toggle_popover(h, Some("cpu"), icon()), Ok(false));
    // A closed popover that is still listed fits at its default anchor.
    assert!(fit(200.0).is_ok());
    // The icon's popover: blur closes it, Escape (`tray_close`) too; destroyed, it is forgotten.
    assert_eq!(toggle_popover(h, None, icon()), Ok(true));
    let tray = app.get_webview_window("tray").unwrap();
    let os = tray.as_ref().window();
    on_window_event(&os, &WindowEvent::Focused(false));
    assert!(!app.state::<Popovers>().is_open("tray"));
    on_window_event(&os, &WindowEvent::Focused(true));
    on_window_event(&os, &WindowEvent::Destroyed);
    invoke(&tray, "tray_close", json!({})).unwrap();
    let direct = ::tauri::async_runtime::block_on(tray_close(h.clone(), main));
    assert_eq!(direct.unwrap_err().code, crate::WindowErrorCode::NotAllowed);
    close_popover(h, "tray-none");
    // Items that are not set up, or not valid.
    assert_eq!(
        toggle_popover(h, Some("BAD"), icon()).unwrap_err().code,
        crate::WindowErrorCode::NotAllowed
    );
    assert_eq!(
        toggle_popover(h, Some("gpu"), icon()).unwrap_err().code,
        crate::WindowErrorCode::NotAllowed
    );
    assert!(!Popovers::default().is_open("tray"));
}

#[test]
fn a_tray_click_opens_the_popover_or_the_window() {
    let app = popover_app();
    let h = app.handle();
    let drawn = Arc::new(AtomicUsize::new(0));
    let d = drawn.clone();
    let with_popover = Tray::new(FakeTray::default(), move |_: &Handle| {
        d.fetch_add(1, Ordering::SeqCst);
        TrayView::new("Dyno", vec![], "idle")
    })
    .popover(true);
    create_tray(h, with_popover);
    icon_clicked(h, Some(icon()));
    assert!(app.state::<Popovers>().is_open("tray"));
    // Without a place to anchor it: the window.
    icon_clicked(h, None);
    assert!(app.get_webview_window("main").is_some());
    assert!(drawn.load(Ordering::SeqCst) >= 1);
    // No popover: the menu (macOS) or the window.
    let app = app_with(windows(WindowConfig::new("Pitwall"), &calls()).build());
    let plain = Tray::new(FakeTray::default(), |_: &Handle| {
        TrayView::new("P", vec![], "idle")
    });
    create_tray(app.handle(), plain);
    icon_clicked(app.handle(), Some(icon()));
    assert_eq!(
        app.get_webview_window("main").is_some(),
        !cfg!(target_os = "macos")
    );
    // A popover click in an app without popovers set up is logged.
    let app = app_with(windows(WindowConfig::new("Pitwall"), &calls()).build());
    let broken = Tray::new(FakeTray::default(), |_: &Handle| {
        TrayView::new("P", vec![], "idle")
    })
    .popover(true);
    create_tray(app.handle(), broken);
    icon_clicked(app.handle(), Some(icon()));
}

#[test]
fn the_material_and_monitor_reach_the_window_builder() {
    let mac = Style {
        os: "macos",
        material: Material::Vibrancy,
    };
    assert!(effects(mac).is_some_and(|e| e.radius == Some(12.0)));
    let win = Style {
        os: "windows",
        material: Material::Acrylic,
    };
    assert!(effects(win).is_some_and(|e| e.color.is_some()));
    let linux = Style {
        os: "linux",
        material: Material::None,
    };
    assert!(effects(linux).is_none());
    let position = ::tauri::PhysicalPosition::new(-1920, 25);
    let size = ::tauri::PhysicalSize::new(1920, 1055);
    let m: ::tauri::Monitor = tauri_runtime::monitor::Monitor {
        name: Some("left".into()),
        size,
        position,
        work_area: ::tauri::PhysicalRect { position, size },
        scale_factor: 1.5,
    }
    .into();
    assert_eq!(
        work_rect(&m),
        IconRect {
            x: -1920.0,
            y: 25.0,
            w: 1920.0,
            h: 1055.0
        }
    );
    assert_eq!(
        PopoverSpec::new("r", "t").width(10.0).width,
        crate::popover::MIN_WIDTH
    );
    // Linux (the page's own background) builds its popover too; the materials need a real window handle.
    let app = app_with(windows(WindowConfig::new("Spotter"), &calls()).build());
    app.manage(
        Popovers::new()
            .style(linux)
            .popover(None, PopoverSpec::new("index.html#/tray", "Spotter")),
    );
    assert_eq!(toggle_popover(app.handle(), None, icon()), Ok(true));
}

#[test]
fn the_configuration_and_the_feel_apply_to_built_windows() {
    let config = WindowConfig::new("App")
        .url("index.html#/main")
        .size(900.0, 700.0)
        .min_size(400.0, 300.0);
    assert_eq!(
        (config.url.as_str(), config.size, config.min_size),
        ("index.html#/main", (900.0, 700.0), (400.0, 300.0))
    );
    let app = app_with(windows(config.clone(), &calls()).build());
    assert_eq!(state(&app).config(), &config);
    let main = create_main(app.handle()).unwrap();
    // A release build turns the browser keys off (a no-op off Windows, never a failure).
    super::manager::apply_feel(&main, crate::WebviewFeel::app_window(true));
    let e: crate::WindowError = ::tauri::Error::WindowNotFound.into();
    assert_eq!(e.code, crate::WindowErrorCode::Failed);
    // The main window declared in the app's configuration (`create: false`) is built from it.
    let mut context = mock_context(noop_assets());
    let declared = ::tauri::utils::config::WindowConfig {
        label: "main".into(),
        title: "Declared".into(),
        create: false,
        ..Default::default()
    };
    context.config_mut().app.windows.push(declared);
    let app = mock_builder()
        .invoke_handler(Guard::new(&crate::COMMANDS).wrap(invoke_handler()))
        .build(context)
        .unwrap();
    app.manage(windows(WindowConfig::new("App"), &calls()).build());
    assert!(
        app.get_webview_window("main").is_none(),
        "not created by Tauri"
    );
    let main = create_main(app.handle()).unwrap();
    assert_eq!(main.label(), "main", "the mock keeps no title to compare");
    assert!(!prevents_close_of(app.handle(), "d-main-1"));
}

fn prevents_close_of(app: &Handle, label: &str) -> bool {
    super::manager::keeps_open(app, label)
}

#[test]
fn malformed_requests_are_refused_before_they_run() {
    let app = app_with(windows(multi(), &calls()).build());
    let main = create_main(app.handle()).unwrap();
    for (cmd, body) in [
        ("window_focus", json!({})),
        ("window_set_title", json!({ "title": 3 })),
        (
            "window_resolve_close",
            json!({ "choice": "later", "remember": true }),
        ),
        ("window_resolve_open", json!({ "choice": "x" })),
        ("window_resolve_close", json!({ "choice": "later" })),
        (
            "window_resolve_close",
            json!({ "choice": "close", "remember": "yes" }),
        ),
        (
            "window_resolve_close",
            json!({ "choice": 7, "remember": true }),
        ),
    ] {
        assert!(invoke(&main, cmd, body).is_err(), "{cmd}");
    }
    let pop = {
        app.manage(
            Popovers::new()
                .style(Style {
                    os: "linux",
                    material: Material::None,
                })
                .popover(None, PopoverSpec::new("index.html#/tray", "App")),
        );
        toggle_popover(app.handle(), None, icon()).unwrap();
        app.get_webview_window("tray").unwrap()
    };
    assert!(invoke(&pop, "tray_fit_height", json!({ "height": "tall" })).is_err());
    assert!(prevents_close_of(app.handle(), "main") || !prevents_close_of(app.handle(), "w-9"));
}

#[test]
fn a_bounds_save_that_fails_after_a_window_closed_is_logged() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_with(windows(multi(), &calls()).bounds_file(dir.path()).build());
    create_main(app.handle()).unwrap();
    destroyed(app.handle(), "main");
    // The save runs on a thread of its own; the exit teardown saves (and fails) once more, synchronously.
    exit(app.handle());
}
