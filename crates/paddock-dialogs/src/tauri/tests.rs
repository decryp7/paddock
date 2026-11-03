//! The adapter on Tauri's mock runtime (`tauri::test`): no window ever reaches the desktop. Ported from Debrief's
//! `dialog_windows.rs` tests, plus the guard on a real invoke path with this crate's grants.

use super::*;
use crate::contract::Contracts;
use crate::registry::Scheduler;
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
use std::time::Duration;

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
    Quiet [DIALOG] => crate::tauri::dialog_ready,
    Quiet [DIALOG] => crate::tauri::dialog_fit,
    Query [WINDOW] => app_status,
    Query [WINDOW] => app_secret,
}

/// Contracts for the ids these tests open.
fn contracts() -> Arc<Contracts> {
    use crate::table::{Entry, Shape};
    const STATUS: &[paddock_commands::DialogCallable] =
        &[paddock_commands::DialogCallable::__command("app_status")];
    let form = Shape::Form(crate::Modality::Modeless);
    let entry = |id: &'static str, shape: Shape| Entry {
        commands: STATUS,
        ..Entry::new(id, shape)
    };
    let mut e = vec![
        Entry {
            instanced: true,
            ..entry("ai-setup", form)
        },
        entry("settings", Shape::Tool),
        entry("about", Shape::Viewer),
        Entry {
            required_choice: true,
            ..entry("close-prompt", Shape::Decision)
        },
        entry("unsaved-changes", Shape::Decision),
        entry("confirm-x", Shape::Decision),
        entry("too-many", form),
    ];
    e.extend(
        (0..MAX_OPEN_PER_HOST).map(|i| entry(Box::leak(format!("d{i}").into_boxed_str()), form)),
    );
    Arc::new(Contracts::from_entries(&e, &[]).expect("test contracts"))
}

/// A log record: (level, message, dialog, command, inputs).
type Logged = (log::Level, String, String, String, String);
/// Every log record of the guard and the dialog layer.
static LOGGED: Mutex<Vec<Logged>> = Mutex::new(Vec::new());

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
            kv("Inputs"),
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
        .map(|r| (r.0, r.1.clone(), r.2.clone(), r.3.clone()))
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
        remembered: None,
        theme: None,
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

/// Rule 56: an open of a dialog that is open already (Settings asked again for a section) is logged with the names of the
/// inputs it came with, never their values. The Dyno 1.0 re-target bug left only "Opening from another app" in the log.
#[test]
fn opening_an_open_dialog_again_logs_the_names_of_its_inputs_only() {
    let app = app();
    let h = app.handle().clone();
    open(&h, "main", request("settings")).unwrap();
    let mut again = request("settings");
    again.input =
        Some(json!({ "page": "dy.general", "anchor": { "id": "secret-section", "at": 7 } }));
    assert!(open(&h, "main", again).unwrap().existing);
    let mut bare = request("settings");
    bare.input = None;
    open(&h, "main", bare).unwrap();
    let again: Vec<String> = LOGGED
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .filter(|r| r.1 == "Dialog window opened again" && r.2 == "settings")
        .map(|r| r.4.clone())
        .collect();
    assert_eq!(again, ["anchor,page", "(none)"]);
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

fn measure(content_h: f64, min_h: f64) -> Measure {
    let size = |width, height| crate::Size { width, height };
    Measure {
        content: size(720.0, content_h),
        min_content: size(400.0, min_h),
        viewport_height: None,
        device_pixel_ratio: None,
    }
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
        assert_eq!(fit(&h, &dialog, measure(700.0, 200.0), Some(m), os), Ok(()));
    }
    assert_eq!(
        fit(&h, &dialog, measure(700.0, 200.0), None, "linux"),
        Ok(())
    );
    assert_eq!(
        state(&app).get("d-main-1").unwrap().measure,
        Some(measure(700.0, 200.0)),
        "the last measure is kept for a scale change"
    );
    let e = fit(&h, &dialog, measure(f64::NAN, 200.0), None, "linux").unwrap_err();
    assert_eq!(e.code, DialogErrorCode::Invalid);
    let e = fit(
        &h,
        &os_window(&app, "main"),
        measure(300.0, 100.0),
        None,
        "linux",
    )
    .unwrap_err();
    assert_eq!(e.code, DialogErrorCode::NotADialog);
}

