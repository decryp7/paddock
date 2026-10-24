//! The extraction as a paddock-jobs job: Jobs-list snapshots, progress on the job, Cancel.

mod common;

use common::*;
use paddock_archive::{Options, extract_job};
use paddock_jobs::{CancelReason, EventSink, JobError, JobRegistry, JobSnapshot, JobState};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Rec(Mutex<Vec<JobSnapshot>>);

impl EventSink for Rec {
    fn emit(&self, s: &JobSnapshot) {
        self.0.lock().unwrap().push(s.clone());
    }
}

fn registry() -> (JobRegistry, Arc<Rec>) {
    let rec = Arc::new(Rec::default());
    (JobRegistry::new(rec.clone()), rec)
}

#[test]
fn a_job_unpacks_reports_a_bytes_phase_and_ends_done() {
    let d = tempfile::tempdir().unwrap();
    let a = write(
        d.path(),
        "a.tar.gz",
        &gz(&tar_bytes(&[E::File("x", &vec![1u8; 100_000])])),
    );
    let (reg, rec) = registry();
    let ctx = reg.register("unpack", "Unpacking the runtime", Some("llama"));
    let report = ctx
        .run(|c| extract_job(c, &a, &d.path().join("o"), &Options::default()))
        .unwrap();
    assert_eq!(report.files, ["x"]);
    let snaps = rec.0.lock().unwrap();
    let last = snaps.last().unwrap();
    assert!(matches!(last.state, JobState::Done));
    assert!(
        snaps.iter().any(
            |s| matches!(&s.state, JobState::Running { phase: Some(p) } if p.phase.id == "unpack")
        ),
        "a phase called unpack was shown"
    );
}

#[test]
fn cancel_ends_the_job_cancelled_and_leaves_nothing() {
    let d = tempfile::tempdir().unwrap();
    let a = write(d.path(), "a.zip", &zip_bytes(&[E::File("x", b"1")]));
    let (reg, rec) = registry();
    let ctx = reg.register("unpack", "Unpacking", None);
    ctx.cancel_token().cancel();
    let r = ctx.run(|c| extract_job(c, &a, &d.path().join("o"), &Options::default()));
    assert!(matches!(r, Err(JobError::Cancelled(CancelReason::User))));
    assert!(matches!(
        rec.0.lock().unwrap().last().unwrap().state,
        JobState::Cancelled { .. }
    ));
    assert!(!d.path().join("o").exists());
    assert_eq!(others(d.path(), &["a.zip"]), Vec::<String>::new());
}

#[test]
fn a_bad_archive_fails_the_job_with_a_message_that_says_what_to_do() {
    let d = tempfile::tempdir().unwrap();
    let a = write(d.path(), "a.zip", b"not an archive at all");
    let (reg, rec) = registry();
    let ctx = reg.register("unpack", "Unpacking", None);
    let r = ctx.run(|c| extract_job(c, &a, &d.path().join("o"), &Options::default()));
    assert!(matches!(r, Err(JobError::Failed(ref m)) if m.contains("Download it again")));
    assert!(matches!(
        rec.0.lock().unwrap().last().unwrap().state,
        JobState::Failed { .. }
    ));
}
