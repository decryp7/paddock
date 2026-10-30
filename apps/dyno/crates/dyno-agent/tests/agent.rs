#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The agent end to end over the fake platform: a real IPC connection, a real SQLite file in a temp folder.

use dyno_agent::cli::{ask, ask_within, connect};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::history::HistoryRange;
use dyno_core::protocol::{ErrorCode, LiveDetail, LiveUpdate, Message, Reply, Request};
use dyno_core::time::Timestamp;
use dyno_ipc::{Client, Peer, PeerOptions};
use dyno_platform::fake::{process, Fake};
use dyno_platform::Platform;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

mod common;
use common::{full_disk_sample, wait_for_alarm};

struct Harness {
    running: Option<Running>,
    fake: Fake,
    dirs: Dirs,
    _tmp: tempfile::TempDir,
}

impl Harness {
    fn start() -> Harness {
        Harness::start_with(|platform| platform)
    }

    /// Starts the agent over the fake, with `adapt` free to replace ports of the platform first.
    fn start_with(adapt: impl FnOnce(Platform) -> Platform) -> Harness {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let fake = Fake::new();
        for (pid, name, cpu) in [(100, "busy", 60.0), (101, "calm", 1.0), (102, "launchd", 0.1)] {
            fake.add_process(process(pid, name, cpu));
        }
        let running = Running::start(dirs.clone(), adapt(fake.platform()), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
        Harness {
            running: Some(running),
            fake,
            dirs,
            _tmp: tmp,
        }
    }

    fn client(&self) -> Peer<Message> {
        connect(&self.dirs).unwrap().0
    }

    fn subscriber(&self, detail: LiveDetail) -> (Peer<Message>, Receiver<LiveUpdate>) {
        let d = dyno_agent::discovery::Discovery::read(&self.dirs.discovery()).unwrap();
        let conn = Client::connect(&d.endpoint, &d.token().unwrap()).unwrap();
        let (tx, rx) = channel();
        let peer = Peer::start(
            conn,
            PeerOptions::new().on_push(move |m| {
                if let Message::Live(l) = m {
                    let _ = tx.send(*l);
                }
            }),
        )
        .unwrap();
        ask(&peer, Request::Subscribe { detail }).unwrap();
        (peer, rx)
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop();
        }
    }
}

#[test]
fn a_subscriber_gets_live_updates_with_processes_only_when_it_asks() {
    let h = Harness::start();
    let (_p, rx) = h.subscriber(LiveDetail::Processes);
    let live = rx.recv_timeout(Duration::from_secs(10)).expect("a live update");
    assert!(live.processes.as_ref().is_some_and(|p| p.len() == 3));
    assert!(live.groups.is_some());
    assert!(live.overhead.sampling_interval_ms >= 500);
    let (_q, summary) = h.subscriber(LiveDetail::Summary);
    let s = summary.recv_timeout(Duration::from_secs(10)).expect("a summary update");
    assert!(s.processes.is_none() && s.groups.is_none());
    assert!(!s.top_cpu.is_empty());
}

#[test]
fn other_users_processes_are_folded_into_one_row_in_the_live_update() {
    let h = Harness::start();
    for pid in [200, 201] {
        let mut p = process(pid, "theirs", 3.0);
        p.owner = dyno_core::process::Owner::Other;
        h.fake.add_process(p);
    }
    let (_p, rx) = h.subscriber(LiveDetail::Processes);
    let live = loop {
        let l = rx.recv_timeout(Duration::from_secs(10)).expect("a live update");
        if l.other_users.is_some() {
            break l;
        }
    };
    let other = live.other_users.unwrap();
    assert_eq!(other.count, 2);
    let listed = live.processes.unwrap();
    assert!(
        listed.iter().all(|p| p.owner != dyno_core::process::Owner::Other),
        "folded ones are not listed"
    );
    assert_eq!(listed.len(), 3);
}

#[test]
fn settings_round_trip_and_survive_a_restart() {
    let h = Harness::start();
    let peer = h.client();
    let Reply::Settings(mut s) = ask(&peer, Request::Settings).unwrap() else {
        panic!("settings")
    };
    s.launch_at_login = true;
    s.sampling.interval_ms = 700;
    ask(&peer, Request::SaveSettings { settings: s }).unwrap();
    let Reply::Settings(back) = ask(&peer, Request::Settings).unwrap() else {
        panic!("settings")
    };
    assert!(back.launch_at_login && back.sampling.interval_ms == 700);
    let saved = std::fs::read_to_string(h.dirs.settings()).unwrap();
    assert!(saved.contains("\"launchAtLogin\": true"));
}

