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
        queue_stall: None,
        run_max: None,
        stall: None,
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

// A CI job on this host (ADR 0102): the job's holder process announces it with `heavy mark ci-<slot>`, agents' normal runs yield
// while it lives, the bound is the job's own (`ci_priority_max`), and a dead holder or an old marker never blocks for good.

#[test]
fn a_ci_job_marker_holds_normal_runs_past_the_push_gates_bound() {
    let rig = Rig::new();
    let mut cfg = rig.cfg();
    cfg.priority_max = Duration::from_secs(600);
    cfg.ci_priority_max = Duration::from_secs(7200);
    let ci = rig.agent_with(81, cfg.clone());
    let n = rig.agent_with(12, cfg);
    rig.procs.run(81, rig.now() - 50);
    ci.h.mark("ci-s1", 81).unwrap();
    rig.clock.advance(Duration::from_secs(601));
    rig.procs.run(81, rig.now() - 700);
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    assert_eq!(
        acq.poll(),
        Poll::Waiting,
        "a job that runs 10 minutes still goes first: its bound is the CI one"
    );
}

#[test]
fn a_ci_job_marker_older_than_the_ci_bound_is_ignored_and_said() {
    let rig = Rig::new();
    let mut cfg = rig.cfg();
    cfg.ci_priority_max = Duration::from_secs(7200);
    let ci = rig.agent_with(82, cfg.clone());
    let n = rig.agent_with(12, cfg);
    rig.procs.run(82, rig.now() - 50);
    ci.h.mark("ci-s1", 82).unwrap();
    rig.clock.advance(Duration::from_secs(7201));
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    assert_eq!(acq.poll(), Poll::Held, "a crashed or hung job cannot block the agents for good");
    assert!(n.said().contains("priority bound 7200 s"), "{}", n.said());
}

#[test]
fn a_ci_job_marker_of_a_dead_holder_is_removed_and_does_not_block() {
    let rig = Rig::new();
    let ci = rig.agent(83);
    let n = rig.agent(12);
    rig.procs.run(83, rig.now() - 50);
    ci.h.mark("ci-s1", 83).unwrap();
    rig.procs.end(83);
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    assert_eq!(acq.poll(), Poll::Held);
    assert_eq!(rig.count("pitcrew-heavy.high."), 0);
}

#[test]
fn unmark_withdraws_the_marker_so_the_next_run_goes_at_once() {
    let rig = Rig::new();
    let ci = rig.agent(84);
    let n = rig.agent(12);
    rig.procs.run(84, rig.now() - 50);
    ci.h.mark("ci-s1", 84).unwrap();
    assert_eq!(rig.count("pitcrew-heavy.high."), 1);
    assert!(ci.h.unmark("ci-s1", 84));
    assert_eq!(rig.count("pitcrew-heavy.high."), 0);
    assert!(!ci.h.unmark("ci-s1", 84), "a second withdrawal finds nothing and is no error");
    assert_eq!(Acquirer::new(&n.h, req("N")).unwrap().poll(), Poll::Held);
}

/// The queue is bounded by progress: a holder that never lets go stalls it, and the wait names who held the slot.
#[test]
fn a_queue_that_makes_no_progress_ends_stalled_and_names_the_holder() {
    let rig = Rig::new();
    let n = rig.agent(12);
    rig.procs.run(31, rig.now() - 50);
    let _holder = rig.plant_holder(&format!("31\nx\nholder\n{}\n/work\nsleep 9\n", rig.now()));
    let args = vec!["true".to_string()];
    let code = run(
        &n.h,
        req("N"),
        &args,
        RunOpts {
            queue_stall: Some(Duration::from_secs(600)),
            ..opts()
        },
        &|| None,
    );
    assert_eq!(code, exit::QUEUE_LIMIT);
    let said = n.said();
    assert!(said.contains("queue stalled") && said.contains("holder (pid 31)"), "{said}");
}

