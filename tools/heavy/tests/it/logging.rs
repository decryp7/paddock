//! The troubleshooting log (rule 56): the key lines, the evidence, the correlation id, waits every 60 s, gate, slots.

use crate::helpers::*;
use heavy::lock::{Acquirer, Poll};
use std::time::Duration;

fn json(line: &str) -> serde_json::Value {
    serde_json::from_str(line).unwrap_or_else(|e| panic!("not JSON ({e}): {line}"))
}

#[test]
fn every_line_is_clef_with_the_run_id_as_session_id() {
    let rig = Rig::new();
    let a = rig.agent(11);
    let mut acq = Acquirer::new(&a.h, req("A")).unwrap();
    assert_eq!(acq.poll(), Poll::Held);
    acq.release();
    let lines = a.h.log.lines();
    assert!(lines.len() >= 3);
    for l in &lines {
        let v = json(l);
        assert!(v["@t"].as_str().is_some_and(|t| t.ends_with('Z') && t.contains('T')), "{l}");
        assert!(v["@m"].is_string() && v["SourceContext"].is_string(), "{l}");
        assert_eq!(v["SessionId"], "run-11", "{l}");
        assert_eq!(v["Application"], "heavy");
        assert_eq!(v["ProcessId"], 11);
    }
    // and the same lines are in the file
    let file = std::fs::read_to_string(rig.root().join("log-11.clef")).unwrap();
    assert_eq!(file.lines().count(), lines.len());
}

#[test]
fn phases_carry_their_durations() {
    let rig = Rig::new();
    let a = rig.agent(11);
    let mut acq = Acquirer::new(&a.h, req("A")).unwrap();
    assert_eq!(acq.poll(), Poll::Held);
    acq.release();
    let lines = a.h.log.lines();
    for phase in ["take", "queue_wait"] {
        let l = lines
            .iter()
            .find(|l| l.contains(&format!("\"Phase\":\"{phase}\"")))
            .unwrap_or_else(|| panic!("no {phase} phase in {lines:?}"));
        assert!(json(l)["DurationMs"].is_u64(), "{l}");
    }
    assert!(a.h.log.has("released the lock"));
}

#[test]
fn a_break_names_the_identity_evidence() {
    let rig = Rig::new();
    let z = rig.agent(11);
    rig.procs.run(99003, rig.now());
    let epoch = rig.now() - 100;
    rig.plant_lock(&format!("99003\nx\nreused\n{epoch}\n"));
    let mut acq = Acquirer::new(&z.h, req("Z")).unwrap();
    assert_eq!(acquire(&mut acq, &z.h, 5), Poll::Held);
    let l =
        z.h.log
            .lines()
            .into_iter()
            .find(|l| l.contains("heavy.break"))
            .expect("break event");
    let v = json(&l);
    assert_eq!(v["@l"], "Warning");
    let ev = v["Evidence"].as_str().unwrap();
    assert!(
        ev.contains("pid 99003") && ev.contains("os start") && ev.contains(&format!("record epoch {epoch}")),
        "{ev}"
    );
    assert!(v["Why"].as_str().unwrap().contains("recycled"));
}

#[test]
fn stale_removals_are_logged_with_evidence() {
    let rig = Rig::new();
    let n = rig.agent(12);
    std::fs::write(
        rig.root().join("pitcrew-heavy.ticket.0000000000000001-99100"),
        format!("99100\nx\nghost\n{}\n", rig.now()),
    )
    .unwrap();
    std::fs::write(rig.root().join("pitcrew-heavy.high.99101-m"), "99101\n").unwrap();
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    assert_eq!(acq.poll(), Poll::Held);
    let ev: Vec<String> = n.h.log.lines().into_iter().filter(|l| l.contains("heavy.stale")).collect();
    assert_eq!(ev.len(), 2, "{ev:?}");
    assert!(ev.iter().all(|l| l.contains("no such process")), "{ev:?}");
}

