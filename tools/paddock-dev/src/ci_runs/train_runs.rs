// ci-runs-reader: train release: finds the run it dispatched and reads its facts; pass or red is decided only in ci_runs::verdict
//! The runs side of `train release`: finds the run a dispatch started and reads one run's facts, and the triage document. It
//! decides nothing about pass or red (that is [`super::verdict`], the one reader). Gitea's dispatch answers with no run id, so the
//! run is the newest run of that workflow on the train's ref and commit that is newer than the newest one seen before the dispatch.

use super::client::{Client, RunInfo};
use super::verdict::{RunResult, run_result};
use crate::wait::CurlHttp;
use serde_json::Value;
use std::time::Duration;

/// How many times the run list is read after a dispatch, and the pause between reads: 10 minutes in all, the script's bound.
pub const TRIES: u32 = 40;
/// The pause between reads.
pub const PAUSE: Duration = Duration::from_secs(15);

/// The newest run id of `workflow` on `git_ref` at `sha` in `runs`, or 0.
pub fn newest(runs: &[RunInfo], workflow: &str, git_ref: &str, sha: &str) -> u64 {
    let path = format!("{workflow}@refs/heads/{git_ref}");
    runs.iter()
        .filter(|r| r.path == path && r.head_sha == sha)
        .map(|r| r.id)
        .max()
        .unwrap_or(0)
}

/// Reads the list until a run newer than `before` shows, at most `tries` times with `pause` between reads.
pub fn find_new_run(
    list: &mut dyn FnMut() -> Result<Vec<RunInfo>, String>,
    sleep: &mut dyn FnMut(Duration),
    (workflow, git_ref, sha): (&str, &str, &str),
    before: u64,
    tries: u32,
    pause: Duration,
) -> Result<u64, String> {
    for attempt in 0..tries {
        let id = newest(&list()?, workflow, git_ref, sha);
        if id > before {
            return Ok(id);
        }
        if attempt + 1 < tries {
            sleep(pause);
        }
    }
    Err(format!(
        "no new run of {workflow} on {git_ref} at {} showed after {tries} reads ({} s apart)",
        &sha[..sha.len().min(9)],
        pause.as_secs()
    ))
}

/// What the sequence knows of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunFacts {
    /// The workflow file.
    pub workflow: String,
    /// The ref it ran on (`refs/heads/train/x`).
    pub git_ref: String,
    /// The commit it tested.
    pub head_sha: String,
    /// How it ended ([`run_result`]).
    pub result: RunResult,
}

impl RunFacts {
    /// The facts of a listed run.
    pub fn of(r: &RunInfo) -> RunFacts {
        RunFacts {
            workflow: r.workflow().to_string(),
            git_ref: r.git_ref().to_string(),
            head_sha: r.head_sha.clone(),
            result: run_result(r),
        }
    }
}

/// The facts of run `run`; `None` when the server has no such run.
pub fn facts(server: &str, repo: &str, token: &str, run: u64) -> Result<Option<RunFacts>, String> {
    let mut http = CurlHttp;
    Ok(Client::new(&mut http, server, repo, token).run(run)?.map(|r| RunFacts::of(&r)))
}

/// The newest 50 runs.
pub fn newest_runs(server: &str, repo: &str, token: &str) -> Result<Vec<RunInfo>, String> {
    let mut http = CurlHttp;
    Ok(Client::new(&mut http, server, repo, token).runs_page(1)?.runs)
}

/// The successful runs of `workflow` on `branch` (the newest 200 runs of the branch, newest first): run id and the commit tested.
pub fn release_runs(server: &str, repo: &str, token: &str, workflow: &str, branch: &str) -> Result<Vec<(u64, String)>, String> {
    let mut http = CurlHttp;
    let mut client = Client::new(&mut http, server, repo, token);
    let want_ref = format!("refs/heads/{branch}");
    let mut out = Vec::new();
    for page in 1..=4 {
        let p = client.runs_page_where(page, &format!("branch={branch}"))?;
        let n = p.runs.len();
        for r in &p.runs {
            let f = RunFacts::of(r);
            if f.workflow == workflow && f.git_ref == want_ref && f.result == RunResult::Success {
                out.push((r.id, f.head_sha));
            }
        }
        if n < 50 {
            break;
        }
    }
    out.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
    Ok(out)
}

/// The commits of the runs of `workflow` on `branch` newer than run `after` (the newest 200 runs of the branch, newest first), one
/// entry per run: the rounds of a fix ladder that already ran, read from the server.
pub fn runs_after(server: &str, repo: &str, token: &str, workflow: &str, branch: &str, after: u64) -> Result<Vec<String>, String> {
    let mut http = CurlHttp;
    let mut client = Client::new(&mut http, server, repo, token);
    let want_ref = format!("refs/heads/{branch}");
    let mut out = Vec::new();
    for page in 1..=4 {
        let p = client.runs_page_where(page, &format!("branch={branch}"))?;
        let n = p.runs.len();
        out.extend(
            p.runs
                .iter()
                .filter(|r| r.id > after && r.workflow() == workflow && r.git_ref() == want_ref)
                .map(|r| r.head_sha.clone()),
        );
        if n < 50 {
            break;
        }
    }
    Ok(out)
}

