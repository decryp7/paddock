//! How many tests each job of a finished run ran, read from its log (`ci triage <run> --ran`): the evidence the train's fix ladder
//! needs that a run it dispatched with `only_tests` ran the named tests and not nothing (ER-33). A run that is green because its
//! filter matched no test (nextest's `--no-tests=warn`) looks exactly like a green run of the tests; only the count tells them apart.
//!
//! The count of a log is the sum of its runners' own totals: nextest's `N tests run` summaries, Playwright's `Running N tests`
//! and vitest's `Tests ... (N)`. A job whose log shows no runner has 0.

use super::client::Client;
use crate::log_text::plain;
use regex::Regex;
use serde_json::{Value, json};
use std::sync::LazyLock;

fn re(pattern: &str) -> Regex {
    // The patterns are literals of this file, checked by the tests below.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("a valid pattern")
}

static NEXTEST: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*Summary \[[^\]]*\]\s+(\d+) tests? run:"));
static PLAYWRIGHT: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*Running (\d+) tests? using"));
static VITEST: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*Tests\s+.*\((\d+)\)\s*$"));

/// The tests the runners of a job log ran, in total.
pub fn tests_run(log: &str) -> usize {
    let mut total = 0;
    for raw in log.lines() {
        let l = plain(raw);
        for r in [&*NEXTEST, &*PLAYWRIGHT, &*VITEST] {
            if let Some(m) = r.captures(&l) {
                total += m[1].parse::<usize>().unwrap_or(0);
                break;
            }
        }
    }
    total
}

/// One job's count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRan {
    /// The job's display name.
    pub job: String,
    /// The tests its runners ran.
    pub tests_run: usize,
}

/// The count of every job that ran, or only of the jobs `only` names (a job that never ran, or only calls a reusable workflow, has no
/// log and is not listed). `Err` when a log cannot be read: a count that is missing is never read as 0.
pub fn collect(client: &mut Client, run: u64, only: Option<&[String]>, log: &mut dyn FnMut(&str)) -> Result<Vec<JobRan>, String> {
    let jobs = client
        .timed_jobs(run)?
        .ok_or_else(|| format!("the server has no job list for run {run}"))?;
    let mut out = Vec::new();
    for j in jobs
        .iter()
        .filter(|j| !j.is_caller() && !j.never_ran())
        .filter(|j| only.is_none_or(|names| names.contains(&j.name)))
    {
        // per-item: a log is one request per job and the server has no bulk log read; a rung names few jobs (the ones it ran), and
        // only the stage 1 count reads them all.
        log(&format!("run {run} job {} \"{}\": counting the tests it ran", j.id, j.name));
        let text = client
            .log(j.id)?
            .ok_or_else(|| format!("job \"{}\": the server has no log for it", j.name))?;
        out.push(JobRan {
            job: j.name.clone(),
            tests_run: tests_run(&text),
        });
    }
    Ok(out)
}

/// The counts as the `ran` field of `ci triage --json`.
pub fn json(ran: &[JobRan]) -> Value {
    json!({
        "total": ran.iter().map(|r| r.tests_run).sum::<usize>(),
        "jobs": ran.iter().map(|r| json!({"job": r.job, "tests_run": r.tests_run})).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_runner_is_counted_and_a_log_with_none_counts_zero() {
        let nextest = "     Starting 3 tests across 2 binaries\n     Summary [   1.2s] 3 tests run: 3 passed, 0 skipped\n";
        assert_eq!(tests_run(nextest), 3);
        assert_eq!(tests_run("Running 2 tests using 1 worker\n  2 passed (3s)\n"), 2);
        assert_eq!(tests_run(" Test Files  1 passed (1)\n      Tests  12 passed (12)\n"), 12);
        // Two nextest runs in a job add up; the colours do not matter.
        let two = format!("{nextest}\u{1b}[32m     Summary [   0.4s] 2 tests run: 2 passed\u{1b}[0m\n");
        assert_eq!(tests_run(&two), 5);
        // A filter that matched nothing, and a log with no runner, both ran nothing (the positive control of the ladder).
        assert_eq!(tests_run("     Summary [   0.0s] 0 tests run: 0 passed, 40 skipped\n"), 0);
        assert_eq!(tests_run("cargo clippy\nfinished\n"), 0);
        // A mention in the middle of a line is not a runner's total.
        assert_eq!(tests_run("echo Running 9 tests using x\n"), 0);
    }

    #[test]
    fn the_json_carries_the_total_and_every_job() {
        let v = json(&[
            JobRan {
                job: "a".into(),
                tests_run: 2,
            },
            JobRan {
                job: "b".into(),
                tests_run: 0,
            },
        ]);
        assert_eq!(v["total"], 2);
        assert_eq!(v["jobs"][1]["job"], "b");
        assert_eq!(v["jobs"][1]["tests_run"], 0);
    }
}
