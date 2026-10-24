//! Tests of the managed centre, ported from Debrief's `notification_centre` tests (without its sources and actions).

use super::centre::{NoticeAction, NoticeSeverity};
use super::*;
use crate::clock::ManualClock;
use serde_json::json;

fn alert(n: u32) -> NewNotice {
    NewNotice::new(
        format!("live:spike:WEB0{n}"),
        "liveAlert",
        NoticeSeverity::Error,
        format!("ERROR spike on WEB0{n}"),
        NoticeScope::Project,
    )
}

#[test]
fn app_and_project_lists_are_kept_apart_and_saved_where_they_belong() {
    crate::tests::capture_logs();
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("notifications.json");
    let clock = Arc::new(ManualClock::default());
    let c = NotificationCentre::new(Some(file.clone()), clock.clone());
    c.post(Some("main"), alert(1));
    clock.advance(10);
    c.post(Some("main"), alert(1));
    c.post(Some("w-2"), alert(2));
    let update = c.post(
        Some("main"),
        NewNotice::new(
            "update:2.0",
            "update",
            NoticeSeverity::Info,
            "Version 2.0 is available",
            NoticeScope::App,
        )
        .primary(NoticeAction::new("installUpdate")),
    );
    c.post(
        None,
        NewNotice::new(
            "job:model",
            "job",
            NoticeSeverity::Success,
            "Model downloaded",
            NoticeScope::Project,
        ),
    );
    let main = c.snapshot("main");
    assert_eq!(
        main.notices.len(),
        3,
        "its project message and the two app messages"
    );
    assert_eq!(main.unread, 3);
    assert_eq!(
        main.notices
            .iter()
            .find(|n| n.source.as_str() == "liveAlert")
            .unwrap()
            .count,
        2
    );
    assert_eq!(c.snapshot("w-2").notices.len(), 3);
    // App messages are in the data file; project ones are not.
    assert!(c.flush());
    let saved = persist::load_file(&file);
    assert_eq!(saved.len(), 2);
    assert!(saved.iter().all(|n| n.scope == NoticeScope::App));
    assert!(
        c.mark_read("main", update.notice.id),
        "app messages are reachable from any window"
    );
    assert_eq!(c.snapshot("w-2").unread, 2);
    assert_eq!(c.mark_all_read("main"), 2);
    assert_eq!(c.mark_all_read("main"), 0);
    assert!(c.flush());
    assert!(persist::load_file(&file).iter().all(|n| n.read));
    assert!(c.dismiss("w-2", update.notice.id));
    assert!(!c.dismiss("w-2", update.notice.id));
    assert!(!c.mark_read("main", NoticeId(999)));
    assert!(c.flush());
    // A restart reads the app file back, with fresh ids after the saved ones.
    let again = NotificationCentre::new(Some(file), clock);
    assert_eq!(again.snapshot("main").notices.len(), 1);
    assert!(again.post(None, alert(3)).notice.id.0 > update.notice.id.0);
}

#[test]
fn a_once_message_posted_again_is_not_saved_or_logged_again() {
    let c = NotificationCentre::new(None, Arc::new(ManualClock::default()));
    let offer = || {
        NewNotice::new(
            "update:3",
            "update",
            NoticeSeverity::Info,
            "Version 3",
            NoticeScope::App,
        )
        .once()
    };
    assert!(!c.post(None, offer()).unchanged);
    assert!(c.post(None, offer()).unchanged);
    assert_eq!(c.snapshot("main").notices[0].count, 1);
    assert!(c.flush(), "in memory: nothing to write");
}

#[test]
fn a_resolved_cause_goes_from_every_list() {
    let c = NotificationCentre::new(None, Arc::new(ManualClock::default()));
    let health = |scope| {
        NewNotice::new(
            "health:store",
            "health",
            NoticeSeverity::Error,
            "stopped",
            scope,
        )
    };
    c.post(Some("main"), health(NoticeScope::Project));
    c.post(Some("w-2"), health(NoticeScope::Project));
    assert!(c.resolve("health:store"));
    assert!(c.snapshot("main").notices.is_empty() && c.snapshot("w-2").notices.is_empty());
    c.post(None, health(NoticeScope::App));
    assert!(c.resolve("health:store"));
    assert!(!c.resolve("health:store"), "nothing left");
}