/// Runs the show deadline's job by hand: the tests never sleep (ER-12).
#[derive(Default)]
struct Manual(Mutex<Vec<Job>>);

type Job = (Duration, Box<dyn FnOnce() + Send>);

impl std::fmt::Debug for Manual {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Manual({} jobs)", self.0.lock().unwrap().len())
    }
}

impl Scheduler for Manual {
    fn after(&self, delay: Duration, job: Box<dyn FnOnce() + Send + 'static>) {
        self.0.lock().unwrap().push((delay, job));
    }
}

impl Manual {
    /// The deadline passes: every scheduled job runs.
    fn deadline_passes(&self) {
        let jobs: Vec<_> = self.0.lock().unwrap().drain(..).collect();
        for (_, job) in jobs {
            job();
        }
    }

    fn delays(&self) -> Vec<Duration> {
        self.0.lock().unwrap().iter().map(|j| j.0).collect()
    }
}

fn manual_app() -> (::tauri::App<MockRuntime>, Arc<Manual>) {
    let timer = Arc::new(Manual::default());
    let dialogs = Dialogs::new(contracts()).with_scheduler(timer.clone());
    (app_with(Some(dialogs)), timer)
}

/// What the lifecycle did natively on this thread since `mark`.
fn native_since(mark: usize) -> Vec<String> {
    NATIVE.with(|n| n.borrow()[mark..].to_vec())
}

fn native_mark() -> usize {
    NATIVE.with(|n| n.borrow().len())
}

#[test]
fn a_dialog_is_not_shown_before_its_page_is_ready_and_is_shown_focused_once_it_is() {
    for os in ["linux", "windows", "macos"] {
        let (app, timer) = manual_app();
        let h = app.handle().clone();
        let mark = native_mark();
        let info = open_on(&h, "main", request("ai-setup"), os).unwrap();
        assert!(state(&app).is_pending(&info.label), "{os}");
        let seen = native_since(mark);
        assert!(
            !seen.iter().any(|o| o.starts_with("show")),
            "{os}: {seen:?}"
        );
        assert_eq!(
            timer.delays(),
            [crate::SHOW_DEADLINE],
            "the deadline is scheduled at create"
        );
        // The page reports: the window is sized, shown and focused, once.
        let dialog = os_window(&app, &info.label);
        ready(&h, &dialog, measure(400.0, 150.0), os).unwrap();
        assert!(!state(&app).is_pending(&info.label));
        let seen = native_since(mark);
        assert!(
            seen.contains(&format!("show:{}", info.label))
                && seen.contains(&format!("focus:{}", info.label)),
            "{os}: {seen:?}"
        );
        // A second ready (a late one) fits but does not show again; the deadline finds it shown.
        let before = native_since(mark).len();
        ready(&h, &dialog, measure(420.0, 150.0), os).unwrap();
        timer.deadline_passes();
        assert_eq!(native_since(mark).len(), before, "{os}");
    }
}

#[test]
fn a_page_that_never_reports_is_shown_at_the_deadline_and_a_late_ready_fits_it_once() {
    let (app, timer) = manual_app();
    let h = app.handle().clone();
    let mark = native_mark();
    let info = open(&h, "main", request("ai-setup")).unwrap();
    assert!(state(&app).is_pending(&info.label));
    timer.deadline_passes();
    assert!(!state(&app).is_pending(&info.label));
    let shown: Vec<_> = native_since(mark)
        .into_iter()
        .filter(|o| o.starts_with("show"))
        .collect();
    assert_eq!(shown, [format!("show:{}", info.label)]);
    // The page wakes up late: it is fitted, not shown again.
    let dialog = os_window(&app, &info.label);
    ready(&h, &dialog, measure(300.0, 120.0), "linux").unwrap();
    assert_eq!(
        native_since(mark)
            .iter()
            .filter(|o| o.starts_with("show"))
            .count(),
        1
    );
    assert!(state(&app).get(&info.label).unwrap().measure.is_some());
    // A dialog closed before its deadline is simply gone when the deadline passes.
    let gone = open(&h, "main", request("settings")).unwrap();
    on_destroyed(&h, &gone.label);
    timer.deadline_passes();
    assert!(!native_since(mark).contains(&format!("show:{}", gone.label)));
}

