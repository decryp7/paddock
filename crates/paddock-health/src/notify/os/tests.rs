//! The policy table, the limits on a fake clock, quiet hours across midnight, toasts and clicks: no real notification.

use super::*;
use crate::FakeClock;
use crate::notify::centre::{
    Centre, NewNotice, NoticeAction, NoticeId, NoticeScope, NoticeSeverity,
};

const MIN: i64 = 60_000;

fn notice_with(
    id: u64,
    cause: &str,
    severity: NoticeSeverity,
    f: impl FnOnce(NewNotice) -> NewNotice,
) -> Notice {
    let new = f(NewNotice::new(
        cause,
        "job",
        severity,
        "Title",
        NoticeScope::App,
    ));
    Centre::new(10).post(new, NoticeId(id), 1_000).notice
}

fn notice(id: u64, cause: &str, severity: NoticeSeverity) -> Notice {
    notice_with(id, cause, severity, |n| n)
}

/// The hour of the (UTC) day the manual clock is at; the zone is UTC.
fn at_hour(h: i64, m: i64) -> i64 {
    h * 60 * MIN + m * MIN
}

struct Rig {
    clock: FakeClock,
    toaster: Arc<FakeToaster>,
    settings: SharedOsSettings,
    notifier: OsNotifier,
}

fn rig(settings: OsSettings, start_ms: i64) -> Rig {
    let clock = FakeClock::at_wall_ms(start_ms);
    let toaster = Arc::new(FakeToaster::default());
    let settings = SharedOsSettings::new(settings);
    let notifier = OsNotifier::new(settings.clone(), toaster.clone())
        .with_time(Arc::new(clock.clone()), Arc::new(FixedOffset(0)));
    Rig {
        clock,
        toaster,
        settings,
        notifier,
    }
}

fn t(s: &str) -> ClockTime {
    ClockTime::try_from(s.to_string()).unwrap()
}

#[test]
fn the_policy_table_level_by_severity_seen_and_quiet() {
    use NoticeSeverity::*;
    use OsLevel::*;
    let noon = t("12:00");
    let table = [
        // level, severity, seen, expected
        (Off, Critical, false, Err(Skip::Off)),
        (Alerts, Info, false, Err(Skip::NotAnAlert)),
        (Alerts, Success, false, Err(Skip::NotAnAlert)),
        (Alerts, Warning, false, Ok(())),
        (Alerts, Error, false, Ok(())),
        (Alerts, Critical, false, Ok(())),
        (All, Info, false, Ok(())),
        (All, Success, false, Ok(())),
        (All, Error, true, Err(Skip::Seen)),
        (Alerts, Critical, true, Err(Skip::Seen)),
    ];
    for (level, severity, seen, expected) in table {
        let s = OsSettings {
            level,
            ..OsSettings::default()
        };
        assert_eq!(
            screen(&s, severity, seen, noon),
            expected,
            "{level:?} {severity:?} seen={seen}"
        );
    }
}

#[test]
fn quiet_hours_hold_everything_but_a_critical_failure_unless_the_user_says_otherwise() {
    use NoticeSeverity::*;
    let night = QuietHours {
        start: t("22:00"),
        end: t("07:00"),
    };
    let s = OsSettings {
        level: OsLevel::All,
        quiet_hours: Some(night),
        ..OsSettings::default()
    };
    assert_eq!(screen(&s, Error, false, t("23:00")), Err(Skip::QuietHours));
    assert_eq!(screen(&s, Info, false, t("03:00")), Err(Skip::QuietHours));
    assert_eq!(
        screen(&s, Critical, false, t("03:00")),
        Ok(()),
        "a stopped service still notifies"
    );
    assert_eq!(
        screen(&s, Error, false, t("07:00")),
        Ok(()),
        "the end is exclusive"
    );
    assert_eq!(screen(&s, Error, false, t("21:59")), Ok(()));
    let strict = OsSettings {
        critical_in_quiet_hours: false,
        ..s
    };
    assert_eq!(
        screen(&strict, Critical, false, t("03:00")),
        Err(Skip::QuietHours)
    );
}

