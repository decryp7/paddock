//! The OS bridge on a mock Tauri app: the shared policy decides, a fake toaster records (never a real notification), and
//! clicks played back through the toaster's sink do what the message says.

use super::*;
use crate::FakeClock;
use crate::model::{Cause, RepairAction};
use crate::notify::ACTION_EVENT;
use crate::notify::centre::{NoticeAction, NoticeSeverity};
use crate::notify::os::{
    ActionLabels, Click, FakeToaster, FixedOffset, OsLevel, OsSettings, Permission,
    SharedOsSettings, Slot, Target, Toaster,
};
use ::tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use ::tauri::{App, Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use serde_json::{Value, json};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

type Heard = Arc<Mutex<Vec<Value>>>;

struct Rig {
    app: App<MockRuntime>,
    health: Arc<Health>,
    toaster: Arc<FakeToaster>,
    clock: FakeClock,
    settings: SharedOsSettings,
    repaired: mpsc::Receiver<()>,
    handled: Arc<Mutex<Vec<String>>>,
}

fn rig(settings: OsSettings) -> Rig {
    let clock = FakeClock::at_wall_ms(1_000_000);
    let toaster = Arc::new(FakeToaster::default());
    let settings = SharedOsSettings::new(settings);
    let notifier = OsNotifier::new(settings.clone(), toaster.clone())
        .with_time(Arc::new(clock.clone()), Arc::new(FixedOffset(0)))
        .with_labels(ActionLabels::default().label("viewJob", "View job"));
    let (tx, repaired) = mpsc::channel();
    let tx = Mutex::new(tx);
    let handled: Arc<Mutex<Vec<String>>> = Arc::default();
    let (h1, h2) = (handled.clone(), handled.clone());
    let setup = Setup::new(vec![ServiceSpec::new("engine", "the engine").repair(
        move || {
            let _ = tx.lock().unwrap().send(());
            Ok(())
        },
    )])
    .clock(Arc::new(clock.clone()))
    .os_notifier(Arc::new(notifier))
    .on_notice_action("viewJob", move |_, n, a| {
        h1.lock().unwrap().push(format!("{}:{}", a.kind, n.cause));
        true
    })
    .on_notice_action("declines", move |_, _, a| {
        h2.lock().unwrap().push(format!("declined {}", a.kind));
        false
    });
    let app = mock_builder().build(mock_context(noop_assets())).unwrap();
    let health = init(app.handle(), setup).unwrap();
    Rig {
        app,
        health,
        toaster,
        clock,
        settings,
        repaired,
        handled,
    }
}

fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .build()
        .unwrap()
}

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

fn outage(r: &Rig) {
    r.health.down(
        "engine",
        Cause::new("The engine stopped.").hint("Choose Repair."),
        Some(RepairAction::Restart),
    );
}

fn error(cause: &str, title: &str) -> NewNotice {
    NewNotice::new(cause, "job", NoticeSeverity::Error, title, NoticeScope::App)
}

fn click(r: &Rig, id: NoticeId, target: Target) -> bool {
    r.toaster.click(Click { notice: id, target })
}

#[test]
fn an_outage_becomes_one_toast_with_a_repair_button_and_the_repair_runs_from_the_toast() {
    crate::tests::capture_logs();
    let r = rig(OsSettings::default());
    let main = window(&r.app, "main");
    let opened = hear(&main, OPEN_EVENT);
    outage(&r);
    let toast = r.toaster.shown().remove(0);
    assert_eq!(
        (toast.title.as_str(), toast.cause.as_str()),
        ("The engine stopped.", "health:engine")
    );
    assert_eq!(toast.buttons.len(), 1);
    assert_eq!(toast.buttons[0].label, "Repair");
    // The repair button runs the repair in the shell, not through the service, and does not steal the focus.
    assert!(click(&r, toast.notice, Target::Button(Slot::Primary)));
    r.repaired
        .recv_timeout(Duration::from_secs(10))
        .expect("the repair ran");
    assert!(take(&opened).is_empty());
    let c = centre_of(r.app.handle()).unwrap();
    assert!(
        c.find(toast.notice).is_none_or(|n| n.read),
        "the repair acknowledged the message (or it resolved)"
    );
}

#[test]
fn clicking_the_toast_opens_the_message_in_its_window_and_reads_it() {
    let r = rig(OsSettings::default());
    let (main, second) = (window(&r.app, "main"), window(&r.app, "w-2"));
    let (opened_main, opened_second) = (hear(&main, OPEN_EVENT), hear(&second, OPEN_EVENT));
    let n = post(
        r.app.handle(),
        Some("w-2"),
        error("job:export", "Export failed").body("Disk full."),
        true,
    )
    .unwrap();
    assert_eq!(r.toaster.shown().len(), 1);
    assert!(click(&r, n.id, Target::Toast));
    assert_eq!(take(&opened_second), [json!(n.id.0)]);
    assert!(take(&opened_main).is_empty());
    assert_eq!(centre_of(r.app.handle()).unwrap().snapshot("w-2").unread, 0);
    // The click answered the toast: gaining the focus afterwards does not open it a second time.
    on_focus(r.app.handle(), "main");
    assert!(take(&opened_main).is_empty() && take(&opened_second).is_empty());
}

