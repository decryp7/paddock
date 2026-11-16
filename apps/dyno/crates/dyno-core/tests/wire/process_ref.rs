#![allow(clippy::unwrap_used, clippy::expect_used)]

//! How a process is named on the UI wire (rule 53): `ProcessRef { pid, started }` everywhere. The Rust types serialise into
//! `app/src/app/core/process-ref.wire.json`; `process-ref.contract.spec.ts` holds the same values as TypeScript literals typed by
//! `api.types.ts` and checks they equal this file, so the hand-mirrored types cannot drift. To update the file after a deliberate
//! change, run this test with `DYNO_UPDATE_WIRE_FIXTURE=1` and commit the result together with the TypeScript.

use dyno_core::extras::gpu::{GpuProcess, GpuReport};
use dyno_core::extras::sleep::{Prevents, SleepBlocker, SleepReport};
use dyno_core::network::{ConnectionRow, Exposure, ListenerRow, Owner, Proto, SocketState};
use dyno_core::network_traffic::TrafficRow;
use dyno_core::process::{
    ListOrReason, Measured, Owner as ProcOwner, ParentLink, ProcessDetails, ProcessRef, ProcessSample, RunState, SignatureInfo,
};
use dyno_core::snapshot::{ProcessChange, ProcessSummary, SnapshotDiff, SnapshotInfo};
use dyno_core::time::Timestamp;
use serde_json::{json, to_value, Value};
use std::path::PathBuf;

fn started(pid: u32) -> ProcessRef {
    ProcessRef::new(pid, Timestamp(1_700_000_000_000 + i64::from(pid)))
}

fn sample() -> ProcessSample {
    ProcessSample {
        pid: 7,
        parent: Some(1),
        name: "node".into(),
        exe: Some("/bin/node".into()),
        user: Measured::Value("ana".into()),
        cpu: Measured::Value(1.5),
        memory: Measured::Value(1024),
        disk_read_bps: Measured::Value(0.0),
        disk_write_bps: Measured::Value(0.0),
        started: Measured::Value(Timestamp(1_700_000_000_007)),
        owner: ProcOwner::Own,
        state: RunState::Running,
    }
}

fn info(id: &str, at: i64) -> SnapshotInfo {
    SnapshotInfo {
        id: id.into(),
        at: Timestamp(at),
        label: id.into(),
        process_count: 1,
    }
}

fn sample_json() -> Value {
    let process = Owner::Process {
        process: started(7),
        name: "node".into(),
        path: Some("/bin/node".into()),
    };
    let pid_only = Owner::Process {
        process: ProcessRef::pid_only(8),
        name: "tool".into(),
        path: None,
    };
    let listener = ListenerRow {
        proto: Proto::Tcp,
        ipv6: false,
        local: "0.0.0.0".into(),
        port: 3000,
        exposure: Exposure::AllInterfaces,
        owner: process.clone(),
        first_seen: None,
    };
    let connection = ConnectionRow {
        proto: Proto::Tcp,
        ipv6: false,
        local: "192.0.2.1".into(),
        local_port: 50_000,
        remote: "198.51.100.2".into(),
        remote_port: 443,
        remote_is_loopback: false,
        lookup_ok: true,
        state: SocketState::Established,
        state_label: "ESTABLISHED".into(),
        owner: pid_only.clone(),
        first_seen: None,
        country: None,
        flagged_by: vec![],
    };
    let traffic = TrafficRow {
        process: started(7),
        name: "node".into(),
        rx_bps: 10.0,
        tx_bps: 2.0,
    };
    let details = ProcessDetails {
        process: sample(),
        command_line: vec!["node".into()],
        cwd: None,
        priority: None,
        affinity: None,
        efficiency_mode: None,
        threads: None,
        handles: None,
        parents: vec![
            ParentLink {
                process: started(1),
                name: "init".into(),
            },
            ParentLink {
                process: ProcessRef::pid_only(0),
                name: "kernel".into(),
            },
        ],
        signature: SignatureInfo::Unsigned,
        open_files: ListOrReason::Unavailable("r".into()),
        connections: ListOrReason::Unavailable("r".into()),
        modules: ListOrReason::Unavailable("r".into()),
        startup_source: None,
    };
    let gpu = GpuReport {
        processes: vec![GpuProcess {
            process: started(311),
            name: "WindowServer".into(),
            gpu_percent: 14.0,
            memory_bytes: Some(2048),
        }],
        source: Some("counters".into()),
        unavailable: None,
    };
    let sleep = SleepReport {
        blockers: vec![
            SleepBlocker {
                name: "zoom".into(),
                process: Some(started(4821)),
                prevents: Prevents::DisplaySleep,
                reason: "call".into(),
                held_for: None,
            },
            SleepBlocker {
                name: "driver".into(),
                process: None,
                prevents: Prevents::Sleep,
                reason: "audio".into(),
                held_for: Some("00:01:00".into()),
            },
        ],
        ..SleepReport::default()
    };
    let summary = ProcessSummary {
        process: started(9),
        name: "old".into(),
        exe: None,
        cpu: Measured::Value(1.0),
        memory: Measured::Value(2),
    };
    let diff = SnapshotDiff {
        before: info("a", 1),
        after: info("b", 2),
        added: vec![summary.clone()],
        removed: vec![ProcessSummary {
            process: ProcessRef::pid_only(10),
            ..summary
        }],
        changed: vec![ProcessChange {
            process: started(7),
            name: "node".into(),
            cpu_before: 1.0,
            cpu_after: 9.0,
            memory_before: 1,
            memory_after: 99,
        }],
        memory_used_delta: 98,
        cpu_delta: 8.0,
    };
    json!({
        "listener": to_value(&listener).unwrap(),
        "connection": to_value(&connection).unwrap(),
        "ownerUnknown": to_value(Owner::Unknown { reason: "why".into() }).unwrap(),
        "traffic": to_value(&traffic).unwrap(),
        "details": to_value(&details).unwrap(),
        "gpu": to_value(&gpu).unwrap(),
        "sleep": to_value(&sleep).unwrap(),
        "diff": to_value(&diff).unwrap(),
    })
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/process-ref.wire.json")
}

#[test]
fn the_wire_fixture_matches_the_rust_types() {
    let now = sample_json();
    let path = fixture_path();
    if std::env::var_os("DYNO_UPDATE_WIRE_FIXTURE").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&now).unwrap() + "\n").unwrap();
    }
    let on_disk: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("the fixture exists")).unwrap();
    assert_eq!(
        on_disk, now,
        "run with DYNO_UPDATE_WIRE_FIXTURE=1 and update process-ref.contract.spec.ts to match"
    );
}

#[test]
fn a_process_without_a_start_time_serialises_it_as_null() {
    let v = to_value(ProcessRef::pid_only(8)).unwrap();
    assert_eq!(v, json!({"pid": 8, "started": null}));
}
