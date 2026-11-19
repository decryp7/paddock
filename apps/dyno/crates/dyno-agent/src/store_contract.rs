//! The store-writer contract (Dyno ADR 0008, "Store writers"), checked over every writer: with every table refusing writes,
//! a writer stores nothing, announces nothing and keeps nothing in memory. A positive control shows the harness fails a writer
//! that keeps a change it did not store.

use crate::agent::{lock, Shared};
use crate::alarms::AlarmCentre;
use crate::rollup::HistoryWriter;
use crate::sampler::Sampler;
use crate::store::Store;
use dyno_core::alarms::{AlarmConfig, AlarmEngine, AlarmEvent};
use dyno_core::health::Severity;
use dyno_core::protocol::{Message, Overhead};
use dyno_core::time::{Timestamp, HOUR, MINUTE};
use dyno_platform::fake::{quiet_sample, Fake};
use std::sync::Arc;

/// What a writer did under refusal.
pub(crate) struct Did {
    /// Changes, alerts or rows it handed on to be announced or pushed.
    pub(crate) announced: usize,
    /// Whether its in-memory state kept the change.
    pub(crate) kept_in_memory: bool,
}

/// Runs `writer` with every table of `store` refusing writes; an `Err` names what broke the contract.
pub(crate) fn nothing_kept(name: &str, store: &Store, writer: impl FnOnce() -> Did) -> Result<(), String> {
    let before = store.row_counts_for_test();
    store.refuse_writes_for_test(true);
    let did = writer();
    store.refuse_writes_for_test(false);
    let after = store.row_counts_for_test();
    if after != before {
        return Err(format!("{name}: rows changed: {before:?} -> {after:?}"));
    }
    if did.announced > 0 {
        return Err(format!("{name}: announced {} changes it did not store", did.announced));
    }
    if did.kept_in_memory {
        return Err(format!("{name}: kept in memory a change it did not store"));
    }
    Ok(())
}

fn raise(rule: &str) -> AlarmEvent {
    AlarmEvent::Raise {
        rule: rule.into(),
        key: format!("{rule}:x"),
        severity: Severity::Warning,
        title: "t".into(),
        detail: String::new(),
        value: None,
        metric: None,
        ttl_ms: None,
        at: Timestamp(1),
    }
}

fn zone() -> dyno_core::time::Zone {
    dyno_core::time::FixedZone::shared(0)
}