#[test]
fn an_alert_rule_is_added_changed_and_deleted_without_touching_the_other_settings() {
    use dyno_core::alarms::custom::{CustomAlertRule, CustomMetric, CustomOp};
    use dyno_core::health::Severity;
    let h = Harness::start();
    let peer = h.client();
    let rule = |limit: f64| CustomAlertRule {
        id: "busy".into(),
        name: "Busy".into(),
        metric: CustomMetric::CpuTotal,
        op: CustomOp::Above,
        limit,
        for_secs: 30,
        severity: Severity::Warning,
        enabled: true,
    };
    let Reply::Settings(before) = ask(&peer, Request::Settings).unwrap() else {
        panic!("settings")
    };
    let Reply::Settings(added) = ask(&peer, Request::UpsertAlertRule { rule: rule(90.0) }).unwrap() else {
        panic!("settings")
    };
    assert_eq!(added.custom_alerts.len(), 1);
    let Reply::Settings(changed) = ask(&peer, Request::UpsertAlertRule { rule: rule(80.0) }).unwrap() else {
        panic!("settings")
    };
    assert_eq!((changed.custom_alerts.len(), changed.custom_alerts[0].limit), (1, 80.0));
    assert_eq!(changed.sampling, before.sampling);
    let saved = std::fs::read_to_string(h.dirs.settings()).unwrap();
    assert!(saved.contains("\"busy\""));
    let mut bad = rule(1.0);
    bad.id = "not valid".into();
    assert!(ask(&peer, Request::UpsertAlertRule { rule: bad }).is_err());
    assert!(ask(&peer, Request::DeleteAlertRule { id: "nope".into() }).is_err());
    let Reply::Settings(gone) = ask(&peer, Request::DeleteAlertRule { id: "busy".into() }).unwrap() else {
        panic!("settings")
    };
    assert!(gone.custom_alerts.is_empty());
}

#[test]
fn history_comes_back_in_the_shape_the_app_expects() {
    let h = Harness::start();
    let (peer, rx) = h.subscriber(LiveDetail::Summary);
    rx.recv_timeout(Duration::from_secs(10)).unwrap();
    for range in [HistoryRange::Hour, HistoryRange::Day, HistoryRange::Week, HistoryRange::Month] {
        let Reply::History(hist) = ask(&peer, Request::History { range, end: None }).unwrap() else {
            panic!("history")
        };
        assert_eq!(hist.range, range);
        assert!(hist.to.since(hist.from) > 0);
    }
    let Reply::History(hour) = ask(
        &peer,
        Request::History {
            range: HistoryRange::Hour,
            end: None,
        },
    )
    .unwrap() else {
        panic!("history")
    };
    assert!(!hour.points.is_empty(), "the first tick wrote a raw point");
    let _ = Timestamp(0);
}

#[test]
fn unknown_things_are_not_found() {
    let h = Harness::start();
    let peer = h.client();
    let e = ask(&peer, Request::WeeklyReport { weeks_ago: 500 }).unwrap_err();
    assert!(e.contains("at most"), "{e}");
    assert!(matches!(ask(&peer, Request::Lockers { path: "relative/path".into() }), Err(m) if m.contains("full path")));
    assert!(ask(
        &peer,
        Request::DiffSnapshots {
            before: "a".into(),
            after: "b".into()
        }
    )
    .is_err());
    let Reply::Snapshot(info) = ask(&peer, Request::TakeSnapshot { label: "before".into() }).unwrap() else {
        panic!("snap")
    };
    assert_eq!(info.label, "before");
    let Reply::Plugins(p) = ask(&peer, Request::Plugins).unwrap() else {
        panic!("plugins")
    };
    assert!(p.is_empty());
    let _ = ErrorCode::NotFound;
}