#[test]
fn the_wait_reason_is_logged_at_once_and_said_every_sixty_seconds() {
    let rig = Rig::new();
    let mut cfg = rig.cfg();
    cfg.wait_log = Duration::from_secs(60);
    let (h, w) = (rig.agent_with(11, cfg.clone()), rig.agent_with(12, cfg));
    let mut holder = Acquirer::new(&h.h, req("Holder")).unwrap();
    assert_eq!(holder.poll(), Poll::Held);
    let mut waiter = Acquirer::new(&w.h, req("Waiter")).unwrap();
    assert_eq!(waiter.poll(), Poll::Waiting);
    assert!(w.said().is_empty(), "nothing said before 60 s");
    let first =
        w.h.log
            .lines()
            .into_iter()
            .find(|l| l.contains("\"Reason\":\"held\""))
            .expect("reason logged at once");
    assert_eq!(json(&first)["HolderPid"], 11);
    rig.clock.advance(Duration::from_secs(59));
    assert_eq!(waiter.poll(), Poll::Waiting);
    assert!(w.said().is_empty(), "59 s: still silent");
    rig.clock.advance(Duration::from_secs(1));
    assert_eq!(waiter.poll(), Poll::Waiting);
    let said = w.said();
    assert!(
        said.contains("Waiter waiting 60s") && said.contains("held by Holder (pid 11) for 1 min"),
        "{said}"
    );
    rig.clock.advance(Duration::from_secs(60));
    assert_eq!(waiter.poll(), Poll::Waiting);
    assert_eq!(w.said().lines().filter(|l| l.contains("waiting")).count(), 2);
}

#[test]
fn queue_position_and_high_priority_reasons_are_logged() {
    let rig = Rig::new();
    let (a, b, c) = (rig.agent(11), rig.agent(12), rig.agent(13));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    rig.clock.advance(Duration::from_millis(3));
    let mut hc = Acquirer::new(&c.h, req("C")).unwrap();
    assert_eq!(hb.poll(), Poll::Waiting);
    assert_eq!(hc.poll(), Poll::Waiting);
    let l =
        c.h.log
            .lines()
            .into_iter()
            .find(|l| l.contains("\"Reason\":\"queued\""))
            .expect("queued reason");
    let v = json(&l);
    assert_eq!(
        (v["Position"].as_u64(), v["QueueLength"].as_u64(), v["Head"].as_str()),
        (Some(2), Some(2), Some("B"))
    );
}

#[test]
fn the_load_gate_is_a_threshold_on_the_one_minute_load() {
    let rig = Rig::new();
    let a = rig.agent(11);
    rig.load.set(29.99);
    assert!(a.h.gate_closed().is_none());
    rig.load.set(30.0);
    assert!(a.h.gate_closed().unwrap().contains(">= 30"));
    let mut cfg = rig.cfg();
    cfg.gate_load = 5.5;
    let b = rig.agent_with(12, cfg);
    rig.load.set(5.4);
    assert!(b.h.gate_closed().is_none());
    rig.load.set(5.5);
    assert!(b.h.gate_closed().is_some());
    let none = heavy::gate::FakeLoad::none();
    assert!(
        heavy::gate::LoadSource::load(&none).is_none(),
        "no load average: the gate is skipped, not failed"
    );
}

#[test]
fn two_slots_run_two_holders_and_a_third_waits() {
    let rig = Rig::new();
    let mut cfg = rig.cfg();
    cfg.slots = 2;
    let (a, b, c) = (
        rig.agent_with(11, cfg.clone()),
        rig.agent_with(12, cfg.clone()),
        rig.agent_with(13, cfg),
    );
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    let mut hc = Acquirer::new(&c.h, req("C")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    assert_eq!(hb.poll(), Poll::Held);
    assert_eq!(hc.poll(), Poll::Waiting);
    assert!(rig.lock_dir().exists() && rig.root().join("pitcrew-heavy-v2.lock.slot2").exists());
    assert!(a.h.status().contains("slot 1: held by A") && a.h.status().contains("slot 2: held by B"));
    hb.release();
    assert_eq!(hc.poll(), Poll::Held);
}