#[test]
fn seen_means_a_focused_visible_window_of_that_host() {
    let w = |host: &str, focused, visible| WindowPresence {
        host: host.into(),
        focused,
        visible,
    };
    let windows = [w("main", true, true), w("w-2", false, true)];
    assert!(user_sees(Some("main"), &windows));
    assert!(
        !user_sees(Some("w-2"), &windows),
        "visible but not in front"
    );
    assert!(
        user_sees(None, &windows),
        "an app-wide message is seen in any window"
    );
    assert!(
        !user_sees(Some("main"), &[w("main", true, false)]),
        "minimised"
    );
    assert!(!user_sees(Some("main"), &[w("main", false, true)]));
    assert!(!user_sees(None, &[]), "no windows at all");
}

#[test]
fn a_shown_toast_carries_the_title_body_count_and_labelled_buttons() {
    let r = rig(OsSettings::default(), 0);
    let n = notice_with(7, "health:engine", NoticeSeverity::Error, |n| {
        n.body("The engine\nstopped.\t Choose Repair.")
            .primary(NoticeAction::repair_service("engine"))
            .secondary(NoticeAction::open_settings("general"))
    });
    let Outcome::Shown(toast) = r.notifier.offer(&n, false) else {
        panic!("shown")
    };
    assert_eq!(toast, r.toaster.shown()[0]);
    assert_eq!(
        (toast.notice, toast.cause.as_str(), toast.title.as_str()),
        (NoticeId(7), "health:engine", "Title")
    );
    assert_eq!(toast.body, "The engine stopped.  Choose Repair.");
    assert_eq!(
        toast.buttons,
        [
            ToastButton {
                slot: Slot::Primary,
                label: "Repair".into()
            },
            ToastButton {
                slot: Slot::Secondary,
                label: "Open settings".into()
            }
        ]
    );
    assert_eq!(
        toast.action_type().as_deref(),
        Some("pd|primary:Repair|secondary:Open settings")
    );
}

#[test]
fn text_is_bounded_a_repeat_shows_its_count_and_unlabelled_actions_have_no_button() {
    let long = "x".repeat(1000);
    let mut n = notice_with(1, "c", NoticeSeverity::Error, |n| {
        n.body(long.clone())
            .primary(NoticeAction::new("viewJob"))
            .tertiary(NoticeAction::open_settings("a"))
    });
    n.title = "T".repeat(500);
    n.count = 4;
    let toast = toast_for(&n, &ActionLabels::default());
    assert!(
        toast.title.chars().count() <= MAX_TOAST_TITLE && toast.title.ends_with("(4)"),
        "{}",
        toast.title
    );
    assert!(toast.title.contains('\u{2026}'));
    assert_eq!(toast.body.chars().count(), MAX_TOAST_BODY);
    assert!(toast.body.ends_with('\u{2026}'));
    assert_eq!(
        toast.buttons,
        [ToastButton {
            slot: Slot::Tertiary,
            label: "Open settings".into()
        }],
        "viewJob has no label"
    );
    let labelled = toast_for(
        &n,
        &ActionLabels::default()
            .label("viewJob", "View job")
            .label("blank", "  "),
    );
    assert_eq!(labelled.buttons.len(), 2);
    assert!(
        toast_for(&n, &ActionLabels::empty())
            .action_type()
            .is_none(),
        "no buttons: no action type"
    );
    // Control characters never reach the OS.
    n.title = "a\u{7}b\u{1b}[31mc".into();
    n.count = 1;
    assert_eq!(toast_for(&n, &ActionLabels::empty()).title, "ab[31mc");
}

#[test]
fn one_hundred_posts_of_one_cause_give_one_toast_and_one_hundred_causes_give_five() {
    let r = rig(
        OsSettings {
            level: OsLevel::All,
            ..OsSettings::default()
        },
        0,
    );
    for i in 0..100 {
        r.clock.advance_ms(100);
        r.notifier
            .offer(&notice(i, "job:1", NoticeSeverity::Error), false);
    }
    assert_eq!(r.toaster.shown().len(), 1);
    for i in 0..100 {
        r.clock.advance_ms(100);
        r.notifier.offer(
            &notice(i, &format!("job:other{i}"), NoticeSeverity::Error),
            false,
        );
    }
    assert_eq!(
        r.toaster.shown().len(),
        5,
        "the burst limit holds: 1 above + 4 more"
    );
    // The window passes, and the first cause may speak again.
    r.clock.advance_ms(301_000);
    assert!(
        r.notifier
            .offer(&notice(1, "job:1", NoticeSeverity::Error), false)
            .shown()
    );
}

