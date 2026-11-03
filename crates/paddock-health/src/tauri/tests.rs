//! The adapter on a real invoke path: a mock Tauri app (`tauri::test`) with the commands behind the shell's guard, a
//! fake service and several windows.

use super::*;
use crate::FakeClock;
use crate::model::{Cause, RepairAction, ServiceState};
use crate::notify::centre::{NoticeAction, NoticeSeverity};
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{App, Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use paddock_commands::tauri::Guard;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

/// The dialog `d-main-1` may use the notification commands (its contract, ADR 0009).
struct NotificationsDialog;

impl paddock_commands::DialogGrant for NotificationsDialog {
    fn dialog(&self) -> &str {
        "notifications"
    }
    fn allows(&self, command: &str) -> bool {
        command.starts_with("notifications_")
    }
}

fn bare_app() -> App<MockRuntime> {
    let guard = Guard::new(&COMMANDS)
        .dialog_grants(|_app, label| (label == "d-main-1").then_some(NotificationsDialog));
    mock_builder()
        .invoke_handler(guard.wrap(invoke_handler()))
        .build(mock_context(noop_assets()))
        .unwrap()
}

/// A fake service: a supervisor flag the repair sets again (the repair is the shell's code, not the service's).
struct FakeService {
    running: Arc<AtomicBool>,
    repairs: Arc<AtomicUsize>,
}

impl FakeService {
    fn new() -> FakeService {
        FakeService {
            running: Arc::new(AtomicBool::new(true)),
            repairs: Arc::default(),
        }
    }

    fn spec(&self) -> ServiceSpec {
        let (running, repairs) = (self.running.clone(), self.repairs.clone());
        ServiceSpec::new("engine", "the engine").repair(move || {
            repairs.fetch_add(1, Ordering::SeqCst);
            running.store(true, Ordering::SeqCst);
            Ok(())
        })
    }

    /// What the supervisor reports when the engine dies.
    fn died(&self, health: &Health) {
        self.running.store(false, Ordering::SeqCst);
        health.down(
            "engine",
            Cause::new("The engine stopped.")
                .hint("Choose Restart.")
                .code("engine.stopped"),
            Some(RepairAction::Restart),
        );
    }
}

fn app_with(setup: Setup<MockRuntime>) -> (App<MockRuntime>, Arc<Health>) {
    let app = bare_app();
    let health = init(app.handle(), setup).unwrap();
    (app, health)
}

fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .build()
        .unwrap()
}

fn invoke(w: &WebviewWindow<MockRuntime>, cmd: &str, args: Value) -> Result<Value, Value> {
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
        body: InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(w, request).map(|b| b.deserialize::<Value>().unwrap())
}

type Heard = Arc<Mutex<Vec<Value>>>;

/// Every payload of `event` the window hears.
fn hear(w: &WebviewWindow<MockRuntime>, event: &str) -> Heard {
    let heard: Heard = Arc::default();
    let h = heard.clone();
    w.listen(event, move |e| {
        h.lock()
            .unwrap()
            .push(serde_json::from_str(e.payload()).unwrap())
    });
    heard
}

fn take(heard: &Heard) -> Vec<Value> {
    std::mem::take(&mut *heard.lock().unwrap())
}

/// Takes what a window heard once it has heard at least `n` events. Listeners of one event may run on different threads (on
/// Windows they do), so another listener having seen an event does not mean this one has.
fn take_n(heard: &Heard, n: usize) -> Vec<Value> {
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while heard.lock().unwrap().len() < n {
        // conformance: hang-guard — a listener that never hears must fail the test, not hang it
        assert!(
            std::time::Instant::now() < end,
            "the window heard fewer than {n} events"
        );
        std::thread::yield_now();
    }
    take(heard)
}

