//! The job reporter as the index's progress port.

use crate::support::Sink;
use pacenotes_engine::JobProgress;
use pacenotes_index::Progress;
use paddock_jobs::{JobRegistry, JobState};
use std::sync::Arc;

#[test]
fn phases_are_labelled_a_repeated_phase_does_not_restart_and_save_cannot_be_cancelled() {
    let sink = Arc::new(Sink::default());
    let registry = JobRegistry::with_clock(sink.clone(), Arc::new(paddock_jobs::FakeClock::new()), 0);
    let ctx = registry.register("t", "Test", None);
    let _ = ctx.run(|ctx| -> Result<(), paddock_jobs::JobError> {
        let progress = JobProgress::new(ctx);
        progress.phase("walk", "Finding files", None);
        progress.add(3);
        progress.phase("walk", "Finding files", None);
        progress.set_total(10);
        let snap = ctx.reporter().snapshot();
        let JobState::Running { phase: Some(p) } = &snap.state else {
            panic!("{snap:?}")
        };
        assert_eq!(
            (p.phase.id.as_str(), p.done, p.phase.total),
            ("walk", 3, Some(10)),
            "a repeated phase keeps its count"
        );
        assert!(snap.cancellable);

        progress.phase("read", "Reading files", Some(5));
        progress.phase("save", "Saving the index", Some(1));
        let snap = ctx.reporter().snapshot();
        assert!(
            !snap.cancellable && snap.not_cancellable_reason.is_some(),
            "the swap cannot be interrupted"
        );
        progress.phase("save", "Saving the index", Some(2));
        assert!(!ctx.reporter().snapshot().cancellable, "repeating save keeps the guard");
        progress.phase("done", "Done", Some(0));
        assert!(ctx.reporter().snapshot().cancellable, "the next phase lifts it");
        Ok(())
    });
    let ids: Vec<String> = crate::support::phase_ids(&sink.all());
    assert_eq!(ids, ["walk", "read", "save", "done"]);
}

#[test]
fn a_count_that_passes_its_total_raises_the_total_and_each_phase_counts_from_zero() {
    let sink = Arc::new(Sink::default());
    let registry = JobRegistry::with_clock(sink, Arc::new(paddock_jobs::FakeClock::new()), 0);
    let ctx = registry.register("t", "Test", None);
    let _ = ctx.run(|ctx| -> Result<(), paddock_jobs::JobError> {
        let progress = JobProgress::new(ctx);
        let shown = || match ctx.reporter().snapshot().state {
            JobState::Running { phase: Some(p) } => (p.done, p.phase.total),
            other => panic!("{other:?}"),
        };
        progress.phase("scan", "Scanning the folder", None);
        progress.add(4);
        progress.set_total(3);
        assert_eq!(shown(), (4, Some(4)), "a total below the count is raised to it");
        progress.add(5);
        assert_eq!(shown(), (9, Some(9)));
        progress.phase("walk", "Finding files", Some(2));
        progress.add(3);
        assert_eq!(shown(), (3, Some(3)), "a new phase starts at zero, and a known total is raised too");
        progress.phase("walk", "Finding files", Some(1));
        assert_eq!(shown(), (3, Some(3)), "a repeat never lowers it");
        Ok(())
    });
}
