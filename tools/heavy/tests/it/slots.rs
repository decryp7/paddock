//! Load-aware slots: how many steps run together follows the 15-minute load and the CPU idle share, up to a configured maximum;
//! a container run is alone; a waiter holds a ticket and never a slot (ER-43). Fixture folders and fake loads only (ER-14).
use crate::helpers::*;
use heavy::clock::Clock;
use heavy::config::Config;
use heavy::gate::{Pressure, allowed_slots, auto_max_slots, is_container_command};
use heavy::lock::{Acquirer, Poll, Request};

fn rig_with(max: u32) -> (Rig, heavy::config::Config) {
    let rig = Rig::new();
    let mut cfg = rig.cfg();
    cfg.slots = max;
    (rig, cfg)
}

fn req_cmd(name: &str, cmd: &str) -> Request {
    Request {
        cmdline: cmd.into(),
        ..req(name)
    }
}

#[test]
fn two_jobs_run_together_at_low_load_and_one_at_a_time_at_high_load() {
    // Low: 16 cores, 15-minute load 1.0, idle 100 %: room for several; the third still waits for its own turn, not for the load.
    let (rig, cfg) = rig_with(5);
    let (a, b) = (rig.agent_with(11, cfg.clone()), rig.agent_with(12, cfg.clone()));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    assert_eq!(hb.poll(), Poll::Held, "two together at low load");

    // High: the 15-minute load fills the cores: the second waits until the first is done.
    let (rig, cfg) = rig_with(5);
    rig.load.set(15.0);
    let (a, b) = (rig.agent_with(21, cfg.clone()), rig.agent_with(22, cfg));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(ha.poll(), Poll::Held, "the first always runs");
    assert_eq!(hb.poll(), Poll::Waiting, "serialised at high load");
    ha.release();
    assert_eq!(hb.poll(), Poll::Held, "it runs when the first is done");
}

#[test]
fn a_busy_cpu_or_an_unknown_load_keeps_it_to_one() {
    let (rig, cfg) = rig_with(5);
    rig.load.set_idle(Some(10.0));
    let (a, b) = (rig.agent_with(31, cfg.clone()), rig.agent_with(32, cfg));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    assert_eq!(hb.poll(), Poll::Waiting, "load is low but the CPU is busy");
    rig.load.set_idle(None);
    assert_eq!(hb.poll(), Poll::Waiting, "an unknown idle share is not a guess in favour of more");
    rig.load.set_idle(Some(80.0));
    assert_eq!(hb.poll(), Poll::Held);
}

#[test]
fn the_configured_maximum_bounds_the_count_at_low_load() {
    let (rig, cfg) = rig_with(2);
    let ags: Vec<_> = (41..44).map(|p| rig.agent_with(p, cfg.clone())).collect();
    let mut hs: Vec<_> = ags.iter().map(|a| Acquirer::new(&a.h, req("X")).unwrap()).collect();
    assert_eq!(hs[0].poll(), Poll::Held);
    assert_eq!(hs[1].poll(), Poll::Held);
    assert_eq!(hs[2].poll(), Poll::Waiting, "a maximum of 2");
}

