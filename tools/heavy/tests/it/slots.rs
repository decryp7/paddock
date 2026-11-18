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
    assert_eq!(allowed_slots(5, 14, &p(0.0, 100.0), true), 1);
    assert_eq!(allowed_slots(1, 14, &p(0.0, 100.0), false), 1);
    assert_eq!(
        allowed_slots(
            5,
            14,
            &Pressure {
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