#[test]
fn every_store_writer_keeps_and_announces_nothing_it_did_not_store() {
    let mut broken = Vec::new();
    let mut check = |r: Result<(), String>| {
        if let Err(e) = r {
            broken.push(e);
        }
    };

    // The alarm requests (ack, mute, shelve, raise all go through apply).
    let s = Store::memory().unwrap();
    let mut alarms = AlarmCentre::load(&s, AlarmConfig::default(), zone());
    check(nothing_kept("alarm request", &s, || {
        let pushed = alarms.apply(&s, raise("alert:a")).len();
        Did {
            announced: pushed,
            kept_in_memory: alarms.exists(1),
        }
    }));

    // The history writer (rollup), across a minute boundary so it flushes apps and rolls up.
    let s = Store::memory().unwrap();
    let mut w = HistoryWriter::default();
    let mut sample = quiet_sample(Timestamp(0));
    for i in 0..12 {
        sample.at = Timestamp(i * 5_000);
        w.record(&s, &sample, &[]).unwrap();
    }
    check(nothing_kept("history writer", &s, || {
        let mark = w.raw_mark();
        sample.at = Timestamp(MINUTE + 1);
        let r = w.record(&s, &sample, &[]);
        Did {
            announced: usize::from(r.is_ok()),
            kept_in_memory: w.raw_mark() != mark,
        }
    }));

    // The store's own whole writers.
    let s = Store::memory().unwrap();
    check(nothing_kept("put_changes", &s, || Did {
        announced: usize::from(
            s.put_changes(
                &[dyno_core::extras::changes::Change {
                    at: Timestamp(1),
                    kind: dyno_core::extras::changes::ChangeKind::Installed,
                    title: "t".into(),
                    detail: String::new(),
                }],
                Timestamp(2),
            )
            .is_ok(),
        ),
        kept_in_memory: false,
    }));
    check(nothing_kept("put_issues", &s, || Did {
        announced: s.put_issues(&[issue()]).map_or(0, |added| added.len()),
        kept_in_memory: false,
    }));
    s.apply_baseline(&[dyno_core::unusual::baseline::Change::Since(Timestamp(5))])
        .unwrap();
    check(nothing_kept("apply_baseline", &s, || Did {
        announced: usize::from(
            s.apply_baseline(&[dyno_core::unusual::baseline::Change::Since(Timestamp(9))])
                .is_ok(),
        ),
        kept_in_memory: false,
    }));
    check(nothing_kept("clear_baseline", &s, || Did {
        announced: usize::from(s.clear_baseline().is_ok()),
        kept_in_memory: false,
    }));

    // The sampler tick: an alarm (a custom rule that always holds) and an alert (a full disk) under the core lock.
    let tmp = paddock_testkit::private_tempdir().unwrap();
    let fake = Fake::new();
    let (shared, sensors, processes) = Shared::build(
        crate::paths::Dirs::at(tmp.path().join("data")),
        fake.platform(),
        Box::new(fake.clone()),
        Box::new(|_| Ok(())),
    )
    .unwrap();
    lock(&shared.core).settings.custom_alerts = vec![dyno_core::alarms::custom::CustomAlertRule {
        id: "always".into(),
        name: "Always".into(),
        metric: dyno_core::alarms::custom::CustomMetric::CpuTotal,
        op: dyno_core::alarms::custom::CustomOp::Below,
        limit: 1_000.0,
        for_secs: 0,
        severity: Severity::Warning,
        enabled: true,
    }];
    let pending = subscribe(&shared);
    let mut sampler = Sampler::new(sensors, processes);
    let t = Timestamp(HOUR + 1);
    let mut full = quiet_sample(t);
    full.disks.push(dyno_core::metrics::DiskSample {
        id: "d".into(),
        name: "Disk".into(),
        mount: "/".into(),
        total: 100,
        free: 1,
        removable: false,
    });
    let overhead = Overhead {
        agent_cpu: 0.0,
        agent_memory: 0,
        sampling_interval_ms: 2_000,
        process_interval_ms: 10_000,
    };
    {
        let core = lock(&shared.core);
        let store = &core.store;
        let before = store.row_counts_for_test();
        store.refuse_writes_for_test(true);
        drop(core);
        sampler.record_for_test(&shared, &full, overhead, t);
        let core = lock(&shared.core);
        core.store.refuse_writes_for_test(false);
        let after = core.store.row_counts_for_test();
        let mut announced = 0;
        pending.drain(&mut |m| {
            announced += usize::from(matches!(m, Message::Alarm(_) | Message::Alert(_)));
            true
        });
        if after != before {
            broken.push(format!("sampler tick: rows changed: {before:?} -> {after:?}"));
        }
        if announced > 0 {
            broken.push(format!("sampler tick: announced {announced} changes it did not store"));
        }
        if core.alarms.exists(1) {
            broken.push("sampler tick: the alarm engine kept an alarm it did not store".into());
        }
    }
    assert!(broken.is_empty(), "the store-writer contract is broken: {broken:#?}");
}

/// The harness's positive control: a writer shaped like the bug of the third review (the engine keeps a change whose write
/// failed, and returns it to push) is caught.
#[test]
fn the_harness_catches_a_writer_that_keeps_what_it_did_not_store() {
    let s = Store::memory().unwrap();
    let mut engine = AlarmEngine::new(AlarmConfig::default(), zone());
    let r = nothing_kept("broken writer", &s, || {
        let changes = engine.apply(raise("alert:a"));
        let _ = s.put_alarm(&changes[0].alarm);
        Did {
            announced: changes.len(),
            kept_in_memory: engine.alarm(1).is_some(),
        }
    });
    assert!(r.is_err(), "the harness passed a writer that keeps and pushes an unstored change");
}