#[test]
fn shutdown_removes_the_discovery_file_and_a_second_agent_is_not_needed() {
    let mut h = Harness::start();
    let peer = h.client();
    assert!(h.dirs.discovery().exists());
    // The agent may close the connection as it stops, before or after the reply arrives: both are a clean shutdown.
    let _ = ask(&peer, Request::Shutdown);
    let running = h.running.take().unwrap();
    for _ in 0..50 {
        if running.stopped() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(running.stopped());
    running.stop();
    assert!(!h.dirs.discovery().exists(), "the file goes away with the agent");
    assert!(connect(&h.dirs).is_err());
}

#[test]
fn a_wrong_token_is_refused_and_the_agent_keeps_serving() {
    let h = Harness::start();
    let d = dyno_agent::discovery::Discovery::read(&h.dirs.discovery()).unwrap();
    let bad = dyno_ipc::Token::from_bytes([1; 32]);
    assert!(Client::connect(&d.endpoint, &bad).is_err());
    let peer = h.client();
    ask(&peer, Request::Settings).unwrap();
}

/// A generous deadline for requests that query the real machine (a loaded Windows runner needs well over 15 s).
const SLOW: std::time::Duration = std::time::Duration::from_secs(120);

#[test]
fn the_inventory_is_read_once_and_cached() {
    let h = Harness::start();
    let peer = h.client();
    let Reply::Inventory(a) = ask_within(&peer, Request::Inventory, SLOW).unwrap() else {
        panic!("inventory")
    };
    assert!(!a.os.is_empty());
    assert!(a.memory.total() > 0);
    let Reply::Inventory(b) = ask_within(&peer, Request::Inventory, SLOW).unwrap() else {
        panic!("inventory")
    };
    assert_eq!(*a, *b);
}

#[test]
fn a_low_disk_alert_raises_one_alarm_that_can_be_acknowledged_noted_shelved_and_muted() {
    use dyno_core::alarms::{AlarmFilter, AlarmState, ShelveFor};
    let h = Harness::start();
    h.fake.push_sample(full_disk_sample());
    let peer = h.client();
    ask(
        &peer,
        Request::Subscribe {
            detail: LiveDetail::Summary,
        },
    )
    .unwrap();
    let alarm = wait_for_alarm(&peer);
    assert_eq!(alarm.rule, "alert:lowDisk");
    assert_eq!(alarm.state, AlarmState::Active);
    assert_eq!(alarm.severity, dyno_core::health::Severity::Critical);

    // Ack + note.
    let Reply::Alarms(changed) = ask(&peer, Request::AcknowledgeAlarms { ids: vec![alarm.id] }).unwrap() else {
        panic!("alarms")
    };
    assert_eq!(changed[0].state, AlarmState::Acknowledged);
    ask(
        &peer,
        Request::NoteAlarm {
            id: alarm.id,
            text: "freed space on Monday".into(),
        },
    )
    .unwrap();
    let Reply::AlarmChanges(log) = ask(
        &peer,
        Request::AlarmHistory {
            alarm_id: Some(alarm.id),
            limit: 10,
        },
    )
    .unwrap() else {
        panic!("changes")
    };
    assert!(log.len() >= 3, "raised, acknowledged and noted are logged: {log:?}");

    // Shelve hides it from the default list; unshelve brings it back.
    ask(
        &peer,
        Request::ShelveAlarm {
            id: alarm.id,
            until: ShelveFor::OneHour,
        },
    )
    .unwrap();
    let Reply::Alarms(visible) = ask(
        &peer,
        Request::Alarms {
            filter: AlarmFilter::default(),
        },
    )
    .unwrap() else {
        panic!("alarms")
    };
    assert!(visible.is_empty());
    ask(&peer, Request::UnshelveAlarm { id: alarm.id }).unwrap();
    let Reply::Alarms(visible) = ask(
        &peer,
        Request::Alarms {
            filter: AlarmFilter::default(),
        },
    )
    .unwrap() else {
        panic!("alarms")
    };
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].notes[0].text, "freed space on Monday");

    ask(
        &peer,
        Request::MuteRule {
            rule: "alert:lowDisk".into(),
            muted: true,
        },
    )
    .unwrap();
    let Reply::AlarmStats(stats) = ask(&peer, Request::AlarmStats).unwrap() else {
        panic!("stats")
    };
    assert_eq!(stats.open, 1);

    // Bad input is refused with a message.
    for bad in [
        Request::AcknowledgeAlarms { ids: vec![] },
        Request::ShelveAlarm {
            id: alarm.id,
            until: ShelveFor::Custom(Timestamp(1)),
        },
        Request::NoteAlarm {
            id: alarm.id,
            text: "  ".into(),
        },
        Request::MuteRule {
            rule: String::new(),
            muted: true,
        },
    ] {
        assert!(ask(&peer, bad).is_err());
    }
    for missing in [
        Request::UnshelveAlarm { id: 9_999 },
        Request::NoteAlarm {
            id: 9_999,
            text: "x".into(),
        },
        Request::ShelveAlarm {
            id: 9_999,
            until: ShelveFor::OneHour,
        },
    ] {
        assert!(ask(&peer, missing).is_err());
    }
}

