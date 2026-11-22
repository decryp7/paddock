// ci-runs-reader: trial push profile: registers and cancels one dispatched run; no pass/fail verdict on a commit
//! The trial push profile's records and its cancel (ER-9 overlap, ER-33, ER-51; docs/adr/0114-the-trial-push-profile-starts-ci-before-the-slow-local-rows-finish.md).
//!
//! A push of a `wip/` or `trial/` ref runs only the fast static rows before the push ([`crate::pre_push::trial_profile`]) and
//! starts the slow rows in a background process that outlives the hook. CI for that ref is dispatched at the same time
//! (`./dev ci dispatch --guard`). The two sides meet here, through two files per commit in `<state>/trial/`:
//!
//! - `<sha>.slow.json`: the slow rows' record: running (with the process's pid and start time, the handle its starter holds),
//!   passed, failed or not measured, with the row and the next step.
//! - `<sha>.dispatch.json`: the CI run dispatched from the ref (its id comes from the dispatch tool, never found later by name).
//!
//! Whoever writes second calls [`settle`]: it reads both and cancels the run when the slow rows failed, were not measured, or
//! never finished. Each side writes its own file first and reads the other's afterwards, so at least one of them sees both and
//! no lock is needed; a cancel is idempotent (a finished run is left alone).
//!
//! Never green by skipping: [`judge`] has no "fine" for a record that is still running past its deadline or whose process is
//! gone. A trial whose slow rows never finished is not evidence, and the run is cancelled.

use crate::ci_runs::client::{Client, RunInfo};
use crate::wait::Http;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The folder of the records inside the push gate's state folder.
pub fn dir(state: &Path) -> PathBuf {
    state.join("trial")
}

/// How the slow rows of one trial push stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Started and not finished.
    Running,
    /// Every slow row passed.
    Passed,
    /// A slow row ran and failed (or its check is broken).
    Failed,
    /// A slow row could not look, or the slow rows could not be started: nothing is proven.
    NotMeasured,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            State::Running => "running",
            State::Passed => "passed",
            State::Failed => "failed",
            State::NotMeasured => "not-measured",
        }
    }
    fn parse(s: &str) -> Option<State> {
        [State::Running, State::Passed, State::Failed, State::NotMeasured]
            .into_iter()
            .find(|k| k.name() == s)
    }
}

/// The record of a trial push's slow rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slow {
    /// The pushed commit.
    pub sha: String,
    /// How the rows stand.
    pub state: State,
    /// The first row that failed or was not measured (empty otherwise).
    pub row: String,
    /// Why, in one line (empty otherwise).
    pub why: String,
    /// What to do (empty otherwise).
    pub next: String,
    /// The background process holding the rows (0: not known yet) and its start time, its identity (rule 53).
    pub pid: u32,
    /// Start time of that process in microseconds (0: not known).
    pub start_micros: u64,
    /// Unix seconds after which a record that is still running counts as never finished.
    pub finish_by: u64,
    /// The folder of the rows' logs.
    pub logs: String,
}

impl Slow {
    /// The record as JSON.
    pub fn to_json(&self) -> Value {
        json!({"sha": self.sha, "state": self.state.name(), "row": self.row, "why": self.why, "next": self.next,
               "pid": self.pid, "start_micros": self.start_micros, "finish_by": self.finish_by, "logs": self.logs})
    }

