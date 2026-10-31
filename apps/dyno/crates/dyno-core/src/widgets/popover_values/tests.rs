//! Every popup kind's value builder for ready, empty, error, working and stale, from synthetic inputs (no clock, no OS).

use super::*;
use crate::grouping::AppGroup;
use crate::metrics as dyno_core_reading;
use crate::metrics::fixtures::sample;
use crate::process::Measured;
use crate::time::{FixedZone, HOUR, MINUTE};
use paddock_widgets::WidgetState as S;
use serde_json::json;

const NOW: Timestamp = Timestamp(1_800_000_000_000);

fn id(s: &str) -> PlacementId {
    PlacementId::new(s).unwrap()
}

fn alarm(n: u64, severity: &str, state: &str, last_seen_ago: i64) -> Alarm {
    serde_json::from_value(json!({
        "id": n, "rule": "r", "key": format!("k{n}"), "severity": severity, "state": state,
        "title": format!("Alarm {n}"), "detail": "", "firstSeen": NOW.0 - 2 * HOUR, "lastSeen": NOW.0 - last_seen_ago,
        "repeatCount": 1, "currentValue": null, "metric": null, "notes": [], "acknowledgedAt": null, "clearedAt": null,
        "shelved": null, "muted": false, "chattering": false, "expiresAt": null
    }))
    .unwrap()
}

fn volume(mount: &str, name: &str, location: serde_json::Value, capacity: Option<u64>, free: Option<u64>, reachable: bool) -> Volume {
    serde_json::from_value(json!({
        "id": mount, "name": name, "mount": mount, "fs": "apfs", "location": location, "capacity": capacity, "free": free,
        "rotational": null, "readOnly": false, "reachable": reachable, "wholeVolume": true
    }))
    .unwrap()
}

fn live() -> LiveUpdate {
    LiveUpdate {
        system: sample(NOW.0),
        pressure: crate::metrics::MemoryPressure::Normal,
        top_cpu: ["Chrome", "Xcode", "Slack", "Mail"]
            .iter()
            .enumerate()
            .map(|(i, n)| AppGroup {
                id: (*n).into(),
                name: (*n).into(),
                exe: None,
                pids: vec![],
                cpu: if i == 2 {
                    Measured::Unavailable {
                        reason: crate::process::UnavailableReason::NotPermitted,
                    }
                } else {
                    Measured::Value(40.0 - i as f32)
                },
                memory: Measured::Value(1),
                disk_bps: Measured::Value(0.0),
                partial: false,
            })
            .collect(),
        top_memory: vec![],
        overhead: Default::default(),
        processes: None,
        groups: None,
        other_users: None,
    }
}

static NO_MINUTE: Minute = Minute {
    cpu: std::collections::VecDeque::new(),
    rx: std::collections::VecDeque::new(),
    tx: std::collections::VecDeque::new(),
};

fn run<'a>(widget: &PopoverWidget, f: impl FnOnce(&mut Inputs<'a>)) -> Option<State> {
    let mut inputs = Inputs::empty(NOW, &NO_MINUTE);
    f(&mut inputs);
    state(widget, &id("x"), &inputs)
}

fn value(s: Option<State>) -> PopoverValue {
    match s {
        Some(S::Ready { value, as_of, stale_since }) => {
            assert_eq!(as_of, NOW.0);
            assert_eq!(stale_since, None);
            value
        }
        other => panic!("a ready value, got {other:?}"),
    }
}

#[test]
fn a_source_that_has_not_answered_leaves_the_widget_loading() {
    for w in [
        PopoverWidget::ActiveAlarms,
        PopoverWidget::disk(VolumeChoice::All),
        PopoverWidget::CpuMemory,
        PopoverWidget::Network,
        PopoverWidget::BackupHealth,
        PopoverWidget::LastIssue,
        PopoverWidget::PinnedCounters,
        PopoverWidget::TopApps,
    ] {
        assert_eq!(run(&w, |_| {}), None, "{w:?}");
    }
}

// ---- active alarms ----