#[test]
fn showing_what_is_gone_does_nothing() {
    let (app, _timer) = manual_app();
    let h = app.handle().clone();
    let mark = native_mark();
    // Unknown label; a record without a window (its webview never reached the manager); no dialogs state at all.
    show(&h, "d-main-9", "linux", true);
    let about = state(&app).contracts().get("about").unwrap().clone();
    let orphan = state(&app)
        .reserve(OpenDialog::new("main", "main", "about", &about))
        .unwrap();
    show(&h, &orphan, "linux", false);
    let bare = app_with(None);
    show(bare.handle(), "d-main-1", "linux", true);
    assert!(native_since(mark).is_empty());
}

#[test]
fn decorations_are_the_outer_size_less_the_inner_one() {
    let size = |w, h| PhysicalSize::new(w, h);
    let d = decor(size(840, 656), size(800, 600), 2.0);
    assert_eq!((d.width, d.height), (20.0, 28.0));
    // A platform that reports the inner as the larger one (or both as zero) has none.
    let d = decor(size(0, 0), size(800, 600), 1.0);
    assert_eq!((d.width, d.height), (0.0, 0.0));
}

#[test]
fn the_deadline_is_configurable() {
    let timer = Arc::new(Manual::default());
    let dialogs = Dialogs::new(contracts())
        .with_scheduler(timer.clone())
        .with_show_deadline(Duration::from_millis(250));
    let app = app_with(Some(dialogs));
    open(app.handle(), "main", request("about")).unwrap();
    assert_eq!(timer.delays(), [Duration::from_millis(250)]);
    assert_eq!(state(&app).show_deadline(), Duration::from_millis(250));
}

#[test]
fn the_parent_stays_enabled_until_the_dialog_is_shown() {
    for os in ["windows", "linux", "macos"] {
        let (app, timer) = manual_app();
        let h = app.handle().clone();
        let mark = native_mark();
        let info = open_on(&h, "main", request("unsaved-changes"), os).unwrap();
        let disabled = |m: usize| native_since(m).contains(&"disable:main".to_string());
        assert!(
            !disabled(mark),
            "{os}: opening a modal does not lock its parent"
        );
        // Closed before it ever showed: the parent was never locked.
        on_destroyed_on(&h, &info.label, os);
        timer.deadline_passes();
        assert!(!disabled(mark), "{os}");
        // Shown: now it is (never on macOS, where the UI's scrim does it).
        let info = open_on(&h, "main", request("confirm-x"), os).unwrap();
        ready(&h, &os_window(&app, &info.label), measure(200.0, 100.0), os).unwrap();
        assert_eq!(disabled(mark), os != "macos", "{os}");
    }
}

#[test]
fn the_opener_is_told_when_the_window_is_shown_once_not_when_it_is_opened() {
    let (app, timer) = manual_app();
    let h = app.handle().clone();
    let shown = recorder(&window(&app, "main"), SHOWN_EVENT);
    let info = open(&h, "main", request("unsaved-changes")).unwrap();
    assert!(shown.lock().unwrap().is_empty(), "opening shows nothing");
    ready(
        &h,
        &os_window(&app, &info.label),
        measure(200.0, 100.0),
        "linux",
    )
    .unwrap();
    // The deadline passing afterwards, and a second ready, show nothing again.
    timer.deadline_passes();
    ready(
        &h,
        &os_window(&app, &info.label),
        measure(200.0, 100.0),
        "linux",
    )
    .unwrap();
    let seen = shown.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert!(
        seen[0].contains(&info.label) && seen[0].contains("unsaved-changes"),
        "{seen:?}"
    );
}

