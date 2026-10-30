#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Local days end to end: the agent runs in a zone eight hours from UTC (the fake platform's zone port), and today's
//! issue score, the history's crash days, quiet hours, "until tomorrow", maintenance windows, learned baselines and a
//! backup's local time all follow that zone, never the UTC date. Each test fails when the agent reads UTC.

use dyno_agent::cli::{ask, connect, run_job};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::alarms::{ChangeKind, Shelf, ShelveFor};
use dyno_core::backup::BackupTool;
use dyno_core::extras::maintenance::{Weekday, Window};
use dyno_core::history::{HistoryPoint, HistoryRange, Tier};
use dyno_core::issues::{civil_from_days, IssuesQuery};
use dyno_core::jobs::{JobOutput, JobSpec};
use dyno_core::ports::PlatformError;
use dyno_core::protocol::{LiveDetail, Message, Reply, Request};
use dyno_core::time::{day_start, local_minute, next_day_start, to_local, FixedZone, Timestamp, DAY, HOUR, MINUTE, SECOND};
use dyno_ipc::Peer;
use dyno_platform::fake::{process, quiet_sample, Fake};
use dyno_platform::issues::ToolRunner;
use std::sync::Arc;
use std::time::Duration;

mod common;
use common::{full_disk_sample, wait_for_alarm};

/// UTC+8 (Singapore, no daylight saving): every local midnight is 16:00 UTC, never a UTC midnight.
const ZONE: FixedZone = FixedZone(480);

struct Harness {
    running: Option<Running>,
    fake: Fake,
    dirs: Dirs,
    _tmp: tempfile::TempDir,
}

