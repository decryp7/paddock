use paddock_jobs::*;
use std::io;
use std::time::Duration;

#[test]
fn state_machine_allows_the_documented_path_only() {
    let running = JobState::Queued
        .transition(JobState::Running { phase: None })
        .unwrap();
    let phase = JobState::Running {
        phase: Some(PhaseProgress {
            phase: Phase::items("a", "A", 3),
            done: 1,
        }),
    };
    let running = running.transition(phase).unwrap();
    assert!(!running.is_terminal());
    for end in [
        JobState::Done,
        JobState::Failed {
            message: "x".into(),
            limit: None,
        },
        JobState::Cancelled {
            reason: CancelReason::User,
        },
    ] {
        assert!(
            running
                .clone()
                .transition(end.clone())
                .unwrap()
                .is_terminal()
        );
        let e = end
            .clone()
            .transition(JobState::Running { phase: None })
            .unwrap_err();
        assert!(e.to_string().contains("cannot go from"));
    }
    assert!(
        JobState::Queued
            .transition(JobState::Cancelled {
                reason: CancelReason::User
            })
            .is_ok()
    );
    assert!(JobState::Queued.transition(JobState::Done).is_err());
    assert!(
        JobState::Queued
            .transition(JobState::Failed {
                message: String::new(),
                limit: None,
            })
            .is_err()
    );
}

#[test]
fn fraction_is_none_without_total_and_clamped() {
    let mut p = PhaseProgress {
        phase: Phase::discovering("walk", "Finding files"),
        done: 5,
    };
    assert_eq!(p.fraction(), None);
    p.phase.total = Some(0);
    assert_eq!(p.fraction(), None);
    p.phase.total = Some(10);
    assert_eq!(p.fraction(), Some(0.5));
    p.done = 99;
    assert_eq!(p.fraction(), Some(1.0));
    assert_eq!(Phase::bytes("d", "Downloading", Some(9)).unit, Unit::Bytes);
}

#[test]
fn snapshot_json_is_camel_case_and_round_trips() {
    let snap = JobSnapshot {
        id: JobId(7),
        kind: JobKind::new("import"),
        title: "Import".into(),
        subject: Some("3 folders".into()),
        state: JobState::Running {
            phase: Some(PhaseProgress {
                phase: Phase::bytes("download", "Downloading", None),
                done: 42,
            }),
        },
        seq: 3,
        elapsed_ms: 1500,
        eta_ms: None,
        cancellable: false,
        not_cancellable_reason: Some("Saving".into()),
    };
    let json = serde_json::to_value(&snap).unwrap();
    assert_eq!(json["id"], 7);
    assert_eq!(json["state"], "running");
    assert_eq!(json["phase"]["id"], "download");
    assert_eq!(json["phase"]["unit"], "bytes");
    let text = serde_json::to_string(&snap).unwrap();
    assert!(text.contains("\"elapsedMs\":1500") && text.contains("\"notCancellableReason\""));
    let back: JobSnapshot = serde_json::from_str(&text).unwrap();
    assert_eq!(back, snap);
    for st in [
        JobState::Queued,
        JobState::Done,
        JobState::Failed {
            message: "m".into(),
            limit: None,
        },
        JobState::Cancelled {
            reason: CancelReason::Deadline,
        },
    ] {
        let s = JobSnapshot {
            state: st,
            ..snap.clone()
        };
        assert_eq!(
            serde_json::from_str::<JobSnapshot>(&serde_json::to_string(&s).unwrap()).unwrap(),
            s
        );
    }
    assert_eq!(JobId(7).to_string(), "job-7");
}

#[test]
fn cancel_token_first_reason_wins_and_shares_state() {
    let t = CancelToken::new();
    assert!(!t.is_cancelled() && t.check().is_ok() && t.reason().is_none());
    let c = t.clone();
    c.cancel_with(CancelReason::Deadline);
    t.cancel();
    assert_eq!(t.reason(), Some(CancelReason::Deadline));
    assert_eq!(t.check().unwrap_err(), Cancelled(CancelReason::Deadline));
    assert!(
        Cancelled(CancelReason::User)
            .to_string()
            .contains("request")
    );
    assert!(CancelReason::Deadline.to_string().contains("time limit"));
}

#[test]
fn cancel_token_deadline_fires() {
    let t = CancelToken::with_deadline(Duration::ZERO);
    assert_eq!(t.reason(), Some(CancelReason::Deadline));
    let far = CancelToken::with_deadline(Duration::from_secs(3600));
    assert!(!far.is_cancelled());
    far.cancel();
    assert_eq!(far.reason(), Some(CancelReason::User));
    // An absurd limit cannot overflow.
    assert!(!CancelToken::with_deadline(Duration::MAX).is_cancelled());
}

#[test]
fn job_error_display_and_conversions() {
    assert!(
        JobError::from(Cancelled(CancelReason::User))
            .to_string()
            .contains("cancelled")
    );
    assert_eq!(
        JobError::Failed("disk full".into()).to_string(),
        "disk full"
    );
    assert!(
        JobError::Panicked("boom".into())
            .to_string()
            .contains("boom")
    );
    let e: JobError = io::Error::other("nope").into();
    assert_eq!(e.to_string(), "nope");
}

#[test]
fn workers_are_bounded() {
    assert_eq!(Workers::new(0).get(), 1);
    assert_eq!(Workers::new(10_000).get(), Workers::MAX);
    assert_eq!(Workers::one().get(), 1);
    assert!((1..=16).contains(&Workers::auto().get()));
    assert_eq!(Workers::default().get(), Workers::auto().get());
}

#[test]
fn clocks() {
    let c = FakeClock::new();
    assert_eq!(c.now_ms(), 0);
    c.advance_ms(5);
    assert_eq!(c.clone().now_ms(), 5);
    let s = SystemClock;
    let a = s.now_ms();
    assert!(s.now_ms() >= a);
}

#[test]
fn panic_payloads_of_any_type_are_described() {
    let reg = JobRegistry::new(std::sync::Arc::new(NullSink));
    let msg = String::from("formatted");
    let a = reg.register("x", "X", None).run::<()>(|_| panic!("{msg}"));
    assert!(matches!(a, Err(JobError::Panicked(m)) if m == "formatted"));
    let b = reg
        .register("x", "X", None)
        .run::<()>(|_| std::panic::panic_any(42u8));
    assert!(matches!(b, Err(JobError::Panicked(m)) if m == "unknown panic"));
}

struct NullSink;
impl EventSink for NullSink {
    fn emit(&self, _: &JobSnapshot) {}
}

#[test]
fn reporter_new_uses_the_system_clock() {
    let r = Reporter::new(
        JobId(3),
        JobKind::new("k"),
        "T",
        None,
        std::sync::Arc::new(NullSink),
    );
    assert_eq!(r.snapshot().state, JobState::Queued);
}

#[test]
fn ids_start_where_the_app_says() {
    let reg = JobRegistry::new(std::sync::Arc::new(NullSink));
    reg.start_ids_at(1000);
    assert_eq!(reg.register("x", "X", None).id(), JobId(1000));
    assert_eq!(reg.register("x", "X", None).id(), JobId(1001));
}
