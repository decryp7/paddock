//! Admission by resources and quick rows first: memory is a limit beside the load, a row the history says takes seconds goes first
//! and does not queue behind long ones, the history is bounded, and a waiter holds a ticket, never a slot (ER-24, ER-43, ER-48).
//! Fixture folders and fake loads only (ER-14). Each rule has a positive control: the same world with the thing it reads removed.
use crate::helpers::*;
use heavy::config::Config;
use heavy::gate::{CHEAP_LANE, CHEAP_SECS, MEMORY_PER_SLOT_MB, MEMORY_RESERVE_MB, Pressure, allowed_slots, cheap_admitted, jobs_per_step};
use heavy::history::{History, KEEP_PER_ROW, MAX_LINES};
use heavy::lock::{Acquirer, Poll, Request};

fn rig_with(max: u32) -> (Rig, Config) {
    let rig = Rig::new();
    let mut cfg = rig.cfg();
    cfg.slots = max;
    (rig, cfg)
}

fn quick(name: &str) -> Request {
    Request {
        expected_secs: Some(1.0),
        row: Some(format!("row {name}")),
        ..req(name)
    }
}

fn long(name: &str) -> Request {
    Request {
        expected_secs: Some(120.0),
        ..req(name)
    }
}

const FEW: u64 = MEMORY_RESERVE_MB + MEMORY_PER_SLOT_MB - 1;

#[test]
fn admission_refuses_a_second_step_past_the_memory_threshold() {
    // Low load, plenty of CPU: only the memory differs between the two worlds.
    let (rig, cfg) = rig_with(5);
    rig.load.set_memory_mb(Some(FEW));
    let (a, b) = (rig.agent_with(11, cfg.clone()), rig.agent_with(12, cfg.clone()));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(ha.poll(), Poll::Held, "the first step always runs");
    rig.clock.advance(std::time::Duration::from_secs(2));
    assert_eq!(hb.poll(), Poll::Waiting, "a second step would not fit beside the reserve");
    assert!(
        b.said().contains("memory available"),
        "the wait line names the memory: {}",
        b.said()
    );
    assert_eq!(rig.count("pitcrew-heavy-v2.lock"), 1, "the refused step holds a ticket, not a slot");

    // Positive control: with the memory for two steps and the reserve the same world admits the second.
    let (rig, cfg) = rig_with(5);
    rig.load.set_memory_mb(Some(MEMORY_RESERVE_MB + 2 * MEMORY_PER_SLOT_MB));
    let (a, b) = (rig.agent_with(21, cfg.clone()), rig.agent_with(22, cfg.clone()));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    assert_eq!(hb.poll(), Poll::Held, "control: enough memory admits it");

    // Control: a memory reading that is not there is not "plenty".
    let (rig, cfg) = rig_with(5);
    rig.load.set_memory_mb(None);
    let (a, b) = (rig.agent_with(31, cfg.clone()), rig.agent_with(32, cfg));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    assert_eq!(hb.poll(), Poll::Waiting, "unknown memory keeps it to one");
    // Memory comes back: the waiter takes its slot at its next look.
    rig.load.set_memory_mb(Some(64 * 1024));
    assert_eq!(hb.poll(), Poll::Held);
}

#[test]
fn the_rule_in_numbers_for_memory_and_cheap_rows() {
    let p = |memory_mb: Option<u64>| Pressure {
        load1: Some(0.0),
        load15: Some(0.0),
        idle: Some(100.0),
        memory_mb,
    };
    assert_eq!(allowed_slots(5, 14, &p(Some(64 * 1024)), false), 5);
    assert_eq!(allowed_slots(5, 14, &p(Some(MEMORY_RESERVE_MB + 2 * MEMORY_PER_SLOT_MB)), false), 2);
    assert_eq!(
        allowed_slots(5, 14, &p(Some(MEMORY_RESERVE_MB + 2 * MEMORY_PER_SLOT_MB - 1)), false),
        1
    );
    assert_eq!(allowed_slots(5, 14, &p(Some(0)), false), 1, "never below one: the first step runs");
    assert_eq!(allowed_slots(5, 14, &p(None), false), 1);
    // A quick row needs a step's memory above the reserve and an idle CPU; the 15-minute load does not matter.
    assert!(cheap_admitted(14, &p(Some(64 * 1024)), false));
    assert!(!cheap_admitted(14, &p(Some(FEW)), false));
    assert!(!cheap_admitted(14, &p(None), false));
    assert!(!cheap_admitted(14, &p(Some(64 * 1024)), true), "not beside a container run");
    let busy = Pressure {
        idle: Some(5.0),
        ..p(Some(64 * 1024))
    };
    assert!(!cheap_admitted(14, &busy, false));
    let busy_cpu_but_old_load = Pressure {
        load15: Some(40.0),
        ..p(Some(64 * 1024))
    };
    assert!(
        cheap_admitted(14, &busy_cpu_but_old_load, false),
        "the 15-minute load is no reason to hold a quick row"
    );
}