#[test]
fn alarms_are_the_most_severe_then_newest_two_rows_and_a_count() {
    let list = vec![
        alarm(1, "warning", "active", 5 * MINUTE),
        alarm(2, "critical", "acknowledged", 30 * MINUTE),
        alarm(3, "warning", "active", MINUTE),
        alarm(4, "info", "active", 0),
        alarm(5, "critical", "cleared", 0),
        alarm(6, "critical", "clearedUnacknowledged", 0),
    ];
    let PopoverValue::ActiveAlarms { rows, more } = value(run(&PopoverWidget::ActiveAlarms, |i| i.alarms = Source::Ready(&list))) else {
        panic!()
    };
    let ids: Vec<u64> = rows.iter().map(|r| r.id).collect();
    assert_eq!(ids, [2, 3]);
    assert_eq!(more, 2);
    assert!(rows[0].acknowledged && !rows[1].acknowledged);
    assert_eq!(rows[0].level, Level::Critical);
}

#[test]
fn a_shelved_alarm_is_not_active_and_no_alarms_is_a_calm_empty_that_opens_alarms() {
    let mut shelved = alarm(1, "critical", "active", 0);
    shelved.shelved = Some(crate::alarms::Shelf::Until(NOW.plus(HOUR)));
    let list = vec![shelved];
    let s = run(&PopoverWidget::ActiveAlarms, |i| i.alarms = Source::Ready(&list)).unwrap();
    let S::Empty { title, next, .. } = s else { panic!("{s:?}") };
    assert_eq!(title, "No active alarms");
    assert_eq!(next.unwrap().action, WidgetActionId::OpenPage(Page::Alarms));
}

#[test]
fn an_alarm_store_that_fails_is_an_error_with_retry_for_this_placement() {
    let minute = Minute::default();
    let mut inputs = Inputs::empty(NOW, &minute);
    inputs.alarms = Source::Failed("locked".into());
    let s = state(&PopoverWidget::ActiveAlarms, &id("active-alarms"), &inputs).unwrap();
    let S::Error { title, cause, next } = s else { panic!("{s:?}") };
    assert_eq!(title, "Couldn't read alarms");
    assert_eq!(cause.as_deref(), Some("The alarm store didn't answer."));
    assert_eq!(next.unwrap().action, WidgetActionId::Retry(id("active-alarms")));
}

// ---- disk space ----

fn volumes() -> Vec<Volume> {
    let ext = json!({"external": {"bus": "USB"}});
    let net = json!({"network": {"protocol": "smb", "host": "nas"}});
    vec![
        volume("/Volumes/NAS", "NAS", net, Some(1000), Some(500), false),
        volume("/Volumes/Backup", "Backup", ext, Some(1000), Some(30), true),
        volume("/", "Macintosh HD", json!("internal"), Some(1000), Some(400), true),
        volume("/Volumes/Data", "Data", json!("internal"), Some(2000), Some(1500), true),
        volume("/Volumes/Odd", "Odd", json!("internal"), None, None, true),
    ]
}

#[test]
fn all_volumes_show_the_system_one_first_three_rows_and_a_count_with_tones() {
    let v = volumes();
    let PopoverValue::DiskSpace { rows, more } = value(run(&PopoverWidget::disk(VolumeChoice::All), |i| i.volumes = Source::Ready(&v)))
    else {
        panic!()
    };
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Macintosh HD", "Data", "Odd"]);
    assert_eq!(more, 2);
    assert_eq!(rows[0].used_percent, Some(60.0));
    assert_eq!(rows[0].tone, ValueTone::Ok);
    assert_eq!(rows[2].used_percent, None);
}

#[test]
fn the_disk_tone_changes_at_85_and_95_percent_used() {
    let tone_at = |used: u64| {
        let v = vec![volume("/", "HD", json!("internal"), Some(100), Some(100 - used), true)];
        let PopoverValue::DiskSpace { rows, .. } = value(run(&PopoverWidget::disk(VolumeChoice::All), |i| i.volumes = Source::Ready(&v)))
        else {
            panic!()
        };
        rows[0].tone
    };
    assert_eq!(tone_at(84), ValueTone::Ok);
    assert_eq!(tone_at(85), ValueTone::Warn);
    assert_eq!(tone_at(94), ValueTone::Warn);
    assert_eq!(tone_at(95), ValueTone::Bad);
}

