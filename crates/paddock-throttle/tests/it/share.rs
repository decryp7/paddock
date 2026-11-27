//! Slots by the 5-minute load, and each step's share of the cores (ER-20, ER-24, ER-48): how many steps run together follows the
//! 5-minute load (several only when the machine is idle enough, one otherwise), and every step admitted while `n` may run gets
//! `1/n` of the test threads through its environment, so steps times threads stay within the cores. Fake loads only (ER-14).
use crate::helpers::*;
use paddock_throttle::gate::{
    MAX_AUTO_SLOTS, Pressure, StepThreads, allowed_slots, slot_load_limit, step_env, step_threads,
};
use paddock_throttle::lock::{Acquirer, Poll};
use std::time::Duration;

fn at(load5: f64) -> Pressure {
    Pressure {
        load1: Some(load5),
        load5: Some(load5),
        load15: Some(load5),
        idle: Some(100.0),
        memory_mb: Some(64 * 1024),
    }
}

/// A rig of `cpus` cores with up to `max` slots at a 5-minute load of `load5`.
fn rig_at(cpus: u32, max: u32, load5: f64) -> (Rig, paddock_throttle::config::Config) {
    let rig = Rig::new();
    rig.load.set(load5);
    let mut cfg = rig.cfg();
    cfg.cpus = cpus;
    cfg.slots = max;
    (rig, cfg)
}

/// A load between the limit for `n + 1` slots and the one for `n`: the rule allows exactly `n`.
fn load_for(cpus: u32, n: u32) -> f64 {
    let upper = if n == 1 {
        f64::from(cpus) * 4.0
    } else {
        slot_load_limit(cpus, n).expect("a limit for n")
    };
    let lower = slot_load_limit(cpus, n + 1).unwrap_or(0.0);
    (upper + lower) / 2.0
}

#[test]
fn the_slot_count_steps_down_as_the_5_minute_load_rises() {
    let cpus = 14;
    let mut last = f64::INFINITY;
    for n in 2..=MAX_AUTO_SLOTS {
        let limit =
            slot_load_limit(cpus, n).expect("every count up to the auto maximum has a limit");
        assert!(
            limit < last && limit > 0.0,
            "{n} slots: limit {limit} must be below the one for fewer slots ({last})"
        );
        assert_eq!(
            allowed_slots(MAX_AUTO_SLOTS, cpus, &at(limit - 0.01), false),
            n,
            "just under the limit for {n}"
        );
        assert!(
            allowed_slots(MAX_AUTO_SLOTS, cpus, &at(limit), false) < n,
            "at the limit for {n}"
        );
        last = limit;
    }
    assert_eq!(
        slot_load_limit(cpus, MAX_AUTO_SLOTS + 1),
        None,
        "no count above the auto maximum"
    );
    assert_eq!(
        allowed_slots(MAX_AUTO_SLOTS, cpus, &at(0.0), false),
        MAX_AUTO_SLOTS
    );
    assert_eq!(
        allowed_slots(MAX_AUTO_SLOTS, cpus, &at(f64::from(cpus)), false),
        1
    );
    assert_eq!(
        allowed_slots(1, cpus, &at(0.0), false),
        1,
        "a maximum of one is one, always"
    );
    assert_eq!(
        allowed_slots(MAX_AUTO_SLOTS, cpus, &at(0.0), true),
        1,
        "a container run is alone"
    );
}

#[test]
fn only_the_5_minute_load_decides_and_unknown_is_one() {
    let cpus = 14;
    let idle = load_for(cpus, MAX_AUTO_SLOTS);
    // A 1-minute burst, a high 15-minute average or a busy idle reading do not change the count: the 5-minute load does.
    let p = Pressure {
        load1: Some(40.0),
        load15: Some(40.0),
        idle: Some(5.0),
        ..at(idle)
    };
    assert_eq!(
        allowed_slots(MAX_AUTO_SLOTS, cpus, &p, false),
        MAX_AUTO_SLOTS
    );
    let unknown = Pressure {
        load5: None,
        ..at(idle)
    };
    assert_eq!(
        allowed_slots(MAX_AUTO_SLOTS, cpus, &unknown, false),
        1,
        "an unknown load is not a guess in favour of more"
    );
    let no_memory = Pressure {
        memory_mb: None,
        ..at(idle)
    };
    assert_eq!(allowed_slots(MAX_AUTO_SLOTS, cpus, &no_memory, false), 1);
    let nan = at(f64::NAN);
    assert_eq!(allowed_slots(MAX_AUTO_SLOTS, cpus, &nan, false), 1);
}

#[test]
fn each_step_gets_its_share_of_the_cores() {
    assert_eq!(
        step_threads(14, 1),
        StepThreads {
            build_jobs: 2,
            test_threads: 14
        }
    );
    assert_eq!(
        step_threads(14, 2),
        StepThreads {
            build_jobs: 2,
            test_threads: 7
        }
    );
    assert_eq!(
        step_threads(14, 3),
        StepThreads {
            build_jobs: 2,
            test_threads: 4
        }
    );
    for cpus in [1u32, 2, 4, 8, 14, 16, 64] {
        for share in 1..=8u32 {
            let t = step_threads(cpus, share);
            assert!(
                t.build_jobs >= 1 && t.test_threads >= 1,
                "{cpus}/{share}: {t:?}"
            );
            assert!(
                share * t.test_threads <= cpus.max(share),
                "{share} steps x {t:?} on {cpus} cores"
            );
            assert!(
                share * t.build_jobs <= cpus.max(share),
                "{share} steps x {t:?} on {cpus} cores"
            );
        }
    }
}

