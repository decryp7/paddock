// ci-runs-reader: test-cache trust: is the run a cached pass rests on a main or train run whose job ended success (not a verdict on a commit)
//! The reader side of trust (ADR 0116 §7, amended by user decision 97), independent of who wrote a record. Before a record is counted, the
//! run it names is asked of the Gitea API (through [`crate::ci_runs::client`], the one run reader): the run's ref must be `main` or a
//! `train/*` branch, and the record's own job (its display name, in the attempt it ran in) must have ended `success`. The run's other jobs
//! do not matter: a red or flaky job of the run never takes the passing jobs' records with it. Fail closed: a record that names no job or
//! no attempt, a job the attempt does not have, or two jobs of one name is untrusted. One call per distinct run attempt, remembered for the
//! plan job. A record that fails is ignored and reported by the caller. A record of a local run (a non-numeric run id) is never trusted
//! in CI.
//!
//! Not checked here, and said so: the third check of the ADR (the target's key recomputed at the run's head commit equals the
//! record's key) is not built; the key is content-addressed and the writer guard keeps non-trusted refs from writing, so this check
//! closes the forged-record gap only for the run and the job, not for a record whose key was made up.

use super::store::Record;
use crate::ci_runs::client::{Client, RunInfo, TimedJob};
use crate::job_outcome::JobOutcome;
use std::cell::RefCell;
use std::collections::HashMap;

/// What one run attempt is, as far as the check needs it: the ref and the jobs' outcomes.
#[derive(Debug, Clone, PartialEq)]
pub struct RunFacts {
    /// The ref the workflow ran on (`refs/heads/main`).
    pub git_ref: String,
    /// Each job of the attempt: its display name and whether it ended `success`.
    pub jobs: Vec<(String, bool)>,
}

/// The facts of attempt `attempt` of a run from the API: `Ok(None)` when the server has no such run. The run's last attempt is read
/// through the job list, an earlier one through its own attempt list.
pub fn facts(client: &mut Client, id: u64, attempt: u64) -> Result<Option<RunFacts>, String> {
    let Some(run): Option<RunInfo> = client.run(id)? else {
        return Ok(None);
    };
    let jobs: Option<Vec<TimedJob>> = if attempt == run.run_attempt {
        client.timed_jobs(id)?
    } else {
        client.attempt_jobs(id, Some(attempt))?
    };
    let jobs = jobs.ok_or_else(|| format!("run {id} has no job list for attempt {attempt}"))?;
    Ok(Some(RunFacts {
        git_ref: if run.git_ref().is_empty() {
            format!("refs/heads/{}", run.head_branch)
        } else {
            run.git_ref().to_string()
        },
        jobs: jobs.iter().map(|j| (j.name.clone(), j.outcome == JobOutcome::Passed)).collect(),
    }))
}

/// How a run attempt's facts are fetched (run, attempt); `Ok(None)` is a run the server does not have.
type Fetch<'a> = Box<dyn FnMut(u64, u64) -> Result<Option<RunFacts>, String> + 'a>;

/// A reader-side check with its per-run-attempt memory. `fetch` asks for facts; an error or an unknown run refuses the record.
pub struct Provenance<'a> {
    fetch: RefCell<Fetch<'a>>,
    seen: RefCell<HashMap<(u64, u64), Result<Option<RunFacts>, String>>>,
}

impl<'a> Provenance<'a> {
    /// A check over `fetch`.
    pub fn new(fetch: impl FnMut(u64, u64) -> Result<Option<RunFacts>, String> + 'a) -> Provenance<'a> {
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
        if r.job.is_empty() || r.attempt == 0 {
            return Err(format!(
                "the record of run {run} names no job or no attempt, so no job's result can vouch for it"
            ));
        }
        let facts = self
            .seen
            .borrow_mut()
            .entry((run, r.attempt))
            .or_insert_with(|| (self.fetch.borrow_mut())(run, r.attempt))
            .clone();
        let facts = facts
            .map_err(|e| format!("run {run} could not be read: {e}"))?
            .ok_or_else(|| format!("run {run} does not exist"))?;
        if !(facts.git_ref == "refs/heads/main" || facts.git_ref.starts_with("refs/heads/train/")) {
            return Err(format!("run {run} is on `{}`, not main or a train", facts.git_ref));
        }
        let mut named = facts.jobs.iter().filter(|(name, _)| *name == r.job);
        match (named.next(), named.next()) {
            (Some((_, true)), None) => Ok(()),
            (Some((_, false)), None) => Err(format!("job `{}` of run {run} attempt {} did not end success", r.job, r.attempt)),
            (None, _) => Err(format!("run {run} attempt {} has no job `{}`", r.attempt, r.job)),
            (Some(_), Some(_)) => Err(format!("run {run} attempt {} has several jobs named `{}`", r.attempt, r.job)),
        }
    }
}