#[test]
fn project_records_round_trip_and_damage_gives_an_empty_list() {
    let c = NotificationCentre::with_caps(
        None,
        Arc::new(ManualClock::default()),
        Caps { app: 5, project: 5 },
    );
    c.post(Some("main"), alert(1));
    assert!(
        c.mark_read("main", c.snapshot("main").notices[0].id),
        "a project message"
    );
    let record = serde_json::to_value(c.record("main")).unwrap();
    assert_eq!(record["version"], 1);
    assert_eq!(c.restore("w-2", Some(&record)), None);
    let restored = c.snapshot("w-2");
    assert_eq!(restored.notices[0].link.window.as_deref(), Some("w-2"));
    assert_ne!(
        restored.notices[0].id,
        c.snapshot("main").notices[0].id,
        "renumbered"
    );
    assert!(
        c.restore("w-2", Some(&json!({"version": "x"})))
            .unwrap()
            .contains("damaged")
    );
    assert!(c.snapshot("w-2").notices.is_empty());
    c.forget_window("main");
    assert!(c.record("main").notices.is_empty());
    assert_eq!(
        Caps::default(),
        Caps {
            app: APP_CAP,
            project: PROJECT_CAP
        }
    );
}

#[test]
fn a_clicked_os_notification_opens_its_message_once_and_only_soon_after() {
    let clock = Arc::new(ManualClock::default());
    let c = NotificationCentre::new(None, clock.clone());
    c.note_shown(NoticeId(4), Some("main"));
    assert_eq!(
        c.take_pending(),
        Some((NoticeId(4), Some("main".to_string())))
    );
    assert_eq!(c.take_pending(), None, "once");
    c.note_shown(NoticeId(5), None);
    clock.advance(OPEN_WITHIN_MS + 1);
    assert_eq!(c.take_pending(), None, "too late: an ordinary focus");
}

#[test]
fn a_file_that_cannot_be_written_is_logged_and_the_list_keeps_working() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("blocked"), "a file").unwrap();
    let c = NotificationCentre::new(
        Some(tmp.path().join("blocked/notifications.json")),
        Arc::new(ManualClock::default()),
    );
    c.post(None, alert(1));
    assert!(c.flush());
    assert_eq!(c.snapshot("main").notices.len(), 1);
}

#[test]
fn the_events_are_spelled_like_the_ui() {
    let c = NotificationCentre::new(None, Arc::new(ManualClock::default()));
    let posted = c.post(Some("main"), alert(1)).notice;
    let event = NoticesEvent {
        snapshot: c.snapshot("main"),
        posted: Some(posted),
    };
    let v = serde_json::to_value(&event).unwrap();
    assert_eq!(v["unread"], 1);
    assert_eq!(v["notices"].as_array().unwrap().len(), 1);
    assert_eq!(v["posted"]["count"], 1);
    let restored = serde_json::to_value(RestoredNotices {
        snapshot: c.snapshot("w-9"),
        warning: Some("w".into()),
    })
    .unwrap();
    assert_eq!(
        restored,
        json!({"snapshot": {"notices": [], "unread": 0}, "warning": "w"})
    );
}

#[test]
fn a_poisoned_centre_keeps_working() {
    let c = NotificationCentre::new(None, Arc::new(ManualClock::default()));
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _a = c.app.lock().unwrap();
        let _p = c.projects.lock().unwrap();
        let _o = c.pending.lock().unwrap();
        panic!("poison the centre");
    }));
    assert!(c.app.is_poisoned() && c.projects.is_poisoned() && c.pending.is_poisoned());
    c.post(Some("main"), alert(1));
    c.note_shown(NoticeId(1), None);
    assert_eq!(c.snapshot("main").notices.len(), 1);
    assert!(c.take_pending().is_some());
}
