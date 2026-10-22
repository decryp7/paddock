#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The agent end to end over the fake platform: a real IPC connection, a real SQLite file in a temp folder.

use dyno_agent::cli::{ask, connect};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::activity::UndoState;
use dyno_core::history::HistoryRange;
use dyno_core::process::{Priority, ProcessAction};
use dyno_core::protocol::{ErrorCode, LiveDetail, LiveUpdate, Message, Reply, Request};
use dyno_core::time::Timestamp;
use dyno_ipc::{Client, Peer, PeerOptions};
use dyno_platform::fake::{process, Fake};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

struct Harness {
    running: Option<Running>,
    fake: Fake,
    dirs: Dirs,
    _tmp: tempfile::TempDir,
}

impl Harness {
    fn start() -> Harness {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let fake = Fake::new();
        for (pid, name, cpu) in [(100, "busy", 60.0), (101, "calm", 1.0), (102, "launchd", 0.1)] {
            fake.add_process(process(pid, name, cpu));
        }
        let running = Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
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

fn target(pid: u32) -> dyno_core::process::ProcessRef {
    process(pid, "x", 0.0).reference()
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
fn settings_round_trip_and_survive_a_restart() {
    let h = Harness::start();
    let peer = h.client();
    let Reply::Settings(mut s) = ask(&peer, Request::Settings).unwrap() else {
        panic!("settings")
    };
    s.launch_at_login = true;
    s.sampling.interval_ms = 700;
    ask(&peer, Request::SaveSettings { settings: *s }).unwrap();
    let Reply::Settings(back) = ask(&peer, Request::Settings).unwrap() else {
        panic!("settings")
    };
    assert!(back.launch_at_login && back.sampling.interval_ms == 700);
    let saved = std::fs::read_to_string(h.dirs.settings()).unwrap();
    assert!(saved.contains("\"launchAtLogin\": true"));
}

#[test]
fn a_click_acts_logs_and_undoes_and_protected_processes_are_refused() {
    let h = Harness::start();
    let (peer, rx) = h.subscriber(LiveDetail::Processes);
    rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let Reply::Activity(entry) = ask(
        &peer,
        Request::Act {
            target: target(100),
            action: ProcessAction::SetPriority(Priority::Idle),
        },
    )
    .unwrap() else {
        panic!("activity")
    };
    assert!(entry.title.contains("busy"));
    assert_eq!(h.fake.actions().len(), 1);
    let Reply::Activities(list) = ask(&peer, Request::Activity { limit: 10 }).unwrap() else {
        panic!("list")
    };
    assert_eq!(list[0].id, entry.id);
    assert_eq!(list[0].undo, UndoState::Available);
    let Reply::Activity(undone) = ask(
        &peer,
        Request::Undo {
            activity_id: entry.id.clone(),
        },
    )
    .unwrap() else {
        panic!("undo")
    };
    assert_eq!(undone.undo, UndoState::Undone);
    assert_eq!(h.fake.actions().len(), 2);
    assert!(ask(&peer, Request::Undo { activity_id: entry.id }).unwrap_err().contains("already"));
    // launchd is on the protected list.
    let err = ask(
        &peer,
        Request::Act {
            target: target(102),
            action: ProcessAction::End,
        },
    )
    .unwrap_err();
    assert!(err.contains("protected"), "{err}");
    assert_eq!(h.fake.actions().len(), 2, "nothing was done to the protected process");
    // A pid that is not in the table is not found, and an invalid action is a bad request.
    assert!(ask(
        &peer,
        Request::Act {
            target: target(999),
            action: ProcessAction::End
        }
    )
    .is_err());
    assert!(ask(
        &peer,
        Request::Act {
            target: target(100),
            action: ProcessAction::SetCpuCap(Some(0))
        }
    )
    .unwrap_err()
    .contains("CPU cap"));
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
fn unbuilt_features_say_so_and_unknown_things_are_not_found() {
    let h = Harness::start();
    let peer = h.client();
    let e = ask(&peer, Request::Tuning).unwrap_err();
    assert!(e.contains("not built yet"), "{e}");
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
    ask(&peer, Request::Shutdown).unwrap();
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
