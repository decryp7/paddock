#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The `via` wire contract (Dyno helper-wiring plan section 3.3). Every reply type that carries a reading the helper can give
//! says where the reading came from (`ReadVia`). The Rust types serialise into `app/src/app/core/read-via.wire.json`;
//! `read-via.contract.spec.ts` holds the same values as TypeScript literals typed by `api.types.ts` and checks they equal this file.
//! To update the file after a deliberate change, run this test with `DYNO_UPDATE_WIRE_FIXTURE=1` and commit it with the TypeScript.
//!
//! The agent wire is postcard (`wire_roundtrip.rs`): a `via` field is always written, never skipped, so a type that carries one
//! also round-trips through postcard here.

use dyno_core::backup::BackupReport;
use dyno_core::extras::sleep::SleepReport;
use dyno_core::firewall::{FirewallSource, RuleSource};
use dyno_core::inventory::{MemoryLayout, SystemInventory};
use dyno_core::network::NetworkSnapshot;
use dyno_core::process::{Locker, ProcessRef};
use dyno_core::read_via::{LiveVia, NextStep, ReadVia, NEEDS_ADMIN};
use serde_json::{json, to_value, Value};
use std::path::PathBuf;

fn needs_helper() -> ReadVia {
    ReadVia::needs_admin("Other accounts' processes are not readable without the helper.")
}

fn variants() -> Vec<ReadVia> {
    vec![
        ReadVia::Helper,
        ReadVia::Local,
        needs_helper(),
        ReadVia::Unavailable {
            reason: "This virtual machine has no sensors.".into(),
        },
    ]
}

#[test]
fn local_is_the_default_and_the_only_value_that_needs_no_helper() {
    assert_eq!(ReadVia::default(), ReadVia::Local);
    assert!(ReadVia::Local.is_local());
    assert!(!ReadVia::Helper.is_local());
}

#[test]
fn the_shared_needs_admin_sentence_is_in_every_needs_helper_reason_and_opens_settings_helper() {
    let ReadVia::NeedsHelper { reason, next } = needs_helper() else {
        panic!("needs_admin makes NeedsHelper")
    };
    assert!(reason.contains(NEEDS_ADMIN), "{reason}");
    assert_eq!(next, NextStep::OpenSettings("helper".into()));
}

#[test]
fn every_via_round_trips_through_postcard_and_json() {
    for v in variants() {
        let bytes = postcard::to_stdvec(&v).unwrap();
        assert_eq!(postcard::from_bytes::<ReadVia>(&bytes).unwrap(), v);
        assert_eq!(serde_json::from_value::<ReadVia>(to_value(&v).unwrap()).unwrap(), v);
    }
}

#[test]
fn every_reply_type_that_carries_a_reading_has_a_via_that_defaults_to_local_and_survives_postcard() {
    let h = ReadVia::Helper;
    let fw = FirewallSource::new(RuleSource::Pf);
    assert_eq!(fw.via, ReadVia::Local);
    let fw = fw.with_via(h.clone());
    let inv = SystemInventory::new(
        "os".into(),
        None,
        Default::default(),
        MemoryLayout::TotalOnly {
            total: 1,
            reason: dyno_core::inventory::Unavailable { reason: "x".into() },
        },
        vec![],
    );
    assert_eq!(inv.memory_via, ReadVia::Local);
    let inv = inv.with_memory_via(h.clone());
    let net = NetworkSnapshot {
        traffic_via: h.clone(),
        owners_via: needs_helper(),
        ..NetworkSnapshot::default()
    };
    let sleep = SleepReport {
        blockers_via: h.clone(),
        ..SleepReport::default()
    };
    let backup = BackupReport {
        via: h.clone(),
        ..BackupReport::default()
    };
    let startup = dyno_core::startup::overview(vec![], Default::default(), dyno_core::os::Os::Windows).with_btm_via(h.clone());
    assert_eq!(startup.btm_via, h);
    let live_via = LiveVia {
        processes: h.clone(),
        others: needs_helper(),
        temperatures: ReadVia::Unavailable { reason: "x".into() },
    };
    let locker = Locker::new(ProcessRef { pid: 1, started: None }, "x".into(), None).with_via(h);
    assert_eq!(locker.via, ReadVia::Helper);
    round(&fw);
    round(&inv);
    round(&net);
    round(&sleep);
    round(&backup);
    round(&locker);
    round(&startup);
    round(&live_via);
}

fn round<T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(v: &T) {
    let bytes = postcard::to_stdvec(v).unwrap();
    assert_eq!(&postcard::from_bytes::<T>(&bytes).unwrap(), v);
}

fn sample() -> Value {
    json!({
        "variants": variants().iter().map(|v| to_value(v).unwrap()).collect::<Vec<_>>(),
        "firewall": to_value(FirewallSource::new(RuleSource::Pf).with_via(ReadVia::Helper)).unwrap(),
        "network": to_value(NetworkSnapshot { traffic_via: ReadVia::Helper, owners_via: needs_helper(), ..NetworkSnapshot::default() }).unwrap(),
        "sleep": to_value(SleepReport { blockers_via: ReadVia::Helper, ..SleepReport::default() }).unwrap(),
        "backup": to_value(BackupReport { via: ReadVia::Unavailable { reason: "No versions.".into() }, ..BackupReport::default() }).unwrap(),
    })
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/read-via.wire.json")
}

#[test]
fn the_wire_fixture_matches_the_rust_types() {
    let now = sample();
    let path = fixture_path();
    if std::env::var_os("DYNO_UPDATE_WIRE_FIXTURE").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&now).unwrap() + "\n").unwrap();
    }
    let on_disk: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("the fixture exists")).unwrap();
    assert_eq!(
        on_disk, now,
        "run with DYNO_UPDATE_WIRE_FIXTURE=1 and update read-via.contract.spec.ts to match"
    );
}
