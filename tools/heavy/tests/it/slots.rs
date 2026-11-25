//! Load-aware slots: how many steps run together follows the 5-minute load and the memory available, up to a configured maximum;
//! a container run is alone; a waiter holds a ticket and never a slot (ER-43). Fixture folders and fake loads only (ER-14).
use crate::helpers::*;
use heavy::clock::Clock;
use heavy::config::Config;
use heavy::gate::{Pressure, SLOT_LOAD_SHARE, allowed_slots, auto_max_slots, is_container_command};
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
    // Low: 16 cores, 5-minute load 1.0: room for several.
    let (rig, cfg) = rig_with(5);
    let (a, b) = (rig.agent_with(11, cfg.clone()), rig.agent_with(12, cfg.clone()));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    assert_eq!(hb.poll(), Poll::Held, "two together at low load");

    // High: the 5-minute load fills the cores: the second waits until the first is done.
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
fn an_unknown_load_keeps_it_to_one_and_a_busy_cpu_alone_does_not() {
    let (rig, cfg) = rig_with(5);
    rig.load.set_idle(Some(10.0));
    let (a, b) = (rig.agent_with(31, cfg.clone()), rig.agent_with(32, cfg));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    // The control: a low 5-minute load admits a second step whatever the momentary idle share says (the threads are split).
    assert_eq!(hb.poll(), Poll::Held, "the 5-minute load decides, not a momentary idle reading");
    // An unknown load: one at a time.
    let (rig, cfg) = rig_with(5);
    let (a, b) = (rig.agent_with(34, cfg.clone()), rig.agent_with(35, cfg));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    rig.load.set_memory_mb(None);
    assert_eq!(hb.poll(), Poll::Waiting, "an unknown reading is not a guess in favour of more");
    rig.load.set_memory_mb(Some(64 * 1024));
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
    assert_eq!(rig.held("heavy-"), 1, "only the running step holds a slot folder");
    assert_eq!(rig.count("pitcrew-heavy.ticket."), 2, "the waiters hold tickets");
    // The load drops: the head of the queue takes the second slot at once; the next takes one when a holder ends (at most two).
    rig.load.set(1.0);
    assert_eq!(hb.poll(), Poll::Held);
    assert_eq!(hc.poll(), Poll::Waiting);
    ha.release();
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
        b.said().contains("1 of at most 5 at once") && b.said().contains("5-minute load 15.0"),
        "{}",
        b.said()
    );
}