    /// Reads a record; `None` when it is not one.
    pub fn from_json(v: &Value) -> Option<Slow> {
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        Some(Slow {
            sha: s("sha"),
            state: State::parse(v["state"].as_str()?)?,
            row: s("row"),
            why: s("why"),
            next: s("next"),
            pid: v["pid"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
            start_micros: v["start_micros"].as_u64().unwrap_or(0),
            finish_by: v["finish_by"].as_u64().unwrap_or(0),
            logs: s("logs"),
        })
        .filter(|r| valid_sha(&r.sha))
    }
}

/// A commit id as the records are named: 7 to 64 hex digits (it becomes a file name).
pub fn valid_sha(sha: &str) -> bool {
    (7..=64).contains(&sha.len()) && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

fn path(state: &Path, sha: &str, what: &str) -> Result<PathBuf, String> {
    if !valid_sha(sha) {
        return Err(format!("`{sha}` is not a commit id"));
    }
    Ok(dir(state).join(format!("{sha}.{what}.json")))
}

/// Writes `v` whole beside its place and renames it over: a reader never sees half a record.
fn write_whole(file: &Path, v: &Value) -> Result<(), String> {
    let parent = file.parent().ok_or("a record has a folder")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{} cannot be created: {e}", parent.display()))?;
    let tmp = file.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, format!("{v}\n")).map_err(|e| format!("{} cannot be written: {e}", tmp.display()))?;
    std::fs::rename(&tmp, file).map_err(|e| format!("{} cannot be renamed: {e}", file.display()))
}

fn read_json(file: &Path) -> Result<Option<Value>, String> {
    match std::fs::read_to_string(file) {
        Ok(t) => serde_json::from_str(&t)
            .map(Some)
            .map_err(|e| format!("{} is not JSON: {e}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{} cannot be read: {e}", file.display())),
    }
}

/// Writes the slow record of `rec.sha`.
pub fn write_slow(state: &Path, rec: &Slow) -> Result<(), String> {
    write_whole(&path(state, &rec.sha, "slow")?, &rec.to_json())
}

/// Records the process holding the rows, unless the rows already ended (a finished record is never put back to running).
pub fn note_holder(state: &Path, sha: &str, pid: u32, start_micros: u64) -> Result<(), String> {
    if let Some(mut r) = read_slow(state, sha)?
        && r.state == State::Running
    {
        r.pid = pid;
        r.start_micros = start_micros;
        write_slow(state, &r)?;
    }
    Ok(())
}

/// Reads the slow record of `sha`; `Ok(None)` when none was made.
pub fn read_slow(state: &Path, sha: &str) -> Result<Option<Slow>, String> {
    let file = path(state, sha, "slow")?;
    match read_json(&file)? {
        None => Ok(None),
        Some(v) => Slow::from_json(&v)
            .map(Some)
            .ok_or_else(|| format!("{} is not a slow-rows record", file.display())),
    }
}

/// The CI run dispatched for a trial push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatched {
    /// The pushed commit the run tests.
    pub sha: String,
    /// The run's id.
    pub run_id: u64,
    /// The workflow file.
    pub workflow: String,
    /// The ref it was dispatched on.
    pub git_ref: String,
    /// The Gitea server.
    pub server: String,
    /// The repository (`owner/name`).
    pub repo: String,
}

/// Writes the dispatch record of `d.sha`.
pub fn write_dispatched(state: &Path, d: &Dispatched) -> Result<(), String> {
    write_whole(
        &path(state, &d.sha, "dispatch")?,
        &json!({"sha": d.sha, "run_id": d.run_id, "workflow": d.workflow, "git_ref": d.git_ref, "server": d.server, "repo": d.repo}),
    )
}

/// Reads the dispatch record of `sha`; `Ok(None)` when no run was registered.
pub fn read_dispatched(state: &Path, sha: &str) -> Result<Option<Dispatched>, String> {
    let file = path(state, sha, "dispatch")?;
    let Some(v) = read_json(&file)? else { return Ok(None) };
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    match v["run_id"].as_u64().filter(|n| *n > 0) {
        Some(run_id) => Ok(Some(Dispatched {
            sha: s("sha"),
            run_id,
            workflow: s("workflow"),
            git_ref: s("git_ref"),
            server: s("server"),
            repo: s("repo"),
        })),
        None => Err(format!("{} names no run", file.display())),
    }
}

/// What the slow rows' record says about the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Still running inside its deadline: nothing to do yet.
    Pending,
    /// Every slow row passed.
    Pass,
    /// The run must be cancelled: the line says why and what to do (printed as the `::error::`).
    Cancel(String),
}

/// The verdict of a record at `now` (unix seconds); `alive(pid, start_micros)` says whether the holder may still run (it errs
/// towards "yes", so only a process that is surely gone counts). Pure.
pub fn judge(rec: &Slow, now: u64, alive: &dyn Fn(u32, u64) -> bool) -> Verdict {
    let at = |what: &str| {
        let row = if rec.row.is_empty() {
            String::new()
        } else {
            format!(" ({})", rec.row)
        };
        format!(
            "trial {}: the slow rows {what}{row}: {}. Next: {}",
            &rec.sha[..rec.sha.len().min(12)],
            if rec.why.is_empty() { "no cause was recorded" } else { &rec.why },
            if rec.next.is_empty() {
                "see the logs in the record's folder"
            } else {
                &rec.next
            }
        )
    };
    match rec.state {
        State::Passed => Verdict::Pass,
        State::Failed => Verdict::Cancel(at("failed")),
        State::NotMeasured => Verdict::Cancel(at("were not measured (not evidence)")),
        State::Running if rec.finish_by != 0 && now > rec.finish_by => Verdict::Cancel(format!(
            "trial {}: the slow rows never finished before their deadline; a trial whose slow rows never finished is not evidence. Next: read the logs in {}, then run `./dev check --pre-push` on the commit",
            &rec.sha[..rec.sha.len().min(12)],
            rec.logs
        )),
        State::Running if rec.pid != 0 && !alive(rec.pid, rec.start_micros) => Verdict::Cancel(format!(
            "trial {}: the process running the slow rows (pid {}) is gone and left no result; a trial whose slow rows never finished is not evidence. Next: read the logs in {}, then run `./dev check --pre-push` on the commit",
            &rec.sha[..rec.sha.len().min(12)],
            rec.pid,
            rec.logs
        )),
        State::Running => Verdict::Pending,
    }
}