#[test]
fn without_a_click_report_the_focus_fallback_still_opens_the_message() {
    let r = rig(OsSettings::default());
    let main = window(&r.app, "main");
    let opened = hear(&main, OPEN_EVENT);
    let n = post(r.app.handle(), None, error("job:a", "Failed"), true).unwrap();
    on_focus(r.app.handle(), "main");
    assert_eq!(take(&opened), [json!(n.id.0)]);
}

#[test]
fn buttons_without_a_shell_meaning_go_to_the_apps_handler_else_to_the_window() {
    let r = rig(OsSettings::default());
    let main = window(&r.app, "main");
    let actions = hear(&main, ACTION_EVENT);
    let notice = |cause: &str, kind: &str| {
        post(
            r.app.handle(),
            None,
            error(cause, cause).primary(NoticeAction::new(kind).arg("jobId", 3)),
            true,
        )
        .unwrap()
    };
    let view = notice("job:3", "viewJob");
    assert!(click(&r, view.id, Target::Button(Slot::Primary)));
    assert_eq!(*r.handled.lock().unwrap(), ["viewJob:job:3"]);
    assert!(take(&actions).is_empty(), "the app handled it");
    // An app handler that declines leaves it to the window, which gets the action and the message id.
    let declined = notice("job:4", "declines");
    let shown = r.toaster.shown();
    assert_eq!(shown.len(), 2);
    assert_eq!(shown[0].buttons[0].label, "View job");
    assert!(
        shown[1].buttons.is_empty(),
        "'declines' has no label, so the toast has no button for it"
    );
    assert_eq!(
        dispatch(
            r.app.handle(),
            &ActionHandlers::default(),
            Click {
                notice: declined.id,
                target: Target::Button(Slot::Primary)
            }
        ),
        Dispatched::Forwarded(declined.id)
    );
    let heard = take(&actions);
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0]["id"], json!(declined.id.0));
    assert_eq!(
        heard[0]["action"],
        json!({ "type": "declines", "jobId": 3 })
    );
    // And an app handler that returns false does the same.
    let mut handlers = ActionHandlers::<MockRuntime>::default();
    handlers.insert("declines", |_, _, _| false);
    assert_eq!(
        dispatch(
            r.app.handle(),
            &handlers,
            Click {
                notice: declined.id,
                target: Target::Button(Slot::Primary)
            }
        ),
        Dispatched::Forwarded(declined.id)
    );
}

#[test]
fn a_stale_or_hostile_click_does_nothing_harmful() {
    crate::tests::capture_logs();
    let r = rig(OsSettings::default());
    let main = window(&r.app, "main");
    let opened = hear(&main, OPEN_EVENT);
    let h = ActionHandlers::<MockRuntime>::default();
    // A message that is gone (dismissed, resolved, the app restarted).
    assert_eq!(
        dispatch(
            r.app.handle(),
            &h,
            Click {
                notice: NoticeId(999),
                target: Target::Toast
            }
        ),
        Dispatched::Nothing
    );
    // A button the message does not have.
    let n = post(r.app.handle(), None, error("c", "Plain"), false).unwrap();
    assert_eq!(
        dispatch(
            r.app.handle(),
            &h,
            Click {
                notice: n.id,
                target: Target::Button(Slot::Tertiary)
            }
        ),
        Dispatched::Nothing
    );
    // A repair of a service that works: the shell refuses, the window comes forward.
    let stale = post(
        r.app.handle(),
        None,
        error("health:engine", "Old outage").primary(NoticeAction::repair_service("engine")),
        false,
    )
    .unwrap();
    assert_eq!(
        dispatch(
            r.app.handle(),
            &h,
            Click {
                notice: stale.id,
                target: Target::Button(Slot::Primary)
            }
        ),
        Dispatched::Nothing
    );
    assert!(
        r.repaired.try_recv().is_err(),
        "no repair ran for a service that works"
    );
    // A repair of a service that is not registered.
    let ghost = post(
        r.app.handle(),
        None,
        error("g", "Ghost").primary(NoticeAction::repair_service("ghost")),
        false,
    )
    .unwrap();
    assert_eq!(
        dispatch(
            r.app.handle(),
            &h,
            Click {
                notice: ghost.id,
                target: Target::Button(Slot::Primary)
            }
        ),
        Dispatched::Nothing
    );
    assert!(take(&opened).is_empty());
    // No centre at all.
    let bare = mock_builder().build(mock_context(noop_assets())).unwrap();
    assert_eq!(
        dispatch(
            bare.handle(),
            &ActionHandlers::<MockRuntime>::default(),
            Click {
                notice: NoticeId(1),
                target: Target::Toast
            }
        ),
        Dispatched::Nothing
    );
}