#[test]
fn quiet_hours_across_midnight_on_the_fake_clock_and_the_zone() {
    let night = QuietHours {
        start: t("22:00"),
        end: t("07:00"),
    };
    let s = OsSettings {
        quiet_hours: Some(night),
        cooldown_secs: 0,
        ..OsSettings::default()
    };
    let r = rig(s, at_hour(21, 59));
    let n = notice(1, "c", NoticeSeverity::Error);
    assert!(r.notifier.offer(&n, false).shown(), "21:59");
    r.clock.advance_ms(MIN as u64);
    assert_eq!(
        r.notifier.offer(&n, false),
        Outcome::Skipped(Skip::QuietHours),
        "22:00"
    );
    r.clock.advance_ms((5 * 60 * MIN) as u64);
    assert_eq!(
        r.notifier.offer(&n, false),
        Outcome::Skipped(Skip::QuietHours),
        "03:00"
    );
    r.clock.advance_ms((4 * 60 * MIN - MIN) as u64);
    assert_eq!(
        r.notifier.offer(&n, false),
        Outcome::Skipped(Skip::QuietHours),
        "06:59"
    );
    r.clock.advance_ms(MIN as u64);
    assert!(r.notifier.offer(&n, false).shown(), "07:00");
    assert_eq!(r.toaster.shown().len(), 2);
    // The same instants in another zone are outside the quiet hours.
    let zoned = OsNotifier::new(SharedOsSettings::new(s), r.toaster.clone()).with_time(
        Arc::new(FakeClock::at_wall_ms(at_hour(21, 59) + 60 * MIN)),
        Arc::new(FixedOffset(-180)),
    );
    assert!(zoned.offer(&n, false).shown(), "22:59 UTC is 19:59 at -3 h");
}

#[test]
fn a_skipped_message_does_not_charge_the_limits() {
    let r = rig(
        OsSettings {
            level: OsLevel::All,
            cooldown_secs: 60,
            burst: 1,
            ..OsSettings::default()
        },
        0,
    );
    let n = notice(1, "c", NoticeSeverity::Error);
    assert_eq!(r.notifier.offer(&n, true), Outcome::Skipped(Skip::Seen));
    assert!(
        r.notifier.offer(&n, false).shown(),
        "being seen earlier did not start a cool-down"
    );
    assert_eq!(
        r.notifier.offer(&n, false),
        Outcome::Skipped(Skip::Cooldown)
    );
    assert_eq!(
        r.notifier
            .offer(&notice(2, "d", NoticeSeverity::Error), false),
        Outcome::Skipped(Skip::Burst)
    );
}

#[test]
fn settings_changes_apply_to_the_next_message_and_off_stops_everything() {
    let r = rig(OsSettings::default(), 0);
    let n = notice(1, "c", NoticeSeverity::Info);
    assert_eq!(
        r.notifier.offer(&n, false),
        Outcome::Skipped(Skip::NotAnAlert)
    );
    r.settings.set(OsSettings {
        level: OsLevel::All,
        ..OsSettings::default()
    });
    assert!(r.notifier.offer(&n, false).shown());
    r.settings.set(OsSettings {
        level: OsLevel::Off,
        ..OsSettings::default()
    });
    assert_eq!(
        r.notifier
            .offer(&notice(2, "x", NoticeSeverity::Critical), false),
        Outcome::Skipped(Skip::Off)
    );
}

#[test]
fn a_failing_os_is_reported_and_costs_no_quota() {
    crate::tests::capture_logs();
    let r = rig(
        OsSettings {
            burst: 1,
            ..OsSettings::default()
        },
        0,
    );
    let n = notice(1, "c", NoticeSeverity::Error);
    r.toaster.fail_with(Some("notifications are blocked"));
    assert_eq!(
        r.notifier.offer(&n, false),
        Outcome::Failed(ToastError("notifications are blocked".into()))
    );
    assert!(r.toaster.shown().is_empty());
    r.toaster.fail_with(None);
    assert!(
        r.notifier.offer(&n, false).shown(),
        "the failure started neither cool-down nor burst"
    );
    assert_eq!(ToastError("x".into()).to_string(), "x");
}