#[test]
fn a_container_run_is_alone_and_nothing_starts_beside_it() {
    // A container run holds the machine: the steps that follow wait even at low load.
    let (rig, cfg) = rig_with(5);
    let (a, b) = (rig.agent_with(51, cfg.clone()), rig.agent_with(52, cfg));
    let mut ha = Acquirer::new(&a.h, req_cmd("A", "bash tools/linux-container.sh clippy")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    assert_eq!(hb.poll(), Poll::Waiting, "nothing beside a running container");
    ha.release();
    assert_eq!(hb.poll(), Poll::Held);
    // And a container run waits until the running steps are done.
    let (rig, cfg) = rig_with(5);
    let (a, b) = (rig.agent_with(61, cfg.clone()), rig.agent_with(62, cfg));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req_cmd("B", "docker run --rm x")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    assert_eq!(hb.poll(), Poll::Waiting, "a container run starts only on its own");
    ha.release();
    assert_eq!(hb.poll(), Poll::Held);
}

#[test]
fn a_waiter_holds_a_ticket_and_never_a_slot() {
    let (rig, cfg) = rig_with(5);
    rig.load.set(15.0);
    let (a, b, c) = (
        rig.agent_with(71, cfg.clone()),
        rig.agent_with(72, cfg.clone()),
        rig.agent_with(73, cfg),
    );
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    let mut hc = Acquirer::new(&c.h, req("C")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    assert_eq!(hb.poll(), Poll::Waiting);
    assert_eq!(hc.poll(), Poll::Waiting);
    assert_eq!(rig.count("pitcrew-heavy-v2.lock"), 1, "only the running step holds a slot folder");
    assert_eq!(rig.count("pitcrew-heavy.ticket."), 2, "the waiters hold tickets");
    // The load drops: the head of the queue takes a slot at once, and so does the next.
    rig.load.set(1.0);
    assert_eq!(hb.poll(), Poll::Held);
    assert_eq!(hc.poll(), Poll::Held);
}

#[test]
fn the_wait_line_names_the_limit() {
    let (rig, cfg) = rig_with(5);
    rig.load.set(15.0);
    let (a, b) = (rig.agent_with(81, cfg.clone()), rig.agent_with(82, cfg));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    assert_eq!(hb.poll(), Poll::Waiting);
    rig.clock.sleep(std::time::Duration::from_secs(2));
    assert_eq!(hb.poll(), Poll::Waiting);
    assert!(
        b.said().contains("1 of at most 5 at once") && b.said().contains("15-minute load 15.0"),
        "{}",
        b.said()
    );
}

#[test]
fn the_rule_in_numbers() {
    let p = |l: f64, i: f64| Pressure {
        load1: Some(l),
        load15: Some(l),
        idle: Some(i),
    };
    // 14 cores, 2 cores a step, at most 5
    assert_eq!(allowed_slots(5, 14, &p(0.0, 100.0), false), 5);
    assert_eq!(allowed_slots(5, 14, &p(7.0, 90.0), false), 3);
    assert_eq!(allowed_slots(5, 14, &p(12.0, 90.0), false), 1);
    assert_eq!(allowed_slots(5, 14, &p(14.0, 90.0), false), 1);
    assert_eq!(allowed_slots(5, 14, &p(80.0, 90.0), false), 1);
    assert_eq!(allowed_slots(5, 14, &p(0.0, 29.0), false), 1);
    // The 1-minute load is the fast brake: a burst the 15-minute average has not seen yet closes it.
    let burst = Pressure {
        load1: Some(14.0),
        load15: Some(2.0),
        idle: Some(90.0),
    };
    assert_eq!(allowed_slots(5, 14, &burst, false), 1);
    assert_eq!(allowed_slots(5, 14, &p(0.0, 100.0), true), 1);
    assert_eq!(allowed_slots(1, 14, &p(0.0, 100.0), false), 1);
    assert_eq!(
        allowed_slots(
            5,
            14,
            &Pressure {
                load1: None,
                load15: None,
                idle: Some(100.0)
            },
            false
        ),
        1
    );
    assert_eq!(
        allowed_slots(
            5,
            14,
            &Pressure {
                load1: Some(0.0),
                load15: Some(0.0),
                idle: None
            },
            false
        ),
        1
    );
    assert_eq!(
        (auto_max_slots(1), auto_max_slots(4), auto_max_slots(14), auto_max_slots(64)),
        (1, 2, 5, 5)
    );
    for c in ["bash tools/linux-container.sh x", "/usr/local/bin/docker run a", "podman ps"] {
        assert!(is_container_command(c), "{c}");
    }
    for c in ["cargo clippy", "cargo test docker_tests", "echo dockerfile"] {
        assert!(!is_container_command(c), "{c}");
    }
}

#[test]
fn auto_is_the_default_and_a_number_is_a_maximum() {
    let get = |pairs: &'static [(&'static str, &'static str)]| {
        Config::from_env(&|k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| (*v).to_string()), false)
    };
    let (c, w) = get(&[("HEAVY_CPUS", "14")]);
    assert_eq!((c.slots, c.cpus), (5, 14));
    assert!(w.is_empty(), "{w:?}");
    let (c, _) = get(&[("HEAVY_CPUS", "14"), ("HEAVY_SLOTS", "auto")]);
    assert_eq!(c.slots, 5);
    let (c, w) = get(&[("HEAVY_CPUS", "14"), ("HEAVY_SLOTS", "3")]);
    assert_eq!(c.slots, 3);
    assert!(w.is_empty(), "no other variable has to opt in: {w:?}");
    let (c, w) = get(&[("HEAVY_CPUS", "14"), ("HEAVY_SLOTS", "0")]);
    assert_eq!(c.slots, 5);
    assert_eq!(w.len(), 1);
}

