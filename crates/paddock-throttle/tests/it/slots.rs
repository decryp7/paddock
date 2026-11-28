//! Load-aware slots: how many steps run together follows the 5-minute load and the memory available, up to a configured maximum;
//! a container run is alone; a waiter holds a ticket and never a slot (ER-43). Fixture folders and fake loads only (ER-14).
use crate::helpers::*;
use paddock_throttle::clock::Clock;
use paddock_throttle::clock::FakeClock;
use paddock_throttle::config::Config;
use paddock_throttle::gate::FakeLoad;
use paddock_throttle::gate::{
    Pressure, SLOT_LOAD_SHARE, allowed_slots, auto_max_slots, is_container_command,
};
use paddock_throttle::lock::{Acquirer, Poll, Request, Throttle};
use paddock_throttle::log::Logger;
use paddock_throttle::procs::FakeProcs;
use paddock_throttle::store::{RealStore, Store};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn rig_with(max: u32) -> (Rig, paddock_throttle::config::Config) {
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
    let (a, b) = (
        rig.agent_with(11, cfg.clone()),
        rig.agent_with(12, cfg.clone()),
    );
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
    assert_eq!(
        hb.poll(),
        Poll::Held,
        "the 5-minute load decides, not a momentary idle reading"
    );
    // An unknown load: one at a time.
    let (rig, cfg) = rig_with(5);
    let (a, b) = (rig.agent_with(34, cfg.clone()), rig.agent_with(35, cfg));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    rig.load.set_memory_mb(None);
    assert_eq!(
        hb.poll(),
        Poll::Waiting,
        "an unknown reading is not a guess in favour of more"
    );
    rig.load.set_memory_mb(Some(64 * 1024));
    assert_eq!(hb.poll(), Poll::Held);
}

#[test]
fn the_configured_maximum_bounds_the_count_at_low_load() {
    let (rig, cfg) = rig_with(2);
    let ags: Vec<_> = (41..44).map(|p| rig.agent_with(p, cfg.clone())).collect();
    let mut hs: Vec<_> = ags
        .iter()
        .map(|a| Acquirer::new(&a.h, req("X")).unwrap())
        .collect();
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
    assert_eq!(
        hb.poll(),
        Poll::Waiting,
        "nothing beside a running container"
    );
    ha.release();
    assert_eq!(hb.poll(), Poll::Held);
    // And a container run waits until the running steps are done.
    let (rig, cfg) = rig_with(5);
    let (a, b) = (rig.agent_with(61, cfg.clone()), rig.agent_with(62, cfg));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req_cmd("B", "docker run --rm x")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    assert_eq!(
        hb.poll(),
        Poll::Waiting,
        "a container run starts only on its own"
    );
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
    assert_eq!(
        rig.held("throttle-"),
        1,
        "only the running step holds a slot folder"
    );
    assert_eq!(
        rig.count("pitcrew-heavy.ticket."),
        2,
        "the waiters hold tickets"
    );
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
    assert_eq!(
        allowed_slots(
            5,
            14,
            &Pressure {
                load5: None,
                ..p(0.0)
            },
            false
        ),
        1
    );
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
    for c in [
        "bash tools/linux-container.sh x",
        "/usr/local/bin/docker run a",
        "podman ps",
    ] {
        assert!(is_container_command(c), "{c}");
    }
    for c in ["cargo clippy", "cargo test docker_tests", "echo dockerfile"] {
        assert!(!is_container_command(c), "{c}");
    }
}