fn subscribe(shared: &Arc<Shared>) -> Arc<crate::agent::Pending> {
    let (tx, rx) = std::sync::mpsc::sync_channel(64);
    std::mem::forget(rx);
    let pending = Arc::new(crate::agent::Pending::default());
    lock(&shared.subs).push(crate::agent::Sub {
        id: 1,
        detail: Arc::new(std::sync::atomic::AtomicU8::new(1)),
        tx,
        scans: Default::default(),
        volumes: Default::default(),
        jobs: Default::default(),
        pending: pending.clone(),
    });
    pending
}

fn issue() -> dyno_core::issues::Issue {
    dyno_core::issues::Issue::new(
        dyno_core::issues::IssueSourceId::MacReports,
        Timestamp(1_000),
        dyno_core::issues::IssueKind::AppCrash,
        "App",
        "t",
        "e",
        "a",
    )
}

/// What N ticks wrote and announced: rows added per table, and the alarm and alert messages sent.
struct Ticked {
    added: std::collections::BTreeMap<String, i64>,
    alarms: usize,
    alerts: usize,
}

/// Runs 13 ticks five seconds apart (a minute boundary, a quarter-hour mark, an always-held custom rule, a full disk, one
/// app), with `refused` (a table) refusing every write, if given.
fn ticks(refused: Option<&str>) -> Ticked {
    let tmp = paddock_testkit::private_tempdir().unwrap();
    let fake = Fake::new();
    let (shared, sensors, processes) = Shared::build(
        crate::paths::Dirs::at(tmp.path().join("data")),
        fake.platform(),
        Box::new(fake.clone()),
        Box::new(|_| Ok(())),
    )
    .unwrap();
    lock(&shared.core).settings.custom_alerts = vec![dyno_core::alarms::custom::CustomAlertRule {
        id: "always".into(),
        name: "Always".into(),
        metric: dyno_core::alarms::custom::CustomMetric::CpuTotal,
        op: dyno_core::alarms::custom::CustomOp::Below,
        limit: 1_000.0,
        for_secs: 0,
        severity: Severity::Warning,
        enabled: true,
    }];
    let pending = subscribe(&shared);
    let mut sampler = Sampler::new(sensors, processes);
    let overhead = Overhead {
        agent_cpu: 0.0,
        agent_memory: 0,
        sampling_interval_ms: 2_000,
        process_interval_ms: 10_000,
    };
    let before = lock(&shared.core).store.row_counts_for_test();
    if let Some(t) = refused {
        let core = lock(&shared.core);
        for op in ["INSERT", "UPDATE", "DELETE"] {
            core.store.exec_for_test(&format!(
                "CREATE TRIGGER \"refuse_{op}\" BEFORE {op} ON \"{t}\" BEGIN SELECT RAISE(ABORT, 'refused'); END;"
            ));
        }
    }
    let procs = vec![dyno_platform::fake::process(100, "busy", 40.0)];
    let groups = dyno_core::grouping::group(&procs);
    for i in 0..13 {
        let t = Timestamp(HOUR + 1 + i * 5_000);
        let mut s = quiet_sample(t);
        s.disks.push(dyno_core::metrics::DiskSample {
            id: "d".into(),
            name: "Disk".into(),
            mount: "/".into(),
            total: 100,
            free: 1,
            removable: false,
        });
        sampler.record_with_table_for_test(&shared, &s, (procs.clone(), groups.clone()), overhead, t);
    }
    let after = lock(&shared.core).store.row_counts_for_test();
    let added = after
        .iter()
        .map(|(n, c)| (n.clone(), c - before.iter().find(|(b, _)| b == n).map_or(0, |(_, c)| *c)))
        .collect();
    let (mut alarms, mut alerts) = (0, 0);
    pending.drain(&mut |m| {
        alarms += usize::from(matches!(m, Message::Alarm(_)));
        alerts += usize::from(matches!(m, Message::Alert(_)));
        true
    });
    Ticked { added, alarms, alerts }
}