#[test]
fn only_shown_modals_hold_the_parent_and_the_last_shown_one_releases_it() {
    for os in ["windows", "linux"] {
        let (app, timer) = manual_app();
        let h = app.handle().clone();
        let mark = native_mark();
        let a = open_on(&h, "main", request("unsaved-changes"), os).unwrap();
        let b = open_on(&h, "main", request("confirm-x"), os).unwrap();
        let ops = || {
            native_since(mark)
                .into_iter()
                .filter(|o| o.ends_with(":main"))
                .collect::<Vec<_>>()
        };
        // Only A is shown (B is still pending): A closing releases the parent although B is open.
        ready(&h, &os_window(&app, &a.label), measure(200.0, 100.0), os).unwrap();
        on_destroyed_on(&h, &a.label, os);
        assert_eq!(ops(), ["disable:main", "enable:main"], "{os}");
        // B's deadline passes after that: it shows and holds the parent again; closing it releases it.
        timer.deadline_passes();
        on_destroyed_on(&h, &b.label, os);
        assert_eq!(
            ops(),
            ["disable:main", "enable:main", "disable:main", "enable:main"],
            "{os}"
        );
    }
}

#[test]
fn a_deadline_that_arrives_after_the_close_never_locks_the_parent() {
    for os in ["windows", "linux"] {
        let (app, timer) = manual_app();
        let h = app.handle().clone();
        let mark = native_mark();
        let a = open_on(&h, "main", request("unsaved-changes"), os).unwrap();
        on_destroyed_on(&h, &a.label, os);
        timer.deadline_passes();
        let ops = native_since(mark);
        assert!(
            !ops.iter()
                .any(|o| o == "disable:main" || o == "enable:main"),
            "{os}: {ops:?}"
        );
    }
}

#[test]
fn opening_a_pending_dialog_again_creates_no_window_and_does_not_show_it_early() {
    let (app, _timer) = manual_app();
    let h = app.handle().clone();
    let mark = native_mark();
    let first = open(&h, "main", request("ai-setup")).unwrap();
    let windows = h.webview_windows().len();
    let again = open(&h, "main", request("ai-setup")).unwrap();
    assert_eq!(
        (again.label.as_str(), again.existing),
        (first.label.as_str(), true)
    );
    assert!(
        first.pending && again.pending,
        "not on screen yet: its shown event is still to come"
    );
    assert_eq!(h.webview_windows().len(), windows, "no second window");
    assert!(
        !native_since(mark)
            .iter()
            .any(|o| o.starts_with("focus") || o.starts_with("show")),
        "a pending window is not focused (that would show it unfitted): {:?}",
        native_since(mark)
    );
    // Once shown, opening it again focuses it.
    ready(
        &h,
        &os_window(&app, &first.label),
        measure(300.0, 100.0),
        "linux",
    )
    .unwrap();
    let mark = native_mark();
    let shown = open(&h, "main", request("ai-setup")).unwrap();
    assert!(shown.existing && !shown.pending, "on screen already");
    assert_eq!(native_since(mark), [format!("focus:{}", first.label)]);
}