#[test]
fn a_container_run_that_appears_beside_a_taker_makes_the_younger_give_its_slot_back() {
    let (rig, cfg) = rig_with(5);
    let a = rig.agent_with(91, cfg);
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    // Between A's take and its second look at the folder, a container run takes the second slot (it saw an empty machine too).
    let root = rig.root().to_path_buf();
    let procs = rig.procs.clone();
    let now = rig.now();
    *a.h.faults.after_take.lock().unwrap() = Some(Box::new(move || {
        procs.run(92, now.saturating_sub(150));
        let dir = root.join("pitcrew-heavy-v2.lock.slot2");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(
            dir.join("owner"),
            format!("92\nstart\nC\n{}\n/w\nbash tools/linux-container.sh x\n", now.saturating_sub(100)),
        )
        .unwrap();
    }));
    assert_eq!(ha.poll(), Poll::Waiting, "a container beside it: the younger one steps back");
    assert!(!rig.lock_dir().exists(), "its slot is free again");
    assert!(
        rig.root().join("pitcrew-heavy-v2.lock.slot2").exists(),
        "the container run keeps its slot"
    );
    assert_eq!(rig.count("pitcrew-heavy.ticket."), 1, "it keeps its place in the queue");
}

#[test]
fn a_taker_that_finds_more_holders_than_the_load_allows_steps_back_unless_it_is_the_oldest() {
    let (rig, cfg) = rig_with(5);
    let a = rig.agent_with(93, cfg.clone());
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let load = rig.load.clone();
    let (root, procs, now) = (rig.root().to_path_buf(), rig.procs.clone(), rig.now());
    *a.h.faults.after_take.lock().unwrap() = Some(Box::new(move || {
        // The load rose, and an older holder is there beside A.
        load.set(15.0);
        procs.run(94, now.saturating_sub(150));
        let dir = root.join("pitcrew-heavy-v2.lock.slot2");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(
            dir.join("owner"),
            format!("94\nstart\nO\n{}\n/w\ncargo test\n", now.saturating_sub(100)),
        )
        .unwrap();
    }));
    assert_eq!(ha.poll(), Poll::Waiting);
    // The oldest holder keeps its slot whatever the count: two takers never both step back.
    let (rig, cfg) = rig_with(5);
    let b = rig.agent_with(95, cfg);
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    let load = rig.load.clone();
    let (root, procs, now) = (rig.root().to_path_buf(), rig.procs.clone(), rig.now());
    *b.h.faults.after_take.lock().unwrap() = Some(Box::new(move || {
        load.set(15.0);
        procs.run(96, now.saturating_sub(5));
        let dir = root.join("pitcrew-heavy-v2.lock.slot2");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("owner"), format!("96\nstart\nY\n{}\n/w\ncargo test\n", now)).unwrap();
    }));
    assert_eq!(
        hb.poll(),
        Poll::Held,
        "B was there first: the younger one is the one that steps back"
    );
}

#[test]
fn a_binary_of_the_single_slot_kind_and_the_load_aware_ones_never_hold_more_than_the_maximum() {
    let rig = Rig::new();
    let mut new_cfg = rig.cfg();
    new_cfg.slots = 5;
    let old_cfg = rig.cfg(); // slots = 1: looks at the first slot only, as the binary before this change did
    let mut ags = vec![rig.agent_with(101, old_cfg)];
    ags.extend((102..107).map(|p| rig.agent_with(p, new_cfg.clone())));
    let mut hs: Vec<_> = ags.iter().map(|a| Acquirer::new(&a.h, req("X")).unwrap()).collect();
    for _ in 0..4 {
        for h in hs.iter_mut() {
            let _ = h.poll();
            let held = rig.count("pitcrew-heavy-v2.lock");
            assert!(held <= 5, "{held} holders beside each other");
        }
    }
    assert!(rig.count("pitcrew-heavy-v2.lock") >= 2, "several run together");
}