#[test]
fn one_failure_is_one_chip_and_one_notification_in_two_windows() {
    crate::tests::capture_logs();
    let service = FakeService::new();
    let (app, health) = app_with(Setup::new(vec![service.spec()]));
    let (main, second) = (window(&app, "main"), window(&app, "w-2"));
    let windows = [&main, &second];
    let chips: Vec<Heard> = windows
        .iter()
        .map(|w| hear(w, SERVICE_HEALTH_EVENT))
        .collect();
    let lists: Vec<Heard> = windows
        .iter()
        .map(|w| hear(w, NOTIFICATIONS_EVENT))
        .collect();
    // The last thing a recovery does: the second window (last in label order) hears its emptied list.
    let (back_tx, back_rx) = mpsc::channel();
    second.listen(NOTIFICATIONS_EVENT, move |e| {
        let list: Value = serde_json::from_str(e.payload()).unwrap();
        if list["notices"].as_array().is_some_and(Vec::is_empty) {
            let _ = back_tx.send(());
        }
    });

    // The engine dies; every call that meets it reports the same thing.
    for _ in 0..3 {
        service.died(&health);
    }

    for (chip, list) in chips.iter().zip(&lists) {
        let chip = take_n(chip, 1);
        assert_eq!(chip.len(), 1, "one chip update per window: {chip:?}");
        assert_eq!(chip[0]["services"][0]["state"], "down");
        assert_eq!(
            chip[0]["services"][0]["cause"]["message"],
            "The engine stopped."
        );
        let list = take_n(list, 1);
        assert_eq!(list.len(), 1, "one notification event per window: {list:?}");
        assert_eq!(list[0]["posted"]["count"], 1);
        assert_eq!(list[0]["unread"], 1);
    }
    let ids: Vec<Value> = windows
        .iter()
        .map(|w| {
            let list = invoke(w, "notifications_list", json!({})).unwrap();
            assert_eq!(list["notices"].as_array().unwrap().len(), 1, "{list}");
            let n = &list["notices"][0];
            assert_eq!(n["cause"], "health:engine");
            assert_eq!(n["severity"], "error");
            assert_eq!(n["source"], "health");
            assert_eq!(n["code"], "engine.stopped");
            assert_eq!(
                n["primary"],
                json!({"type": "repairService", "service": "engine"})
            );
            n["id"].clone()
        })
        .collect();
    assert_eq!(ids[0], ids[1], "the same notification in both windows");
    let snap = invoke(&second, "health_snapshot", json!({})).unwrap();
    assert_eq!(snap["services"][0]["repair"], "restart");

    // Repair from the second window: the shell runs it, the chip goes back to ok and the notification is resolved.
    let started = invoke(&second, "health_repair", json!({"service": "engine"})).unwrap();
    assert_eq!(started["services"][0]["state"], "repairing");
    back_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the repair ends");
    assert!(service.running.load(Ordering::SeqCst));
    assert_eq!(service.repairs.load(Ordering::SeqCst), 1);
    for (chip, list) in chips.iter().zip(&lists) {
        let states: Vec<Value> = take_n(chip, 2)
            .iter()
            .map(|s| s["services"][0]["state"].clone())
            .collect();
        assert_eq!(states, [json!("repairing"), json!("ok")]);
        let list = take_n(list, 1);
        assert_eq!(list.len(), 1, "the resolution, once: {list:?}");
        assert_eq!(
            (list[0]["unread"].as_u64(), &list[0]["posted"]),
            (Some(0), &Value::Null)
        );
    }
    for w in windows {
        assert!(
            invoke(w, "notifications_list", json!({})).unwrap()["notices"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    use crate::tests::logged;
    assert!(logged(log::Level::Info, "Service health changed"));
    assert!(logged(log::Level::Info, "Repair started"));
    assert!(logged(log::Level::Info, "Repair finished"));
}

#[test]
fn a_degraded_service_updates_its_one_message_in_place() {
    let (app, health) = app_with(Setup::new(vec![ServiceSpec::new("llm", "the model")]));
    let main = window(&app, "main");
    health.down("llm", Cause::new("Stopped."), None);
    health.set(
        "llm",
        ServiceState::Degraded,
        Some(Cause::new("Slow.")),
        None,
    );
    let list = invoke(&main, "notifications_list", json!({})).unwrap();
    let n = &list["notices"][0];
    assert_eq!(
        (
            n["count"].as_u64(),
            n["severity"].as_str(),
            n["title"].as_str()
        ),
        (Some(2), Some("warning"), Some("Slow."))
    );
    assert!(n["primary"].is_null(), "nothing repairs it");
    // A repair that is refused is the command's error.
    let e = invoke(&main, "health_repair", json!({"service": "llm"})).unwrap_err();
    assert_eq!(
        (e["code"].as_str(), e["retryable"].as_bool()),
        (Some("refused"), Some(false))
    );
    assert_eq!(
        invoke(&main, "health_repair", json!({"service": "nope"})).unwrap_err()["code"],
        "invalid"
    );
    let e = invoke(&main, "health_repair", json!({"service": "x".repeat(100)})).unwrap_err();
    assert_eq!(e["message"], "that is not a service id");
}

#[test]
fn dismiss_many_and_all_remove_what_the_window_sees_and_count_it() {
    let (app, _) = app_with(Setup::new(vec![]).clock(Arc::new(FakeClock::at_wall_ms(1_000))));
    let main = window(&app, "main");
    let other = window(&app, "w-2");
    let mk = |cause: &str, scope| {
        NewNotice::new(cause, "liveAlert", NoticeSeverity::Error, cause, scope)
    };
    let a = post(
        app.handle(),
        Some("main"),
        mk("live:a", NoticeScope::Project),
        false,
    )
    .unwrap();
    let b = post(
        app.handle(),
        Some("main"),
        mk("live:b", NoticeScope::Project),
        false,
    )
    .unwrap();
    let c = post(
        app.handle(),
        Some("w-2"),
        mk("live:c", NoticeScope::Project),
        false,
    )
    .unwrap();
    let count = |w: &_| {
        invoke(w, "notifications_list", json!({})).unwrap()["notices"]
            .as_array()
            .unwrap()
            .len()
    };
    assert_eq!(count(&main), 2);
    // An empty selection, an unknown id and another window's id remove nothing.
    for ids in [json!([]), json!([987_654]), json!([c.id.0])] {
        assert_eq!(
            invoke(&main, "notifications_dismiss_many", json!({"ids": ids})).unwrap(),
            0
        );
    }
    assert_eq!(count(&main), 2);
    assert_eq!(count(&other), 1);
    assert_eq!(
        invoke(
            &main,
            "notifications_dismiss_many",
            json!({"ids": [a.id.0, a.id.0, 987_654]})
        )
        .unwrap(),
        1
    );
    assert_eq!(count(&main), 1);
    let too_many: Vec<u64> = (0..1001).collect();
    let e = invoke(
        &main,
        "notifications_dismiss_many",
        json!({"ids": too_many}),
    )
    .unwrap_err();
    assert_eq!(e["code"], "invalid");
    assert_eq!(
        invoke(&main, "notifications_dismiss_all", json!({})).unwrap(),
        1
    );
    assert_eq!(
        invoke(&main, "notifications_dismiss_all", json!({})).unwrap(),
        0
    );
    assert_eq!(count(&main), 0);
    assert_eq!(count(&other), 1, "another window's messages stay");
    let _ = b;
}

#[test]
fn commands_list_mark_dismiss_and_restore_through_ipc_and_dialogs_see_their_host() {
    let (app, _) = app_with(Setup::new(vec![]).clock(Arc::new(FakeClock::at_wall_ms(1_000))));
    let main = window(&app, "main");
    let dialog = window(&app, "d-main-1");
    let popout = window(&app, "p-main-1");
    let other = window(&app, "w-2");
    let page = window(&app, "browser-page");
    let dialog_heard = hear(&dialog, NOTIFICATIONS_EVENT);
    let other_heard = hear(&other, NOTIFICATIONS_EVENT);
    let page_heard = hear(&page, NOTIFICATIONS_EVENT);
    let alert = || {
        NewNotice::new(
            "live:spike",
            "liveAlert",
            NoticeSeverity::Error,
            "ERROR spike",
            NoticeScope::Project,
        )
    };
    let first = post(app.handle(), Some("p-main-1"), alert(), false).unwrap();
    assert_eq!(
        first.link.window.as_deref(),
        Some("main"),
        "a pop-out posts into its host's list"
    );
    post(app.handle(), Some("main"), alert(), false).unwrap();
    assert_eq!(
        take(&dialog_heard).last().unwrap()["posted"]["count"],
        2,
        "the toast previews the merged message"
    );
    assert!(
        take(&other_heard).is_empty(),
        "another window's project message is not this window's"
    );
    assert!(
        take(&page_heard).is_empty(),
        "a page outside the label scheme hears nothing"
    );
    let list = invoke(&dialog, "notifications_list", json!({})).unwrap();
    assert_eq!(
        (
            list["unread"].as_u64(),
            list["notices"].as_array().unwrap().len()
        ),
        (Some(1), 1)
    );
    assert!(
        invoke(&other, "notifications_list", json!({})).unwrap()["notices"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        invoke(
            &popout,
            "notifications_mark_read",
            json!({"id": first.id.0})
        )
        .unwrap(),
        true
    );
    assert_eq!(
        invoke(&main, "notifications_mark_all_read", json!({})).unwrap(),
        0
    );
    let record = invoke(&main, "notifications_record", json!({})).unwrap();
    assert_eq!(record["notices"][0]["read"], true);
    let restored = invoke(
        &main,
        "notifications_restore",
        json!({"record": {"version": 7, "notices": []}}),
    )
    .unwrap();
    assert!(restored["warning"].as_str().unwrap().contains("newer"));
    take(&other_heard);
    invoke(&main, "notifications_restore", json!({"record": record})).unwrap();
    assert!(
        take(&other_heard).is_empty(),
        "a restore is the window's own"
    );
    let id = invoke(&main, "notifications_list", json!({})).unwrap()["notices"][0]["id"]
        .as_u64()
        .unwrap();
    assert_eq!(
        invoke(&dialog, "notifications_dismiss", json!({"id": id})).unwrap(),
        true
    );
    assert_eq!(
        invoke(&dialog, "notifications_dismiss", json!({"id": id})).unwrap(),
        false
    );
    assert!(
        invoke(&main, "notifications_list", json!({})).unwrap()["notices"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // A quiet message is listed without a toast; a `once` message posted again tells nobody.
    take(&other_heard);
    let offer = || {
        NewNotice::new(
            "update:2",
            "update",
            NoticeSeverity::Info,
            "Version 2",
            NoticeScope::App,
        )
        .once()
        .quiet()
    };
    post(app.handle(), None, offer(), false).unwrap();
    let heard = take(&other_heard);
    assert_eq!((heard.len(), &heard[0]["posted"]), (1, &Value::Null));
    post(app.handle(), None, offer(), false).unwrap();
    assert!(take(&other_heard).is_empty());
    // A closed window's project messages go with it.
    post(app.handle(), Some("w-2"), alert(), false).unwrap();
    forget_window(app.handle(), "w-2");
    assert_eq!(
        invoke(&other, "notifications_list", json!({})).unwrap()["notices"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "only the app's"
    );
    assert!(flush(app.handle()));
}

#[test]
fn os_notifications_go_through_the_apps_hook_and_a_click_opens_the_message() {
    let shown = Arc::new(Mutex::new(Vec::<String>::new()));
    let s = shown.clone();
    let (app, health) = app_with(
        Setup::new(vec![ServiceSpec::new("engine", "the engine")]).os_notifications(
            move |_app, n| {
                s.lock().unwrap().push(n.title.clone());
                n.severity == NoticeSeverity::Error
            },
        ),
    );
    let main = window(&app, "main");
    let second = window(&app, "w-2");
    let opened_main = hear(&main, OPEN_EVENT);
    let opened_second = hear(&second, OPEN_EVENT);
    // An outage always asks for an OS notification.
    health.down("engine", Cause::new("The engine stopped."), None);
    assert_eq!(*shown.lock().unwrap(), ["The engine stopped."]);
    on_focus(app.handle(), "w-2");
    assert_eq!(
        take(&opened_second).len(),
        1,
        "an app message opens in the window that got the focus"
    );
    on_focus(app.handle(), "w-2");
    assert!(take(&opened_second).is_empty(), "once");
    // A project message opens in its own window.
    let failed = || {
        NewNotice::new(
            "job:export",
            "job",
            NoticeSeverity::Error,
            "Export failed",
            NoticeScope::Project,
        )
    };
    let n = post(app.handle(), Some("main"), failed(), true).unwrap();
    on_focus(app.handle(), "w-2");
    assert_eq!(take(&opened_main), [json!(n.id.0)]);
    // The hook said it did not show one: a focus is an ordinary focus; and no `os` asks nothing.
    post(
        app.handle(),
        None,
        NewNotice::new("i", "x", NoticeSeverity::Info, "Info", NoticeScope::App),
        true,
    );
    on_focus(app.handle(), "main");
    assert!(take(&opened_main).is_empty());
    post(
        app.handle(),
        None,
        NewNotice::new("j", "x", NoticeSeverity::Error, "Quiet", NoticeScope::App),
        false,
    );
    assert_eq!(shown.lock().unwrap().len(), 3);
}

#[test]
fn without_init_producers_are_not_affected_and_commands_say_not_ready() {
    let app = bare_app();
    let main = window(&app, "main");
    let n = NewNotice::new("a", "x", NoticeSeverity::Info, "a", NoticeScope::App);
    assert!(post(app.handle(), None, n, true).is_none());
    resolve_cause(app.handle(), "a");
    on_focus(app.handle(), "main");
    forget_window(app.handle(), "main");
    assert!(flush(app.handle()));
    for (cmd, what) in [
        ("health_snapshot", "Service health"),
        ("notifications_list", "The notification centre"),
    ] {
        let e = invoke(&main, cmd, json!({})).unwrap_err();
        assert_eq!(
            e,
            json!({"code": "unavailable", "message": format!("{what} is not ready yet"), "retryable": true})
        );
    }
    assert_eq!(
        invoke(&main, "health_repair", json!({"service": "a"})).unwrap_err()["code"],
        "unavailable"
    );
    for cmd in ["notifications_mark_read", "notifications_dismiss"] {
        assert!(invoke(&main, cmd, json!({"id": 1})).is_err());
    }
    for cmd in [
        "notifications_mark_all_read",
        "notifications_record",
        "notifications_restore",
    ] {
        assert!(invoke(&main, cmd, json!({})).is_err());
    }
}

#[test]
fn a_bad_service_list_is_refused_and_the_app_file_is_used() {
    let app = bare_app();
    let e = init(
        app.handle(),
        Setup::new(vec![ServiceSpec::new("a", "x"), ServiceSpec::new("a", "y")]),
    );
    assert!(matches!(e, Err(SpecError::Duplicate(_))));
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("data/notifications.json");
    let (app, _) = app_with(
        Setup::new(vec![])
            .notifications_file(&file)
            .caps(Caps { app: 1, project: 1 }),
    );
    for cause in ["a", "b"] {
        post(
            app.handle(),
            None,
            NewNotice::new(cause, "x", NoticeSeverity::Info, cause, NoticeScope::App),
            false,
        );
    }
    assert!(flush(app.handle()));
    let saved = crate::notify::persist::load_file(&file);
    assert_eq!(
        saved.iter().map(|n| n.cause.as_str()).collect::<Vec<_>>(),
        ["b"],
        "capped at one"
    );
    assert_eq!(NoticeAction::repair_service("a").kind, "repairService");
}

#[test]
fn every_command_is_async_and_in_the_table_and_its_names_are_generated() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    paddock_commands::testing::assert_async(&COMMANDS, &[&src]);
    let regenerate =
        "PADDOCK_WRITE_TS=1 cargo test -p paddock-health --all-features --lib generated";
    let text = paddock_commands::ts::names_module(
        &COMMANDS,
        "HEALTH_COMMANDS",
        "HealthCommand",
        regenerate,
    )
    .unwrap();
    crate::tests::sync("health-commands.ts", &text);
    assert_eq!(
        COMMANDS.kind_of("notifications_record"),
        paddock_commands::Kind::Quiet
    );
    assert_eq!(
        COMMANDS.spec("notifications_restore").map(|s| s.limit()),
        Some(4 * paddock_commands::MIB)
    );
}

#[test]
fn a_failed_send_is_logged_and_nothing_else_happens() {
    crate::tests::capture_logs();
    warn_on_err(Ok(()), "never logged");
    warn_on_err(
        Err(::tauri::Error::WindowNotFound),
        "The event could not be sent",
    );
    assert!(crate::tests::logged(
        log::Level::Warn,
        "The event could not be sent: "
    ));
    assert!(!crate::tests::logged(log::Level::Warn, "never logged"));
}

#[test]
fn a_window_notice_reaches_the_centre_and_every_window_once_per_cause() {
    let (app, _health) = app_with(Setup::new(vec![]));
    let (main, second, dialog) = (
        window(&app, "main"),
        window(&app, "w-2"),
        window(&app, "d-main-1"),
    );
    let lists: Vec<Heard> = [&main, &second]
        .iter()
        .map(|w| hear(w, NOTIFICATIONS_EVENT))
        .collect();
    let notice =
        json!({ "notice": { "severity": "error", "title": "The app context failed to load." } });
    let posted = invoke(&second, "notifications_post", notice.clone()).unwrap();
    assert_eq!(posted["cause"], "ui:error:The app context failed to load.");
    assert_eq!(
        (
            posted["scope"].as_str(),
            posted["source"].as_str(),
            posted["count"].as_u64()
        ),
        (Some("app"), Some("app"), Some(1))
    );
    // The same notice again is the same message, counted.
    let again = invoke(&main, "notifications_post", notice).unwrap();
    assert_eq!(
        (again["id"].clone(), again["count"].as_u64()),
        (posted["id"].clone(), Some(2))
    );
    for list in &lists {
        let heard = take(list);
        assert_eq!(heard.len(), 2, "every window hears both posts: {heard:?}");
        assert_eq!(
            heard[1]["posted"]["count"], 2,
            "with the message for its toast"
        );
    }
    // Every window lists it, a dialog through its host.
    for w in [&main, &second, &dialog] {
        let list = invoke(w, "notifications_list", json!({})).unwrap();
        assert_eq!(
            list["notices"][0]["cause"],
            "ui:error:The app context failed to load.",
            "{}",
            w.label()
        );
    }
}

#[test]
fn a_window_scoped_notice_stays_with_its_window_and_a_bad_one_is_refused() {
    let (app, _health) = app_with(Setup::new(vec![]));
    let (main, second) = (window(&app, "main"), window(&app, "w-2"));
    let heard = hear(&second, NOTIFICATIONS_EVENT);
    let posted = invoke(
        &main,
        "notifications_post",
        json!({ "notice": { "severity": "info", "title": "Saved", "kind": "sd.save", "scope": "window" } }),
    )
    .unwrap();
    assert_eq!(
        (posted["scope"].as_str(), posted["source"].as_str()),
        (Some("project"), Some("sd.save"))
    );
    assert!(take(&heard).is_empty(), "the other window hears nothing");
    let other = invoke(&second, "notifications_list", json!({})).unwrap();
    assert_eq!(other["notices"].as_array().map(Vec::len), Some(0));
    for bad in [
        json!({ "notice": { "severity": "critical", "title": "x" } }),
        json!({ "notice": { "severity": "info", "title": "x", "kind": "health" } }),
        json!({ "notice": { "severity": "info", "title": "" } }),
    ] {
        let e = invoke(&main, "notifications_post", bad.clone()).unwrap_err();
        assert_eq!(e["code"], "invalid", "{bad}: {e}");
    }
    let unknown = invoke(
        &main,
        "notifications_post",
        json!({ "notice": { "severity": "info", "title": "x", "cause": "health:a" } }),
    );
    assert!(
        unknown.is_err(),
        "a cause of its own is not part of the contract"
    );
    let huge =
        json!({ "notice": { "severity": "info", "title": "x", "body": "y".repeat(20 * 1024) } });
    assert_eq!(
        invoke(&main, "notifications_post", huge).unwrap_err()["code"],
        "limit"
    );
}

#[test]
fn a_window_notice_without_the_centre_is_unavailable() {
    let app = bare_app();
    let main = window(&app, "main");
    let e = invoke(
        &main,
        "notifications_post",
        json!({ "notice": { "severity": "info", "title": "x" } }),
    )
    .unwrap_err();
    assert_eq!(
        (e["code"].as_str(), e["retryable"].as_bool()),
        (Some("unavailable"), Some(true))
    );
}