/// The clock and the process table a verdict is read with: `now` in unix seconds, `alive(pid, start_micros)` whether a holder may
/// still run (it errs towards "yes").
pub struct Time<'a> {
    /// Unix seconds.
    pub now: u64,
    /// Whether the process may still run.
    pub alive: &'a dyn Fn(u32, u64) -> bool,
}

/// What [`settle`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settled {
    /// No slow record: the trial is not guarded.
    Unguarded,
    /// No run is registered (yet): the dispatch side will look when it registers one.
    NoRun(Verdict),
    /// The slow rows are still running; the run stays.
    Waiting,
    /// The slow rows passed; the run stays.
    Fine,
    /// The run was cancelled (or was over already): `why` is the printed line.
    Cancelled {
        /// The run.
        run: u64,
        /// The cause and the next step.
        why: String,
        /// The run had finished before the cancel.
        already_over: bool,
    },
}

/// Cancels run `run` (`POST .../actions/runs/<id>/cancel`); a run that is over, or gone, is left alone. `Ok(true)`: it was already over.
pub fn cancel_run(http: &mut dyn Http, server: &str, repo: &str, token: &str, run: u64) -> Result<bool, String> {
    let mut client = Client::new(http, server, repo, token);
    client.backoff_ms = 0;
    match client.run(run)? {
        None => return Ok(true),
        Some(r) if r.finished() => return Ok(true),
        Some(_) => {}
    }
    let url = format!("{}/api/v1/repos/{repo}/actions/runs/{run}/cancel", server.trim_end_matches('/'));
    http.post_json(&url, token, &json!({}))
        .map(|_| false)
        .map_err(|e| format!("run {run} could not be cancelled: {e}"))
}

/// Reads both records of `sha` and cancels the registered run when the verdict says so. Called by whoever wrote last; idempotent.
pub fn settle(
    state: &Path,
    sha: &str,
    http: &mut dyn Http,
    token: &str,
    now: u64,
    alive: &dyn Fn(u32, u64) -> bool,
) -> Result<Settled, String> {
    let Some(slow) = read_slow(state, sha)? else {
        return Ok(Settled::Unguarded);
    };
    let verdict = judge(&slow, now, alive);
    let Some(d) = read_dispatched(state, sha)? else {
        return Ok(Settled::NoRun(verdict));
    };
    match verdict {
        Verdict::Pending => Ok(Settled::Waiting),
        Verdict::Pass => Ok(Settled::Fine),
        Verdict::Cancel(why) => {
            let already_over = cancel_run(http, &d.server, &d.repo, token, d.run_id)?;
            Ok(Settled::Cancelled {
                run: d.run_id,
                why,
                already_over,
            })
        }
    }
}

/// The one run of `runs` that did not exist before the dispatch (`before`) and is `workflow` on `git_ref` at `sha`: the handle a
/// dispatch gets back is found by difference, never as "the newest run of the branch" (ER-51). `Err` when several qualify (another
/// dispatch of the same ref raced this one), `Ok(None)` when none shows yet.
pub fn find_new_run(runs: &[RunInfo], before: &[u64], workflow: &str, git_ref: &str, sha: &str) -> Result<Option<u64>, String> {
    let ref_full = if git_ref.starts_with("refs/") {
        git_ref.to_string()
    } else {
        format!("refs/heads/{git_ref}")
    };
    let hits: Vec<u64> = runs
        .iter()
        .filter(|r| {
            !before.contains(&r.id)
                && r.workflow() == workflow
                && r.git_ref() == ref_full
                && r.head_sha == sha
                && r.event == "workflow_dispatch"
        })
        .map(|r| r.id)
        .collect();
    match hits[..] {
        [] => Ok(None),
        [one] => Ok(Some(one)),
        _ => Err(format!(
            "runs {} all look like this dispatch ({workflow} on {git_ref} at {sha}); none is registered",
            hits.iter().map(u64::to_string).collect::<Vec<_>>().join(", ")
        )),
    }
}

#[cfg(test)]
pub(crate) mod tests;