#[test]
fn auto_is_the_default_and_a_number_is_a_maximum() {
    let get = |pairs: &'static [(&'static str, &'static str)]| {
        Config::from_env(
            &|k| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| (*v).to_string())
            },
            &paddock_throttle::places::Host::none(),
        )
    };
    let (c, w) = get(&[("THROTTLE_CPUS", "14")]);
    assert_eq!((c.slots, c.cpus), (2, 14));
    assert!(w.is_empty(), "{w:?}");
    let (c, _) = get(&[("THROTTLE_CPUS", "14"), ("THROTTLE_SLOTS", "auto")]);
    assert_eq!(c.slots, 2);
    let (c, w) = get(&[("THROTTLE_CPUS", "14"), ("THROTTLE_SLOTS", "3")]);
    assert_eq!(c.slots, 3);
    assert!(w.is_empty(), "no other variable has to opt in: {w:?}");
    let (c, w) = get(&[("THROTTLE_CPUS", "14"), ("THROTTLE_SLOTS", "0")]);
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
    let (kept, planted) = (
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        rig.clone_paths(),
    );
    let keep = kept.clone();
    *a.h.faults.after_take.lock().unwrap() = Some(Box::new(move || {
        let held = planted.plant_slot(
            2,
            &format!(
                "92\nstart\nC\n{}\n/w\nbash tools/linux-container.sh x\n",
                now.saturating_sub(100)
            ),
        );
        keep.lock().unwrap().push(held);
    }));
    assert_eq!(
        ha.poll(),
        Poll::Waiting,
        "a container beside it: the younger one steps back"
    );
    assert!(!rig.lock_dir().exists(), "its slot is free again");
    assert!(
        rig.root().join("throttle-slot2.info").exists()
            && paddock_throttle::slotfile::is_held(&rig.root().join("throttle-slot2.lock"))
                .unwrap(),
        "the container run keeps its slot"
    );
    drop(kept);
    assert_eq!(
        rig.count("pitcrew-heavy.ticket."),
        1,
        "it keeps its place in the queue"
    );
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
        let held = planted.plant_slot(
            2,
            &format!(
                "94\nstart\nO\n{}\n/w\ncargo test\n",
                now.saturating_sub(100)
            ),
        );
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
    let mut hs: Vec<_> = ags
        .iter()
        .map(|a| Acquirer::new(&a.h, req("X")).unwrap())
        .collect();
    for _ in 0..4 {
        for h in hs.iter_mut() {
            let _ = h.poll();
            let held = rig.held("throttle-");
            assert!(held <= 5, "{held} holders beside each other");
        }
    }
    assert!(rig.held("throttle-") >= 2, "several run together");
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
    let l = paddock_throttle::gate::SysLoad::default();
    if cfg!(windows) {
        assert!(paddock_throttle::gate::LoadSource::idle_percent(&l).is_none());
        return;
    }
    let t = std::time::Instant::now();
    let mut got = None;
    while got.is_none() && t.elapsed() < std::time::Duration::from_secs(10) {
        got = paddock_throttle::gate::LoadSource::idle_percent(&l);
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
    std::fs::write(
        rig.root().join("pitcrew-heavy.high.123-H"),
        format!("123\nstart\n{}\n", rig.now()),
    )
    .unwrap();
    assert_eq!(hc.poll(), Poll::Waiting);
    assert_eq!(rig.count("pitcrew-heavy.drain."), 0, "not yet");
    // ... but after drain_after it must still ask the machine to drain, not wait behind the high marker until priority_max.
    rig.clock.advance(std::time::Duration::from_secs(301));
    assert_eq!(hc.poll(), Poll::Waiting);
    assert_eq!(rig.count("pitcrew-heavy.drain."), 1, "{}", c.said());
}