#[test]
fn macos_attaches_an_owned_dialog_to_its_parent_when_it_shows_never_when_it_is_built() {
    let (app, _t) = manual_app();
    let h = app.handle().clone();
    let mark = native_mark();
    let modal = open_on(&h, "main", request("unsaved-changes"), "macos").unwrap();
    // A modeless dialog is independent on macOS: it is never attached.
    let modeless = open_on(&h, "main", request("settings"), "macos").unwrap();
    let seen = native_since(mark);
    assert!(
        seen.iter()
            .all(|o| !o.starts_with("hide") && !o.starts_with("attach")),
        "{seen:?}"
    );
    let mark = native_mark();
    ready(
        &h,
        &os_window(&app, &modal.label),
        measure(300.0, 200.0),
        "macos",
    )
    .unwrap();
    ready(
        &h,
        &os_window(&app, &modeless.label),
        measure(300.0, 200.0),
        "macos",
    )
    .unwrap();
    let seen = native_since(mark);
    let attach = seen
        .iter()
        .position(|o| o == &format!("attach:{}", modal.label));
    let show = seen
        .iter()
        .position(|o| o == &format!("show:{}", modal.label));
    assert!(attach.is_some() && attach < show, "{seen:?}");
    assert!(
        !seen.contains(&format!("attach:{}", modeless.label)),
        "{seen:?}"
    );
    // Elsewhere the parent is attached when the window is built, never at show.
    let mark = native_mark();
    let linux = open_on(&h, "main", request("about"), "linux").unwrap();
    ready(
        &h,
        &os_window(&app, &linux.label),
        measure(300.0, 200.0),
        "linux",
    )
    .unwrap();
    assert!(native_since(mark).iter().all(|o| !o.starts_with("attach")));
    assert!(
        attaches_at_show("macos") && !attaches_at_show("windows") && !attaches_at_show("linux")
    );
}

#[test]
fn a_scale_change_refits_a_dialog_from_its_last_measure() {
    let (app, _t) = manual_app();
    let h = app.handle().clone();
    let info = open(&h, "main", request("ai-setup")).unwrap();
    let dialog = os_window(&app, &info.label);
    // (`WindowEvent::ScaleFactorChanged` is non-exhaustive and cannot be built here; the event arm only calls this.)
    // Nothing reported yet: nothing to refit, and nothing fails.
    scale_changed(&dialog);
    assert!(state(&app).get(&info.label).unwrap().measure.is_none());
    ready(&h, &dialog, measure(300.0, 100.0), "linux").unwrap();
    scale_changed(&dialog);
    // A window that is not a dialog has nothing to refit.
    scale_changed(&os_window(&app, "main"));
    // A refit that fails (a measure that is not a size) is logged, not raised.
    let bad = crate::Size {
        width: f64::NAN,
        height: 1.0,
    };
    state(&app).set_measure(
        &info.label,
        Measure {
            content: bad,
            min_content: bad,
            viewport_height: None,
            device_pixel_ratio: None,
        },
    );
    scale_changed(&dialog);
}

#[test]
fn a_scale_change_refits_after_the_window_systems_own_resize_not_inside_its_event() {
    // The window system (tao on Windows) resizes the window itself after it has told the app about the new scale factor;
    // a refit made inside that event is overwritten by it. The refit is scheduled, so it runs after the event returns.
    let (app, timer) = manual_app();
    let h = app.handle().clone();
    let info = open(&h, "main", request("ai-setup")).unwrap();
    let dialog = os_window(&app, &info.label);
    ready(&h, &dialog, measure(300.0, 100.0), "linux").unwrap();
    timer.deadline_passes();
    let before = state(&app).last_requested(&info.label);
    assert!(before.is_some());
    state(&app).set_measure(&info.label, measure(300.0, 400.0));
    scale_changed(&dialog);
    assert_eq!(state(&app).last_requested(&info.label), before);
    timer.deadline_passes();
    assert_ne!(state(&app).last_requested(&info.label), before);
}