impl Harness {
    /// The agent over the fake in [`ZONE`], with `first` as the sample it reads from the start.
    fn start(first: Option<dyno_core::metrics::SystemSample>) -> Harness {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let fake = Fake::new();
        fake.add_process(process(100, "calm", 1.0));
        if let Some(s) = first {
            fake.push_sample(s);
        }
        let mut platform = fake.platform();
        platform.zone = Arc::new(ZONE);
        let running = Running::start(dirs.clone(), platform, Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
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

    fn shared(&self) -> Arc<dyno_agent::agent::Shared> {
        self.running.as_ref().unwrap().shared.clone()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop();
        }
    }
}

fn now() -> Timestamp {
    dyno_agent::agent::now()
}

/// `f` of the time before and after a request: the request saw one of them, even across a local midnight.
fn either(before: Timestamp, after: Timestamp, f: impl Fn(Timestamp) -> Timestamp) -> [Timestamp; 2] {
    [f(before), f(after)]
}

fn settings(peer: &Peer<Message>) -> dyno_core::settings::Settings {
    let Reply::Settings(s) = ask(peer, Request::Settings).unwrap() else {
        panic!("settings")
    };
    *s
}

fn save(peer: &Peer<Message>, s: dyno_core::settings::Settings) {
    ask(peer, Request::SaveSettings { settings: Box::new(s) }).unwrap();
}

/// Minutes `m` around the current local minute, wrapping past midnight.
fn around_local_now(m: i32) -> (u16, u16) {
    let at = i32::from(local_minute(now(), &ZONE));
    ((at - m).rem_euclid(1_440) as u16, (at + m).rem_euclid(1_440) as u16)
}

#[test]
fn todays_issue_score_and_the_crash_chart_start_at_local_midnight() {
    let h = Harness::start(None);
    let peer = h.client();
    let before = now();
    let Reply::Issues(o) = ask(
        &peer,
        Request::Issues {
            query: IssuesQuery { days: 1 },
        },
    )
    .unwrap() else {
        panic!("issues")
    };
    let after = now();
    assert!(
        either(before, after, |t| day_start(t, &ZONE)).contains(&o.from),
        "today starts at local midnight: {o:?}"
    );
    assert_eq!(o.from.millis().rem_euclid(DAY), 16 * HOUR, "a local midnight in UTC+8 is 16:00 UTC");

    let Reply::History(hist) = ask(
        &peer,
        Request::History {
            range: HistoryRange::Week,
            end: None,
        },
    )
    .unwrap() else {
        panic!("history")
    };
    let first = hist.crashes.first().expect("a crash count per day").day;
    assert_eq!(first, day_start(hist.from, &ZONE));
    assert!(
        hist.crashes.iter().all(|d| d.day.millis().rem_euclid(DAY) == 16 * HOUR),
        "{:?}",
        hist.crashes
    );
}

#[test]
fn quiet_hours_and_until_tomorrow_follow_the_local_clock() {
    let h = Harness::start(None);
    let peer = h.client();
    // Quiet for the hour around the local time now, which is eight hours away from the UTC time.
    let mut s = settings(&peer);
    let (start, end) = around_local_now(60);
    s.alarms.quiet.enabled = true;
    s.alarms.quiet.start_minute = start;
    s.alarms.quiet.end_minute = end;
    s.alarms.quiet.critical_bypass = false;
    save(&peer, s);
    ask(
        &peer,
        Request::Subscribe {
            detail: LiveDetail::Summary,
        },
    )
    .unwrap();
    h.fake.push_sample(full_disk_sample());
    let alarm = wait_for_alarm(&peer);
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
    let raised = log.iter().find(|c| c.kind == ChangeKind::Raised).expect("the raise is logged");
    assert!(!raised.notify, "raised inside the local quiet hours: no notification");

    let before = now();
    let Reply::Alarms(changed) = ask(
        &peer,
        Request::ShelveAlarm {
            id: alarm.id,
            until: ShelveFor::UntilTomorrow,
        },
    )
    .unwrap() else {
        panic!("alarms")
    };
    let after = now();
    let Some(Shelf::Until(until)) = changed[0].shelved else {
        panic!("shelved until a time: {:?}", changed[0].shelved)
    };
    assert!(
        either(before, after, |t| next_day_start(t, &ZONE)).contains(&until),
        "until the next local midnight: {until:?}"
    );
}

#[test]
fn maintenance_windows_are_local_times() {
    let h = Harness::start(None);
    let peer = h.client();
    let mut s = settings(&peer);
    let (start, end) = around_local_now(60);
    s.maintenance.enabled = true;
    s.maintenance.windows = vec![Window {
        days: vec![
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ],
        start_minute: start,
        end_minute: end,
    }];
    save(&peer, s);
    let Reply::Maintenance(m) = ask(&peer, Request::Maintenance).unwrap() else {
        panic!("maintenance")
    };
    assert!(m.open_now, "the window around the local time now is open: {m:?}");
}

#[test]
fn baselines_learn_and_judge_by_the_local_hour() {
    use dyno_core::baseline::State;
    let mut busy = quiet_sample(Timestamp(0));
    busy.cpu.total = 95.0;
    let h = Harness::start(Some(busy));
    let s = h.shared();
    // Four weeks where the CPU is busy only in this local hour of each day and the next (the judgement may come after the
    // hour turns), and quiet otherwise.
    let t = now();
    let hour_of = |at: Timestamp| to_local(at, &ZONE).rem_euclid(DAY) / HOUR;
    let busy_hours = [hour_of(t), (hour_of(t) + 1) % 24];
    {
        let core = dyno_agent::agent::lock(&s.core);
        core.store.set_meta("firstSeen", &t.minus(30 * DAY).millis().to_string()).unwrap();
        for q in 0..(28 * 96) {
            let at = t.minus(28 * DAY).plus(q * 15 * MINUTE);
            let cpu = if busy_hours.contains(&hour_of(at)) { 95.0 } else { 5.0 };
            core.store
                .put_point(
                    Tier::QuarterHour,
                    &HistoryPoint {
                        at,
                        cpu,
                        memory: 50.0,
                        ..Default::default()
                    },
                )
                .unwrap();
        }
    }
    let peer = h.client();
    let mut last = None;
    for _ in 0..100 {
        let Reply::Baselines(o) = ask(&peer, Request::Baselines).unwrap() else {
            panic!("baselines")
        };
        last = o.items.into_iter().find(|i| i.subject == "System" && i.label == "CPU");
        if last.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let cpu = last.expect("a CPU item");
    assert_eq!(cpu.judgement.state, State::Usual, "busy is usual at this local hour: {cpu:?}");
    // The words name the local weekday and part of the day. Learning and judging in UTC would cancel out in the
    // numbers, but not in the words: eight hours always change the part of the day (the longest, nights, is seven).
    let after = now();
    let words = |at: Timestamp| {
        let local = to_local(at, &ZONE);
        let weekday = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]
            [(local.div_euclid(DAY) + 3).rem_euclid(7) as usize];
        let part = match local.rem_euclid(DAY) / HOUR {
            5..=11 => "mornings",
            12..=16 => "afternoons",
            17..=21 => "evenings",
            _ => "nights",
        };
        format!("Usual for {weekday} {part}")
    };
    assert!([words(t), words(after)].contains(&cpu.judgement.phrase), "{cpu:?}");
}

/// Answers `tmutil` with a destination and the given latest backup.
struct TimeMachine(String);

impl ToolRunner for TimeMachine {
    fn run(&self, _program: &str, args: &[String], _timeout: Duration) -> Result<String, PlatformError> {
        match args.first().map(String::as_str) {
            Some("destinationinfo") => Ok("Name : Disk\n".into()),
            Some("latestbackup") => Ok(self.0.clone()),
            _ => Err(PlatformError::Unsupported("not scripted".into())),
        }
    }
}

#[test]
fn a_time_machine_folder_name_is_read_as_local_time() {
    let h = Harness::start(None);
    let s = h.shared();
    s.extras.use_os("macos");
    // The folder of a backup 30 hours ago, named in local time as Time Machine names it.
    let at = Timestamp((now().millis() - 30 * HOUR) / SECOND * SECOND);
    let local = to_local(at, &ZONE);
    let (y, m, d) = civil_from_days(local.div_euclid(DAY));
    let secs = local.rem_euclid(DAY) / SECOND;
    let name = format!(
        "/Volumes/.timemachine/X/{y:04}-{m:02}-{d:02}-{:02}{:02}{:02}.backup\n",
        secs / 3_600,
        secs / 60 % 60,
        secs % 60
    );
    s.extras.use_backup_tools(Arc::new(TimeMachine(name)));
    let JobOutput::Backup(r) = run_job(&h.client(), JobSpec::RefreshBackup, Duration::from_secs(30)).unwrap() else {
        panic!("backup")
    };
    let tm = r.statuses.iter().find(|x| x.tool == BackupTool::TimeMachine).expect("Time Machine");
    assert_eq!(tm.last_backup, Some(at), "{tm:?}");
}

#[test]
fn the_diagnostics_bundle_names_the_zone_in_use() {
    let h = Harness::start(None);
    let bundle = dyno_agent::diagnostics::collect(&h.shared()).unwrap();
    let zone = bundle
        .files
        .iter()
        .find(|f| f.path == "system/time-zone.json")
        .expect("the time zone file");
    assert!(zone.text.contains("UTC+08:00"), "{}", zone.text);
}