#[test]
fn a_nearly_full_volume_is_bad_and_an_unanswering_share_says_so() {
    let v = volumes();
    let PopoverValue::DiskSpace { rows, .. } = value(run(&PopoverWidget::disk(VolumeChoice::Mount("/Volumes/Backup".into())), |i| {
        i.volumes = Source::Ready(&v)
    })) else {
        panic!()
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tone, ValueTone::Bad);
    let PopoverValue::DiskSpace { rows, .. } = value(run(&PopoverWidget::disk(VolumeChoice::Mount("/Volumes/NAS".into())), |i| {
        i.volumes = Source::Ready(&v)
    })) else {
        panic!()
    };
    assert!(!rows[0].reachable);
}

#[test]
fn the_system_volume_is_found_by_mount_or_as_the_first_internal_one() {
    let v = volumes();
    assert_eq!(system_volume(&v).unwrap().id, "/");
    let windows = vec![
        volume("C:\\", "C:", json!("internal"), Some(10), Some(5), true),
        volume("D:\\", "D:", json!("internal"), Some(10), Some(5), true),
    ];
    assert_eq!(system_volume(&windows).unwrap().id, "C:\\");
    let only_data = vec![
        volume("/Volumes/X", "X", json!({"external": {"bus": null}}), None, None, true),
        volume("/Volumes/Y", "Y", json!("internal"), None, None, true),
    ];
    assert_eq!(system_volume(&only_data).unwrap().id, "/Volumes/Y");
    assert!(system_volume(&[]).is_none());
}

#[test]
fn a_disconnected_volume_is_an_empty_that_says_what_the_person_gains_and_offers_another() {
    let v = volumes();
    let s = run(&PopoverWidget::disk(VolumeChoice::Mount("/Volumes/Time Machine".into())), |i| {
        i.volumes = Source::Ready(&v)
    })
    .unwrap();
    let S::Empty { title, detail, next } = s else { panic!("{s:?}") };
    assert_eq!(title, "See Time Machine's space when it's connected");
    assert_eq!(detail, "The disk isn't connected now.");
    assert_eq!(next.unwrap().action, WidgetActionId::ChooseVolume);
    let none: Vec<Volume> = vec![];
    assert!(matches!(
        run(&PopoverWidget::disk(VolumeChoice::All), |i| i.volumes = Source::Ready(&none)),
        Some(S::Empty { .. })
    ));
}

#[test]
fn a_failing_volume_list_is_an_error() {
    let s = run(&PopoverWidget::disk(VolumeChoice::All), |i| i.volumes = Source::Failed("x".into())).unwrap();
    assert!(matches!(&s, S::Error { title, .. } if title == "Couldn't read volumes"));
}

// ---- live kinds ----

#[test]
fn cpu_and_memory_come_from_the_live_push_with_tones_decided_here() {
    let mut l = live();
    l.system.cpu.total = 95.0;
    l.system.memory.total = 100;
    l.system.memory.used = 85;
    let mut minute = Minute::default();
    minute.push(&l);
    let mut inputs = Inputs::empty(NOW, &minute);
    inputs.live = Source::Ready(&l);
    let PopoverValue::CpuMemory {
        cpu_percent,
        cpu_tone,
        memory_percent,
        memory_tone,
        cpu_minute,
        ..
    } = value(state(&PopoverWidget::CpuMemory, &id("x"), &inputs))
    else {
        panic!()
    };
    assert_eq!(cpu_percent, 95.0);
    assert_eq!(cpu_tone, ValueTone::Bad);
    assert_eq!(memory_percent, 85.0);
    assert_eq!(memory_tone, ValueTone::Warn);
    assert_eq!(cpu_minute, [95.0]);
}

#[test]
fn network_and_top_apps_come_from_the_live_push() {
    let l = live();
    let mut minute = Minute::default();
    minute.push(&l);
    let mut inputs = Inputs::empty(NOW, &minute);
    inputs.live = Source::Ready(&l);
    let (rx, tx) = l.system.network_totals();
    let PopoverValue::Network {
        rx_bps, tx_bps, rx_minute, ..
    } = value(state(&PopoverWidget::Network, &id("x"), &inputs))
    else {
        panic!()
    };
    assert_eq!((rx_bps, tx_bps), (rx, tx));
    assert_eq!(rx_minute.len(), 1);
    let PopoverValue::TopApps { rows } = value(state(&PopoverWidget::TopApps, &id("x"), &inputs)) else {
        panic!()
    };
    assert_eq!(rows.len(), APP_ROWS);
    assert_eq!(rows[0].name, "Chrome");
    assert_eq!(rows[2].cpu_percent, None);
}