/// The other half: a queue whose holders change keeps its waiter far past the stall limit, then the waiter takes the slot and runs.
#[test]
fn a_queue_that_keeps_moving_is_waited_for_past_the_stall_limit() {
    use std::sync::atomic::{AtomicU64, Ordering};
    let rig = Rig::new();
    let n = rig.agent(12);
    let args = vec!["true".to_string()];
    let (swaps, last) = (AtomicU64::new(0), AtomicU64::new(rig.now()));
    let start = rig.now();
    rig.procs.run(99, start - 50);
    let cur = std::sync::Mutex::new(None);
    *cur.lock().unwrap() = Some(rig.plant_holder(&format!("99\nx\nholder-first\n{start}\n/work\nsleep 9\n")));
    // The stop callback is read once per poll; it hands the slot to a new holder every 100 s of fake time (12 times: four times the
    // stall limit), then leaves it free.
    let stop = || {
        let (now, done) = (rig.now(), swaps.load(Ordering::SeqCst));
        if now.saturating_sub(last.load(Ordering::SeqCst)) >= 100 && done < 12 {
            drop(cur.lock().unwrap().take());
            let pid = 100 + done as u32;
            rig.procs.run(pid, now - 50);
            *cur.lock().unwrap() = Some(rig.plant_holder(&format!("{pid}\nx\nholder-{done}\n{now}\n/work\nsleep 9\n")));
            swaps.store(done + 1, Ordering::SeqCst);
            last.store(now, Ordering::SeqCst);
        } else if done >= 12 && now.saturating_sub(last.load(Ordering::SeqCst)) >= 100 {
            drop(cur.lock().unwrap().take());
        }
        None
    };
    let code = run(
        &n.h,
        req("N"),
        &args,
        RunOpts {
            queue_stall: Some(Duration::from_secs(300)),
            ..opts()
        },
        &stop,
    );
    assert_eq!(code, 0, "{}", n.said());
    assert!(rig.now() - start >= 1200, "it waited 1200 s of fake time, 4 times the stall limit");
}

/// A holder running inside the bound this waiter gives its own command is progress, even when nothing else in the queue changes: the
/// waiter is not stalled after the stall limit but waits until the holder lets go (here 20 minutes, four times the limit).
#[test]
fn a_waiter_behind_a_live_holder_inside_its_cap_is_not_stalled() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let rig = Rig::new();
    let n = rig.agent(12);
    let args = vec!["true".to_string()];
    let start = rig.now();
    rig.procs.run(31, start - 50);
    let cur = std::sync::Mutex::new(None);
    *cur.lock().unwrap() = Some(rig.plant_holder(&format!("31\nx\nholder\n{start}\n/work\nsleep 9\n")));
    let released = AtomicBool::new(false);
    let stop = || {
        if rig.now() - start >= 1200 && !released.swap(true, Ordering::SeqCst) {
            drop(cur.lock().unwrap().take());
        }
        None
    };
    let code = run(
        &n.h,
        req("N"),
        &args,
        RunOpts {
            queue_stall: Some(Duration::from_secs(300)),
            run_max: Some(Duration::from_secs(3600)),
            ..opts()
        },
        &stop,
    );
    assert_eq!(code, 0, "{}", n.said());
    assert!(rig.now() - start >= 1200);
}

/// The same holder past that bound is not trusted any more: it is the holder's own guard that ends it, and a waiter that sees nothing
/// move for the stall limit after that ends the wait.
#[test]
fn a_waiter_behind_a_holder_past_its_cap_stalls_after_the_limit() {
    let rig = Rig::new();
    let n = rig.agent(12);
    let args = vec!["true".to_string()];
    rig.procs.run(31, rig.now() - 8000);
    // The record is two hours old: past the 3600 s a command may run.
    let _holder = rig.plant_holder(&format!("31\nx\nholder\n{}\n/work\nsleep 9\n", rig.now() - 7200));
    let code = run(
        &n.h,
        req("N"),
        &args,
        RunOpts {
            queue_stall: Some(Duration::from_secs(300)),
            run_max: Some(Duration::from_secs(3600)),
            ..opts()
        },
        &|| None,
    );
    assert_eq!(code, exit::QUEUE_LIMIT);
    assert!(
        n.said().contains("queue stalled") && n.said().contains("holder (pid 31)"),
        "{}",
        n.said()
    );
}

/// A slot given back (taken just as the load rose) keeps the claim's age: the ticket comes back under its own name, so a younger waiter
/// does not pass it, and a load that keeps swinging cannot push a run to the back again and again.
#[test]
fn a_slot_given_back_keeps_the_place_of_the_ticket() {
    let rig = Rig::new();
    let (a, b) = (rig.agent(12), rig.agent(13));
    let mut first = Acquirer::new(&a.h, req("A")).unwrap();
    // A younger waiter queues behind A.
    rig.clock.advance(Duration::from_secs(5));
    let mut second = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(first.poll(), Poll::Held);
    first.step_back();
    // B is the only ticket left in the folder unless A's came back; B must wait for A, which is older.
    assert_eq!(second.poll(), Poll::Waiting, "A's ticket kept its age, B is second");
    assert_eq!(first.poll(), Poll::Held, "A is first again");
}