#[test]
fn a_container_run_held_by_a_single_slot_binary_keeps_the_load_aware_ones_from_starting_beside_it()
{
    // The binary before this change (slots = 1) holds the first slot with a container run; the new ones read its owner record.
    let rig = Rig::new();
    let old_cfg = rig.cfg();
    let mut new_cfg = rig.cfg();
    new_cfg.slots = 5;
    let old = rig.agent_with(131, old_cfg);
    let mut ho = Acquirer::new(&old.h, req_cmd("old", "docker run --rm x")).unwrap();
    assert_eq!(ho.poll(), Poll::Held);
    let ags: Vec<_> = (132..135)
        .map(|p| rig.agent_with(p, new_cfg.clone()))
        .collect();
    let mut hs: Vec<_> = ags
        .iter()
        .map(|a| Acquirer::new(&a.h, req("n")).unwrap())
        .collect();
    for h in hs.iter_mut() {
        assert_eq!(
            h.poll(),
            Poll::Waiting,
            "nothing starts beside the old binary's container run"
        );
    }
    ho.release();
    assert_eq!(hs[0].poll(), Poll::Held);
    assert_eq!(
        hs[1].poll(),
        Poll::Held,
        "and the load-aware ones run together once it is gone"
    );
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
    assert_eq!(rig.held("throttle-"), 2);
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
    assert_eq!(
        got.1,
        Poll::Held,
        "the drain owner goes first, whatever its place in the queue"
    );
}

/// The world of one model run: the fake clock, process table and load, and a lock folder that is either a real temp folder or in memory.
/// Every file operation goes through a [`Recording`] store, so a sweep says which operations on which files it reached.
struct Sim {
    clock: Arc<FakeClock>,
    procs: Arc<FakeProcs>,
    load: Arc<FakeLoad>,
    store: Arc<dyn Store>,
    mem: Option<Arc<MemStore>>,
    root: PathBuf,
    _dir: Option<tempfile::TempDir>,
}

type Seen = Arc<Mutex<BTreeSet<String>>>;

impl Sim {
    fn new(real: bool, seen: &Seen) -> Sim {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1_800_000_000, |d| d.as_secs());
        let clock = Arc::new(FakeClock::at_secs(now));
        let (inner, mem, dir, root): (Arc<dyn Store>, _, _, _) = if real {
            let d = tempfile::tempdir().expect("tempdir");
            let root = d.path().to_path_buf();
            (Arc::new(RealStore), None, Some(d), root)
        } else {
            let m = Arc::new(MemStore::new(clock.clone()));
            (
                m.clone(),
                Some(m),
                None,
                std::env::temp_dir().join("model-lock-folder"),
            )
        };
        Sim {
            clock,
            procs: Arc::new(FakeProcs::new()),
            load: Arc::new(FakeLoad::new(1.0)),
            store: Arc::new(Recording {
                inner,
                seen: Arc::clone(seen),
            }),
            mem,
            root,
            _dir: dir,
        }
    }

    fn cfg(&self, max: u32) -> Config {
        let (mut c, _) = Config::from_env(&|_| None, &paddock_throttle::places::Host::none());
        c.root = self.root.clone();
        c.poll = std::time::Duration::from_millis(200);
        c.wait_log = std::time::Duration::from_secs(1);
        c.unreadable_limit = 3;
        c.cpus = 16;
        c.slots = max;
        c.drain_after = std::time::Duration::from_secs(4);
        c
    }

    fn agent(&self, pid: u32, cfg: Config) -> Agent {
        self.procs
            .run(pid, self.clock.now_secs().saturating_sub(100));
        let said = Arc::new(Mutex::new(Vec::new()));
        let s2 = Arc::clone(&said);
        // The real folder keeps its troubleshooting log there too; the in-memory run needs none.
        let log_file = self
            .mem
            .is_none()
            .then(|| self.root.join(format!("log-{pid}.clef")));
        let log = Arc::new(Logger::new(
            format!("run-{pid}"),
            pid,
            self.clock.clone(),
            log_file,
            false,
        ));
        let mut h = Throttle::new(
            cfg,
            self.clock.clone(),
            self.procs.clone(),
            self.load.clone(),
            log,
            pid,
            Arc::new(move |m: &str| {
                if let Ok(mut v) = s2.lock() {
                    v.push(m.to_string());
                }
            }),
        );
        h.store = self.store.clone();
        if self.mem.is_some() {
            // No folder to ask the OS about (on Windows a path resolve and a drive query per agent).
            h.fs_kind = Arc::new(|_| paddock_throttle::fsinfo::FsKind::Local);
        }
        Agent { h, said }
    }

    /// The slots with a holder's record now.
    fn holders(&self) -> usize {
        self.store
            .list(&self.root)
            .iter()
            .filter(|p| {
                let n = p.file_name().map(|n| n.to_string_lossy().into_owned());
                n.is_some_and(|n| n.starts_with("throttle-") && n.ends_with(".info"))
            })
            .count()
    }
}

