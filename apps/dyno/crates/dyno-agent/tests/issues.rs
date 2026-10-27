#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Issues (WP-06) and new and unusual programs (WP-07) end to end over the fake platform: scripted log sources, a real
//! IPC connection and a real SQLite file in a temp folder. Nothing here reads the machine's own logs.

use dyno_agent::cli::{ask, connect};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::issues::{Issue, IssueKind, IssueSource, IssueSourceId, IssuesQuery, SourceState};
use dyno_core::ports::PlatformError;
use dyno_core::protocol::{LiveDetail, Message, Reply, Request};
use dyno_core::time::{Timestamp, HOUR, MINUTE};
use dyno_core::unusual::baseline::UnusualAction;
use dyno_ipc::Peer;
use dyno_platform::fake::{process, Fake};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn real_now() -> Timestamp {
    Timestamp(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
    )
}

/// A source that returns what the test put in it, and counts how often it was read.
#[derive(Clone)]
struct Scripted {
    id: IssueSourceId,
    result: Arc<Mutex<Result<Vec<Issue>, PlatformError>>>,
    reads: Arc<Mutex<u32>>,
    panics: bool,
}

impl Scripted {
    fn new(id: IssueSourceId, result: Result<Vec<Issue>, PlatformError>) -> Scripted {
        Scripted {
            id,
            result: Arc::new(Mutex::new(result)),
            reads: Arc::default(),
            panics: false,
        }
    }
}

impl IssueSource for Scripted {
    fn id(&self) -> IssueSourceId {
        self.id
    }
    fn read(&self, _since: Timestamp, _until: Timestamp) -> Result<Vec<Issue>, PlatformError> {
        *self.reads.lock().unwrap() += 1;
        assert!(!self.panics, "a reader that crashes");
        self.result.lock().unwrap().clone()
    }
}

struct Harness {
    running: Option<Running>,
    fake: Fake,
    dirs: Dirs,
    _tmp: tempfile::TempDir,
}

