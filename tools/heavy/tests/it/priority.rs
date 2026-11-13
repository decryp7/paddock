//! The priority lane (ADR 0102): a landing push's heavy step goes first, other runs yield while it is queued or running, the
//! yielding is bounded, and queue time and run time are separate (queue limit, run limit). Fixture folders and fake clocks only (ER-14).
use crate::helpers::*;
use heavy::exit;
use heavy::lock::{Acquirer, Poll};
use heavy::runner::{RunOpts, run};
use std::fs;
use std::time::Duration;

fn live_marker(rig: &Rig, pid: u32, name: &str, age_secs: u64) -> std::path::PathBuf {
    rig.procs.run(pid, rig.now() - 50 - age_secs);
    let m = rig.root().join(format!("pitcrew-heavy.high.{pid}-{name}"));
    fs::write(&m, format!("{pid}\nx\n{}\n", rig.now() - age_secs)).unwrap();
    m
}

fn opts() -> RunOpts {
    RunOpts {
        gate: false,
        low: false,
        queue_max: None,
        run_max: None,
    }
}

#[test]
fn a_priority_marker_older_than_the_bound_is_ignored_and_said() {
    let rig = Rig::new();
    let mut cfg = rig.cfg();
    cfg.priority_max = Duration::from_secs(600);
    let n = rig.agent_with(12, cfg);
    live_marker(&rig, 72, "hung", 601);
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    assert_eq!(
        acq.poll(),
        Poll::Held,
        "a priority marker older than the bound does not starve everyone"
    );
    assert!(n.said().contains("priority bound"), "{}", n.said());
}

#[test]
fn a_waiter_stops_yielding_to_priority_after_waiting_the_bound_itself() {
    let rig = Rig::new();
    let mut cfg = rig.cfg();
    cfg.priority_max = Duration::from_secs(600);
    let n = rig.agent_with(12, cfg);
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    rig.clock.advance(Duration::from_secs(601));
    // a stream of fresh priority runs: each marker is young, but this waiter has been waiting longer than the bound
    live_marker(&rig, 73, "next", 0);
    assert_eq!(acq.poll(), Poll::Held);
}

#[test]
fn a_young_marker_and_a_young_waiter_still_yield() {
    let rig = Rig::new();
    let mut cfg = rig.cfg();
    cfg.priority_max = Duration::from_secs(600);
    let n = rig.agent_with(12, cfg);
    live_marker(&rig, 74, "go", 10);
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    assert_eq!(acq.poll(), Poll::Waiting);
}

#[test]
fn with_several_slots_the_marker_lasts_while_the_priority_run_holds_one() {
    let rig = Rig::new();
    let mut cfg = rig.cfg();
    cfg.slots = 2;
    let (hh, b) = (rig.agent_with(13, cfg.clone()), rig.agent_with(12, cfg));
    let mut hi = Acquirer::new(&hh.h, req_high("H")).unwrap();
    assert_eq!(hi.poll(), Poll::Held);
    assert_eq!(rig.count("pitcrew-heavy.high."), 1, "still announced while it runs");
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(
        hb.poll(),
        Poll::Waiting,
        "a free slot is not taken by a normal run while a priority run is going"
    );
    hi.release();
    assert_eq!(rig.count("pitcrew-heavy.high."), 0);
    assert_eq!(hb.poll(), Poll::Held);
}

#[test]
fn a_run_that_never_gets_a_slot_within_the_queue_limit_ends_with_its_own_code_and_queue_time() {
    let rig = Rig::new();
    let n = rig.agent(12);
    rig.procs.run(31, rig.now() - 50);
    rig.plant_lock(&format!("31\nx\nholder\n{}\n/work\nsleep 9\n", rig.now()));
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
    let said = n.said();
    assert!(said.contains("queue limit") && said.contains("queued 2."), "{said}");
}

#[test]
fn queue_and_run_time_are_reported_apart() {
    let rig = Rig::new();
    let n = rig.agent(12);
    let args = vec!["true".to_string()];
    assert_eq!(run(&n.h, req("N"), &args, opts(), &|| None), 0);
    let said = n.said();
    assert!(said.contains("N timing queued 0.0 s, ran "), "{said}");
}

#[cfg(unix)]
#[test]
fn a_command_over_the_run_limit_is_ended_with_the_run_limit_code() {
    let rig = Rig::new();
    let n = rig.agent(12);
    let args = ["sh", "-c", "sleep 30"].map(String::from).to_vec();
    let t = std::time::Instant::now();
    let code = run(
        &n.h,
        req("N"),
        &args,
        RunOpts {
            run_max: Some(Duration::from_secs(1)),
            ..opts()
        },
        &|| None,
    );
    assert_eq!(code, exit::RUN_LIMIT);
    assert!(t.elapsed() < Duration::from_secs(20));
    assert!(n.said().contains("run limit"), "{}", n.said());
}
