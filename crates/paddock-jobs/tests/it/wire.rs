//! The wire form of a snapshot (plan S9, ADR 0012 §4): it round-trips through postcard, which the UI's JSON shape cannot, and
//! converts to the UI's shape and back without loss.

use paddock_jobs::wire::{WirePhase, WireSnapshot, WireState};
use paddock_jobs::{
    CancelReason, JobId, JobKind, JobSnapshot, JobState, Phase, PhaseProgress, Unit,
};

fn snapshot(state: JobState) -> JobSnapshot {
    JobSnapshot {
        id: JobId(4),
        kind: JobKind::new("find-files"),
        title: "Finding files".into(),
        subject: Some("/x".into()),
        state,
        seq: 9,
        elapsed_ms: 1500,
        eta_ms: Some(300),
        cancellable: false,
        not_cancellable_reason: Some("reading".into()),
    }
}

fn every_state() -> Vec<JobState> {
    vec![
        JobState::Queued,
        JobState::Running { phase: None },
        JobState::Running {
            phase: Some(PhaseProgress {
                phase: Phase::bytes("hash", "Reading the file", Some(10)),
                done: 4,
            }),
        },
        JobState::Running {
            phase: Some(PhaseProgress {
                phase: Phase::discovering("walk", "Listing files"),
                done: 0,
            }),
        },
        JobState::Done,
        JobState::Failed {
            message: "no".into(),
        },
        JobState::Cancelled {
            reason: CancelReason::User,
        },
        JobState::Cancelled {
            reason: CancelReason::Deadline,
        },
    ]
}

#[test]
fn every_state_survives_postcard_and_the_conversion_both_ways() {
    for state in every_state() {
        let original = snapshot(state);
        let wire = WireSnapshot::from(&original);
        let bytes = postcard::to_stdvec(&wire).unwrap();
        let back: WireSnapshot = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(back, wire);
        assert_eq!(JobSnapshot::from(back), original);
    }
}

#[test]
fn only_an_ended_job_is_terminal() {
    let terminal: Vec<bool> = every_state()
        .into_iter()
        .map(|s| WireSnapshot::from(&snapshot(s)).is_terminal())
        .collect();
    assert_eq!(
        terminal,
        [false, false, false, false, true, true, true, true]
    );
    let w = WireSnapshot::from(&snapshot(JobState::Running { phase: None }));
    assert!(matches!(w.state, WireState::Running(None)));
    let p = WirePhase {
        id: "a".into(),
        label: "A".into(),
        unit: Unit::Items,
        total: None,
        done: 1,
    };
    assert_eq!(p.done, 1);
}
