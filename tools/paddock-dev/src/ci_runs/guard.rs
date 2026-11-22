//! `ci dispatch --guard`: the dispatch side of the trial push profile (docs/adr/0114-trial-push-profile.md).
//!
//! Before the dispatch: the ref's head commit is read from the server and its slow-rows record from the push gate's state folder
//! ([`crate::trial_guard`]). A record that already says failed, not measured or never finished refuses the dispatch (nothing is
//! sent, so nothing needs cancelling); a missing record dispatches unguarded with a warning (CI is still the full evidence a train
//! needs). After the dispatch: the new run's id is found by difference against the runs listed before ([`find_new_run`]), written
//! into the dispatch record, and settled at once, because the slow rows may have failed while the dispatch waited for a runner.

use super::client::Client;
use super::dispatch::{Request, Target};
use crate::pre_push::trial_profile::{is_trial_ref, settle_with};
use crate::trial_guard::{self, Dispatched, Time, Verdict, find_new_run, judge, read_slow};
use crate::wait::{Clock, Http};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What the guard found before the dispatch.
#[derive(Debug)]
pub enum Before {
    /// Dispatch, and register the run afterwards.
    Guarded(Guard),
    /// Dispatch without a guard; the line says why (printed as a warning).
    Unguarded(String),
    /// Do not dispatch: the line is the `::error::` (the slow rows already failed, were not measured or never finished).
    Refuse(String),
}

/// A guarded dispatch in flight: the commit and the runs that existed before it.
#[derive(Debug)]
pub struct Guard {
    state: PathBuf,
    sha: String,
    before: Vec<u64>,
}

/// The branch name of a `--ref` that the profile covers; `Err` for any other ref (`train/*`, `main`, a tag, a feature branch keep the
/// full gate and need no guard).
pub fn guarded_branch(git_ref: &str) -> Result<String, String> {
    let branch = git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref);
    if is_trial_ref(&format!("refs/heads/{branch}")) {
        Ok(branch.to_string())
    } else {
        Err(format!(
            "--guard is for refs under wip/ and trial/ (the trial push profile); {git_ref} keeps the full push gate and needs no guard"
        ))
    }
}

/// Reads the head of `git_ref` and the record of its slow rows, and lists the runs that exist now.
pub fn before(http: &mut dyn Http, target: &Target<'_>, state: &Path, git_ref: &str, time: &Time<'_>) -> Result<Before, String> {
    let Target { server, repo, token } = *target;
    let branch = guarded_branch(git_ref)?;
    let mut client = Client::new(http, server, repo, token);
    client.backoff_ms = 0;
    let sha = client
        .branch_head(&branch)?
        .ok_or_else(|| format!("{branch} is not on {server}: push it first (`git push origin HEAD:refs/heads/{branch}`)"))?;
    let Some(slow) = read_slow(state, &sha)? else {
        return Ok(Before::Unguarded(format!(
            "no slow-rows record for {} in {}: the commit was not pushed from this checkout with the trial profile, so nothing cancels this run; it is still the full CI run",
            &sha[..sha.len().min(12)],
            trial_guard::dir(state).display()
        )));
    };
    if let Verdict::Cancel(why) = judge(&slow, time.now, time.alive) {
        return Ok(Before::Refuse(why));
    }
    let ids = client
        .runs_page_where(1, "event=workflow_dispatch")?
        .runs
        .iter()
        .map(|r| r.id)
        .collect();
    Ok(Before::Guarded(Guard {
        state: state.to_path_buf(),
        sha,
        before: ids,
    }))
}

/// After a successful dispatch: finds the new run, records it and settles it against the slow rows. The line printed says what
/// happened; a run that cannot be found is said loudly (it will not be cancelled by the background rows).
pub fn register(
    g: &Guard,
    http: &mut dyn Http,
    target: &Target<'_>,
    req: &Request,
    clock: &mut dyn Clock,
    time: &Time<'_>,
    log: &mut dyn Write,
) -> String {
    let Target { server, repo, token } = *target;
    let (workflow, git_ref) = (req.workflow.as_str(), req.git_ref.as_str());
    let mut found = None;
    for look in 0..8 {
        if look > 0 {
            clock.sleep(Duration::from_secs(4));
        }
        let listed = {
            let mut client = Client::new(http, server, repo, token);
            client.backoff_ms = 0;
            client.runs_page_where(1, "event=workflow_dispatch")
        };
        match listed.map(|p| find_new_run(&p.runs, &g.before, workflow, git_ref, &g.sha)) {
            Ok(Ok(Some(id))) => {
                found = Some(id);
                break;
            }
            Ok(Ok(None)) => {}
            Ok(Err(e)) => {
                return format!(
                    "::warning::guard: {e}. The run is NOT guarded: cancel it by hand if the slow rows fail (their record is in {})",
                    trial_guard::dir(&g.state).display()
                );
            }
            Err(e) => {
                let _ = writeln!(log, "guard: the run list could not be read ({e}); looking again");
            }
        }
    }
    let Some(run_id) = found else {
        return format!(
            "::warning::guard: the dispatched run of {workflow} on {git_ref} did not show in the run list within 30 s; it is NOT guarded. Next: find it with `./dev ci verdict`/the Actions page and cancel it by hand if the slow rows fail"
        );
    };
    let rec = Dispatched {
        sha: g.sha.clone(),
        run_id,
        workflow: workflow.to_string(),
        git_ref: git_ref.to_string(),
        server: server.to_string(),
        repo: repo.to_string(),
    };
    if let Err(e) = trial_guard::write_dispatched(&g.state, &rec) {
        return format!(
            "::warning::guard: run {run_id} could not be recorded ({e}); it is NOT guarded. Next: cancel it by hand if the slow rows fail"
        );
    }
    format!(
        "guard: run {run_id} is registered. {}",
        settle_with(&g.state, &g.sha, http, token, time.now, time.alive)
    )
}

#[cfg(test)]
mod tests;