#[test]
fn permission_and_subscription_go_through_the_port() {
    let r = rig(OsSettings::default(), 0);
    assert_eq!(r.notifier.toaster().permission(), Permission::Unknown);
    r.toaster.set_permission(Permission::Denied);
    assert_eq!(r.notifier.toaster().permission(), Permission::Denied);
    assert!(
        !r.toaster.click(Click {
            notice: NoticeId(1),
            target: Target::Toast
        }),
        "nobody listens yet"
    );
    let got = Arc::new(Mutex::new(Vec::new()));
    let g = got.clone();
    r.notifier
        .toaster()
        .subscribe(Arc::new(move |c| g.lock().unwrap().push(c)));
    let click = Click {
        notice: NoticeId(3),
        target: Target::Button(Slot::Secondary),
    };
    assert!(r.toaster.click(click));
    assert_eq!(*got.lock().unwrap(), [click]);
    struct Bare;
    impl Toaster for Bare {
        fn show(&self, _: &OsToast) -> Result<(), ToastError> {
            Ok(())
        }
    }
    Bare.subscribe(Arc::new(|_| {}));
    assert_eq!(Bare.permission(), Permission::Unknown);
    assert_eq!(Bare.request_permission(), Permission::Unknown);
    assert!(
        Bare.open_system_settings()
            .unwrap_err()
            .0
            .contains("notification settings")
    );
    r.toaster.set_permission_after_request(Permission::Granted);
    assert_eq!(r.toaster.request_permission(), Permission::Granted);
    r.toaster.open_system_settings().unwrap();
    assert_eq!(r.toaster.settings_opened(), 1);
}

#[test]
fn button_ids_from_the_os_are_parsed_strictly() {
    assert_eq!(Target::from_id("tap"), Some(Target::Toast));
    assert_eq!(Target::from_id("default"), Some(Target::Toast));
    assert_eq!(
        Target::from_id("primary"),
        Some(Target::Button(Slot::Primary))
    );
    assert_eq!(
        Target::from_id("tertiary"),
        Some(Target::Button(Slot::Tertiary))
    );
    for bad in ["", "dismiss", "Primary", "primary ", "../x"] {
        assert_eq!(Target::from_id(bad), None, "{bad:?}");
    }
}

#[test]
fn a_click_routes_to_open_repair_or_the_app_and_never_trusts_the_payload() {
    let n = notice_with(9, "health:engine", NoticeSeverity::Error, |n| {
        n.primary(NoticeAction::repair_service("engine"))
            .secondary(NoticeAction::open_settings("general"))
    });
    assert_eq!(route(&n, Target::Toast), Route::Open(NoticeId(9)));
    assert_eq!(
        route(&n, Target::Button(Slot::Primary)),
        Route::Repair("engine".into())
    );
    assert_eq!(
        route(&n, Target::Button(Slot::Secondary)),
        Route::Forward(NoticeAction::open_settings("general"))
    );
    assert_eq!(
        route(&n, Target::Button(Slot::Tertiary)),
        Route::Nothing,
        "the message has no third action"
    );
    for bad in [
        json_args("service", serde_json::json!("../etc")),
        json_args("service", serde_json::json!(5)),
        json_args("other", serde_json::json!("x")),
    ] {
        let mut m = n.clone();
        m.primary = Some(NoticeAction {
            kind: NoticeAction::REPAIR_SERVICE.into(),
            args: bad,
        });
        assert_eq!(route(&m, Target::Button(Slot::Primary)), Route::Nothing);
    }
}

fn json_args(k: &str, v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    m.insert(k.into(), v);
    m
}

#[test]
fn slots_name_the_three_actions() {
    for s in [Slot::Primary, Slot::Secondary, Slot::Tertiary] {
        assert_eq!(Slot::from_id(s.id()), Some(s));
    }
    assert_eq!(PAYLOAD_KEY, "paddockNotice");
    assert!(is_alert(NoticeSeverity::Warning) && !is_alert(NoticeSeverity::Success));
}

fn quiet_night() -> OsSettings {
    OsSettings {
        quiet_hours: Some(QuietHours {
            start: t("22:00"),
            end: t("07:00"),
        }),
        cooldown_secs: 30,
        ..OsSettings::default()
    }
}

