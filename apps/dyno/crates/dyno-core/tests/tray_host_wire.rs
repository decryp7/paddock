#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The wire contract of the tray's availability with the UI. The Rust values serialise into
//! `app/src/app/core/tray-host.wire.json`; `tray-host.contract.spec.ts` holds the same values as TypeScript literals typed by
//! `api.types.ts` and checks they equal this file. To update it after a deliberate change, run this test with
//! `DYNO_UPDATE_WIRE_FIXTURE=1` and commit the result together with the TypeScript.

use dyno_core::tray_host::{availability, HostProbe, TrayAvailability};
use serde_json::{json, to_value, Value};
use std::path::PathBuf;

fn sample() -> Value {
    json!({
        "shown": to_value(availability(&HostProbe::Present)).unwrap(),
        "watcherAbsent": to_value(availability(&HostProbe::WatcherAbsent)).unwrap(),
        "noHostRegistered": to_value(availability(&HostProbe::NoHostRegistered)).unwrap(),
        "cantTell": to_value(availability(&HostProbe::CantTell("no session bus".into()))).unwrap(),
        "standardNote": to_value(TrayAvailability::no_host()).unwrap(),
    })
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/tray-host.wire.json")
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
        "run with DYNO_UPDATE_WIRE_FIXTURE=1 and update tray-host.contract.spec.ts to match"
    );
}
