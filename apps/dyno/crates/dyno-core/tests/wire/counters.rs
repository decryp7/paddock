#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The performance counters' wire contract with the UI (ADR 0029 WP-6): the Rust types serialise into
//! `app/src/app/core/counters.wire.json`; `counters.contract.spec.ts` holds the same values as TypeScript literals typed by
//! `api.types.ts` and checks they equal this file. To update the file after a deliberate change, run this test with
//! `DYNO_UPDATE_WIRE_FIXTURE=1` and commit the result together with the TypeScript.

use dyno_core::counters::{
    CounterHistory, CounterHit, CounterObject, CounterObjectList, CounterObjectSummary, CounterPoint, CounterReading, CounterSeries,
    CounterUnit,
};
use dyno_core::metrics::Reading;
use dyno_core::protocol::{Request, TimelineEvent, TimelineEventKind};
use dyno_core::time::Timestamp;
use serde_json::{json, to_value, Value};
use std::path::PathBuf;

fn list(saved_at: Option<Timestamp>) -> CounterObjectList {
    CounterObjectList {
        objects: vec![
            CounterObjectSummary {
                name: "Memory".into(),
                counters: 12,
                instances_total: 0,
            },
            CounterObjectSummary {
                name: "Processor".into(),
                counters: 9,
                instances_total: 16,
            },
        ],
        truncated: false,
        saved_at,
    }
}

fn sample() -> Value {
    let hit = CounterHit {
        path: "\\Processor(0)\\% Processor Time".into(),
        object: "Processor".into(),
        counter: "% Processor Time".into(),
        instance: Some("0".into()),
        unit: CounterUnit::Percent,
    };
    let global = CounterHit {
        path: "\\Memory\\Available Bytes".into(),
        object: "Memory".into(),
        counter: "Available Bytes".into(),
        instance: None,
        unit: CounterUnit::Bytes,
    };
    let object: CounterObject = serde_json::from_value(json!({
        "name": "Processor",
        "counters": [
            {"name": "% Processor Time", "unit": "percent", "kind": "gauge", "help": "How busy the CPU is."},
            {"name": "Interrupts/sec", "unit": "perSec", "kind": "cumulative", "help": null}
        ],
        "instances": ["0", "1"],
        "instancesTotal": 16
    }))
    .unwrap();
    let history = CounterHistory {
        from: Timestamp(1_700_000_000_000),
        to: Timestamp(1_700_000_060_000),
        series: vec![CounterSeries {
            path: hit.path.clone(),
            unit: CounterUnit::Percent,
            points: vec![
                CounterPoint {
                    at: Timestamp(1_700_000_015_000),
                    value: 12.5,
                },
                CounterPoint {
                    at: Timestamp(1_700_000_030_000),
                    value: 80.0,
                },
            ],
        }],
        events: vec![TimelineEvent {
            at: Timestamp(1_700_000_020_000),
            kind: TimelineEventKind::Alert,
            label: "CPU high".into(),
        }],
    };
    json!({
        "requests": {
            "objects": to_value(Request::CounterObjects).unwrap(),
            "object": to_value(Request::CounterObject { name: "Processor".into() }).unwrap(),
            "search": to_value(Request::CounterSearch { query: "processor time".into(), limit: 20 }).unwrap(),
            "values": to_value(Request::CounterValues { paths: vec![hit.path.clone()] }).unwrap(),
            "history": to_value(Request::CounterHistory { paths: vec![hit.path.clone()], minutes: 5 }).unwrap(),
        },
        "objectList": to_value(list(None)).unwrap(),
        "objectListSaved": to_value(list(Some(Timestamp(1_700_000_100_000)))).unwrap(),
        "object": to_value(&object).unwrap(),
        "hits": [to_value(&hit).unwrap(), to_value(&global).unwrap()],
        "readings": [
            to_value(CounterReading::new(hit.path.clone(), CounterUnit::Percent, Reading::Available(12.5))).unwrap(),
            to_value(CounterReading::new(hit.path.clone(), CounterUnit::Percent, Reading::Available(95.0))).unwrap(),
            to_value(CounterReading::new(global.path.clone(), CounterUnit::Bytes, Reading::Available(4096.0))).unwrap(),
            to_value(CounterReading::new(global.path.clone(), CounterUnit::Bytes, Reading::Unavailable("needs a second reading".into()))).unwrap(),
        ],
        "history": to_value(&history).unwrap(),
    })
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/counters.wire.json")
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
        "run with DYNO_UPDATE_WIRE_FIXTURE=1 and update counters.contract.spec.ts to match"
    );
}

#[test]
fn an_older_saved_answer_without_the_saved_time_still_reads() {
    // New keys are optional (the cache keeps documents across versions).
    let l: CounterObjectList = serde_json::from_value(json!({"objects": [], "truncated": false})).unwrap();
    assert_eq!(l.saved_at, None);
}