#[test]
fn a_burst_of_posts_is_one_toast_per_cause_and_every_message_is_still_in_the_centre() {
    let r = rig(OsSettings::default());
    window(&r.app, "main");
    for _ in 0..100 {
        r.clock.advance_ms(10);
        post(
            r.app.handle(),
            None,
            error("job:same", "Failed again"),
            true,
        );
    }
    for i in 0..40 {
        r.clock.advance_ms(10);
        post(
            r.app.handle(),
            None,
            error(&format!("job:{i}"), "Failed"),
            true,
        );
    }
    assert_eq!(
        r.toaster.shown().len(),
        5,
        "one for the repeated cause, the burst limit for the rest"
    );
    let c = centre_of(r.app.handle()).unwrap();
    assert_eq!(
        c.snapshot("main").notices.len(),
        41,
        "skipped toasts are still listed"
    );
    assert_eq!(
        c.snapshot("main")
            .notices
            .iter()
            .find(|n| n.cause == "job:same")
            .unwrap()
            .count,
        100
    );
}

#[test]
fn settings_and_quiet_hours_apply_on_the_next_message() {
    let r = rig(OsSettings::default());
    window(&r.app, "main");
    r.settings.set(OsSettings {
        level: OsLevel::Off,
        ..OsSettings::default()
    });
    post(r.app.handle(), None, error("a", "A"), true);
    assert!(r.toaster.shown().is_empty());
    r.settings.set(OsSettings::default());
    post(r.app.handle(), None, error("b", "B"), true);
    assert_eq!(r.toaster.shown().len(), 1);
    // 00:16 UTC is inside 00:00-07:00.
    let q = crate::notify::os::QuietHours {
        start: crate::notify::os::ClockTime::new(0, 0).unwrap(),
        end: crate::notify::os::ClockTime::new(7, 0).unwrap(),
    };
    r.settings.set(OsSettings {
        quiet_hours: Some(q),
        ..OsSettings::default()
    });
    post(r.app.handle(), None, error("c", "C"), true);
    assert_eq!(r.toaster.shown().len(), 1, "held during quiet hours");
    assert_eq!(r.toaster.permission(), Permission::Unknown);
}

#[test]
fn the_hook_asks_only_posts_that_want_an_os_notification() {
    let r = rig(OsSettings::default());
    window(&r.app, "main");
    post(r.app.handle(), None, error("quiet", "Quiet"), false);
    assert!(r.toaster.shown().is_empty());
    let presence = os::presence(r.app.handle());
    assert_eq!(presence.len(), 1);
    assert_eq!(presence[0].host, "main");
}

#[test]
fn the_digest_is_toasted_once_when_quiet_hours_end_and_merged_the_next_night() {
    use crate::notify::os::{ClockTime, QuietHours};
    let q = QuietHours {
        start: ClockTime::new(0, 0).unwrap(),
        end: ClockTime::new(7, 0).unwrap(),
    };
    // The rig's clock starts at 1_000_000 ms: 00:16 UTC, inside the quiet hours.
    let r = rig(OsSettings {
        quiet_hours: Some(q),
        ..OsSettings::default()
    });
    let main = window(&r.app, "main");
    let opened = hear(&main, OPEN_EVENT);
    post(r.app.handle(), None, error("job:1", "One failed"), true);
    post(r.app.handle(), None, error("job:2", "Two failed"), true);
    assert!(r.toaster.shown().is_empty());
    assert!(!poll_digest(r.app.handle()), "still quiet");
    r.clock.advance_ms(7 * 3_600_000);
    assert!(poll_digest(r.app.handle()));
    let toast = r.toaster.shown().remove(0);
    assert_eq!(toast.title, "2 notifications held during quiet hours");
    assert!(!poll_digest(r.app.handle()), "once");
    // Its toast opens it like any other.
    assert!(click(&r, toast.notice, Target::Toast));
    assert_eq!(take(&opened), [json!(toast.notice.0)]);
    // A second night; the focus of a window also looks (mock windows are never in front, so it toasts).
    r.clock.advance_ms(17 * 3_600_000 + 40 * 60_000);
    post(r.app.handle(), None, error("job:3", "Three failed"), true);
    r.clock.advance_ms(8 * 3_600_000);
    on_focus(r.app.handle(), "main");
    assert_eq!(
        r.toaster.shown().len(),
        2,
        "the second night has its own digest"
    );
    let c = centre_of(r.app.handle()).unwrap();
    let digest = c
        .snapshot("main")
        .notices
        .into_iter()
        .find(|n| n.cause == "os:quiet-digest")
        .unwrap();
    assert_eq!(digest.count, 2, "both digests merged into the one message");
    assert_eq!(digest.title, "1 notification held during quiet hours");
}