#[test]
fn the_last_minute_keeps_sixty_samples_and_never_a_nan() {
    let mut minute = Minute::default();
    assert!(minute.is_empty());
    let mut l = live();
    for i in 0..70 {
        l.system.cpu.total = if i == 3 { f32::NAN } else { i as f32 };
        minute.push(&l);
    }
    assert_eq!(minute.len(), MINUTE_SAMPLES);
    assert!(minute.cpu.iter().all(|v| v.is_finite()));
    assert_eq!(minute.cpu.back(), Some(&69.0));
}

#[test]
fn which_kinds_come_from_the_live_push() {
    let live_kinds: Vec<PopoverKind> = [
        PopoverKind::ActiveAlarms,
        PopoverKind::DiskSpace,
        PopoverKind::CpuMemory,
        PopoverKind::Network,
        PopoverKind::BackupHealth,
        PopoverKind::LastIssue,
        PopoverKind::PinnedCounters,
        PopoverKind::TopApps,
    ]
    .into_iter()
    .filter(|k| is_live_push(*k))
    .collect();
    assert_eq!(live_kinds, [PopoverKind::CpuMemory, PopoverKind::Network, PopoverKind::TopApps]);
}

// ---- backup ----

fn backup_report(statuses: serde_json::Value, checked: bool, checking: bool) -> BackupReport {
    serde_json::from_value(json!({
        "statuses": statuses, "checkedAt": if checked { json!(NOW.0) } else { json!(null) }, "checking": checking, "loginPathUsed": true
    }))
    .unwrap()
}

fn status(name: &str, state: &str, last: Option<i64>) -> serde_json::Value {
    json!({"tool": "timeMachine", "name": name, "state": state, "lastBackup": last, "summary": format!("{name} {state}"), "nextStep": null})
}

#[test]
fn backup_shows_the_tool_that_needs_attention_most() {
    let r = backup_report(
        json!([
            status("Time Machine", "healthy", Some(NOW.0 - HOUR)),
            status("restic", "stale", Some(NOW.0 - 90 * HOUR))
        ]),
        true,
        false,
    );
    let PopoverValue::BackupHealth(b) = value(run(&PopoverWidget::BackupHealth, |i| {
        i.backup = Source::Ready(BackupInput { report: &r, job: None })
    })) else {
        panic!()
    };
    assert_eq!((b.name.as_str(), b.state, b.tools), ("restic", BackupVerdict::Stale, 2));
    assert_eq!(b.last_backup, Some(NOW.0 - 90 * HOUR));
}

#[test]
fn no_backup_set_up_is_an_empty_with_the_setup_action() {
    for r in [
        backup_report(json!([]), true, false),
        backup_report(json!([status("Time Machine", "notConfigured", None)]), true, false),
    ] {
        let s = run(&PopoverWidget::BackupHealth, |i| {
            i.backup = Source::Ready(BackupInput { report: &r, job: None })
        })
        .unwrap();
        let S::Empty { title, next, .. } = s else { panic!("{s:?}") };
        assert_eq!(title, "Know when backups ran");
        assert_eq!(next.unwrap().action, WidgetActionId::SetUpBackupChecks);
    }
}

#[test]
fn a_running_check_shows_the_jobs_own_progress_and_keeps_the_last_value() {
    let r = backup_report(json!([status("Time Machine", "healthy", Some(NOW.0))]), true, true);
    let job = JobProgress {
        phase: "Time Machine".into(),
        done: 2,
        unit: "items".into(),
    };
    let s = run(&PopoverWidget::BackupHealth, |i| {
        i.backup = Source::Ready(BackupInput {
            report: &r,
            job: Some(job),
        })
    })
    .unwrap();
    let S::Working {
        title,
        phase,
        progress,
        last,
        cancel,
    } = s
    else {
        panic!("{s:?}")
    };
    assert_eq!((title.as_str(), phase.as_str()), ("Checking backup", ""));
    assert_eq!(
        progress,
        Progress::Count {
            done: 2,
            unit: "items".into()
        }
    );
    assert!(last.is_some() && cancel.is_none());
}