#[test]
fn quiet_hours_end_with_one_digest_of_what_was_held_and_it_is_not_repeated() {
    let r = rig(quiet_night(), at_hour(23, 0));
    for i in 0..3 {
        r.notifier.offer(
            &notice(i, &format!("job:{i}"), NoticeSeverity::Error),
            false,
        );
        r.notifier.offer(
            &notice(i, &format!("job:{i}"), NoticeSeverity::Error),
            false,
        );
    }
    assert!(r.toaster.shown().is_empty());
    assert_eq!(r.notifier.pending_digest(), None, "still quiet");
    r.clock.advance_ms((7 * 60 * MIN + MIN) as u64); // 06:01 next morning... 23:00 + 7 h 1 min
    assert_eq!(
        r.notifier.pending_digest(),
        None,
        "06:01 is still inside 22:00-07:00"
    );
    r.clock.advance_ms((59 * MIN) as u64); // 07:00
    assert_eq!(
        r.notifier.pending_digest(),
        Some(3),
        "three causes, not six posts"
    );
    let digest = notice_with(50, DIGEST_CAUSE, NoticeSeverity::Info, |n| n.body("3 held"));
    assert!(r.notifier.send_digest(&digest, false).shown());
    assert_eq!(r.toaster.shown().len(), 1);
    assert_eq!(r.toaster.shown()[0].cause, DIGEST_CAUSE);
    assert_eq!(r.notifier.pending_digest(), None, "once");
}

#[test]
fn the_digest_obeys_the_limits_the_window_the_os_and_the_off_switch() {
    // Seen: dropped.
    let r = rig(quiet_night(), at_hour(23, 0));
    r.notifier
        .offer(&notice(1, "a", NoticeSeverity::Error), false);
    r.clock.advance_ms((8 * 60 * MIN) as u64);
    let digest = notice(50, DIGEST_CAUSE, NoticeSeverity::Info);
    assert_eq!(
        r.notifier.send_digest(&digest, true),
        Outcome::Skipped(Skip::Seen)
    );
    assert_eq!(r.notifier.pending_digest(), None);
    // The OS refuses: what was held stays and nothing is charged.
    let r = rig(quiet_night(), at_hour(23, 0));
    r.notifier
        .offer(&notice(1, "a", NoticeSeverity::Error), false);
    r.clock.advance_ms((8 * 60 * MIN) as u64);
    r.toaster.fail_with(Some("blocked"));
    assert!(matches!(
        r.notifier.send_digest(&digest, false),
        Outcome::Failed(_)
    ));
    assert_eq!(r.notifier.pending_digest(), Some(1));
    r.toaster.fail_with(None);
    assert!(r.notifier.send_digest(&digest, false).shown());
    // The burst limit holds it back, and it is asked again once the window slides.
    let r = rig(
        OsSettings {
            burst: 1,
            burst_window_secs: 60,
            ..quiet_night()
        },
        at_hour(22, 30),
    );
    r.notifier
        .offer(&notice(1, "a", NoticeSeverity::Error), false);
    r.clock.advance_ms(((8 * 60 + 45) * MIN) as u64);
    assert!(
        r.notifier
            .offer(&notice(2, "b", NoticeSeverity::Error), false)
            .shown()
    );
    assert_eq!(r.notifier.pending_digest(), None, "the burst is used up");
    assert_eq!(
        r.notifier.send_digest(&digest, false),
        Outcome::Skipped(Skip::Burst)
    );
    r.clock.advance_ms(61_000);
    assert_eq!(r.notifier.pending_digest(), Some(1));
    // Off forgets what was held.
    r.settings.set(OsSettings {
        level: OsLevel::Off,
        ..OsSettings::default()
    });
    assert_eq!(r.notifier.pending_digest(), None);
    r.settings.set(OsSettings::default());
    assert_eq!(
        r.notifier.pending_digest(),
        None,
        "forgotten, not just hidden"
    );
}

#[test]
fn what_quiet_hours_hold_is_bounded() {
    let r = rig(quiet_night(), at_hour(23, 0));
    for i in 0..(MAX_HELD + 50) {
        r.notifier
            .offer(&notice(1, &format!("c{i}"), NoticeSeverity::Error), false);
    }
    r.clock.advance_ms((9 * 60 * MIN) as u64);
    assert_eq!(
        r.notifier.pending_digest(),
        Some(u32::try_from(MAX_HELD).unwrap())
    );
}