#[test]
fn a_waiting_container_run_is_not_kept_out_for_good_by_high_priority_runs() {
    let (rig, mut cfg) = rig_with(5);
    cfg.drain_after = std::time::Duration::from_secs(300);
    let (a, c, h) = (
        rig.agent_with(111, cfg.clone()),
        rig.agent_with(112, cfg.clone()),
        rig.agent_with(113, cfg),
    );
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hc = Acquirer::new(&c.h, req_cmd("C", "docker run --rm x")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    assert_eq!(hc.poll(), Poll::Waiting, "alone only");
    // It has waited long enough: it asks for the machine to drain.
    rig.clock.advance(std::time::Duration::from_secs(301));
    assert_eq!(hc.poll(), Poll::Waiting);
    assert_eq!(
        rig.count("pitcrew-heavy.drain."),
        1,
        "the container run asked for the machine to drain: {}",
        c.said()
    );
    let mut hh = Acquirer::new(&h.h, req_high("H")).unwrap();
    assert_eq!(
        hh.poll(),
        Poll::Waiting,
        "a high-priority run does not start beside A while a container run waits to drain the machine"
    );
    ha.release();
    assert_eq!(hc.poll(), Poll::Held, "the container run goes first");
    assert_eq!(hh.poll(), Poll::Waiting, "and nothing starts beside it");
    hc.release();
    assert_eq!(hh.poll(), Poll::Held);
}

#[test]
fn the_first_cpu_reading_is_primed_and_later_ones_are_percentages() {
    let l = heavy::gate::SysLoad::default();
    if cfg!(windows) {
        assert!(heavy::gate::LoadSource::idle_percent(&l).is_none());
        return;
    }
    let t = std::time::Instant::now();
    let mut got = None;
    while got.is_none() && t.elapsed() < std::time::Duration::from_secs(10) {
        got = heavy::gate::LoadSource::idle_percent(&l);
        std::thread::yield_now();
    }
    let idle = got.expect("a reading after the sampling interval");
    assert!((0.0..=100.0).contains(&idle), "{idle}");
}

#[test]
fn a_container_run_asks_for_the_drain_even_while_a_high_priority_marker_already_makes_it_wait() {
    let (rig, mut cfg) = rig_with(5);
    cfg.drain_after = std::time::Duration::from_secs(300);
    let (a, c) = (rig.agent_with(121, cfg.clone()), rig.agent_with(122, cfg));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hc = Acquirer::new(&c.h, req_cmd("C", "docker run --rm x")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    // A high-priority run announced itself long ago and is still live (a stream of them): the container run yields to it ...
    rig.procs.run(123, rig.now().saturating_sub(100));
    std::fs::write(rig.root().join("pitcrew-heavy.high.123-H"), format!("123\nstart\n{}\n", rig.now())).unwrap();
    assert_eq!(hc.poll(), Poll::Waiting);
    assert_eq!(rig.count("pitcrew-heavy.drain."), 0, "not yet");
    // ... but after drain_after it must still ask the machine to drain, not wait behind the high marker until priority_max.
    rig.clock.advance(std::time::Duration::from_secs(301));
    assert_eq!(hc.poll(), Poll::Waiting);
    assert_eq!(rig.count("pitcrew-heavy.drain."), 1, "{}", c.said());
}

#[test]
fn a_container_run_held_by_a_single_slot_binary_keeps_the_load_aware_ones_from_starting_beside_it() {
    // The binary before this change (slots = 1) holds the first slot with a container run; the new ones read its owner record.
    let rig = Rig::new();
    let old_cfg = rig.cfg();
    let mut new_cfg = rig.cfg();
    new_cfg.slots = 5;
    let old = rig.agent_with(131, old_cfg);
    let mut ho = Acquirer::new(&old.h, req_cmd("old", "docker run --rm x")).unwrap();
    assert_eq!(ho.poll(), Poll::Held);
    let ags: Vec<_> = (132..135).map(|p| rig.agent_with(p, new_cfg.clone())).collect();
    let mut hs: Vec<_> = ags.iter().map(|a| Acquirer::new(&a.h, req("n")).unwrap()).collect();
    for h in hs.iter_mut() {
        assert_eq!(h.poll(), Poll::Waiting, "nothing starts beside the old binary's container run");
    }
    ho.release();
    assert_eq!(hs[0].poll(), Poll::Held);
    assert_eq!(hs[1].poll(), Poll::Held, "and the load-aware ones run together once it is gone");
    // The gap, stated by the test: a binary of the single-slot kind looks at the first slot only, so it neither sees `.slotN`
    // holders nor drain markers. It takes the first slot when that is free, even beside new holders in other slots.
    hs[0].release();
    drop(hs.pop()); // the third one gives up its place in the queue
    let old2 = rig.agent_with(136, rig.cfg());
    let mut ho2 = Acquirer::new(&old2.h, req("old2")).unwrap();
    assert_eq!(
        ho2.poll(),
        Poll::Held,
        "the old binary starts beside the new holder of another slot (the documented gap)"
    );
    assert_eq!(rig.count("pitcrew-heavy-v2.lock"), 2);
}