#[test]
fn without_a_job_there_is_no_made_up_progress() {
    // The report says it is checking, but no job reports a count: the last value stays; with no value yet, it is loading.
    let r = backup_report(json!([status("Time Machine", "healthy", Some(NOW.0 - HOUR))]), true, true);
    let s = run(&PopoverWidget::BackupHealth, |i| {
        i.backup = Source::Ready(BackupInput { report: &r, job: None })
    })
    .unwrap();
    assert!(matches!(s, S::Ready { .. }), "{s:?}");
    let first = backup_report(json!([]), false, true);
    assert_eq!(
        run(&PopoverWidget::BackupHealth, |i| i.backup =
            Source::Ready(BackupInput { report: &first, job: None })),
        None
    );
}

#[test]
fn slow_values_carry_the_age_of_their_own_source() {
    let checked = Timestamp(NOW.0 - 10 * MINUTE);
    let mut r = backup_report(json!([status("Time Machine", "healthy", Some(NOW.0 - HOUR))]), true, false);
    r.checked_at = Some(checked);
    let Some(S::Ready { as_of, .. }) = run(&PopoverWidget::BackupHealth, |i| {
        i.backup = Source::Ready(BackupInput { report: &r, job: None })
    }) else {
        panic!()
    };
    assert_eq!(as_of, checked.0);
    let scanned = Timestamp(NOW.0 - 3 * HOUR);
    let list = vec![issue(1, HOUR)];
    let Some(S::Ready { as_of, .. }) = run(&PopoverWidget::LastIssue, |i| {
        i.issues = Source::Ready(IssuesInput {
            recent: &list,
            reading: None,
            scanned_at: Some(scanned),
        })
    }) else {
        panic!()
    };
    assert_eq!(as_of, scanned.0);
    let rs = vec![reading(r"\A\B", json!({"available": 1.0}))];
    let read_at = Timestamp(NOW.0 - 2 * MINUTE);
    let Some(S::Ready { as_of, .. }) = run(&PopoverWidget::PinnedCounters, |i| {
        i.counters = Source::Ready(CountersInput {
            pinned: 1,
            readings: &rs,
            read_at: Some(read_at),
        })
    }) else {
        panic!()
    };
    assert_eq!(as_of, read_at.0);
}

#[test]
fn a_check_that_could_not_read_any_tool_is_an_error_that_starts_the_check_again() {
    let r = backup_report(json!([status("Time Machine", "cannotCheck", None)]), true, false);
    let s = run(&PopoverWidget::BackupHealth, |i| {
        i.backup = Source::Ready(BackupInput { report: &r, job: None })
    })
    .unwrap();
    let S::Error { title, cause, next } = s else { panic!("{s:?}") };
    assert_eq!(title, "Couldn't check");
    assert_eq!(cause.as_deref(), Some("Time Machine cannotCheck"));
    assert_eq!(next.unwrap().action, WidgetActionId::StartBackupCheck);
}

// ---- last issue ----

fn issue(n: u32, ago: i64) -> Issue {
    serde_json::from_value(json!({
        "id": format!("i{n}"), "at": NOW.0 - ago, "kind": "appCrash", "severity": "warning", "app": "Notes", "title": format!("Notes quit {n}"),
        "evidence": "", "source": "macReports"
    }))
    .unwrap()
}

#[test]
fn the_last_issue_is_the_newest_of_the_last_seven_days() {
    let list = vec![issue(1, 3 * HOUR), issue(2, HOUR), issue(3, 8 * 24 * HOUR)];
    let PopoverValue::LastIssue(v) = value(run(&PopoverWidget::LastIssue, |i| {
        i.issues = Source::Ready(IssuesInput {
            recent: &list,
            reading: None,
            scanned_at: None,
        })
    })) else {
        panic!()
    };
    assert_eq!(v.title, "Notes quit 2");
    assert_eq!(v.kind, "App crash");
    assert_eq!(v.level, Level::Warning);
    assert_eq!(v.days, 7, "the engine sends the range");
}

#[test]
fn no_issue_in_a_week_is_an_empty_that_opens_issues() {
    let old = vec![issue(1, 9 * 24 * HOUR)];
    let s = run(&PopoverWidget::LastIssue, |i| {
        i.issues = Source::Ready(IssuesInput {
            recent: &old,
            reading: None,
            scanned_at: None,
        })
    })
    .unwrap();
    let S::Empty { title, detail, next } = s else { panic!("{s:?}") };
    assert_eq!(title, "No issues in the last 7 days");
    assert!(!detail.is_empty());
    assert_eq!(next.unwrap().action, WidgetActionId::OpenPage(Page::Issues));
}