/// The ways a model run can be broken on purpose (the controls): the model must find each.
#[derive(Clone, Copy, Default)]
struct Broken {
    /// The admission queue keeps a drain owner behind the head (the deadlock of 2026-11-18).
    drain_owner_in_queue: bool,
    /// The model checks against a maximum one lower than the configured one: its over-maximum check must fire.
    check_one_below_max: bool,
    /// The model takes every agent for a container run when it checks for one beside another step: that check must fire.
    check_all_as_container: bool,
}

/// A model of the queue: random agents (normal, high-priority, container) arrive at random rounds, run a few rounds once they hold a
/// slot, and the load flips between low and high. Every agent must finish (no wait cycle between the queue, the drain, the high markers
/// and the step-back), no more than the maximum hold slots at the end of a round, and a container run is never beside another step then.
/// The rules do not depend on the disk, so 400 seeds per maximum run against an in-memory folder; a few run against the real one (the
/// OS's renames, removes and locks) and must reach every kind of operation the full sweep reaches. The controls: with a [`Broken`] rule
/// the model finds a stuck agent, or more holders than the maximum.
fn model_stuck_with(seed: u64, max: u32, sim: &Sim, broken: Broken) -> Option<String> {
    let mut x = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let mut rnd = |n: u64| {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (x >> 33) % n
    };
    let cfg = sim.cfg(max);
    let n = 3 + rnd(5) as usize;
    let kinds: Vec<u64> = (0..n).map(|_| rnd(4)).collect(); // 0,1 normal, 2 high, 3 container
    let arrive: Vec<u64> = (0..n).map(|_| rnd(6)).collect();
    let agents: Vec<_> = (0..n)
        .map(|i| {
            let a = sim.agent(200 + i as u32, cfg.clone());
            a.h.faults.admission_follows_queue.store(
                broken.drain_owner_in_queue,
                std::sync::atomic::Ordering::SeqCst,
            );
            a
        })
        .collect();
    let mut acq: Vec<Option<Acquirer<'_>>> = (0..n).map(|_| None).collect();
    let mut ran_for: Vec<Option<u64>> = vec![None; n];
    let mut done = vec![false; n];
    for round in 0..500u64 {
        sim.load.set(if rnd(3) == 0 { 15.0 } else { 1.0 });
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
        let holders = sim.holders();
        if holders > (max as usize).saturating_sub(usize::from(broken.check_one_below_max)) {
            return Some(format!("seed {seed}: {holders} holders, max {max}"));
        }
        let container_holding = (0..n).any(|i| {
            (kinds[i] == 3 || broken.check_all_as_container) && ran_for[i].is_some() && !done[i]
        });
        if container_holding && holders > 1 {
            return Some(format!(
                "seed {seed}: a container run beside {} other holder(s) in round {round}",
                holders - 1
            ));
        }
        if done.iter().all(|d| *d) {
            return None;
        }
        sim.clock.advance(std::time::Duration::from_secs(1));
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

const MAXIMA: [u32; 3] = [1, 3, 5];
/// Seeds per maximum of the full sweep (in memory).
const MODEL_SEEDS: u64 = 400;
/// Seeds per maximum against the real folder: 2 reach every kind of operation the full sweep reaches, 1 does not (measured, docs/benchmarks.md,
/// "2026-11-28, the throttle tests off the real folder"). A real seed costs 2 to 3 s on a Windows runner (file operations, run 9373), so
/// no more run, and the test names what is missed if a rule change ever makes this too few.
const REAL_SEEDS: u64 = 2;

/// Runs `seeds` seeds per maximum in a fresh world each, in memory or on the real folder; returns the first failure and what it reached.
fn sweep(real: bool, seeds: u64, broken: Broken) -> (Option<String>, BTreeSet<String>) {
    let seen: Seen = Arc::default();
    for max in MAXIMA {
        for seed in 0..seeds {
            let sim = Sim::new(real, &seen);
            if let Some(why) = model_stuck_with(seed, max, &sim, broken) {
                let reached = seen.lock().unwrap().clone();
                return (Some(why), reached);
            }
        }
    }
    let reached = seen.lock().unwrap().clone();
    (None, reached)
}

#[test]
fn the_queue_model_has_no_wait_cycle_and_never_exceeds_the_maximum() {
    let (why, _) = sweep(false, MODEL_SEEDS, Broken::default());
    assert!(why.is_none(), "{}", why.unwrap_or_default());
}

#[test]
fn the_queue_model_on_the_real_folder_reaches_every_operation_the_full_sweep_reaches() {
    let (why, full) = sweep(false, MODEL_SEEDS, Broken::default());
    assert!(why.is_none(), "{}", why.unwrap_or_default());
    let (why, real) = sweep(true, REAL_SEEDS, Broken::default());
    assert!(why.is_none(), "{}", why.unwrap_or_default());
    let missed: Vec<_> = full.difference(&real).collect();
    assert!(
        missed.is_empty(),
        "the real-folder seeds miss what the in-memory sweep reaches (raise REAL_SEEDS until it reaches them): {missed:?}"
    );
}

/// The controls (ER-33): a model that cannot see the thing is not a check.
#[test]
fn the_queue_model_finds_the_deadlock_of_2026_11_18_when_the_drain_owner_waits_in_the_queue() {
    let broken = Broken {
        drain_owner_in_queue: true,
        ..Broken::default()
    };
    let (why, _) = sweep(false, MODEL_SEEDS, broken);
    let why = why.expect("the model must find a stuck agent with the old queue rule");
    assert!(why.contains("stuck after 500 rounds"), "{why}");
}

#[test]
fn the_queue_models_checks_see_an_over_maximum_and_a_container_run_beside_another_step() {
    for (broken, want) in [
        (
            Broken {
                check_one_below_max: true,
                ..Broken::default()
            },
            "holders, max",
        ),
        (
            Broken {
                check_all_as_container: true,
                ..Broken::default()
            },
            "a container run beside",
        ),
    ] {
        let (why, _) = sweep(false, MODEL_SEEDS, broken);
        let why = why.expect("the check must fire on a model that breaks its limit");
        assert!(why.contains(want), "{why}");
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
            ag.h.faults
                .old_drain_rule
                .store(old_rule, std::sync::atomic::Ordering::SeqCst);
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
            assert!(
                c.said().contains("H -> C -> H") || c.said().contains("C -> H -> C"),
                "{}",
                c.said()
            );
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
            assert!(
                !said.contains("wait-for cycle"),
                "no cycle under the rules as they are: {said}"
            );
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
    let by = |n: &str| {
        w.iter()
            .find(|x| x.name == n)
            .unwrap_or_else(|| panic!("no waiter {n}: {w:?}"))
    };
    assert!(
        by("C").container && !by("C").drain_owner,
        "a container run without a drain marker"
    );
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
    assert_eq!(
        lines("pitcrew-heavy.ticket.", "A"),
        4,
        "a normal ticket keeps its four lines"
    );
    assert_eq!(
        lines("pitcrew-heavy.ticket.", "C"),
        5,
        "a container ticket says so"
    );
}