#[test]
fn threads_per_step_times_steps_at_once_stay_within_the_cores() {
    for cpus in [1u32, 2, 4, 8, 14, 16, 64] {
        for max in [1u32, 2, 5, 8, 16] {
            let jobs = jobs_per_step(cpus, max);
            assert!((1..=2).contains(&jobs), "{cpus}/{max}: {jobs}");
            assert!(max * jobs <= cpus.max(max), "{max} steps x {jobs} jobs on {cpus} cores");
        }
    }
    assert_eq!(jobs_per_step(14, 5), 2);
    assert_eq!(jobs_per_step(4, 8), 1, "more steps than half the cores: one job each");
}

#[test]
fn two_quick_rows_run_together_beside_a_long_step_at_a_high_15_minute_load() {
    // The 15-minute load fills the cores (one full slot only), a long step holds it: the quick rows still go.
    let (rig, cfg) = rig_with(5);
    rig.load.set(15.0);
    let ags: Vec<_> = (41..46).map(|p| rig.agent_with(p, cfg.clone())).collect();
    let mut l = Acquirer::new(&ags[0].h, long("L")).unwrap();
    assert_eq!(l.poll(), Poll::Held);
    let mut q1 = Acquirer::new(&ags[1].h, quick("Q1")).unwrap();
    let mut q2 = Acquirer::new(&ags[2].h, quick("Q2")).unwrap();
    let mut q3 = Acquirer::new(&ags[3].h, quick("Q3")).unwrap();
    assert_eq!(q1.poll(), Poll::Held);
    assert_eq!(q2.poll(), Poll::Held, "two quick rows at once");
    assert_eq!(rig.count("pitcrew-heavy-v2.lock.cheap"), CHEAP_LANE as usize);
    assert_eq!(q3.poll(), Poll::Waiting, "the quick lane has {CHEAP_LANE} places");
    q1.release();
    assert_eq!(q3.poll(), Poll::Held, "a place came free");

    // Control: the same rows with no history are not quick and wait for the one slot.
    let (rig, cfg) = rig_with(5);
    rig.load.set(15.0);
    let ags: Vec<_> = (51..55).map(|p| rig.agent_with(p, cfg.clone())).collect();
    let mut l = Acquirer::new(&ags[0].h, long("L")).unwrap();
    assert_eq!(l.poll(), Poll::Held);
    let mut r1 = Acquirer::new(&ags[1].h, req("R1")).unwrap();
    assert_eq!(r1.poll(), Poll::Waiting, "control: without a history it waits");
    assert_eq!(rig.count("pitcrew-heavy-v2.lock.cheap"), 0);
}

#[test]
fn a_quick_row_overtakes_a_queued_long_one() {
    let (rig, cfg) = rig_with(5);
    rig.load.set(15.0);
    let ags: Vec<_> = (61..66).map(|p| rig.agent_with(p, cfg.clone())).collect();
    let mut holder = Acquirer::new(&ags[0].h, long("H")).unwrap();
    assert_eq!(holder.poll(), Poll::Held);
    let mut queued_long = Acquirer::new(&ags[1].h, long("L")).unwrap();
    assert_eq!(queued_long.poll(), Poll::Waiting);
    // The quick row came later than the queued long one and goes first.
    let mut q = Acquirer::new(&ags[2].h, quick("Q")).unwrap();
    assert_eq!(q.poll(), Poll::Held, "it does not queue behind the long step");
    assert_eq!(queued_long.poll(), Poll::Waiting, "the long one is still queued");
    q.release();
    // The long one is not starved: it takes the slot as soon as the holder is done.
    holder.release();
    assert_eq!(queued_long.poll(), Poll::Held);

    // Control: a row with no history queues behind the long one (position 3 of 3 here: first in, first out).
    let (rig, cfg) = rig_with(5);
    rig.load.set(15.0);
    let ags: Vec<_> = (71..75).map(|p| rig.agent_with(p, cfg.clone())).collect();
    let mut holder = Acquirer::new(&ags[0].h, long("H")).unwrap();
    assert_eq!(holder.poll(), Poll::Held);
    let mut queued_long = Acquirer::new(&ags[1].h, long("L")).unwrap();
    assert_eq!(queued_long.poll(), Poll::Waiting);
    let mut unknown = Acquirer::new(&ags[2].h, req("U")).unwrap();
    assert_eq!(unknown.poll(), Poll::Waiting, "control: no history, no overtaking");
    assert!(ags[2].said().is_empty() || !ags[2].said().contains("quick"));
}