#[test]
fn reading_the_logs_is_working_with_a_count_of_events_so_far() {
    let list = vec![issue(1, HOUR)];
    let s = run(&PopoverWidget::LastIssue, |i| {
        i.issues = Source::Ready(IssuesInput {
            recent: &list,
            reading: Some(12_400),
            scanned_at: None,
        })
    })
    .unwrap();
    let S::Working { phase, progress, last, .. } = s else {
        panic!("{s:?}")
    };
    assert_eq!(phase, "Reading system logs · Indexing");
    assert_eq!(
        progress,
        Progress::Count {
            done: 12_400,
            unit: "events".into()
        }
    );
    assert!(last.is_some());
    let s = run(&PopoverWidget::LastIssue, |i| i.issues = Source::Failed("x".into())).unwrap();
    assert!(matches!(&s, S::Error { title, .. } if title == "Couldn't read issues"));
}

// ---- pinned counters ----

fn reading(path: &str, value: serde_json::Value) -> CounterReading {
    let value: dyno_core_reading::Reading<f64> = serde_json::from_value(value).unwrap();
    CounterReading::new(path.into(), crate::counters::CounterUnit::Percent, value)
}

#[test]
fn pinned_counters_show_four_rows_with_unavailable_ones_marked() {
    let rs: Vec<CounterReading> = (0..5)
        .map(|n| {
            reading(
                &format!("\\Processor({n})\\% Processor Time"),
                if n == 1 {
                    json!({"unavailable": "gone"})
                } else {
                    json!({"available": 12.5})
                },
            )
        })
        .collect();
    let PopoverValue::PinnedCounters { rows, more } = value(run(&PopoverWidget::PinnedCounters, |i| {
        i.counters = Source::Ready(CountersInput {
            pinned: 6,
            readings: &rs,
            read_at: None,
        })
    })) else {
        panic!()
    };
    assert_eq!(rows.len(), COUNTER_ROWS);
    assert_eq!(more, 2);
    assert_eq!(rows[0].name, "% Processor Time");
    assert_eq!(rows[0].value, Some(12.5));
    assert_eq!(rows[1].value, None);
}

#[test]
fn nothing_pinned_is_an_empty_and_none_answering_is_an_error() {
    let s = run(&PopoverWidget::PinnedCounters, |i| {
        i.counters = Source::Ready(CountersInput {
            pinned: 0,
            readings: &[],
            read_at: None,
        })
    })
    .unwrap();
    let S::Empty { title, next, .. } = s else { panic!("{s:?}") };
    assert_eq!(title, "Watch your own counters here");
    assert_eq!(next.unwrap().action, WidgetActionId::PinCounters);
    let rs = vec![
        reading("\\A\\B", json!({"unavailable": "x"})),
        reading("\\A\\C", json!({"unavailable": "x"})),
    ];
    let s = run(&PopoverWidget::PinnedCounters, |i| {
        i.counters = Source::Ready(CountersInput {
            pinned: 3,
            readings: &rs,
            read_at: None,
        })
    })
    .unwrap();
    let S::Error { cause, .. } = s else { panic!("{s:?}") };
    assert_eq!(cause.as_deref(), Some("None of the 3 pinned counters answered."));
}

// ---- stale and the agent notice ----

#[test]
fn losing_the_agent_marks_ready_values_stale_since_they_were_read_and_leaves_the_rest() {
    let ready = |as_of| PlacementState {
        id: id("a"),
        state: S::Ready {
            value: PopoverValue::TopApps { rows: vec![] },
            as_of,
            stale_since: None,
        },
    };
    let mut values = vec![
        ready(100),
        PlacementState {
            id: id("b"),
            state: S::Empty {
                title: "t".into(),
                detail: "d".into(),
                next: None,
            },
        },
        PlacementState {
            id: id("c"),
            state: S::Ready {
                value: PopoverValue::TopApps { rows: vec![] },
                as_of: 300,
                stale_since: Some(250),
            },
        },
    ];
    mark_stale(&mut values);
    assert!(matches!(
        values[0].state,
        S::Ready {
            stale_since: Some(100),
            ..
        }
    ));
    assert!(matches!(values[1].state, S::Empty { .. }));
    assert!(
        matches!(
            values[2].state,
            S::Ready {
                stale_since: Some(250),
                ..
            }
        ),
        "an older mark is kept"
    );
}

