// ci-runs-reader: test-cache trust: is the run a cached pass rests on a main or train run whose job ended success (not a verdict on a commit)
//! The reader side of trust (ADR 0116 §7), independent of who wrote a record. Before a record is counted, the run it names is asked of
//! the Gitea API (through [`crate::ci_runs::client`], the one run reader): the run's ref must be `main` or a `train/*` branch and the
//! job it names (or, when it names none, the whole run) must have ended `success`. One call per distinct run, remembered for the plan job. A record that fails is ignored
//! and reported by the caller. A record of a local run (a non-numeric run id) is never trusted in CI.
//!
//! Not checked here, and said so: the third check of the ADR (the target's key recomputed at the run's head commit equals the
//! record's key) is not built; the key is content-addressed and the writer guard keeps non-trusted refs from writing, so this check
//! closes the forged-record gap only for the run and the job, not for a record whose key was made up.

use super::store::Record;
use crate::ci_runs::client::{Client, RunInfo, TimedJob};
use crate::job_outcome::JobOutcome;
use std::cell::RefCell;
use std::collections::HashMap;

/// What one run is, as far as the check needs it: the ref and the jobs' outcomes (job id, passed).
#[derive(Debug, Clone, PartialEq)]
pub struct RunFacts {
    /// The ref the workflow ran on (`refs/heads/main`).
    pub git_ref: String,
    /// Each job of the run and whether it ended `success`.
    pub jobs: Vec<(u64, bool)>,
    /// The run's own conclusion was `success`.
    pub success: bool,
}

/// The facts of a run from the API: `Ok(None)` when the server has no such run.
pub fn facts(client: &mut Client, id: u64) -> Result<Option<RunFacts>, String> {
    let Some(run): Option<RunInfo> = client.run(id)? else {
        return Ok(None);
    };
    let jobs: Vec<TimedJob> = client.timed_jobs(id)?.unwrap_or_default();
    Ok(Some(RunFacts {
        git_ref: if run.git_ref().is_empty() {
            format!("refs/heads/{}", run.head_branch)
        } else {
            run.git_ref().to_string()
        },
        success: run.conclusion == "success",
        jobs: jobs.iter().map(|j| (j.id, j.outcome == JobOutcome::Passed)).collect(),
    }))
}

/// How a run's facts are fetched; `Ok(None)` is a run the server does not have.
type Fetch<'a> = Box<dyn FnMut(u64) -> Result<Option<RunFacts>, String> + 'a>;

/// A reader-side check with its per-run memory. `fetch` asks for a run's facts; an error or an unknown run refuses the record.
pub struct Provenance<'a> {
    fetch: RefCell<Fetch<'a>>,
    seen: RefCell<HashMap<u64, Result<Option<RunFacts>, String>>>,
}

impl<'a> Provenance<'a> {
    /// A check over `fetch`.
    pub fn new(fetch: impl FnMut(u64) -> Result<Option<RunFacts>, String> + 'a) -> Provenance<'a> {
        Provenance {
            fetch: RefCell::new(Box::new(fetch)),
            seen: RefCell::new(HashMap::new()),
        }
    }

    /// `Ok` when the record rests on a successful job of a main or train run.
    pub fn check(&self, r: &Record) -> Result<(), String> {
        let run: u64 = r
            .run
            .parse()
            .map_err(|_| format!("run `{}` is not a CI run id (a local run's record)", r.run))?;
        let facts = self
            .seen
            .borrow_mut()
            .entry(run)
            .or_insert_with(|| (self.fetch.borrow_mut())(run))
            .clone();
        let facts = facts
            .map_err(|e| format!("run {run} could not be read: {e}"))?
            .ok_or_else(|| format!("run {run} does not exist"))?;
        if !(facts.git_ref == "refs/heads/main" || facts.git_ref.starts_with("refs/heads/train/")) {
            return Err(format!("run {run} is on `{}`, not main or a train", facts.git_ref));
        }
        // The Actions environment does not name the numeric job id, so a record written from a job usually has none: it then rests on the
        // whole run having ended `success` (stricter, never looser). A record that names a job rests on that job.
        let Ok(job) = r.job.parse::<u64>() else {
            return if facts.success {
                Ok(())
            } else {
                Err(format!("run {run} did not end success, and the record names no job"))
            };
        };
        match facts.jobs.iter().find(|(id, _)| *id == job) {
            Some((_, true)) => Ok(()),
            Some((_, false)) => Err(format!("job {job} of run {run} did not end success")),
            None => Err(format!("run {run} has no job {job}")),
        }
    }
}
