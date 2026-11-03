#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The scheduled-wake wire contract with the UI (ADR 0032 section 5). The Rust types serialise into `app/src/app/core/wake.wire.json`;
//! `wake.contract.spec.ts` holds the same values as TypeScript literals typed by `api.types.ts` and checks they equal this file.
//! To update the file after a deliberate change, run this test with `DYNO_UPDATE_WIRE_FIXTURE=1` and commit it with the TypeScript.

use dyno_core::power::schedule::{
    DynoJob, LastRun, Repeat, RowState, WakeInput, WakeRoute, WakeRouteState, WakeRow, WakeSchedule, WakesView,
};
use dyno_core::protocol::{Reply, Request};
use dyno_core::time::Timestamp;
use serde_json::{json, to_value, Value};
use std::path::PathBuf;

fn schedule() -> WakeSchedule {
    WakeSchedule {
        id: 1,
        name: "Morning backup check".into(),
        minute_of_day: 420,
        repeat: Repeat::Days { mask: 0b0011111 },
        stay_awake_minutes: Some(30),
        wake_devices: vec![2],
        job: Some(DynoJob::BackupCheck),
        enabled: true,
        next_at: Some(Timestamp(1_700_050_000_000)),
        last_run: Some(LastRun {
            at: Timestamp(1_700_000_000_000),
            summary: "staying awake 30 min; started its job".into(),
        }),
    }
}

fn reply(state: RowState, route: WakeRouteState) -> Value {
    Reply::PowerWakes(Box::new(WakesView {
        rows: vec![WakeRow { entry: schedule(), state }],
        route,
    }))
    .into_json()
    .unwrap()
}

fn sample() -> Value {
    let input = WakeInput {
        id: Some(1),
        name: "Morning backup check".into(),
        minute_of_day: 420,
        repeat: Repeat::Once,
        stay_awake_minutes: None,
        wake_devices: vec![2],
        job: Some(DynoJob::SoftwareInventory),
        enabled: false,
    };
    json!({
        "requests": [
            to_value(Request::PowerWakes).unwrap(),
            to_value(Request::PowerSetWake { entry: input }).unwrap(),
            to_value(Request::PowerCancelWake { id: 1 }).unwrap(),
        ],
        "repeats": [to_value(Repeat::Once).unwrap(), to_value(Repeat::Daily).unwrap(), to_value(Repeat::Days { mask: 96 }).unwrap()],
        "views": {
            "armedHelper": reply(RowState::Armed { route: WakeRoute::Helper }, WakeRouteState::HelperReady),
            "armedAgent": reply(RowState::Armed { route: WakeRoute::Agent }, WakeRouteState::Agent { note: "Windows wakes the computer for a timer that Dyno sets.".into(), cant_tell: vec!["Allow wake timers: Dyno can't tell.".into()] }),
            "off": reply(RowState::Off, WakeRouteState::HelperReady),
            "paused": reply(RowState::Paused, WakeRouteState::HelperReady),
            "needsHelper": reply(RowState::NeedsHelper, WakeRouteState::NeedsHelper { reason: "Administrator readings are off.".into() }),
            "failed": reply(RowState::Failed { why: "The helper refused.".into() }, WakeRouteState::Unavailable { reason: "This build has no helper.".into() }),
            "empty": Reply::PowerWakes(Box::new(WakesView { rows: vec![], route: WakeRouteState::HelperReady })).into_json().unwrap(),
        },
    })
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/wake.wire.json")
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
        "run with DYNO_UPDATE_WIRE_FIXTURE=1 and update wake.contract.spec.ts to match"
    );
}