#[test]
fn the_agent_stopped_notice_names_the_time_of_the_last_reading_and_offers_a_restart() {
    let zone = FixedZone(2 * 60);
    let at = Timestamp(NOW.0 - NOW.0 % crate::time::DAY + 12 * HOUR + 28 * MINUTE);
    let n = agent_stopped(Some(at), &zone);
    assert_eq!(n.tone, Tone::Danger);
    assert_eq!(n.title, "Agent stopped");
    assert_eq!(n.detail, "Readings paused at 14:28");
    assert_eq!(n.action.unwrap().action, WidgetActionId::RestartAgent);
    assert_eq!(agent_stopped(None, &zone).detail, "No readings yet");
}

#[test]
fn the_wire_shape_of_a_state_is_tagged_by_state_with_camel_case_times() {
    let s: State = S::Ready {
        value: PopoverValue::TopApps { rows: vec![] },
        as_of: 5,
        stale_since: None,
    };
    assert_eq!(
        serde_json::to_value(&s).unwrap(),
        json!({"state": "ready", "value": {"topApps": {"rows": []}}, "asOf": 5, "staleSince": null})
    );
    assert_eq!(
        serde_json::to_value(WidgetActionId::OpenPage(Page::Alarms)).unwrap(),
        json!({"openPage": "alarms"})
    );
    assert_eq!(serde_json::to_value(WidgetActionId::RestartAgent).unwrap(), "restartAgent");
}

#[test]
fn every_action_has_one_effect() {
    use Effect::*;
    let nav = |n| Navigate(n);
    assert_eq!(
        WidgetActionId::OpenPage(Page::Issues).effect(),
        nav(super::Navigate::Page(Page::Issues))
    );
    assert_eq!(WidgetActionId::PinCounters.effect(), nav(super::Navigate::Page(Page::Counters)));
    assert_eq!(
        WidgetActionId::SetUpBackupChecks.effect(),
        nav(super::Navigate::Page(Page::Backups))
    );
    let settings = nav(super::Navigate::Settings(SettingsSection::PopupWidgets));
    assert_eq!(WidgetActionId::ChooseVolume.effect(), settings);
    assert_eq!(WidgetActionId::CustomizeWidgets.effect(), settings);
    assert_eq!(
        WidgetActionId::OpenSettings.effect(),
        nav(super::Navigate::Settings(SettingsSection::General))
    );
    assert_eq!(WidgetActionId::Retry(id("x")).effect(), Refresh);
    assert_eq!(WidgetActionId::StartBackupCheck.effect(), StartBackupCheck);
    assert_eq!(WidgetActionId::RestartAgent.effect(), RestartAgent);
}

#[test]
fn no_network_is_the_offline_empty_with_open_network() {
    let mut l = live();
    l.system.network.clear();
    let s = run(&PopoverWidget::Network, |i| i.live = Source::Ready(&l)).unwrap();
    let S::Empty { title, detail, next } = s else { panic!("{s:?}") };
    assert_eq!((title.as_str(), detail.as_str()), ("See traffic when online", "Offline now."));
    assert_eq!(next.unwrap().action, WidgetActionId::OpenPage(Page::Network));
}

#[test]
fn a_percent_counter_has_a_scale_and_a_tone_from_the_engine() {
    let rs = vec![
        reading(r"\A\B", json!({"available": 95.0})),
        reading(r"\A\C", json!({"available": 10.0})),
    ];
    let PopoverValue::PinnedCounters { rows, .. } = value(run(&PopoverWidget::PinnedCounters, |i| {
        i.counters = Source::Ready(CountersInput {
            pinned: 2,
            readings: &rs,
            read_at: None,
        })
    })) else {
        panic!()
    };
    assert_eq!((rows[0].percent, rows[0].tone), (Some(95.0), ValueTone::Bad));
    assert_eq!((rows[1].percent, rows[1].tone), (Some(10.0), ValueTone::Ok));
}