#[test]
fn the_thread_scheduler_runs_its_job() {
    use crate::registry::ThreadScheduler;
    let (tx, rx) = std::sync::mpsc::channel();
    ThreadScheduler.after(Duration::ZERO, Box::new(move || tx.send(7).unwrap()));
    assert_eq!(rx.recv_timeout(Duration::from_secs(30)), Ok(7));
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
        json!({ "label": "d-main-1", "dialogId": "settings", "existing": false, "pending": true })
    );
    let dialog = window(&app, "d-main-1");
    let start = invoke(&dialog, "dialog_take_input", json!({})).unwrap();
    assert_eq!(
        (start["dialogId"].as_str(), start["kind"].as_str()),
        (Some("settings"), Some("tool"))
    );
    assert_eq!(invoke(&dialog, "app_status", json!({})), Ok(json!("ok")));
    let measure = json!({ "content": { "width": 960, "height": 400 }, "minContent": { "width": 500, "height": 200 } });
    assert_eq!(
        invoke(&dialog, "dialog_ready", json!({ "measure": measure })),
        Ok(Value::Null)
    );
    assert_eq!(
        invoke(&dialog, "dialog_fit", json!({ "measure": measure })),
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
    assert!(
        invoke(
            &dialog,
            "dialog_fit",
            json!({ "measure": { "content": "tall" } })
        )
        .is_err()
    );
    let nan = json!({ "content": { "width": 1, "height": 0 }, "minContent": { "width": 1, "height": 1 } });
    assert_eq!(
        invoke(&dialog, "dialog_fit", json!({ "measure": nan })).unwrap_err()["code"],
        "invalid"
    );
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

/// Holds the main thread's queue by hand for one test: what `on_main_thread` is given waits until the test runs it.
struct HeldMain;

impl HeldMain {
    fn new() -> Self {
        DEFER_MAIN.with(|d| d.set(true));
        MAIN_QUEUE.with(|q| q.borrow_mut().clear());
        HeldMain
    }

    fn queued(&self) -> usize {
        MAIN_QUEUE.with(|q| q.borrow().len())
    }

    /// Runs the queued tasks in the order given by `order` (indexes into the queue as it stands).
    fn run(&self, order: &[usize]) {
        let mut tasks: Vec<_> = MAIN_QUEUE
            .with(|q| q.borrow_mut().drain(..).collect::<Vec<_>>())
            .into_iter()
            .map(Some)
            .collect();
        for i in order {
            (tasks[*i].take().expect("each task runs once"))();
        }
    }

    fn run_all(&self) {
        let n = self.queued();
        self.run(&(0..n).collect::<Vec<_>>());
    }
}

impl Drop for HeldMain {
    fn drop(&mut self) {
        DEFER_MAIN.with(|d| d.set(false));
        MAIN_QUEUE.with(|q| q.borrow_mut().clear());
    }
}

fn parent_ops(mark: usize) -> Vec<String> {
    native_since(mark)
        .into_iter()
        .filter(|o| o.ends_with(":main"))
        .collect()
}

#[test]
fn a_deadline_show_then_a_close_leaves_the_parent_enabled() {
    for os in ["windows", "linux"] {
        let (app, timer) = manual_app();
        let h = app.handle().clone();
        let held = HeldMain::new();
        let mark = native_mark();
        let a = open_on(&h, "main", request("unsaved-changes"), os).unwrap();
        timer.deadline_passes();
        assert!(!state(&app).is_pending(&a.label), "{os}: shown");
        assert_eq!(held.queued(), 1, "{os}: the disable is only queued");
        on_destroyed_on(&h, &a.label, os);
        assert_eq!(held.queued(), 2, "{os}");
        // The queued tasks read the latest state: the dialog is gone, so nothing is ever disabled.
        held.run_all();
        assert_eq!(parent_ops(mark), Vec::<String>::new(), "{os}");
        assert!(!state(&app).parent_wants_disabled("main"), "{os}");
    }
}

#[test]
fn a_show_that_ran_before_the_close_disables_then_enables_the_parent() {
    for os in ["windows", "linux"] {
        let (app, timer) = manual_app();
        let h = app.handle().clone();
        let held = HeldMain::new();
        let mark = native_mark();
        let a = open_on(&h, "main", request("unsaved-changes"), os).unwrap();
        timer.deadline_passes();
        held.run_all();
        assert_eq!(parent_ops(mark), ["disable:main"], "{os}");
        on_destroyed_on(&h, &a.label, os);
        held.run_all();
        assert_eq!(parent_ops(mark), ["disable:main", "enable:main"], "{os}");
    }
}

#[test]
fn a_close_before_the_deadline_queues_nothing_and_the_late_deadline_shows_nothing() {
    for os in ["windows", "linux"] {
        let (app, timer) = manual_app();
        let h = app.handle().clone();
        let held = HeldMain::new();
        let mark = native_mark();
        let a = open_on(&h, "main", request("unsaved-changes"), os).unwrap();
        on_destroyed_on(&h, &a.label, os);
        timer.deadline_passes();
        assert_eq!(held.queued(), 0, "{os}");
        assert_eq!(parent_ops(mark), Vec::<String>::new(), "{os}");
    }
}

#[test]
fn two_modals_disable_the_parent_once_and_only_the_last_close_enables_it() {
    for os in ["windows", "linux"] {
        let (app, timer) = manual_app();
        let h = app.handle().clone();
        let held = HeldMain::new();
        let mark = native_mark();
        let a = open_on(&h, "main", request("unsaved-changes"), os).unwrap();
        let b = open_on(&h, "main", request("confirm-x"), os).unwrap();
        timer.deadline_passes();
        held.run_all();
        assert_eq!(parent_ops(mark), ["disable:main"], "{os}: once, not twice");
        on_destroyed_on(&h, &a.label, os);
        held.run_all();
        assert_eq!(parent_ops(mark), ["disable:main"], "{os}: B still holds it");
        on_destroyed_on(&h, &b.label, os);
        held.run_all();
        assert_eq!(parent_ops(mark), ["disable:main", "enable:main"], "{os}");
    }
}

#[test]
fn a_close_whose_task_runs_before_the_queued_disable_still_ends_enabled() {
    for os in ["windows", "linux"] {
        let (app, timer) = manual_app();
        let h = app.handle().clone();
        let held = HeldMain::new();
        let mark = native_mark();
        let a = open_on(&h, "main", request("unsaved-changes"), os).unwrap();
        timer.deadline_passes();
        on_destroyed_on(&h, &a.label, os);
        // The close's task (index 1) runs before the show's (index 0).
        held.run(&[1, 0]);
        assert_eq!(parent_ops(mark), Vec::<String>::new(), "{os}");
        assert!(!state(&app).parent_wants_disabled("main"), "{os}");
    }
}

#[test]
fn a_first_fit_keeps_the_viewport_delta_and_the_zoom_converts_the_measure_to_logical_px() {
    let (app, _t) = manual_app();
    let h = app.handle().clone();
    let info = open(&h, "main", request("ai-setup")).unwrap();
    let dialog = os_window(&app, &info.label);
    assert_eq!(state(&app).get(&info.label).unwrap().viewport_delta, None);
    // The mock window is 0x0 at scale 1: the delta taken at ready is 0; a page at devicePixelRatio 2 is zoom 2.
    let mut m = measure(300.0, 100.0);
    m.viewport_height = Some(150.0);
    m.device_pixel_ratio = Some(2.0);
    ready(&h, &dialog, m, "linux").unwrap();
    let e = state(&app).get(&info.label).unwrap();
    assert_eq!(e.viewport_delta, Some(0.0));
    let kept = e.measure.unwrap();
    assert_eq!(
        (
            kept.content.height,
            kept.min_content.height,
            kept.viewport_height,
            kept.device_pixel_ratio
        ),
        (600.0, 200.0, Some(300.0), None)
    );
    // A later fit reuses the recorded delta; it does not read it off the window again.
    state(&app).remove(&info.label);
    let info = open(&h, "main", request("ai-setup")).unwrap();
    let dialog = os_window(&app, &info.label);
    assert_eq!(
        state(&app).viewport_delta_or(&info.label, || 7.0),
        Some(7.0)
    );
    fit(&h, &dialog, m, None, "linux").unwrap();
    assert_eq!(
        state(&app).get(&info.label).unwrap().viewport_delta,
        Some(7.0)
    );
    assert_eq!(state(&app).viewport_delta_or("d-main-99", || 1.0), None);
}

#[test]
fn a_destroyed_window_and_a_closed_host_are_forgotten_in_the_applied_state() {
    for os in ["windows", "linux"] {
        let (app, timer) = manual_app();
        let h = app.handle().clone();
        let mark = native_mark();
        // The parent is disabled, then destroyed with its modal still open (the host's end closes its dialogs).
        open_on(&h, "main", request("unsaved-changes"), os).unwrap();
        timer.deadline_passes();
        assert_eq!(parent_ops(mark), ["disable:main"], "{os}");
        assert_eq!(state(&app).parent_change("main"), None, "{os}: applied");
        on_window_event(&os_window(&app, "main"), &WindowEvent::Destroyed);
        // A window that later has the label starts enabled, so a new modal must disable it again.
        let a = open_on(&h, "main", request("unsaved-changes"), os).unwrap();
        timer.deadline_passes();
        assert!(!state(&app).is_pending(&a.label), "{os}");
        assert_eq!(parent_ops(mark), ["disable:main", "disable:main"], "{os}");
        // Closing the host's dialogs forgets the label too.
        assert_eq!(close_all_of(&h, "main"), 1, "{os}");
        assert_eq!(
            state(&app).parent_change("main"),
            None,
            "{os}: nothing wanted, nothing applied"
        );
        state(&app).applied("main", true);
        close_all_of(&h, "main");
        assert_eq!(state(&app).parent_change("main"), None, "{os}");
    }
}

#[test]
fn the_applied_state_is_recorded_only_when_the_caller_says_the_window_took_it() {
    let (app, _t) = manual_app();
    let h = app.handle().clone();
    open_on(&h, "main", request("unsaved-changes"), "linux").unwrap();
    let dialogs = state(&app);
    // Showing is not done here, so nothing is wanted; make a shown modal by marking it.
    let label = dialogs.of_host("main").remove(0);
    dialogs.mark_shown(&label).unwrap();
    // Asking does not record: a failed set_enabled (mock windows cannot fail, so this is the seam) leaves it to retry.
    assert_eq!(dialogs.parent_change("main"), Some(true));
    assert_eq!(dialogs.parent_change("main"), Some(true));
    dialogs.applied("main", true);
    assert_eq!(dialogs.parent_change("main"), None);
    dialogs.remove(&label);
    assert_eq!(dialogs.parent_change("main"), Some(false));
    dialogs.applied("main", false);
    assert_eq!(dialogs.parent_change("main"), None);
}

#[test]
fn the_zoom_is_kept_on_the_entry_and_a_monitor_change_does_not_change_it() {
    let (app, _t) = manual_app();
    let h = app.handle().clone();
    let info = open(&h, "main", request("ai-setup")).unwrap();
    let dialogs = state(&app);
    assert_eq!(dialogs.zoom_for(&info.label, Some(2.0), 1.0), Some(2.0));
    assert_eq!(dialogs.get(&info.label).unwrap().zoom.unwrap().zoom, 2.0);
    // The scale factor changed (monitor move) with a stale ratio: kept.
    assert_eq!(dialogs.zoom_for(&info.label, Some(2.0), 2.0), Some(2.0));
    // The page zoomed at the unchanged scale: read again.
    assert_eq!(dialogs.zoom_for(&info.label, Some(3.0), 2.0), Some(1.5));
    assert_eq!(dialogs.zoom_for("d-main-99", Some(3.0), 2.0), None);
}

/// Inside a plugin hook (Tauri holds its plugin store there) a dialog is refused with its own code instead of
/// deadlocking, and leaves no record.
#[test]
fn a_dialog_opened_inside_a_plugin_hook_is_refused() {
    let app = app();
    let h = app.handle().clone();
    let scope = paddock_commands::plugin_hook::plugin_hook_scope();
    let e = open(&h, "main", request("about")).unwrap_err();
    assert_eq!(e.code, DialogErrorCode::InsidePluginHook, "{e}");
    assert!(e.message.contains("tauri::Builder::setup"), "{e}");
    assert!(state(&app).of_host("main").is_empty());
    drop(scope);
    assert!(open(&h, "main", request("about")).is_ok());
}
