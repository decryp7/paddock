#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The positional postcard bytes of the job snapshot inside `Message::Job` (protocol 7, unchanged in 8; the health report is pinned in `health_wire.rs`). A change to these bytes is a new
//! `PROTOCOL_VERSION`: the protocol is postcard, so an older peer reads a changed layout wrongly. Update the expected bytes
//! only together with the version bump and its entry in the doc list at the top of `protocol.rs`.

use dyno_core::protocol::{Message, PROTOCOL_VERSION};
use paddock_jobs::wire::{WirePhase, WireSnapshot, WireState};
use paddock_jobs::{CancelReason, Unit};
use paddock_limits::{LimitExceeded, LimitId, Quantity, Scope};

fn snapshot(state: WireState) -> WireSnapshot {
    WireSnapshot {
        id: 7,
        kind: "k".into(),
        title: "T".into(),
        subject: Some("s".into()),
        state,
        seq: 2,
        elapsed_ms: 300,
        eta_ms: Some(9),
        cancellable: true,
        not_cancellable_reason: None,
    }
}

fn bytes(state: WireState) -> Vec<u8> {
    postcard::to_stdvec(&Message::Job(Box::new(snapshot(state)))).unwrap()
}

#[test]
fn the_protocol_version_is_8_since_health_lists_what_it_did_not_check() {
    assert_eq!(
        PROTOCOL_VERSION, 8,
        "WireState::Failed became {{ message, limit }} in 7; HealthReport gained not_checked in 8"
    );
}

#[test]
fn a_job_snapshot_has_these_postcard_bytes() {
    let limit = LimitExceeded::new(LimitId::new("archive.expansion"), Quantity::Count(3), Scope::File)
        .reaching(Quantity::Count(4))
        .unwrap();
    let phase = WirePhase {
        id: "p".into(),
        label: "L".into(),
        unit: Unit::Bytes,
        total: Some(10),
        done: 5,
    };
    // The head is the same for every state: Message::Job (8), id 7, "k", "T", Some("s"). The state follows (its tag, then its fields),
    // then seq 2, elapsed 300 ms (a two-byte varint), eta Some(9), cancellable, no reason.
    const HEAD: [u8; 9] = [0x08, 0x07, 0x01, 0x6b, 0x01, 0x54, 0x01, 0x01, 0x73];
    const TAIL: [u8; 7] = [0x02, 0xac, 0x02, 0x01, 0x09, 0x01, 0x00];
    let cases: [(&str, WireState, Vec<u8>); 6] = [
        ("queued", WireState::Queued, vec![0x00]),
        (
            "running",
            WireState::Running(Some(phase)),
            vec![0x01, 0x01, 0x01, 0x70, 0x01, 0x4c, 0x01, 0x01, 0x0a, 0x05],
        ),
        ("done", WireState::Done, vec![0x02]),
        (
            "failed without a limit",
            WireState::Failed {
                message: "no".into(),
                limit: None,
            },
            vec![0x03, 0x02, 0x6e, 0x6f, 0x00],
        ),
        (
            "failed with a limit",
            WireState::Failed {
                message: "no".into(),
                limit: Some(limit),
            },
            [
                &[0x03, 0x02, 0x6e, 0x6f, 0x01, 0x11][..],
                b"archive.expansion",
                &[0x01, 0x03, 0x01, 0x01, 0x04, 0x00],
            ]
            .concat(),
        ),
        ("cancelled", WireState::Cancelled(CancelReason::User), vec![0x04, 0x00]),
    ];
    for (name, state, middle) in cases {
        let expected = [&HEAD[..], &middle, &TAIL[..]].concat();
        assert_eq!(bytes(state), expected, "{name}");
    }
}