#[test]
fn alarms_survive_an_agent_restart() {
    let mut h = Harness::start();
    h.fake.push_sample(full_disk_sample());
    let peer = h.client();
    ask(
        &peer,
        Request::Subscribe {
            detail: LiveDetail::Summary,
        },
    )
    .unwrap();
    let before = wait_for_alarm(&peer);
    ask(
        &peer,
        Request::MuteRule {
            rule: "alert:lowDisk".into(),
            muted: true,
        },
    )
    .unwrap();
    drop(peer);
    h.running.take().unwrap().stop();
    let fake = Fake::new();
    let running = Running::start(h.dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
    h.running = Some(running);
    let peer = h.client();
    let after = wait_for_alarm(&peer);
    assert_eq!(after.id, before.id);
    assert_eq!(after.key, before.key);
    // The mute survived too: a new raise of the same rule would be muted (checked via a fresh engine in dyno-core).
    let Reply::AlarmChanges(log) = ask(&peer, Request::AlarmHistory { alarm_id: None, limit: 50 }).unwrap() else {
        panic!("changes")
    };
    assert!(!log.is_empty());
}

#[test]
fn the_flight_recorder_captures_on_request_and_when_an_alarm_fires() {
    let h = Harness::start();
    h.fake.push_sample(full_disk_sample());
    let peer = h.client();
    let alarm = {
        ask(
            &peer,
            Request::Subscribe {
                detail: LiveDetail::Summary,
            },
        )
        .unwrap();
        wait_for_alarm(&peer)
    };
    // The alarm captured the ring automatically.
    let mut auto = Vec::new();
    for _ in 0..50 {
        let Reply::RecorderSnapshots(list) = ask(&peer, Request::RecorderSnapshots).unwrap() else {
            panic!("snapshots")
        };
        auto = list;
        if !auto.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(auto.len(), 1, "one automatic capture");
    assert!(matches!(auto[0].reason, dyno_core::recorder::CaptureReason::Alarm { alarm_id, .. } if alarm_id == alarm.id));
    // A manual one on request, then read it back.
    let Reply::Captured(info) = ask(&peer, Request::Capture).unwrap() else {
        panic!("captured")
    };
    assert!(info.samples >= 1);
    let Reply::RecorderSnapshot(snap) = ask(&peer, Request::RecorderSnapshot { id: info.id }).unwrap() else {
        panic!("snapshot")
    };
    assert_eq!(snap.samples.len() as u32, info.samples);
    assert!(snap.samples.iter().all(|s| s.memory_total > 0 && !s.cpu_cores.is_empty()));
    assert!(ask(&peer, Request::RecorderSnapshot { id: 9_999 }).is_err());
}

#[test]
fn the_diagnostics_bundle_previews_and_saves_a_redacted_zip() {
    let h = Harness::start();
    let peer = h.client();
    let Reply::DiagnosticsPreview(p) = ask_within(&peer, Request::DiagnosticsPreview, SLOW).unwrap() else {
        panic!("preview")
    };
    let names: Vec<&str> = p.files.iter().map(|f| f.path.as_str()).collect();
    assert!(names.contains(&"system/inventory.json") && names.contains(&"settings.json") && names.contains(&"manifest.json"));
    assert!(p.total_bytes > 0);
    let dest = h._tmp.path().join("diag.zip");
    let Reply::DiagnosticsSaved(saved) = ask(
        &peer,
        Request::SaveDiagnostics {
            path: dest.to_string_lossy().into_owned(),
        },
    )
    .unwrap() else {
        panic!("saved")
    };
    assert_eq!(saved.files as usize, p.files.len());
    assert!(std::fs::metadata(&dest).unwrap().len() == saved.bytes);
    // Refused: relative path, wrong extension, missing folder.
    for bad in [
        "diag.zip".to_string(),
        h._tmp.path().join("diag.txt").to_string_lossy().into_owned(),
        h._tmp.path().join("nope").join("d.zip").to_string_lossy().into_owned(),
    ] {
        assert!(ask(&peer, Request::SaveDiagnostics { path: bad }).is_err());
    }
}

// ------------------------------------------------------------------------------------------------------- disk map

#[test]
fn the_weekly_report_is_built_from_stored_data_and_exported_as_markdown_and_html() {
    let h = Harness::start();
    let peer = h.client();
    // A just-started agent has little history; that is a valid report, and a past week is empty and says so.
    let Reply::Report(now) = ask(&peer, Request::WeeklyReport { weeks_ago: 0 }).unwrap() else {
        panic!("report")
    };
    assert_eq!(now.to.since(now.from), 7 * 24 * 3_600_000);
    let Reply::Report(old) = ask(&peer, Request::WeeklyReport { weeks_ago: 5 }).unwrap() else {
        panic!("report")
    };
    assert!(old.nothing_to_report && old.score.is_none() && old.issues.is_empty());
    assert!(old.to < now.from);
    assert!(ask(&peer, Request::WeeklyReport { weeks_ago: 53 })
        .unwrap_err()
        .contains("at most 52"));
    let md = h._tmp.path().join("week.md");
    let Reply::ReportSaved(saved) = ask(
        &peer,
        Request::SaveReport {
            path: md.to_string_lossy().into_owned(),
            format: dyno_core::report::ReportFormat::Markdown,
            weeks_ago: 0,
        },
    )
    .unwrap() else {
        panic!("saved")
    };
    let text = std::fs::read_to_string(&md).unwrap();
    assert_eq!(saved.bytes, text.len() as u64);
    assert!(text.starts_with("# Dyno weekly report"));
    let html = h._tmp.path().join("week.html");
    ask(
        &peer,
        Request::SaveReport {
            path: html.to_string_lossy().into_owned(),
            format: dyno_core::report::ReportFormat::Html,
            weeks_ago: 5,
        },
    )
    .unwrap();
    assert!(std::fs::read_to_string(&html).unwrap().contains("Nothing to report"));
    // Refused: relative path, the wrong extension for the format, a missing folder.
    for bad in [
        "week.md".to_string(),
        html.to_string_lossy().into_owned(),
        h._tmp.path().join("nope").join("w.md").to_string_lossy().into_owned(),
    ] {
        let request = Request::SaveReport {
            path: bad,
            format: dyno_core::report::ReportFormat::Markdown,
            weeks_ago: 0,
        };
        assert!(ask(&peer, request).is_err());
    }
}

/// The counters browser against this machine's real counters (read-only): browse, search, read twice, chart.
#[test]
fn the_counters_browser_lists_searches_reads_and_charts_this_machines_counters() {
    let h = Harness::start();
    let peer = h.client();
    let ask_slow = |r| ask_within(&peer, r, Duration::from_secs(30));
    // Listing every counter can take longer than the agent waits on a busy machine (Windows CI run 7166); it then says to
    // try again, and the retry joins the listing still running. Asked again as the message says (a bounded hang guard).
    let objects = (0..5)
        .find_map(|_| match ask_slow(Request::CounterObjects) {
            Ok(Reply::CounterObjects(o)) => Some(o),
            Err(e) if e.contains("took too long") => None,
            other => panic!("objects: {other:?}"),
        })
        .expect("the counters were listed");
    assert!(objects.objects.iter().any(|o| o.name == "Memory" && o.counters > 0));
    let Reply::CounterObject(memory) = ask_slow(Request::CounterObject { name: "Memory".into() }).unwrap() else {
        panic!("object")
    };
    let path = format!("\\Memory\\{}", memory.counters[0].name);
    let Reply::CounterHits(hits) = ask_slow(Request::CounterSearch {
        query: memory.counters[0].name.clone(),
        limit: 5,
    })
    .unwrap() else {
        panic!("hits")
    };
    assert!(hits.iter().any(|x| x.path == path));
    for _ in 0..2 {
        let Reply::CounterValues(values) = ask_slow(Request::CounterValues { paths: vec![path.clone()] }).unwrap() else {
            panic!("values")
        };
        assert_eq!(values[0].path, path);
        // The counter history keeps only readings that are strictly later than the last one (and a running total needs a
        // later reading to become a rate): wait until the agent's clock has moved on, not for a fixed time.
        let read_at = dyno_agent::agent::now();
        while dyno_agent::agent::now() <= read_at {
            std::thread::yield_now();
        }
    }
    let Reply::CounterHistory(history) = ask_slow(Request::CounterHistory {
        paths: vec![path.clone()],
        minutes: 5,
    })
    .unwrap() else {
        panic!("history")
    };
    assert_eq!(history.series.len(), 1);
    assert!(!history.series[0].points.is_empty());
    // Bad input is refused by the engine, not trusted.
    let bad = ask_slow(Request::CounterValues {
        paths: vec!["junk".into()],
    });
    assert!(bad.is_err());
    let missing = ask_slow(Request::CounterObject {
        name: "No Such Object".into(),
    });
    assert!(missing.unwrap_err().contains("no counter object"));
}

#[test]
fn pinned_counters_and_saved_sets_are_validated_and_survive_a_restart() {
    let h = Harness::start();
    let peer = h.client();
    let Reply::Settings(mut s) = ask(&peer, Request::Settings).unwrap() else {
        panic!("settings")
    };
    s.counters.pinned = vec!["\\Memory\\Free".into(), "junk".into(), "\\Memory\\Free".into()];
    s.counters.sets = vec![dyno_core::counters::CounterSet {
        name: "Memory".into(),
        paths: vec!["\\Memory\\Free".into(), "bad".into()],
    }];
    let Reply::Settings(saved) = ask(&peer, Request::SaveSettings { settings: s }).unwrap() else {
        panic!("settings")
    };
    assert_eq!(saved.counters.pinned, ["\\Memory\\Free"]);
    assert_eq!(saved.counters.sets[0].paths, ["\\Memory\\Free"]);
    let text = std::fs::read_to_string(h.dirs.settings()).unwrap();
    let (reloaded, warning) = dyno_core::settings::parse(&text);
    assert!(warning.is_none());
    assert_eq!(reloaded.counters, saved.counters);
}

// ---- throttling, lifespan and baselines (WP-23, WP-24, WP-25) ----

fn shared(h: &Harness) -> std::sync::Arc<dyno_agent::agent::Shared> {
    h.running.as_ref().unwrap().shared.clone()
}

fn alarm_rules(peer: &Peer<Message>) -> Vec<String> {
    let Reply::Alarms(list) = ask(
        peer,
        Request::Alarms {
            filter: Default::default(),
        },
    )
    .unwrap() else {
        panic!("alarms")
    };
    list.into_iter().map(|a| a.rule).collect()
}

fn episode(start_ms: i64) -> dyno_core::throttle::ThrottleEpisode {
    dyno_core::throttle::ThrottleEpisode {
        id: dyno_core::throttle::ThrottleEpisode::id_for(Timestamp(start_ms)),
        start: Timestamp(start_ms),
        end: Some(Timestamp(start_ms + 120_000)),
        cause: dyno_core::throttle::ThrottleCause::Thermal,
        avg_lost_percent: 35.0,
        peak_lost_percent: 48.0,
        peak_temperature_c: Some(99.0),
    }
}

#[test]
fn a_throttling_episode_with_evidence_raises_an_alarm_and_is_remembered() {
    use dyno_core::hardware::Confidence;
    use dyno_core::throttle::TrackerEvent;
    let h = Harness::start();
    let peer = h.client();
    let s = shared(&h);
    let t = dyno_agent::agent::now();
    // A hint (low confidence) never alarms.
    dyno_agent::hardware::handle_events(&s, vec![TrackerEvent::Started(episode(t.millis()))], Some(Confidence::Low), t);
    assert!(alarm_rules(&peer).is_empty());
    // With evidence it does; the finished episode is stored and the alarm returns.
    dyno_agent::hardware::handle_events(&s, vec![TrackerEvent::Started(episode(t.millis()))], Some(Confidence::High), t);
    assert_eq!(alarm_rules(&peer), vec!["throttling:cpu"]);
    dyno_agent::hardware::handle_events(&s, vec![TrackerEvent::Ended(episode(t.millis()))], Some(Confidence::High), t);
    let Reply::Throttling(o) = ask(&peer, Request::Throttling).unwrap() else {
        panic!("throttling")
    };
    assert_eq!(o.episodes.len(), 1);
    assert_eq!(o.minutes_30d, 2);
    assert_eq!(o.inputs.len(), 4);
}

#[test]
fn throttling_alarms_can_be_switched_off_but_the_episode_is_still_recorded() {
    use dyno_core::hardware::Confidence;
    use dyno_core::throttle::TrackerEvent;
    let h = Harness::start();
    let peer = h.client();
    let mut settings = match ask(&peer, Request::Settings).unwrap() {
        Reply::Settings(x) => *x,
        _ => panic!("settings"),
    };
    settings.hardware_alarms.throttling = false;
    ask(
        &peer,
        Request::SaveSettings {
            settings: Box::new(settings),
        },
    )
    .unwrap();
    let s = shared(&h);
    let t = dyno_agent::agent::now();
    dyno_agent::hardware::handle_events(&s, vec![TrackerEvent::Started(episode(t.millis()))], Some(Confidence::High), t);
    dyno_agent::hardware::handle_events(&s, vec![TrackerEvent::Ended(episode(t.millis()))], Some(Confidence::High), t);
    assert!(alarm_rules(&peer).is_empty(), "switched off means no alarm");
    let Reply::Throttling(o) = ask(&peer, Request::Throttling).unwrap() else {
        panic!("throttling")
    };
    assert_eq!(o.episodes.len(), 1);
}

#[test]
fn lifespan_forecasts_come_from_stored_readings_and_a_worn_battery_raises_an_alarm() {
    use dyno_core::hardware::{BatteryCapacity, DiskWear};
    use dyno_core::metrics::Reading;
    use dyno_core::time::{DAY, HOUR};
    use dyno_platform::hardware::{FakeBattery, FakeThrottle, FakeWear};
    let h = Harness::start();
    let peer = h.client();
    let s = shared(&h);
    let battery = BatteryCapacity {
        id: "b1".into(),
        design: 5_500,
        full: 4_300,
        cycles: Some(640),
        rated_cycles: Some(1_000),
    };
    let disk = DiskWear {
        id: "Test SSD SN1".into(),
        name: "Test SSD".into(),
        capacity: Some(500_000_000_000),
        percent_used: Some(20.0),
        bytes_written: None,
        rated_bytes: None,
        power_on_hours: None,
    };
    s.hardware.replace_sources(
        Box::new(FakeThrottle(vec![Reading::unavailable("none")])),
        Box::new(FakeWear(Reading::Available(vec![disk.clone()]))),
        Box::new(FakeBattery(Reading::Available(battery.clone()))),
    );
    // An earlier history: the SSD wore 0.05 points a day for 200 days.
    let now = dyno_agent::agent::now();
    {
        let core = dyno_agent::agent::lock(&s.core);
        for i in 0..400 {
            let at = now.minus(200 * DAY).plus(i * 12 * HOUR);
            let pct = 10.0 + 0.05 * (i as f32 / 2.0);
            let d = DiskWear {
                percent_used: Some(pct.round()),
                ..disk.clone()
            };
            core.store
                .put_hardware_reading(dyno_agent::store::hardware_kind::SSD, &disk.id, at, &d)
                .unwrap();
        }
    }
    dyno_agent::hardware::slow_cycle(&s);
    let Reply::Lifespan(o) = ask(&peer, Request::Lifespan).unwrap() else {
        panic!("lifespan")
    };
    let ssd = o
        .forecasts
        .iter()
        .find(|f| f.subject == dyno_core::lifespan::LifeSubject::Ssd)
        .unwrap();
    assert!(ssd.mid_days().is_some(), "{ssd:?}");
    let bat = o
        .forecasts
        .iter()
        .find(|f| f.subject == dyno_core::lifespan::LifeSubject::Battery)
        .unwrap();
    // 78 % of the original charge is past the replacement line, which needs no history to say.
    assert_eq!(bat.outcome, dyno_core::lifespan::LifeOutcome::AlreadyPastLimit, "{bat:?}");
    assert!(alarm_rules(&peer).contains(&"lifespan:battery".to_string()));
}

#[test]
fn baselines_learn_silently_then_judge_and_alarm_only_when_asked() {
    use dyno_core::baseline::{Learning, State};
    use dyno_core::history::{HistoryPoint, Tier};
    use dyno_core::time::{DAY, MINUTE};
    let h = Harness::start();
    let peer = h.client();
    let s = shared(&h);
    // A new install learns silently: no items, and it says how far along it is.
    let Reply::Baselines(o) = ask(&peer, Request::Baselines).unwrap() else {
        panic!("baselines")
    };
    assert!(matches!(o.learning, Learning::Learning { days_needed: 7, .. }), "{:?}", o.learning);
    assert!(o.items.is_empty());
    // Four weeks of quiet history, first seen long ago.
    let now = dyno_agent::agent::now();
    let held = h.fake.hold_samples();
    assert!(held.wait_parked(Duration::from_secs(20)), "the sampler reached the hold");
    {
        let mut core = dyno_agent::agent::lock(&s.core);
        core.store.set_meta("firstSeen", &now.minus(30 * DAY).millis().to_string()).unwrap();
        for q in 0..(28 * 96) {
            let p = HistoryPoint {
                at: now.minus(28 * DAY).plus(q * 15 * MINUTE),
                cpu: 5.0,
                memory: 50.0,
                ..Default::default()
            };
            core.store.put_point(Tier::QuarterHour, &p).unwrap();
        }
        // The judged values are the agent's recent samples, and whether its sampler has run yet depends on the machine (on
        // the busy Windows runner it had not, CI runs 7166 and 7168): start from none, so the test never depends on it.
        core.recent.clear();
    }
    h.fake.push_sample(dyno_platform::fake::quiet_sample(Timestamp(0)));
    let Reply::Baselines(o) = ask(&peer, Request::Baselines).unwrap() else {
        panic!("baselines")
    };
    assert!(matches!(o.learning, Learning::Ready { .. }), "{:?}", o.learning);
    assert!(o.items.is_empty(), "nothing is judged before a sample: {:?}", o.items);
    // Sampling goes on: the CPU is judged once the agent has sampled it (waited for, whenever its sampler runs; the loop
    // is a hang guard).
    drop(held);
    s.wake_sampler();
    let mut judged = None;
    for _ in 0..100 {
        let Reply::Baselines(o) = ask(&peer, Request::Baselines).unwrap() else {
            panic!("baselines")
        };
        judged = o.items.into_iter().find(|i| i.subject == "System" && i.label == "CPU");
        if judged.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let cpu = judged.expect("a CPU item");
    assert_eq!(cpu.judgement.state, State::Usual, "{cpu:?}");
    assert!(cpu.judgement.phrase.starts_with("Usual for"), "{}", cpu.judgement.phrase);

    // The machine goes far above its usual: judged Higher, and with baseline alarms on, an alarm after a lasting streak.
    let mut busy = dyno_platform::fake::quiet_sample(Timestamp(0));
    busy.cpu.total = 95.0;
    h.fake.push_sample(busy);
    let mut settings = match ask(&peer, Request::Settings).unwrap() {
        Reply::Settings(x) => *x,
        _ => panic!("settings"),
    };
    settings.baselines.alarms = true;
    settings.sampling.interval_ms = 500;
    ask(
        &peer,
        Request::SaveSettings {
            settings: Box::new(settings),
        },
    )
    .unwrap();
    let mut seen_high = false;
    for _ in 0..100 {
        let Reply::Baselines(o) = ask(&peer, Request::Baselines).unwrap() else {
            panic!("baselines")
        };
        if o.items
            .iter()
            .any(|i| i.label == "CPU" && matches!(i.judgement.state, State::Higher(dyno_core::baseline::Strength::Strong)))
        {
            seen_high = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(seen_high, "a busy CPU is far higher than usual");
    assert!(alarm_rules(&peer).is_empty(), "one evaluation is not a lasting streak");
    for _ in 0..dyno_agent::baselines::ALARM_STREAK {
        dyno_agent::baselines::evaluate_alarms(&s);
    }
    assert!(alarm_rules(&peer).contains(&"baseline:cpu".to_string()), "{:?}", alarm_rules(&peer));
}

/// Rule 52 (coordinator review, 2026-10-30): the sampler fills the live view, so it runs as live work (normal priority on
/// Windows, never stalled behind busy normal threads).
#[test]
fn the_sampler_runs_as_live_work() {
    let h = Harness::start();
    let mut role = None;
    // Waits for the sampler's first sample (a hang guard bounds the loop).
    for _ in 0..200 {
        role = h.fake.with(|s| s.sample_role);
        if role.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(role, Some(dyno_platform::priority::ThreadRole::Live));
}

// ------------------------------------------------------------------------------------------------- one agent per folder
// A second agent on the same data folder refuses to start, says why, and leaves the first one's discovery file and
// history alone (a Dyno 1.0 test VM ran two agents on one `dyno.db` after a slow first start).

fn start_on(dirs: &Dirs) -> Result<Running, String> {
    let fake = Fake::new();
    fake.add_process(process(100, "busy", 60.0));
    Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).map_err(|e| e.to_string())
}

#[test]
fn a_second_agent_on_the_same_data_folder_refuses_to_start_and_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let first = start_on(&dirs).unwrap();
    let before = dyno_agent::discovery::Discovery::read(&dirs.discovery()).unwrap();

    let second = start_on(&dirs);
    let message = match second {
        Ok(r) => {
            r.stop();
            panic!("a second agent started on the same data folder");
        }
        Err(e) => e,
    };
    assert!(message.contains("already running"), "{message}");
    assert_eq!(
        dyno_agent::discovery::Discovery::read(&dirs.discovery()).unwrap(),
        before,
        "the refused agent left the first one's discovery file alone"
    );

    // Once the first one is gone, the folder is free again (the lock ends with its holder).
    first.stop();
    let third = start_on(&dirs).unwrap();
    assert_ne!(
        dyno_agent::discovery::Discovery::read(&dirs.discovery()).unwrap().endpoint,
        before.endpoint
    );
    third.stop();
}

#[test]
fn agents_on_different_data_folders_run_side_by_side() {
    let tmp = tempfile::tempdir().unwrap();
    let a = start_on(&Dirs::at(tmp.path().join("a"))).unwrap();
    let b = start_on(&Dirs::at(tmp.path().join("b"))).unwrap();
    a.stop();
    b.stop();
}