#[test]
fn the_rule_in_numbers() {
    let p = |l: f64| Pressure {
        load1: Some(l),
        load5: Some(l),
        load15: Some(l),
        idle: Some(100.0),
        memory_mb: Some(64 * 1024),
    };
    // 14 cores: two under the measured limit (11.9), else one; never above the auto maximum of two, whatever the setting.
    let two = SLOT_LOAD_SHARE[0] * 14.0;
    assert!((two - 11.9).abs() < 1e-9, "{two}");
    assert_eq!(allowed_slots(5, 14, &p(0.0), false), 2);
    assert_eq!(allowed_slots(5, 14, &p(two - 0.1), false), 2);
    assert_eq!(allowed_slots(5, 14, &p(two), false), 1);
    assert_eq!(allowed_slots(5, 14, &p(20.0), false), 1);
    assert_eq!(allowed_slots(5, 14, &p(80.0), false), 1);
    assert_eq!(allowed_slots(5, 14, &p(0.0), true), 1);
    assert_eq!(allowed_slots(1, 14, &p(0.0), false), 1);
    assert_eq!(allowed_slots(5, 14, &Pressure { load5: None, ..p(0.0) }, false), 1);
    assert_eq!(
        (
            auto_max_slots(1),
            auto_max_slots(4),
            auto_max_slots(8),
            auto_max_slots(14),
            auto_max_slots(64)
        ),
        (1, 1, 2, 2, 2)
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
    assert_eq!((c.slots, c.cpus), (2, 14));
    assert!(w.is_empty(), "{w:?}");
    let (c, _) = get(&[("HEAVY_CPUS", "14"), ("HEAVY_SLOTS", "auto")]);
    assert_eq!(c.slots, 2);
    let (c, w) = get(&[("HEAVY_CPUS", "14"), ("HEAVY_SLOTS", "3")]);
    assert_eq!(c.slots, 3);
    assert!(w.is_empty(), "no other variable has to opt in: {w:?}");
    let (c, w) = get(&[("HEAVY_CPUS", "14"), ("HEAVY_SLOTS", "0")]);
    assert_eq!(c.slots, 2);
    assert_eq!(w.len(), 1);
}

#[test]
fn a_container_run_that_appears_beside_a_taker_makes_the_younger_give_its_slot_back() {
    let (rig, cfg) = rig_with(5);
    let a = rig.agent_with(91, cfg);
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    // Between A's take and its second look at the folder, a container run takes the second slot (it saw an empty machine too).
    let now = rig.now();
    let (kept, planted) = (std::sync::Arc::new(std::sync::Mutex::new(Vec::new())), rig.clone_paths());
    let keep = kept.clone();
    *a.h.faults.after_take.lock().unwrap() = Some(Box::new(move || {
        let held = planted.plant_slot(
            2,
            &format!("92\nstart\nC\n{}\n/w\nbash tools/linux-container.sh x\n", now.saturating_sub(100)),
        );
        keep.lock().unwrap().push(held);
    }));
    assert_eq!(ha.poll(), Poll::Waiting, "a container beside it: the younger one steps back");
    assert!(!rig.lock_dir().exists(), "its slot is free again");
    assert!(
        rig.root().join("heavy-slot2.info").exists() && heavy::slotfile::is_held(&rig.root().join("heavy-slot2.lock")).unwrap(),
        "the container run keeps its slot"
    );
    drop(kept);
    assert_eq!(rig.count("pitcrew-heavy.ticket."), 1, "it keeps its place in the queue");
}

#[test]
fn a_taker_that_finds_more_holders_than_the_load_allows_steps_back_unless_it_is_the_oldest() {
    let (rig, cfg) = rig_with(5);
    let a = rig.agent_with(93, cfg.clone());
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let load = rig.load.clone();
    let (planted, now) = (rig.clone_paths(), rig.now());
    let kept = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let keep = kept.clone();
    *a.h.faults.after_take.lock().unwrap() = Some(Box::new(move || {
        // The load rose, and an older holder is there beside A.
        load.set(15.0);
        let held = planted.plant_slot(2, &format!("94\nstart\nO\n{}\n/w\ncargo test\n", now.saturating_sub(100)));
        keep.lock().unwrap().push(held);
    }));
    assert_eq!(ha.poll(), Poll::Waiting);
    // The oldest holder keeps its slot whatever the count: two takers never both step back.
    let (rig, cfg) = rig_with(5);
    let b = rig.agent_with(95, cfg);
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    let load = rig.load.clone();
    let (planted, now) = (rig.clone_paths(), rig.now());
    let kept2 = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let keep2 = kept2.clone();
    *b.h.faults.after_take.lock().unwrap() = Some(Box::new(move || {
        load.set(15.0);
        let held = planted.plant_slot(2, &format!("96\nstart\nY\n{}\n/w\ncargo test\n", now));
        keep2.lock().unwrap().push(held);
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
            let held = rig.held("heavy-");
            assert!(held <= 5, "{held} holders beside each other");
        }
    }
    assert!(rig.held("heavy-") >= 2, "several run together");
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
    assert_eq!(rig.held("heavy-"), 2);
}

/// The deadlock of 2026-11-18 (train 16): the head of the queue waits for a container run's drain, and the container run (the drain's
/// owner) sits behind that head in the queue. The drain's owner must be able to take a slot whatever its place in the queue.
#[test]
fn a_drain_owner_behind_the_head_still_takes_the_machine_when_it_is_free() {
    let (rig, mut cfg) = rig_with(5);
    cfg.drain_after = std::time::Duration::from_secs(300);
    let (a, h, c) = (
        rig.agent_with(141, cfg.clone()),
        rig.agent_with(142, cfg.clone()),
        rig.agent_with(143, cfg),
    );
    // A busy machine (the load keeps it to one step at a time): A runs, the head H and, behind it, a container run C wait.
    rig.load.set(15.0);
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    let mut hh = Acquirer::new(&h.h, req("H")).unwrap();
    rig.clock.advance(std::time::Duration::from_millis(5));
    let mut hc = Acquirer::new(&c.h, req_cmd("C", "bash tools/linux-container.sh x")).unwrap();
    assert_eq!(hh.poll(), Poll::Waiting);
    assert_eq!(hc.poll(), Poll::Waiting);
    // C has waited long enough to ask for the drain.
    rig.clock.advance(std::time::Duration::from_secs(301));
    assert_eq!(hc.poll(), Poll::Waiting);
    assert_eq!(rig.count("pitcrew-heavy.drain."), 1, "{}", c.said());
    // A ends: nothing runs. The head must not wait for a drain owner that is behind it.
    ha.release();
    let mut got = (hh.poll(), hc.poll());
    for _ in 0..3 {
        if got.0 == Poll::Held || got.1 == Poll::Held {
            break;
        }
        got = (hh.poll(), hc.poll());
    }
    assert!(
        got.0 == Poll::Held || got.1 == Poll::Held,
        "nobody runs on a free machine: {got:?} / {} / {}",
        h.said(),
        c.said()
    );
    assert_eq!(got.1, Poll::Held, "the drain owner goes first, whatever its place in the queue");
}

/// A model of the queue: random agents (normal, high-priority, container) arrive at random rounds, run a few rounds once they hold a
/// slot, and the load flips between low and high. Every agent must finish (no wait cycle between the queue, the drain, the high markers
/// and the step-back), no more than the maximum hold slots at the end of a round, and a container run is never beside another step then.
/// 400 seeds. The control: with the drain owner's queue bypass taken out the model finds a stuck agent (`model_stuck_with`).
fn model_stuck_with(seed: u64, max: u32) -> Option<String> {
    let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let mut rnd = |n: u64| {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (x >> 33) % n
    };
    let rig = Rig::new();
    let mut cfg = rig.cfg();
    cfg.slots = max;
    cfg.drain_after = std::time::Duration::from_secs(4);
    let n = 3 + rnd(5) as usize;
    let kinds: Vec<u64> = (0..n).map(|_| rnd(4)).collect(); // 0,1 normal, 2 high, 3 container
    let arrive: Vec<u64> = (0..n).map(|_| rnd(6)).collect();
    let agents: Vec<_> = (0..n).map(|i| rig.agent_with(200 + i as u32, cfg.clone())).collect();
    let mut acq: Vec<Option<Acquirer<'_>>> = (0..n).map(|_| None).collect();
    let mut ran_for: Vec<Option<u64>> = vec![None; n];
    let mut done = vec![false; n];
    for round in 0..500u64 {
        rig.load.set(if rnd(3) == 0 { 15.0 } else { 1.0 });
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            order.swap(i, rnd(i as u64 + 1) as usize);
        }
        for &i in &order {
            if done[i] || round < arrive[i] {
                continue;
            }
            if acq[i].is_none() {
                let r = match kinds[i] {
                    2 => req_high(&format!("a{i}")),
                    3 => req_cmd(&format!("a{i}"), "docker run --rm x"),
                    _ => req(&format!("a{i}")),
                };
                acq[i] = Some(Acquirer::new(&agents[i].h, r).unwrap());
            }
            if let Some(since) = ran_for[i] {
                if round >= since + 1 + (i as u64 % 3) {
                    acq[i].as_mut().unwrap().release();
                    done[i] = true;
                    acq[i] = None;
                }
                continue;
            }
            if acq[i].as_mut().unwrap().poll() == Poll::Held {
                ran_for[i] = Some(round);
            }
        }
        let holders = rig.held("heavy-");
        if holders > max as usize {
            return Some(format!("seed {seed}: {holders} holders, max {max}"));
        }
        let container_holding = (0..n).any(|i| kinds[i] == 3 && ran_for[i].is_some() && !done[i]);
        if container_holding && holders > 1 {
            return Some(format!(
                "seed {seed}: a container run beside {} other holder(s) in round {round}",
                holders - 1
            ));
        }
        if done.iter().all(|d| *d) {
            return None;
        }
        rig.clock.advance(std::time::Duration::from_secs(1));
    }
    Some(format!(
        "seed {seed}: stuck after 500 rounds: kinds {kinds:?} arrive {arrive:?} done {done:?} ran {ran_for:?}; said: {}",
        agents
            .iter()
            .map(|a| a.said())
            .collect::<Vec<_>>()
            .join(" | ")
            .chars()
            .take(600)
            .collect::<String>()
    ))
}

#[test]
fn the_queue_model_has_no_wait_cycle_and_never_exceeds_the_maximum() {
    for max in [1u32, 3, 5] {
        for seed in 0..400u64 {
            if let Some(why) = model_stuck_with(seed, max) {
                panic!("{why}");
            }
        }
    }
}

/// The wait-for detector (C2 of docs/research/2026-11-19-gate-hang-detection.md) on the real folder: the deadlock of 2026-11-18 cannot
/// happen any more (7a4aafbee), so the old rule is switched on in the detector only, and the head and the drain owner see the cycle,
/// name both members, and the drain owner withdraws its marker. The control: with the rule as it is, nobody reports anything.
#[test]
fn a_wait_for_cycle_is_reported_naming_its_members_and_the_drain_owner_withdraws_its_marker() {
    for old_rule in [false, true] {
        let (rig, mut cfg) = rig_with(5);
        cfg.drain_after = std::time::Duration::from_secs(300);
        let (a, h, c) = (
            rig.agent_with(341, cfg.clone()),
            rig.agent_with(342, cfg.clone()),
            rig.agent_with(343, cfg),
        );
        for ag in [&h, &c] {
            ag.h.faults.old_drain_rule.store(old_rule, std::sync::atomic::Ordering::SeqCst);
        }
        rig.load.set(15.0);
        let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
        assert_eq!(ha.poll(), Poll::Held);
        let mut hh = Acquirer::new(&h.h, req("H")).unwrap();
        rig.clock.advance(std::time::Duration::from_millis(5));
        let mut hc = Acquirer::new(&c.h, req_cmd("C", "bash tools/linux-container.sh x")).unwrap();
        assert_eq!(hh.poll(), Poll::Waiting);
        assert_eq!(hc.poll(), Poll::Waiting);
        rig.clock.advance(std::time::Duration::from_secs(301));
        assert_eq!(hc.poll(), Poll::Waiting);
        if old_rule {
            // The drain owner sees the cycle in the very poll that made its marker, says so naming both members, and withdraws it.
            assert!(c.said().contains("wait-for cycle"), "{}", c.said());
            assert!(c.said().contains("H -> C -> H") || c.said().contains("C -> H -> C"), "{}", c.said());
            assert_eq!(
                rig.count("pitcrew-heavy.drain."),
                0,
                "the drain owner withdrew its marker: {}",
                c.said()
            );
            assert!(
                !h.h.status().contains("WAIT-FOR CYCLE"),
                "the cycle is gone once the marker is withdrawn"
            );
        } else {
            assert_eq!(rig.count("pitcrew-heavy.drain."), 1, "{}", c.said());
            assert!(!h.h.status().contains("WAIT-FOR CYCLE"));
            assert_eq!(hh.poll(), Poll::Waiting);
            assert_eq!(hc.poll(), Poll::Waiting);
            let said = format!("{}\n{}", h.said(), c.said());
            assert!(!said.contains("wait-for cycle"), "no cycle under the rules as they are: {said}");
        }
    }
}

/// Review (b): the queue's wait-for graph knows a container run from its ticket, before it asks for a drain: the container ticket has
/// a fifth line, every other ticket keeps the shell plock's four.
#[test]
fn a_container_run_is_known_to_the_wait_for_graph_from_its_ticket() {
    let (rig, cfg) = rig_with(5);
    let (a, c) = (rig.agent_with(351, cfg.clone()), rig.agent_with(352, cfg));
    let _ha = Acquirer::new(&a.h, req("A")).unwrap();
    let _hc = Acquirer::new(&c.h, req_cmd("C", "bash tools/linux-container.sh x")).unwrap();
    let w = a.h.waiters();
    let by = |n: &str| w.iter().find(|x| x.name == n).unwrap_or_else(|| panic!("no waiter {n}: {w:?}"));
    assert!(by("C").container && !by("C").drain_owner, "a container run without a drain marker");
    assert!(!by("A").container);
    let lines = |prefix: &str, who: &str| -> usize {
        std::fs::read_dir(rig.root())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
            .map(|e| std::fs::read_to_string(e.path()).unwrap())
            .find(|t| t.lines().nth(2) == Some(who))
            .map_or(0, |t| t.lines().count())
    };
    assert_eq!(lines("pitcrew-heavy.ticket.", "A"), 4, "a normal ticket keeps its four lines");
    assert_eq!(lines("pitcrew-heavy.ticket.", "C"), 5, "a container ticket says so");
}
