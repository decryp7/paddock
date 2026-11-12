#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The installed-software list and the inventory report's wire contract with the UI (ADR 0029 WP-6): the Rust types serialise
//! into `app/src/app/core/extras.wire.json`; `extras.contract.spec.ts` holds the same values as TypeScript literals typed by
//! `api.types.ts` and checks they equal this file. To update the file after a deliberate change, run this test with
//! `DYNO_UPDATE_WIRE_FIXTURE=1` and commit the result together with the TypeScript.

use dyno_core::extras::changes::ChangesQuery;
use dyno_core::extras::software::{InventoryFormat, SoftwareInventory, SoftwareItem};
use dyno_core::protocol::{Request, SavedInventory};
use dyno_core::time::Timestamp;
use serde_json::{json, to_value, Value};
use std::path::PathBuf;

fn item(name: &str, version: Option<&str>, publisher: Option<&str>) -> SoftwareItem {
    SoftwareItem {
        name: name.into(),
        version: version.map(str::to_string),
        publisher: publisher.map(str::to_string),
    }
}

fn sample() -> Value {
    let read_now = SoftwareInventory::from_items(vec![item("curl", Some("8.4"), None), item("bash", Some("5.2"), Some("GNU"))]);
    let saved = SoftwareInventory {
        saved_at: Some(Timestamp(1_700_000_100_000)),
        ..read_now.clone()
    };
    let unavailable = SoftwareInventory::unavailable("No package manager Dyno knows answered.");
    json!({
        "requests": {
            "changes": to_value(Request::Changes { query: ChangesQuery { around: None, hours: 72 } }).unwrap(),
            "changesAround": to_value(Request::Changes { query: ChangesQuery { around: Some(Timestamp(1_700_000_000_000)), hours: 6 } }).unwrap(),
            "saveReport": to_value(Request::SaveInventoryReport { path: "/tmp/inventory.md".into(), format: InventoryFormat::Markdown }).unwrap(),
            "saveReportJson": to_value(Request::SaveInventoryReport { path: "/tmp/inventory.json".into(), format: InventoryFormat::Json }).unwrap(),
        },
        "software": to_value(&read_now).unwrap(),
        "softwareSaved": to_value(&saved).unwrap(),
        "softwareUnavailable": to_value(&unavailable).unwrap(),
        "savedReport": to_value(SavedInventory { path: "/tmp/inventory.md".into(), bytes: 2048 }).unwrap(),
    })
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/extras.wire.json")
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
        "run with DYNO_UPDATE_WIRE_FIXTURE=1 and update extras.contract.spec.ts to match"
    );
}

#[test]
fn an_older_saved_list_without_the_saved_time_still_reads() {
    let l: SoftwareInventory = serde_json::from_value(json!({"items": [], "truncated": false, "unavailable": null})).unwrap();
    assert_eq!(l.saved_at, None);
}
