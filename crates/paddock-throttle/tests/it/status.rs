//! The queue status channel (`THROTTLE_STATUS_FILE`, `paddock_throttle::status`): a run appends queued, admitted and done to the file the
//! caller named, and a caller that names none gets none. Fixture folders and fake clocks only (ER-14).
use crate::helpers::*;
use paddock_throttle::exit;
use paddock_throttle::runner::{RunOpts, run};
use paddock_throttle::status::{self, Event};
use std::time::Duration;

fn opts() -> RunOpts {
    RunOpts {
        gate: false,
        low: false,
        queue_max: None,
        queue_stall: None,
        run_max: None,
        stall: None,
    }
}

fn events(path: &std::path::Path) -> Vec<Event> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(Event::parse)
        .collect()
}

#[test]
fn a_run_writes_queued_then_admitted_then_done_to_the_status_file() {
    let rig = Rig::new();
    let file = rig.root().join("row.queue.jsonl");
    let mut cfg = rig.cfg();
    cfg.status_file = Some(file.clone());
    let n = rig.agent_with(12, cfg);
    let args = vec!["true".to_string()];
    assert_eq!(run(&n.h, req("N"), &args, opts(), &|| None), 0);
    let got = events(&file);
    let kinds: Vec<&str> = got
        .iter()
        .map(|e| match e {
            Event::Queued { .. } => "queued",
            Event::Admitted { .. } => "admitted",
            Event::Done { .. } => "done",
        })
        .collect();
    assert_eq!(kinds, ["queued", "admitted", "done"], "{got:?}");
    let s = status::read(&file).expect("the file reads");
    assert!(
        !s.is_waiting() && s.running == 0 && s.unreadable == 0,
        "{s:?}"
    );
}

#[test]
fn a_run_that_never_gets_a_slot_ends_its_wait_with_done_and_its_exit_code() {
    let rig = Rig::new();
    let file = rig.root().join("row.queue.jsonl");
    let mut cfg = rig.cfg();
    cfg.status_file = Some(file.clone());
    let n = rig.agent_with(12, cfg);
    rig.procs.run(31, rig.now() - 50);
    let _holder = rig.plant_holder(&format!("31\nx\nholder\n{}\n/work\nsleep 9\n", rig.now()));
    let args = vec!["true".to_string()];
    let code = run(
        &n.h,
        req("N"),
        &args,
        RunOpts {
            queue_max: Some(Duration::from_secs(2)),
            ..opts()
        },
        &|| None,
    );
    assert_eq!(code, exit::QUEUE_LIMIT);
    let got = events(&file);
    assert!(matches!(got.first(), Some(Event::Queued { .. })), "{got:?}");
    assert!(
        matches!(got.last(), Some(Event::Done { code, ran_secs, .. }) if *code == exit::QUEUE_LIMIT && *ran_secs == 0.0),
        "{got:?}"
    );
    let s = status::read(&file).expect("the file reads");
    assert!(
        !s.is_waiting(),
        "a run that gave up is not waiting any more: {s:?}"
    );
    assert!(
        s.queued_closed_secs >= 2.0,
        "its wait is counted as queue time: {s:?}"
    );
}

#[test]
fn a_run_with_no_status_file_writes_none() {
    let rig = Rig::new();
    let n = rig.agent(12);
    let args = vec!["true".to_string()];
    assert_eq!(run(&n.h, req("N"), &args, opts(), &|| None), 0);
    let stray: Vec<_> = std::fs::read_dir(rig.root())
        .expect("the lock folder")
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains("queue.jsonl"))
        .collect();
    assert!(stray.is_empty(), "{stray:?}");
}

#[test]
fn the_variable_names_the_file_through_the_settings() {
    let (cfg, w) = paddock_throttle::config::Config::from_env(
        &|k| (k == status::STATUS_FILE_VAR).then(|| "/x/row.queue.jsonl".to_string()),
        &paddock_throttle::places::Host::none(),
    );
    assert_eq!(
        cfg.status_file,
        Some(std::path::PathBuf::from("/x/row.queue.jsonl"))
    );
    assert!(w.is_empty(), "{w:?}");
}