/// Cancels run `run` (`POST .../actions/runs/<run>/cancel`: the server's swagger lists it). The token travels in a header only; an
/// error is a text without it.
pub fn cancel_run(server: &str, repo: &str, token: &str, run: u64) -> Result<(), String> {
    use crate::wait::Http as _;
    let url = format!("{}/api/v1/repos/{repo}/actions/runs/{run}/cancel", server.trim_end_matches('/'));
    CurlHttp
        .send_json("POST", &url, token, None)
        .map(|_| ())
        .map_err(|e| format!("POST {url}: {e}"))
}

/// `ci triage <run>` as the JSON `ci triage --json` prints, with `ok: true` (it worked, or this returns the error).
pub fn triage_doc(server: &str, repo: &str, token: &str, run: u64) -> Result<Value, String> {
    let mut http = CurlHttp;
    let mut client = Client::new(&mut http, server, repo, token);
    let req = super::triage::Request {
        run,
        against: None,
        app: None,
        days: 14,
        history: true,
        repo: repo.to_string(),
        now_secs: super::time::now_secs(),
    };
    let report = super::triage::collect(&mut client, &req, &mut |l| eprintln!("{l}"))?;
    let mut doc = report.json();
    doc["ok"] = Value::Bool(true);
    Ok(doc)
}

/// `ci triage <run> --json --ran`: the triage document with the count of the tests every job ran (`ran`), or why they could not be
/// counted (then the document has no `ran`, never a zero).
pub fn triage_ran_doc(server: &str, repo: &str, token: &str, run: u64, only: Option<&[String]>) -> Result<Value, String> {
    let mut doc = triage_doc(server, repo, token, run)?;
    let mut http = CurlHttp;
    let mut client = Client::new(&mut http, server, repo, token);
    let counts = super::ran::collect(&mut client, run, only, &mut |l| eprintln!("{l}"))?;
    doc["ran"] = super::ran::json(&counts);
    Ok(doc)
}

/// The shared reader's verdict of `workflow` on `sha` ([`super::verdict::verdict`]).
pub fn full_verdict(server: &str, repo: &str, token: &str, workflow: &str, sha: &str) -> super::verdict::Verdict {
    let mut http = CurlHttp;
    super::verdict::verdict(&mut Client::new(&mut http, server, repo, token), workflow, sha)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: u64, wf: &str, sha: &str) -> RunInfo {
        RunInfo {
            id,
            path: format!("{wf}@refs/heads/train/x"),
            event: String::new(),
            status: String::new(),
            conclusion: String::new(),
            head_sha: sha.into(),
            head_branch: String::new(),
            created_at: String::new(),
            run_attempt: 1,
        }
    }

    #[test]
    fn the_new_run_is_found_once_it_shows_and_older_or_foreign_runs_are_ignored() {
        let mut reads = 0;
        let mut slept = Vec::new();
        let got = find_new_run(
            &mut || {
                reads += 1;
                let mut v = vec![run(10, "ci.yml", "s"), run(99, "ci.yml", "other-sha"), run(98, "debrief.yml", "s")];
                if reads >= 3 {
                    v.push(run(11, "ci.yml", "s"));
                }
                Ok(v)
            },
            &mut |d| slept.push(d),
            ("ci.yml", "train/x", "s"),
            10,
            4,
            Duration::from_secs(15),
        );
        assert_eq!(got, Ok(11));
        assert_eq!(slept.len(), 2);
    }

    #[test]
    fn it_gives_up_after_its_reads_and_says_so() {
        let mut reads = 0;
        let got = find_new_run(
            &mut || {
                reads += 1;
                Ok(vec![run(10, "ci.yml", "s")])
            },
            &mut |_| {},
            ("ci.yml", "train/x", "s"),
            10,
            3,
            Duration::from_secs(15),
        );
        assert_eq!(reads, 3);
        assert!(
            got.unwrap_err()
                .contains("no new run of ci.yml on train/x at s showed after 3 reads")
        );
    }

    #[test]
    fn an_unreadable_list_is_an_error_not_a_missing_run() {
        let got = find_new_run(
            &mut || Err("500".into()),
            &mut |_| {},
            ("ci.yml", "train/x", "s"),
            0,
            3,
            Duration::ZERO,
        );
        assert_eq!(got, Err("500".to_string()));
    }
}
