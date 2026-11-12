#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The Wake-on-LAN wire contract with the UI (ADR 0032 section 7). The Rust types serialise into `app/src/app/core/wol.wire.json`;
//! `wol.contract.spec.ts` holds the same values as TypeScript literals typed by `api.types.ts` and checks they equal this file.
//! To update the file after a deliberate change, run this test with `DYNO_UPDATE_WIRE_FIXTURE=1` and commit the result together
//! with the TypeScript. The devices and addresses are invented (documentation ranges, a locally administered hardware address).

use dyno_core::jobs::{JobOutput, JobSpec};
use dyno_core::power::Capability;
use dyno_core::protocol::{Reply, Request};
use dyno_core::time::Timestamp;
use dyno_core::wol::wake::{Answer, AnswerHow, SendResult, WolOutcome};
use dyno_core::wol::{
    ItemState, LastWake, Mac, Neighbour, NeighbourState, ReadinessItem, ReadinessKind, SecureOnEdit, ThisPcWol, WolDevice, WolDeviceInput,
    WolView,
};
use serde_json::{json, to_value, Value};
use std::path::PathBuf;

fn device() -> WolDevice {
    WolDevice {
        id: 1,
        name: "Test box".into(),
        mac: Mac([2, 0, 0x5e, 0x10, 0, 1]),
        last_ip: Some("192.0.2.40".into()),
        interface: Some("en0".into()),
        port: 9,
        secure_on: true,
        last_wake: Some(LastWake {
            at: Timestamp(1_700_000_000_000),
            ok: true,
            tested: true,
            summary: "Awake after 23 s (answered ping).".into(),
        }),
    }
}

fn outcome() -> WolOutcome {
    WolOutcome {
        sent: vec![SendResult {
            interface: "en0".into(),
            dest: "255.255.255.255".into(),
            bytes: 102,
            sent: 3,
            error: None,
        }],
        answered: Some(Answer {
            how: AnswerHow::Ping,
            after_secs: 23,
            new_ip: Some("192.0.2.41".into()),
        }),
        tested: true,
        elapsed_secs: 23,
        timed_out: false,
        cancelled: false,
        message: "Awake after 23 s (answered ping). Its address changed; Dyno saved the new one.".into(),
    }
}

fn reply(r: Reply) -> Value {
    r.into_json().unwrap()
}

fn sample() -> Value {
    let input = |secure_on| WolDeviceInput {
        id: Some(1),
        name: "Test box".into(),
        mac: "02:00:5e:10:00:01".into(),
        last_ip: Some("192.0.2.40".into()),
        interface: None,
        secure_on,
    };
    json!({
        "requests": [
            to_value(Request::WolDevices).unwrap(),
            to_value(Request::WolCandidates).unwrap(),
            to_value(Request::WolReadiness).unwrap(),
            to_value(Request::WolSave { device: input(SecureOnEdit::Keep) }).unwrap(),
            to_value(Request::WolSave { device: input(SecureOnEdit::Clear) }).unwrap(),
            to_value(Request::WolSave { device: input(SecureOnEdit::Set { password: "01:02:03:04".into() }) }).unwrap(),
            to_value(Request::WolRemove { id: 1 }).unwrap(),
            to_value(Request::WolWake { id: 1, test: true }).unwrap(),
            to_value(Request::StartJob { spec: JobSpec::WolWake { id: 1, test: false } }).unwrap(),
        ],
        "views": {
            "list": reply(Reply::Wol(Box::new(WolView { enabled: true, devices: vec![device()], can_send: Capability::Known }))),
            "empty": reply(Reply::Wol(Box::new(WolView { enabled: true, devices: vec![], can_send: Capability::Known }))),
            "off": reply(Reply::Wol(Box::new(WolView { enabled: false, devices: vec![], can_send: Capability::Known }))),
            "cantSend": reply(Reply::Wol(Box::new(WolView {
                enabled: true,
                devices: vec![device()],
                can_send: Capability::CantTell { reason: "No network card is connected.".into() },
            }))),
        },
        "candidates": reply(Reply::WolCandidates(vec![
            Neighbour { name: Some("laptop.example".into()), mac: Mac([2, 0, 0x5e, 0x10, 0, 2]), ip: "192.0.2.50".into(), interface: "en0".into(), state: NeighbourState::Reachable },
            Neighbour { name: None, mac: Mac([2, 0, 0x5e, 0x10, 0, 3]), ip: "192.0.2.51".into(), interface: "en0".into(), state: NeighbourState::Stale },
            Neighbour { name: None, mac: Mac([2, 0, 0x5e, 0x10, 0, 4]), ip: "2001:db8::4".into(), interface: "en1".into(), state: NeighbourState::Probe },
            Neighbour { name: None, mac: Mac([2, 0, 0x5e, 0x10, 0, 5]), ip: "192.0.2.52".into(), interface: "en0".into(), state: NeighbourState::Permanent },
            Neighbour { name: None, mac: Mac([2, 0, 0x5e, 0x10, 0, 6]), ip: "192.0.2.53".into(), interface: "en0".into(), state: NeighbourState::Unknown },
        ])),
        "readiness": reply(Reply::WolReadiness(Box::new(ThisPcWol {
            items: vec![
                ReadinessItem { kind: ReadinessKind::OsSetting, subject: None, state: ItemState::Ok { note: "Wake for network access is on.".into() } },
                ReadinessItem {
                    kind: ReadinessKind::NicWake,
                    subject: Some("Ethernet".into()),
                    state: ItemState::Problem { note: "The card may not wake the computer.".into(), fix: "Allow it in Device Manager.".into() },
                },
                ReadinessItem { kind: ReadinessKind::FastStartup, subject: None, state: ItemState::CantTell { reason: "Dyno could not read it.".into() } },
                ReadinessItem { kind: ReadinessKind::SleepMode, subject: None, state: ItemState::Ok { note: "Classic sleep.".into() } },
                ReadinessItem { kind: ReadinessKind::Firmware, subject: None, state: ItemState::CantTell { reason: "The BIOS is not readable.".into() } },
            ],
        }))),
        "outcome": reply(Reply::JobOutput(Box::new(JobOutput::Wol(Box::new(outcome()))))),
        "outcomeTimedOut": reply(Reply::JobOutput(Box::new(JobOutput::Wol(Box::new(WolOutcome {
            answered: None,
            timed_out: true,
            elapsed_secs: 120,
            message: "No answer in 120 s: it may be off at the wall, have Wake-on-LAN off, or block ping.".into(),
            ..outcome()
        }))))),
    })
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/wol.wire.json")
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
        "run with DYNO_UPDATE_WIRE_FIXTURE=1 and update wol.contract.spec.ts to match"
    );
}
