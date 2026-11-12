#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The power wire contract with the UI (ADR 0032 section 7). The Rust types serialise into `app/src/app/core/power.wire.json`;
//! the UI's spec (`power.contract.spec.ts`) holds the same values as TypeScript literals typed by `api.types.ts` and checks they
//! equal this file, so a change on one side without the other fails a test on both. To update the file after a deliberate change,
//! run this test with `DYNO_UPDATE_WIRE_FIXTURE=1` and commit the result together with the TypeScript.

use dyno_core::power::awake::{AwakeScope, AwakeSpec, AwakeUntil, AwakeView, BusyCond, BusyNow};
use dyno_core::power::energy::{report, EnergySettings, PowerLogEntry, PowerLogKind};
use dyno_core::power::idle::{Blocker, BlockerKind, Cause, IdleAction, IdleView, Repeat, TimedAction};
use dyno_core::power::settings::PowerSettings;
use dyno_core::power::{Capability, PowerCaps, PowerView};
use dyno_core::process::ProcessRef;
use dyno_core::protocol::{Reply, Request};
use dyno_core::time::Timestamp;
use serde_json::{json, to_value, Value};
use std::path::PathBuf;

fn spec(until: AwakeUntil) -> AwakeSpec {
    AwakeSpec {
        scope: AwakeScope::System,
        until,
    }
}

fn view(awake: AwakeView, keep_awake: Capability) -> Value {
    Reply::Power(Box::new(PowerView {
        awake,
        caps: PowerCaps {
            keep_awake,
            ..Default::default()
        },
        ..Default::default()
    }))
    .into_json()
    .unwrap()
}

fn energy_log() -> Vec<PowerLogEntry> {
    let at = |ms, kind| PowerLogEntry { at: Timestamp(ms), kind };
    vec![
        at(1_700_000_000_000, PowerLogKind::AgentStarted),
        at(1_700_040_000_000, PowerLogKind::Slept),
        at(1_700_070_000_000, PowerLogKind::Woke),
        at(1_700_090_000_000, PowerLogKind::Alive),
    ]
}

fn sample() -> Value {
    let holding = |until, ends| AwakeView::Holding {
        spec: spec(until),
        since: Timestamp(1_700_000_000_000),
        ends,
        os_ref: "IOPMAssertion 4242".into(),
        busy: None,
    };
    json!({
        "specs": [
            to_value(spec(AwakeUntil::Indefinite)).unwrap(),
            to_value(AwakeSpec { scope: AwakeScope::SystemAndDisplay, until: AwakeUntil::For { minutes: 60 } }).unwrap(),
            to_value(spec(AwakeUntil::At { minute_of_day: 605 })).unwrap(),
            to_value(spec(AwakeUntil::WhileProcess { process: ProcessRef { pid: 7, started: Some(Timestamp(8)) } })).unwrap(),
            to_value(spec(AwakeUntil::WhileBusy {
                conds: vec![
                    BusyCond::Cpu { above_pct: 50.0 },
                    BusyCond::Network { above_bytes_per_s: 512_000 },
                    BusyCond::Disk { above_bytes_per_s: 5_242_880 },
                    BusyCond::Process { program: "build.exe".into() },
                ],
                grace_secs: 120,
            })).unwrap(),
        ],
        "requests": [
            to_value(Request::PowerState).unwrap(),
            to_value(Request::PowerKeepAwake { spec: spec(AwakeUntil::For { minutes: 30 }) }).unwrap(),
            to_value(Request::PowerStopKeepAwake).unwrap(),
            to_value(Request::PowerIdleCancel).unwrap(),
            to_value(Request::PowerIdleSnooze { minutes: 30 }).unwrap(),
            to_value(Request::EnergyReport { days: 7 }).unwrap(),
        ],
        "views": {
            "off": view(AwakeView::Off, Capability::Known),
            "holding": view(holding(AwakeUntil::For { minutes: 60 }, Some(Timestamp(1_700_003_600_000))), Capability::Known),
            "holdingBusy": view(
                AwakeView::Holding {
                    spec: spec(AwakeUntil::WhileBusy { conds: vec![BusyCond::Cpu { above_pct: 50.0 }], grace_secs: 120 }),
                    since: Timestamp(1_700_000_000_000),
                    ends: None,
                    os_ref: "IOPMAssertion 4242".into(),
                    busy: Some(BusyNow { active: true, detail: "CPU at 72%".into(), quiet_secs: 0, release_in_secs: None }),
                },
                Capability::Known,
            ),
            "holdingQuiet": view(
                AwakeView::Holding {
                    spec: spec(AwakeUntil::WhileBusy { conds: vec![BusyCond::Cpu { above_pct: 50.0 }], grace_secs: 120 }),
                    since: Timestamp(1_700_000_000_000),
                    ends: None,
                    os_ref: "IOPMAssertion 4242".into(),
                    busy: Some(BusyNow { active: false, detail: "quiet".into(), quiet_secs: 40, release_in_secs: Some(80) }),
                },
                Capability::Known,
            ),
            "holdingForever": view(holding(AwakeUntil::Indefinite, None), Capability::Known),
            "waiting": view(AwakeView::Waiting { spec: spec(AwakeUntil::For { minutes: 60 }), why: "the computer went to sleep".into(), since: Timestamp(1_700_000_000_000), ends: Some(Timestamp(1_700_003_600_000)) }, Capability::Known),
            "ended": view(AwakeView::Ended { why: "Dyno restarted".into(), at: Timestamp(1_700_000_100_000) }, Capability::Known),
            "cantTell": view(AwakeView::Off, Capability::CantTell { reason: "this system has no logind".into() }),
        },
        "idleViews": {
            "disabled": to_value(IdleView::Disabled).unwrap(),
            "unavailable": to_value(IdleView::Unavailable { reason: "this system gives Dyno no idle time".into() }).unwrap(),
            "watching": to_value(IdleView::Watching {
                idle_secs: 420,
                blockers: vec![Blocker { kind: BlockerKind::Audio, reason: "audio is playing".into(), cant_tell: false }],
                snoozed_until: None,
            }).unwrap(),
            "countdown": to_value(IdleView::Countdown { action: IdleAction::ShutDown, deadline: Timestamp(1_700_000_060_000), cause: Cause::Timed }).unwrap(),
            "acting": to_value(IdleView::Acting { action: IdleAction::Sleep, since: Timestamp(1_700_000_000_000) }).unwrap(),
            "stopped": to_value(IdleView::Stopped { why: "Shutdown did not happen: an app may be asking you to save".into() }).unwrap(),
        },
        "timed": to_value(TimedAction { id: 3, action: IdleAction::Hibernate, at_minute: 1380, repeat: Repeat::Weekdays(31), last_day: None, done: false }).unwrap(),
        "settings": to_value(PowerSettings::default()).unwrap(),
        "energy": {
            "notSet": Reply::Energy(Box::new(report(&energy_log(), 2, Timestamp(1_700_100_000_000), &dyno_core::time::FixedZone::UTC, &EnergySettings::default()))).into_json().unwrap(),
            "priced": Reply::Energy(Box::new(report(&energy_log(), 2, Timestamp(1_700_100_000_000), &dyno_core::time::FixedZone::UTC, &EnergySettings {
                watts_on: Some(60.0), watts_sleep: Some(2.0), tariff_per_kwh: Some(0.5), currency: Some("S$".into()), co2_g_per_kwh: Some(400.0), ..Default::default()
            }))).into_json().unwrap(),
        },
    })
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/power.wire.json")
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
        "run with DYNO_UPDATE_WIRE_FIXTURE=1 and update power.contract.spec.ts to match"
    );
}