/// The tables one writer stores together (all or none for that writer).
const WRITERS: &[&[&str]] = &[
    // HistoryWriter::record: the raw and per-core points and a closed minute's apps.
    &["points", "core_points", "app_minutes"],
    // The rollup: the tiers' points and their progress marks.
    &["points", "meta"],
    // An alarm change.
    &["alarms", "alarm_events"],
    // An alert and its timeline mark.
    &["alerts", "events"],
    &["disk_free"],
    // The recorder's automatic capture, which an alarm change starts.
    &["recorder_snapshots"],
];

/// Writers that run only after another stored its rows: (depends on, dependent).
const DEPENDS: &[(&str, &str)] = &[("alarms", "recorder_snapshots"), ("alarm_events", "recorder_snapshots")];

/// The tables whose writers a refusal of `table` stops: its own writers' tables and the writers that depend on them.
fn affected_by(table: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = WRITERS
        .iter()
        .filter(|w| w.contains(&table))
        .flat_map(|w| w.iter().copied())
        .collect();
    out.extend(DEPENDS.iter().filter(|(on, _)| *on == table).map(|(_, d)| *d));
    out
}

/// Whether `table` is written by the writer of `kind`'s announcements (an alarm change or an alert).
fn owns(table: &str, kind: &str) -> bool {
    WRITERS.iter().any(|w| w.contains(&kind) && w.contains(&table))
}

/// Fourth review: rule 85 for the store. Each table the ticks write refuses on its own for 13 ticks; every other writer's rows
/// keep being stored, and nothing of the refusing writer is announced.
#[test]
fn one_refusing_table_costs_only_its_own_writer() {
    let control = ticks(None);
    let written: Vec<String> = control.added.iter().filter(|(_, n)| **n > 0).map(|(t, _)| t.clone()).collect();
    for needed in ["points", "app_minutes", "alarms", "alerts", "disk_free"] {
        assert!(
            written.iter().any(|t| t == needed),
            "precondition: the ticks write {needed}: {written:?}"
        );
    }
    assert!(
        control.alarms > 0 && control.alerts > 0,
        "precondition: the ticks announce alarms and alerts"
    );
    let mut broken = Vec::new();
    for t in &written {
        let r = ticks(Some(t));
        let affected = affected_by(t);
        assert!(
            !affected.is_empty(),
            "the harness names the writer of every table the ticks write: {t}"
        );
        for other in written.iter().filter(|o| !affected.contains(&o.as_str())) {
            if r.added.get(other).copied().unwrap_or(0) <= 0 {
                broken.push(format!("{t} refused: {other} stopped being stored"));
            }
        }
        if r.added.get(t).copied().unwrap_or(0) != 0 {
            broken.push(format!("{t} refused: rows were still added to it"));
        }
        // All or none for the refused writer: where every call of a writer fills all its tables (an alarm change, an alert),
        // its other tables stay as they were. (The record fills app_minutes only when a minute closes, and meta has writers
        // besides the rollup, so those are judged by the checks above.)
        const EVERY_CALL: &[&[&str]] = &[&["alarms", "alarm_events"], &["alerts", "events"]];
        for w in EVERY_CALL.iter().filter(|w| w.contains(&t.as_str())) {
            for partner in w.iter().filter(|p| **p != t.as_str()) {
                if r.added.get(*partner).copied().unwrap_or(0) != 0 {
                    broken.push(format!("{t} refused: its writer still stored rows in {partner}"));
                }
            }
        }
        if owns(t, "alarms") && r.alarms > 0 {
            broken.push(format!("{t} refused: {} alarm changes announced", r.alarms));
        }
        if owns(t, "alerts") && r.alerts > 0 {
            broken.push(format!("{t} refused: {} alerts announced", r.alerts));
        }
    }
    assert!(broken.is_empty(), "{broken:#?}");
}
