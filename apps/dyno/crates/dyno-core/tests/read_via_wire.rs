#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The `via` wire contract (Dyno helper-wiring plan section 3.3). Every reply type that carries a reading the helper can give
//! says where the reading came from (`ReadVia`). The Rust types serialise into `app/src/app/core/read-via.wire.json`;
//! `read-via.contract.spec.ts` holds the same values as TypeScript literals typed by `api.types.ts` and checks they equal this file.
//! To update the file after a deliberate change, run this test with `DYNO_UPDATE_WIRE_FIXTURE=1` and commit it with the TypeScript.
//!
//! The agent wire is postcard (`wire_roundtrip.rs`): a `via` field is always written, never skipped, so a type that carries one
//! also round-trips through postcard here.

use dyno_core::extras::sleep::SleepReport;
use dyno_core::firewall::{FirewallSource, RuleSource};
use dyno_core::inventory::{MemoryLayout, SystemInventory};
use dyno_core::network::NetworkSnapshot;
use dyno_core::process::{Locker, ProcessRef};
use dyno_core::read_via::{sentence, LiveVia, NextStep, ReadVia, NEEDS_ADMIN};
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
        ReadVia::unavailable("This virtual machine has no sensors."),
        ReadVia::retry("The helper did not answer."),
        ReadVia::needs_update("The helper is older than this Dyno."),
    ]
}

fn next_steps() -> Vec<NextStep> {
    vec![
        NextStep::Retry,
        NextStep::OpenSettings("helper".into()),
        NextStep::UpdateHelper,
        NextStep::UpdateApp,
        NextStep::ShowLog,
        NextStep::None {
            why: "Nothing can be done.".into(),
        },
    ]
}

#[test]
fn reasons_are_sentence_case_full_sentences_and_the_button_carries_the_action() {
    assert_eq!(sentence("other accounts are not readable"), "Other accounts are not readable.");
    assert_eq!(sentence("Already a sentence."), "Already a sentence.");
    assert_eq!(sentence("  "), "");
    assert_eq!(
        sentence("macOS reports a pressure level"),
        "macOS reports a pressure level.",
        "a name keeps its spelling"
    );
    let via = ReadVia::needs_admin("the pf rules need administrator rights");
    assert_eq!(via.reason(), Some("The pf rules need administrator rights."));
    assert!(
        !via.reason().unwrap().contains(NEEDS_ADMIN),
        "the sentence and the button are not both shown"
    );
}

#[test]
fn each_unreadable_state_has_its_own_next_step() {
    assert!(matches!(
        ReadVia::needs_update("x"),
        ReadVia::NeedsHelper {
            next: NextStep::UpdateHelper,
            ..
        }
    ));
    assert!(matches!(
        ReadVia::retry("x"),
        ReadVia::Unavailable {
            next: Some(NextStep::Retry),
            ..
        }
    ));
    assert!(matches!(ReadVia::unavailable("x"), ReadVia::Unavailable { next: None, .. }));
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
    assert!(!reason.contains(NEEDS_ADMIN), "{reason}");
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
    let startup = dyno_core::startup::overview(vec![], Default::default(), dyno_core::os::Os::Windows).with_btm_via(h.clone());
    assert_eq!(startup.btm_via, h);
    let live_via = LiveVia {
        processes: h.clone(),
        others: needs_helper(),
        temperatures: ReadVia::unavailable("x"),
        processes_hint: None,
    };
    let locker = Locker::new(ProcessRef { pid: 1, started: None }, "x".into(), None).with_via(h);
    assert_eq!(locker.via, ReadVia::Helper);
    round(&fw);
    round(&inv);
    round(&net);
    round(&sleep);
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
        "nextSteps": next_steps().iter().map(|n| to_value(n).unwrap()).collect::<Vec<_>>(),
        "inventory": to_value(
            SystemInventory::new("os".into(), None, Default::default(), MemoryLayout::TotalOnly { total: 1, reason: dyno_core::inventory::Unavailable { reason: "x".into() } }, vec![])
                .with_memory_via(needs_helper()),
        ).unwrap(),
        "startup": to_value(dyno_core::startup::overview(vec![], Default::default(), dyno_core::os::Os::Windows).with_btm_via(ReadVia::Helper)).unwrap(),
        "locker": to_value(Locker::new(ProcessRef { pid: 1, started: None }, "x".into(), None).with_via(ReadVia::Helper)).unwrap(),
        "liveVia": to_value(live_via()).unwrap(),
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

fn live_via() -> LiveVia {
    LiveVia {
        processes: ReadVia::Helper,
        others: needs_helper(),
        temperatures: ReadVia::unavailable("macOS reports a pressure level, not temperatures."),
        processes_hint: None,
    }
}

#[test]
fn the_processes_hint_travels_in_both_wire_forms() {
    let mut v = LiveVia::default();
    assert_eq!(serde_json::to_value(&v).unwrap()["processesHint"], serde_json::Value::Null);
    v.processes_hint = Some("Other accounts appear after the helper's first read.".into());
    assert_eq!(
        serde_json::to_value(&v).unwrap()["processesHint"],
        "Other accounts appear after the helper's first read."
    );
    let bytes = postcard::to_stdvec(&v).unwrap();
    assert_eq!(postcard::from_bytes::<LiveVia>(&bytes).unwrap(), v);
}

/// The sentence a reading shows when only the helper can read it names the same switch as its button ("Turn on administrator
/// readings"), in the same case (WP-14 screenshot review, Firewall page).
#[test]
fn the_needs_admin_sentence_uses_the_buttons_words() {
    assert_eq!(
        NEEDS_ADMIN,
        "Turn on administrator readings in Settings so the helper can read this."
    );
}