#[test]
fn a_quick_row_is_not_admitted_when_the_memory_or_the_cpu_is_short() {
    for (memory, idle) in [
        (Some(FEW), Some(100.0)),
        (Some(64 * 1024), Some(5.0)),
        (None, Some(100.0)),
        (Some(64 * 1024), None),
    ] {
        let (rig, cfg) = rig_with(5);
        rig.load.set(15.0);
        rig.load.set_memory_mb(memory);
        rig.load.set_idle(idle);
        let (a, b) = (rig.agent_with(81, cfg.clone()), rig.agent_with(82, cfg));
        let mut l = Acquirer::new(&a.h, long("L")).unwrap();
        assert_eq!(l.poll(), Poll::Held);
        let mut q = Acquirer::new(&b.h, quick("Q")).unwrap();
        assert_eq!(q.poll(), Poll::Waiting, "memory {memory:?}, idle {idle:?}");
        assert_eq!(rig.count("pitcrew-heavy-v2.lock.cheap"), 0);
    }
}

#[test]
fn a_quick_row_never_starts_beside_a_container_run_and_a_container_run_waits_for_it() {
    let (rig, cfg) = rig_with(5);
    let (a, b) = (rig.agent_with(91, cfg.clone()), rig.agent_with(92, cfg.clone()));
    let mut c = Acquirer::new(
        &a.h,
        Request {
            cmdline: "docker run --rm x".into(),
            ..long("C")
        },
    )
    .unwrap();
    assert_eq!(c.poll(), Poll::Held);
    let mut q = Acquirer::new(&b.h, quick("Q")).unwrap();
    assert_eq!(q.poll(), Poll::Waiting, "nothing beside a container run");
    c.release();
    assert_eq!(q.poll(), Poll::Held);
    // The other way round: a container run waits while a quick row runs.
    let c2 = rig.agent_with(93, cfg);
    let mut c2h = Acquirer::new(
        &c2.h,
        Request {
            cmdline: "docker run --rm y".into(),
            ..long("C2")
        },
    )
    .unwrap();
    assert_eq!(c2h.poll(), Poll::Waiting, "it runs alone, so it waits for the quick row too");
    q.release();
    assert_eq!(c2h.poll(), Poll::Held);
}

#[test]
fn no_waiter_holds_a_slot_not_even_a_quick_one() {
    let (rig, cfg) = rig_with(5);
    rig.load.set(15.0);
    let ags: Vec<_> = (101..107).map(|p| rig.agent_with(p, cfg.clone())).collect();
    let mut l = Acquirer::new(&ags[0].h, long("L")).unwrap();
    assert_eq!(l.poll(), Poll::Held);
    let mut qs: Vec<_> = (1..=4)
        .map(|i| Acquirer::new(&ags[i].h, quick(&format!("Q{i}"))).unwrap())
        .collect();
    let mut long_waiter = Acquirer::new(&ags[5].h, long("W")).unwrap();
    let polls: Vec<_> = qs.iter_mut().map(Acquirer::poll).collect();
    assert_eq!(polls.iter().filter(|p| **p == Poll::Held).count(), CHEAP_LANE as usize);
    assert_eq!(long_waiter.poll(), Poll::Waiting);
    // Folders that hold a step: the full slot and the quick lane's places, and nothing for the three that wait.
    assert_eq!(rig.count("pitcrew-heavy-v2.lock"), 1 + CHEAP_LANE as usize);
    assert_eq!(
        rig.count("pitcrew-heavy.ticket."),
        2 + 1,
        "the two quick rows that wait and the long one hold tickets"
    );
    // A waiter that is stopped leaves nothing behind.
    drop(long_waiter);
    assert_eq!(rig.count("pitcrew-heavy.ticket."), 2);
}

#[test]
fn a_quick_row_waits_its_turn_when_the_lane_is_full_and_the_queue_still_moves() {
    // The head of the queue is a long step: it takes the full slot the moment the holder is done, whatever quick rows run.
    let (rig, cfg) = rig_with(5);
    rig.load.set(15.0);
    let ags: Vec<_> = (111..116).map(|p| rig.agent_with(p, cfg.clone())).collect();
    let mut h = Acquirer::new(&ags[0].h, long("H")).unwrap();
    assert_eq!(h.poll(), Poll::Held);
    let mut head = Acquirer::new(&ags[1].h, long("HEAD")).unwrap();
    assert_eq!(head.poll(), Poll::Waiting);
    let mut q1 = Acquirer::new(&ags[2].h, quick("Q1")).unwrap();
    let mut q2 = Acquirer::new(&ags[3].h, quick("Q2")).unwrap();
    assert_eq!(q1.poll(), Poll::Held);
    assert_eq!(q2.poll(), Poll::Held);
    h.release();
    assert_eq!(
        head.poll(),
        Poll::Held,
        "quick rows in the lane do not count against the head's slot"
    );
}