#[test]
fn the_step_environment_sets_the_share_and_keeps_the_callers_values() {
    let none = |_: &str| None::<String>;
    assert_eq!(
        step_env(14, 2, &none),
        vec![
            ("CARGO_BUILD_JOBS".to_string(), "2".to_string()),
            ("NEXTEST_TEST_THREADS".to_string(), "7".to_string()),
            ("RUST_TEST_THREADS".to_string(), "7".to_string()),
        ]
    );
    // The caller's own value wins (ER-47): it is not set again.
    let caller = |k: &str| (k == "NEXTEST_TEST_THREADS").then(|| "3".to_string());
    let env = step_env(14, 2, &caller);
    assert!(
        env.iter().all(|(k, _)| k != "NEXTEST_TEST_THREADS"),
        "{env:?}"
    );
    assert!(
        env.iter()
            .any(|(k, v)| k == "RUST_TEST_THREADS" && v == "7"),
        "{env:?}"
    );
}

#[test]
fn a_low_load_admits_two_with_half_the_threads_and_a_high_load_one_with_all() {
    let cpus = 14;
    // Idle enough for two: both steps run, each with half the test threads.
    let (rig, cfg) = rig_at(cpus, MAX_AUTO_SLOTS, load_for(cpus, 2));
    let (a, b, c) = (
        rig.agent_with(11, cfg.clone()),
        rig.agent_with(12, cfg.clone()),
        rig.agent_with(13, cfg),
    );
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    let mut hc = Acquirer::new(&c.h, req("C")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    assert_eq!(hb.poll(), Poll::Held, "two together when idle enough");
    assert_eq!(hc.poll(), Poll::Waiting, "not three at this load");
    assert_eq!((ha.share(), hb.share()), (2, 2));
    assert_eq!(step_threads(cpus, hb.share()).test_threads, 7);
    // Busy: one at a time, and the one gets all the threads.
    let (rig, cfg) = rig_at(cpus, MAX_AUTO_SLOTS, load_for(cpus, 1));
    let (a, b) = (rig.agent_with(21, cfg.clone()), rig.agent_with(22, cfg));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(ha.poll(), Poll::Held, "the first always runs");
    assert_eq!(hb.poll(), Poll::Waiting, "one at a time when busy");
    assert_eq!(ha.share(), 1);
    assert_eq!(step_threads(cpus, ha.share()).test_threads, cpus);
}

#[test]
fn a_very_low_load_admits_the_auto_maximum() {
    let cpus = 14;
    let (rig, cfg) = rig_at(cpus, MAX_AUTO_SLOTS, 0.0);
    let ags: Vec<_> = (31..31 + MAX_AUTO_SLOTS + 1)
        .map(|p| rig.agent_with(p, cfg.clone()))
        .collect();
    let mut hs: Vec<_> = ags
        .iter()
        .map(|a| Acquirer::new(&a.h, req("X")).unwrap())
        .collect();
    for h in hs.iter_mut().take(MAX_AUTO_SLOTS as usize) {
        assert_eq!(h.poll(), Poll::Held);
        assert_eq!(h.share(), MAX_AUTO_SLOTS);
    }
    assert_eq!(
        hs[MAX_AUTO_SLOTS as usize].poll(),
        Poll::Waiting,
        "never above the auto maximum"
    );
}

#[test]
fn a_ci_job_on_this_host_makes_normal_runs_yield_even_when_idle() {
    let cpus = 14;
    let (rig, cfg) = rig_at(cpus, MAX_AUTO_SLOTS, 0.0);
    let ci = rig.agent_with(41, cfg.clone());
    let n = rig.agent_with(42, cfg);
    rig.procs.run(41, rig.now() - 50);
    ci.h.mark("ci-s1", 41).unwrap();
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    assert_eq!(
        acq.poll(),
        Poll::Waiting,
        "a CI job's mark holds normal runs whatever the load"
    );
    assert!(ci.h.unmark("ci-s1", 41));
    assert_eq!(acq.poll(), Poll::Held);
}

#[test]
fn when_one_of_two_ends_its_slot_goes_to_the_next_and_the_other_keeps_its_own() {
    let cpus = 14;
    let (rig, cfg) = rig_at(cpus, MAX_AUTO_SLOTS, load_for(cpus, 2));
    let (a, b, c) = (
        rig.agent_with(51, cfg.clone()),
        rig.agent_with(52, cfg.clone()),
        rig.agent_with(53, cfg.clone()),
    );
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    let mut hc = Acquirer::new(&c.h, req("C")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    assert_eq!(hb.poll(), Poll::Held);
    assert_eq!(hc.poll(), Poll::Waiting);
    assert_eq!(rig.held("throttle-"), 2);
    ha.release();
    assert_eq!(rig.held("throttle-"), 1, "only A's slot went");
    assert_eq!(hc.poll(), Poll::Held, "the waiter takes the freed slot");
    assert_eq!(hc.share(), 2);
    assert_eq!(rig.held("throttle-"), 2, "B kept its own slot");
    // The load rises while two run: the next waiter is held to one, and the running two are never stopped.
    rig.load.set(load_for(cpus, 1));
    let d = rig.agent_with(54, cfg);
    let mut hd = Acquirer::new(&d.h, req("D")).unwrap();
    rig.clock.advance(Duration::from_secs(1));
    assert_eq!(hd.poll(), Poll::Waiting);
    hb.release();
    assert_eq!(
        hd.poll(),
        Poll::Waiting,
        "C still runs and the rule allows one"
    );
    hc.release();
    assert_eq!(hd.poll(), Poll::Held);
}