impl Harness {
    fn start(sources: Vec<Box<dyn IssueSource>>) -> Harness {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let fake = Fake::new();
        fake.add_process(process(100, "calm", 1.0));
        let mut platform = fake.platform();
        platform.issues = sources;
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

fn crash(minutes_ago: i64, app: &str, key: &str) -> Issue {
    Issue::new(
        IssueSourceId::MacReports,
        real_now().minus(minutes_ago * MINUTE),
        IssueKind::AppCrash,
        app,
        "t",
        "evidence text",
        key,
    )
}

fn overview(peer: &Peer<Message>, rescan: bool) -> dyno_core::issues::IssuesOverview {
    let query = IssuesQuery { days: 14 };
    let req = if rescan {
        Request::RescanIssues { query }
    } else {
        Request::Issues { query }
    };
    let Reply::Issues(o) = ask(peer, req).unwrap() else {
        panic!("issues")
    };
    *o
}

#[test]
fn a_rescan_stores_issues_once_scores_the_day_and_says_which_source_is_unavailable() {
    let good = Scripted::new(
        IssueSourceId::MacReports,
        Ok(vec![crash(30, "Mail", "a"), crash(200, "Mail", "b"), crash(300, "Notes", "c")]),
    );
    let bad = Scripted::new(
        IssueSourceId::MacLog,
        Err(PlatformError::Timeout("log did not finish within 60 s.".into())),
    );
    let h = Harness::start(vec![Box::new(good.clone()), Box::new(bad)]);
    let peer = h.client();
    let o = overview(&peer, true);
    assert_eq!(o.total, 3);
    assert_eq!(o.apps[0].name, "Mail");
    assert_eq!(o.apps[0].count, 2);
    assert!(o.score.is_some_and(|s| s < 100), "{:?}", o.score);
    assert_eq!(o.daily.len(), 14);
    let states: Vec<&SourceState> = o.sources.iter().map(|s| &s.state).collect();
    assert_eq!(states[0], &SourceState::Read(3));
    assert_eq!(states[1], &SourceState::Unavailable("log did not finish within 60 s.".into()));
    // Reading the same log again changes nothing.
    assert_eq!(overview(&peer, true).total, 3);
    assert!(*good.reads.lock().unwrap() >= 2);
    // The plain query only reads what is stored.
    let reads = *good.reads.lock().unwrap();
    assert_eq!(overview(&peer, false).total, 3);
    assert_eq!(*good.reads.lock().unwrap(), reads);
}

#[test]
fn when_no_source_can_be_read_no_score_is_claimed() {
    let bad = Scripted::new(
        IssueSourceId::Journal,
        Err(PlatformError::AccessDenied("not in the journal group".into())),
    );
    let h = Harness::start(vec![Box::new(bad)]);
    let o = overview(&h.client(), true);
    assert_eq!(o.score, None);
    assert!(o.daily.is_empty());
    assert_eq!(o.sources[0].state, SourceState::Unavailable("not in the journal group".into()));
}

#[test]
fn a_reader_that_crashes_is_contained_and_the_others_still_count() {
    let mut boom = Scripted::new(IssueSourceId::MacLog, Ok(vec![]));
    boom.panics = true;
    let good = Scripted::new(IssueSourceId::MacReports, Ok(vec![crash(10, "Mail", "a")]));
    let h = Harness::start(vec![Box::new(boom), Box::new(good)]);
    let o = overview(&h.client(), true);
    assert_eq!(o.total, 1);
    assert!(matches!(&o.sources[0].state, SourceState::Unavailable(m) if m.contains("stopped unexpectedly")));
    assert_eq!(o.sources[1].state, SourceState::Read(1));
}

#[test]
fn crashes_fill_the_history_chart_the_health_reason_and_the_weekly_report() {
    let crashes: Vec<Issue> = (0..6).map(|n| crash(10 + n * 100, "Flaky", &format!("k{n}"))).collect();
    let h = Harness::start(vec![Box::new(Scripted::new(IssueSourceId::MacReports, Ok(crashes)))]);
    let peer = h.client();
    overview(&peer, true);
    // History: the crashes list is no longer empty (it was always empty before issues existed).
    let Reply::History(hist) = ask(
        &peer,
        Request::History {
            range: dyno_core::history::HistoryRange::Week,
            end: None,
        },
    )
    .unwrap() else {
        panic!("history")
    };
    assert!(hist.crashes.iter().map(|d| d.crashes).sum::<u32>() >= 6, "{:?}", hist.crashes);
    // Health: wait for the first sample, then the "crashing apps" reason is there.
    let mut report = None;
    for _ in 0..100 {
        if let Ok(Reply::Health(r)) = ask(&peer, Request::Health) {
            report = Some(r);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let report = report.expect("a health report");
    assert!(
        report
            .reasons
            .iter()
            .any(|r| r.id == "crashes" && r.title.contains("6 app crashes")),
        "{:?}",
        report.reasons
    );
    // The weekly report counts them.
    let Reply::Report(w) = ask(&peer, Request::WeeklyReport { weeks_ago: 0 }).unwrap() else {
        panic!("report")
    };
    assert_eq!(w.crashes, 6);
}

#[test]
fn a_crash_loop_is_an_anomaly_and_a_serious_issue_is_an_alarm() {
    let t = real_now();
    let loop_: Vec<Issue> = (0..3).map(|n| crash(5 + n * 10, "Loopy", &format!("l{n}"))).collect();
    let mut all = loop_;
    all.push(Issue::new(
        IssueSourceId::MacLog,
        t.minus(20 * MINUTE),
        IssueKind::BadShutdown,
        "",
        "The Mac shut down without warning (cause 0)",
        "AppleSMC: Previous shutdown cause: 0",
        "x",
    ));
    all.push(Issue::new(
        IssueSourceId::MacLog,
        t.minus(3 * 24 * HOUR),
        IssueKind::DiskError,
        "",
        "A disk reported an input/output error",
        "old",
        "y",
    ));
    let h = Harness::start(vec![Box::new(Scripted::new(IssueSourceId::MacLog, Ok(all)))]);
    let peer = h.client();
    overview(&peer, true);
    let Reply::Anomalies(list) = ask(&peer, Request::Anomalies).unwrap() else {
        panic!("anomalies")
    };
    assert!(
        list.iter().any(|a| a.id == "restartLoop:Loopy" && a.title.contains("3 times")),
        "{list:?}"
    );
    let Reply::Alarms(alarms) = ask(
        &peer,
        Request::Alarms {
            filter: Default::default(),
        },
    )
    .unwrap() else {
        panic!("alarms")
    };
    let issue_alarms: Vec<_> = alarms.iter().filter(|a| a.rule.starts_with("issue:")).collect();
    assert_eq!(issue_alarms.len(), 1, "only the recent serious issue raises an alarm: {alarms:?}");
    assert_eq!(issue_alarms[0].rule, "issue:badshutdown");
    assert!(issue_alarms[0].detail.contains("Previous shutdown cause"));
}

#[test]
fn issues_export_as_json_lines_to_a_chosen_file_only() {
    let h = Harness::start(vec![Box::new(Scripted::new(
        IssueSourceId::MacReports,
        Ok(vec![crash(10, "Mail", "a"), crash(20, "Notes", "b")]),
    ))]);
    let peer = h.client();
    overview(&peer, true);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("issues.jsonl");
    let Reply::IssuesSaved(saved) = ask(
        &peer,
        Request::SaveIssues {
            path: path.to_string_lossy().into_owned(),
            days: 7,
        },
    )
    .unwrap() else {
        panic!("saved")
    };
    assert_eq!(saved.count, 2);
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert!(text.lines().all(|l| serde_json::from_str::<serde_json::Value>(l).is_ok()));
    assert_eq!(saved.bytes as usize, text.len());
    for bad in [
        "relative.jsonl",
        &dir.path().join("x.txt").to_string_lossy(),
        &dir.path().join("..").join("x.jsonl").to_string_lossy(),
        &dir.path().join("missing").join("x.jsonl").to_string_lossy(),
    ] {
        assert!(
            ask(
                &peer,
                Request::SaveIssues {
                    path: bad.to_string(),
                    days: 7
                }
            )
            .is_err(),
            "{bad}"
        );
    }
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    for _ in 0..200 {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

fn unusual(peer: &Peer<Message>, include_ignored: bool) -> dyno_core::unusual::UnusualOverview {
    let Reply::Unusual(o) = ask(peer, Request::Unusual { include_ignored }).unwrap() else {
        panic!("unusual")
    };
    *o
}

/// Subscribes to the process table. With nobody watching the agent refreshes it only every 10 s, which is the same as
/// the 10 s these tests wait for a new process; watching makes it follow the one-second system interval.
fn watch_processes(peer: &Peer<Message>) {
    ask(
        peer,
        Request::Subscribe {
            detail: LiveDetail::Processes,
        },
    )
    .unwrap();
}

/// A program path in the host operating system's temporary folder, which is what the agent judges it by.
fn temp_exe(name: &str) -> String {
    if cfg!(windows) {
        format!(r"C:\Users\tester\AppData\Local\Temp\{name}.exe")
    } else {
        format!("/tmp/{name}")
    }
}

fn shared_has_process(h: &Harness, name: &str) -> bool {
    dyno_agent::agent::lock(&h.shared().core)
        .latest
        .processes
        .iter()
        .any(|p| p.name == name)
}

#[test]
fn a_new_program_after_learning_is_listed_with_reasons_and_can_be_protected_ignored_and_forgotten() {
    let h = Harness::start(vec![]);
    let peer = h.client();
    watch_processes(&peer);
    // No learning period: the first look still only learns what is already running.
    let Reply::Settings(mut s) = ask(&peer, Request::Settings).unwrap() else {
        panic!("settings")
    };
    s.unusual.learning_days = 0;
    ask(&peer, Request::SaveSettings { settings: s }).unwrap();
    wait_until("the first process table", || shared_has_process(&h, "calm"));
    let shared = h.shared();
    shared.unusual.look(&shared);
    let o = unusual(&peer, false);
    assert!(o.items.is_empty(), "the first look is silent");
    assert_eq!(o.known, 1);
    // A program that starts later, from a temporary folder, is listed and explained, and writes one activity line.
    let mut odd = process(200, "helper", 1.0);
    odd.exe = Some(temp_exe("helper"));
    h.fake.add_process(odd);
    wait_until("the new process", || shared_has_process(&h, "helper"));
    shared.unusual.look(&shared);
    let o = unusual(&peer, false);
    assert_eq!(o.items.len(), 1);
    let item = &o.items[0];
    assert_eq!(item.name, "helper");
    assert!(item.reasons.iter().any(|r| r.contains("temporary folder")), "{:?}", item.reasons);
    assert!(item.reasons.iter().all(|r| !r.to_lowercase().contains("malware")));
    let Reply::Activities(log) = ask(&peer, Request::Activity { limit: 20 }).unwrap() else {
        panic!("activity")
    };
    let lines: Vec<_> = log.iter().filter(|e| e.title == "Unusual program: helper").collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].detail.contains("Unusual because it"));
    shared.unusual.look(&shared);
    let Reply::Activities(log) = ask(&peer, Request::Activity { limit: 20 }).unwrap() else {
        panic!("activity")
    };
    assert_eq!(log.iter().filter(|e| e.title == "Unusual program: helper").count(), 1, "said once");
    // Protect: the rules' protected list gets it and it leaves the list; nothing is acted on.
    let Reply::Unusual(after) = ask(
        &peer,
        Request::UnusualAct {
            key: item.key.clone(),
            action: UnusualAction::Protect,
        },
    )
    .unwrap() else {
        panic!("unusual")
    };
    assert!(after.items.is_empty());
    let Reply::Rules(rules) = ask(&peer, Request::Rules).unwrap() else {
        panic!("rules")
    };
    assert!(rules.protected.iter().any(|p| *p == temp_exe("helper")), "{:?}", rules.protected);
    assert!(h.fake.actions().is_empty());
    // Restore brings it back, Ignore hides it, and the ignored one shows when asked.
    ask(
        &peer,
        Request::UnusualAct {
            key: item.key.clone(),
            action: UnusualAction::Restore,
        },
    )
    .unwrap();
    assert_eq!(unusual(&peer, false).items.len(), 1);
    ask(
        &peer,
        Request::UnusualAct {
            key: item.key.clone(),
            action: UnusualAction::Ignore,
        },
    )
    .unwrap();
    assert!(unusual(&peer, false).items.is_empty());
    assert_eq!(unusual(&peer, true).items.len(), 1);
    assert!(ask(
        &peer,
        Request::UnusualAct {
            key: "nothing".into(),
            action: UnusualAction::Ignore
        }
    )
    .is_err());
    // The baseline survives; forgetting it starts learning again.
    let Reply::Unusual(f) = ask(&peer, Request::ForgetBaseline).unwrap() else {
        panic!("unusual")
    };
    assert_eq!(f.known, 0);
    assert!(f.items.is_empty());
    assert!(f.learning.until.is_none());
}

#[test]
fn a_fresh_install_learns_silently_for_the_configured_period() {
    let h = Harness::start(vec![]);
    let peer = h.client();
    watch_processes(&peer);
    wait_until("the first process table", || shared_has_process(&h, "calm"));
    let shared = h.shared();
    shared.unusual.look(&shared);
    let mut odd = process(300, "tmp-thing", 1.0);
    odd.exe = Some(temp_exe("tmp-thing"));
    h.fake.add_process(odd);
    wait_until("the new process", || shared_has_process(&h, "tmp-thing"));
    shared.unusual.look(&shared);
    let o = unusual(&peer, false);
    assert!(o.learning.active, "seven days by default");
    assert!(o.items.is_empty(), "nothing is flagged while learning");
    assert_eq!(o.known, 2);
    // Turned off, it does nothing at all.
    let Reply::Settings(mut s) = ask(&peer, Request::Settings).unwrap() else {
        panic!("settings")
    };
    s.unusual.enabled = false;
    ask(&peer, Request::SaveSettings { settings: s }).unwrap();
    let mut more = process(301, "another", 1.0);
    more.exe = Some(temp_exe("another"));
    h.fake.add_process(more);
    wait_until("the other process", || shared_has_process(&h, "another"));
    shared.unusual.look(&shared);
    assert_eq!(unusual(&peer, false).known, 2);
    assert!(!unusual(&peer, false).enabled);
}

#[test]
fn the_baseline_survives_an_agent_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let fake = Fake::new();
    fake.add_process(process(100, "calm", 1.0));
    let start = || Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
    let r = start();
    for _ in 0..200 {
        if dyno_agent::agent::lock(&r.shared.core)
            .latest
            .processes
            .iter()
            .any(|p| p.name == "calm")
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    r.shared.unusual.look(&r.shared);
    let known = r.shared.unusual.overview(&r.shared, false).known;
    assert_eq!(known, 1);
    r.stop();
    let r = start();
    let o = r.shared.unusual.overview(&r.shared, false);
    assert_eq!(o.known, 1);
    assert!(o.learning.until.is_some());
    r.stop();
}