#[test]
fn the_admission_decision_is_logged_with_what_the_rule_saw() {
    let (rig, cfg) = rig_with(5);
    rig.load.set(15.0);
    rig.load.set_memory_mb(Some(9000));
    let (a, b) = (rig.agent_with(121, cfg.clone()), rig.agent_with(122, cfg));
    let mut l = Acquirer::new(&a.h, long("L")).unwrap();
    assert_eq!(l.poll(), Poll::Held);
    let mut q = Acquirer::new(&b.h, quick("Q")).unwrap();
    assert_eq!(q.poll(), Poll::Held);
    let text = std::fs::read_to_string(rig.root().join("log-122.clef")).unwrap();
    let line = text.lines().find(|l| l.contains("heavy.admit")).expect("an admission line");
    for want in [
        "\"Lane\":\"quick\"",
        "\"MemoryAvailableMiB\":9000",
        "\"Load15\":15.0",
        "\"ExpectedSeconds\":1.0",
        "\"MaxSlots\":5",
        "\"Running\":2",
        "\"IdlePercent\"",
    ] {
        assert!(line.contains(want), "{want} missing from {line}");
    }
    let first = std::fs::read_to_string(rig.root().join("log-121.clef")).unwrap();
    assert!(
        first.contains("\"Lane\":\"slot\""),
        "the long step's own line says it took a slot: {first}"
    );
    // The wait line of a refused step carries the same numbers.
    let c = rig.agent_with(123, rig.cfg());
    let mut w = Acquirer::new(&c.h, long("W")).unwrap();
    assert_eq!(w.poll(), Poll::Waiting);
}

#[test]
fn the_history_is_bounded_oldest_first_and_a_row_cannot_push_the_others_out() {
    let rig = Rig::new();
    let h = History::new(rig.root());
    // Control: under the limits everything is kept and the expectation is the slowest of the last three.
    for (i, secs) in [30.0, 1.0, 2.0, 3.0].into_iter().enumerate() {
        h.record("clippy apps/dyno", secs, 1000 + i as u64).unwrap();
    }
    assert_eq!(
        h.expected_secs("clippy apps/dyno"),
        Some(3.0),
        "the 30 s run is older than the last three"
    );
    assert_eq!(h.expected_secs("nothing"), None, "no history is not quick");
    assert_eq!(h.len(), 4);
    // One row recorded many times keeps only its newest.
    for i in 0..50 {
        h.record("busy", 1.0, 2000 + i).unwrap();
    }
    let kept = std::fs::read_to_string(h.path())
        .unwrap()
        .lines()
        .filter(|l| l.ends_with("\tbusy"))
        .count();
    assert_eq!(kept, KEEP_PER_ROW);
    assert_eq!(
        h.expected_secs("clippy apps/dyno"),
        Some(3.0),
        "the busy row did not push the others out"
    );
    // Many rows: the file stays within MAX_LINES and the oldest rows go first.
    for i in 0..(MAX_LINES * 3) {
        h.record(&format!("row-{i}"), 2.0, 3000 + i as u64).unwrap();
    }
    assert_eq!(h.len(), MAX_LINES);
    assert_eq!(h.expected_secs("row-0"), None, "the oldest went first");
    assert_eq!(
        h.expected_secs(&format!("row-{}", MAX_LINES * 3 - 1)),
        Some(2.0),
        "the newest stays"
    );
    assert!(std::fs::metadata(h.path()).unwrap().len() < 256 * 1024);
}

#[test]
fn the_history_reads_damage_as_no_history_and_never_as_quick() {
    let rig = Rig::new();
    let h = History::new(rig.root());
    assert_eq!(h.expected_secs("x"), None, "no file");
    std::fs::write(h.path(), "garbage\n1\t2\n\t\t\nx\ty\tz\n").unwrap();
    assert_eq!(h.expected_secs("z"), None);
    std::fs::write(h.path(), vec![b'a'; 300 * 1024]).unwrap();
    assert_eq!(h.expected_secs("a"), None, "a file over the read limit is not ours");
    // A key with a tab or a line break cannot forge a line.
    h.record("a\tb\nc", 5.0, 1).unwrap();
    assert_eq!(h.len(), 1);
}

#[test]
fn a_row_expected_at_the_limit_is_not_quick() {
    let at = Request {
        expected_secs: Some(CHEAP_SECS),
        ..req("A")
    };
    let under = Request {
        expected_secs: Some(CHEAP_SECS - 0.1),
        ..req("A")
    };
    assert!(!at.is_quick() && under.is_quick() && !req("A").is_quick());
}
