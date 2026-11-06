#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The jobs' wire contract with the UI. The Rust types serialise into `app/src/app/core/jobs.wire.json`; the UI's spec
//! (`jobs.contract.spec.ts`) holds the same values as TypeScript literals typed by `api.types.ts` and the shell's parser and checks
//! they equal this file, so a change on one side without the other fails a test on both. To update the file after a deliberate
//! change, run this test with `DYNO_UPDATE_WIRE_FIXTURE=1` and commit the result together with the TypeScript.

use dyno_core::extras::files::{FinderMode, FinderQuery};
use dyno_core::issues::IssuesQuery;
use dyno_core::jobs::{JobOutput, JobSpec};
use dyno_core::process::ProcessRef;
use dyno_core::protocol::{Reply, Request};
use dyno_core::time::Timestamp;
use dyno_core::weblinks::{FileHashInfo, HashSource};
use paddock_jobs::wire::{WirePhase, WireSnapshot, WireState};
use paddock_jobs::{CancelReason, JobSnapshot, Unit};
use serde_json::{json, to_value, Value};
use std::path::PathBuf;

fn snapshot(id: u64, state: WireState) -> WireSnapshot {
    WireSnapshot {
        id,
        kind: "find-files".into(),
        title: "Finding files".into(),
        subject: Some("/Users/alex/Projects".into()),
        state,
        seq: 5,
        elapsed_ms: 1200,
        eta_ms: None,
        cancellable: true,
        not_cancellable_reason: None,
    }
}

fn ui(w: WireSnapshot) -> Value {
    to_value(JobSnapshot::from(w)).unwrap()
}

fn sample() -> Value {
    let walk = WirePhase {
        id: "walk".into(),
        label: "Listing files".into(),
        unit: Unit::Items,
        total: None,
        done: 1200,
    };
    let hash = WirePhase {
        id: "contents".into(),
        label: "Comparing contents".into(),
        unit: Unit::Bytes,
        total: Some(4096),
        done: 1024,
    };
    let spec = |s: JobSpec| to_value(Request::StartJob { spec: s }).unwrap();
    json!({
        "snapshots": {
            "queued": ui(snapshot(1, WireState::Queued)),
            "starting": ui(snapshot(2, WireState::Running(None))),
            "counting": ui(snapshot(3, WireState::Running(Some(walk)))),
            "bytes": ui(WireSnapshot { not_cancellable_reason: Some("A system log is being read; Cancel takes effect when it has been read.".into()), cancellable: false, eta_ms: Some(900), ..snapshot(4, WireState::Running(Some(hash))) }),
            "done": ui(snapshot(5, WireState::Done)),
            "failed": ui(snapshot(6, WireState::Failed("Give the full path of the folder.".into()))),
            "cancelled": ui(snapshot(7, WireState::Cancelled(CancelReason::User))),
        },
        "requests": [
            spec(JobSpec::FindFiles { query: FinderQuery { folder: "/Users/alex/Projects".into(), mode: FinderMode::Both, min_bytes: 1_048_576 } }),
            spec(JobSpec::RescanIssues { query: IssuesQuery { days: 14 } }),
            spec(JobSpec::FileHash { source: HashSource::Path("/Applications/Slack.app/slack".into()) }),
            spec(JobSpec::FileHash { source: HashSource::Process(ProcessRef { pid: 3, started: Some(Timestamp(4)) }) }),
            spec(JobSpec::SleepBlockers),
            spec(JobSpec::BootAnalysis),
            spec(JobSpec::AppUsage),
            spec(JobSpec::SoftwareInventory),
            spec(JobSpec::Helper { action: dyno_core::helper_wire::HelperAction::TurnOn }),
            spec(JobSpec::Helper { action: dyno_core::helper_wire::HelperAction::TurnOff }),
            to_value(Request::HelperStatus).unwrap(),
            to_value(Request::Jobs).unwrap(),
            to_value(Request::CancelJob { id: 3 }).unwrap(),
            to_value(Request::JobResult { id: 3 }).unwrap(),
        ],
        "settings": {
            "disk": to_value(dyno_core::disk::settings::DiskSettings::default()).unwrap(),
            "diskFixed": to_value(dyno_core::disk::settings::DiskSettings { read_threads: dyno_core::threads::Threads::Fixed(6), ..Default::default() }).unwrap(),
            "jobs": to_value(dyno_core::jobs::JobSettings::default()).unwrap(),
        },
        "outputs": [
            Reply::JobOutput(Box::new(JobOutput::FileHash(FileHashInfo { path: "/a".into(), bytes: 3, sha256: "0".repeat(64) }))).into_json().unwrap(),
            Reply::JobCancelled(true).into_json().unwrap(),
        ],
    })
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/jobs.wire.json")
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
        "run with DYNO_UPDATE_WIRE_FIXTURE=1 and update jobs.contract.spec.ts to match"
    );
}
