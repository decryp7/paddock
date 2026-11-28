// ci-runs-reader: mutants record: the result lines of one run's own shard jobs; no pass/fail on a commit
//! `mutants record`: the record job of mutants.yml, the one line that says what a run measured.
//!
//! Every shard job prints one result line ([`RESULT_PREFIX`] and JSON, [`result_line`]) at its end. The record job reads the run's shard
//! jobs and their logs through the Actions API ([`crate::ci_runs::client::Client`]) and says, per planned shard, whether it measured all of
//! its mutants, part of them, or nothing; a shard that printed no result line (it yielded to a train, failed before the gate, was
//! cancelled or never started) is NOT MEASURED with the job's own outcome, never counted as clean (ER-33). Before this, the record line
//! read only the matrix's aggregate result, so a shard that yielded, or refused its slice for the cap, was "success" (runs 9393, 9400).

use super::Summary;
use serde_json::{Value, json};

/// The start of a shard's result line; the rest of the line is one JSON object.
pub const RESULT_PREFIX: &str = "mutants shard result: ";

/// The result line of one shard (`k/n`) from its [`Summary`] and the gate's seconds.
pub fn result_line(shard: &str, s: &Summary, secs: u64) -> String {
    let v = json!({
        "shard": shard, "secs": secs,
        "tested": s.tested, "caught": s.caught, "missed": s.missed, "timeouts": s.timeouts, "unviable": s.unviable,
        "overCap": s.over_cap, "notMeasured": s.not_measured, "broken": s.broken,
    });
    format!("{RESULT_PREFIX}{v}")
}

/// One shard's result as its line says it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShardResult {
    /// Mutants with a verdict.
    pub tested: u64,
    /// Caught ones.
    pub caught: u64,
    /// Missed ones: findings.
    pub missed: u64,
    /// Timed out: not a pass.
    pub timeouts: u64,
    /// Did not compile: not a pass.
    pub unviable: u64,
    /// Left out by a cap: not measured.
    pub over_cap: u64,
    /// Why part of it was not measured.
    pub not_measured: Vec<String>,
    /// Why the check could not be trusted.
    pub broken: Option<String>,
    /// The gate's seconds (0 when the line has none).
    pub secs: u64,
}

/// The result line of `shard` in a job's log: the last one with [`RESULT_PREFIX`] (a log line may start with the runner's timestamp).
/// `None` when the log has none for that shard; `Err` when the line is there but is not the JSON [`result_line`] writes (ER-42).
pub fn parse_result(log: &str, shard: &str) -> Option<Result<ShardResult, String>> {
    let text = log
        .lines()
        .filter_map(|l| l.find(RESULT_PREFIX).map(|i| &l[i + RESULT_PREFIX.len()..]))
        .filter_map(|rest| serde_json::from_str::<Value>(rest.trim()).ok().map(|v| (rest, v)))
        .rfind(|(_, v)| v["shard"].as_str() == Some(shard));
    let (_, v) = match text {
        Some(t) => t,
        None => {
            // A line that names the prefix but does not parse is an error, not "no line".
            return log
                .lines()
                .any(|l| l.contains(RESULT_PREFIX) && l.contains(&format!("\"shard\":\"{shard}\"")))
                .then(|| Err(format!("the result line of shard {shard} is not JSON")));
        }
    };
    let n = |k: &str| {
        v[k].as_u64()
            .ok_or_else(|| format!("the result line of shard {shard} has no `{k}`"))
    };
    Some((|| {
        Ok(ShardResult {
            tested: n("tested")?,
            caught: n("caught")?,
            missed: n("missed")?,
            timeouts: n("timeouts")?,
            unviable: n("unviable")?,
            over_cap: n("overCap")?,
            not_measured: v["notMeasured"]
                .as_array()
                .ok_or_else(|| format!("the result line of shard {shard} has no `notMeasured` list"))?
                .iter()
                .map(|x| x.as_str().unwrap_or("").to_string())
                .collect(),
            broken: v["broken"].as_str().map(str::to_string),
            secs: v["secs"].as_u64().unwrap_or(0),
        })
    })())
}

/// The display name of a shard's job in mutants.yml (`mutants (shard ${{ matrix.shard }})`): the record finds the job by it.
pub fn job_name(shard: &str) -> String {
    format!("mutants (shard {shard})")
}

/// What the record knows of one planned shard's job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShardJob {
    /// `k/n`.
    pub shard: String,
    /// The job's outcome in words (`success`, `cancelled`, `skipped`, `no job`).
    pub outcome: String,
    /// Its log, when it has one.
    pub log: Option<String>,
}

/// How one shard ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShardState {
    /// Every mutant of its slice has a verdict.
    Measured(ShardResult),
    /// Some have, the rest is named in `not_measured` or `over_cap`.
    Partial(ShardResult),
    /// Nothing measured: the reason.
    NotMeasured(String),
}

/// The state of one shard from its job.
pub fn shard_state(job: &ShardJob) -> ShardState {
    let Some(log) = &job.log else {
        return ShardState::NotMeasured(format!("the job has no log (job {})", job.outcome));
    };
    match parse_result(log, &job.shard) {
        None if log.contains(crate::ci_runs::schedule_skip::SKIP_NOTICE) => {
            ShardState::NotMeasured(format!("it yielded to a train before it built anything (job {})", job.outcome))
        }
        None => ShardState::NotMeasured(format!(
            "it printed no result line (job {}): it ended before the gate did",
            job.outcome
        )),
        Some(Err(e)) => ShardState::NotMeasured(e),
        Some(Ok(r)) if r.broken.is_some() => ShardState::NotMeasured(format!("check broken: {}", r.broken.clone().unwrap_or_default())),
        Some(Ok(r)) if r.tested == 0 && (r.over_cap > 0 || !r.not_measured.is_empty()) => {
            ShardState::NotMeasured(r.not_measured.join("; "))
        }
        Some(Ok(r)) if r.over_cap > 0 || !r.not_measured.is_empty() => ShardState::Partial(r),
        Some(Ok(r)) => ShardState::Measured(r),
    }
}

/// The record of a run: one summary line, then one NOT MEASURED line per shard that measured nothing or only part, then one per
/// planned slice the run did not start ([`super::Plan::unplanned`]).
pub fn record_text(base: &str, head: &str, count: usize, selected: usize, slices: usize, jobs: &[ShardJob]) -> String {
    let next = format!("Next step: dispatch mutants.yml with base set to {base} once no train runs");
    let mut lines = Vec::new();
    let (mut measured, mut partial, mut none) = (0, 0, 0);
    let mut sum = ShardResult::default();
    for job in jobs {
        let mut add = |r: &ShardResult| {
            sum.tested += r.tested;
            sum.caught += r.caught;
            sum.missed += r.missed;
            sum.timeouts += r.timeouts;
            sum.unviable += r.unviable;
            sum.secs += r.secs;
        };
        match shard_state(job) {
            ShardState::Measured(r) => {
                measured += 1;
                add(&r);
            }
            ShardState::Partial(r) => {
                partial += 1;
                add(&r);
                lines.push(format!(
                    "::warning::mutants record: NOT MEASURED in part: shard {}: {} not measured by the cap; {}. {next}",
                    job.shard,
                    r.over_cap,
                    if r.not_measured.is_empty() {
                        "nothing else".to_string()
                    } else {
                        r.not_measured.join("; ")
                    }
                ));
            }
            ShardState::NotMeasured(why) => {
                none += 1;
                lines.push(format!(
                    "::warning::mutants record: NOT MEASURED: shard {}: {why}. {next}",
                    job.shard
                ));
            }
        }
    }
    if count > selected {
        lines.push(format!(
            "::warning::mutants record: NOT MEASURED: {} of the {count} mutants are over the per-landing cap of {} (user decision 2026-11-28; the {selected} run were chosen round-robin over the changed functions). Next step: dispatch mutants.yml with base set to a commit between {base} and {head} (a member's merge) to measure part of the rest; base {base} again chooses the same {selected}",
            count - selected,
            super::MAX_PER_LANDING
        ));
    }
    let unstarted = slices.saturating_sub(jobs.len());
    if unstarted > 0 {
        lines.push(format!(
            "::warning::mutants record: NOT MEASURED: {unstarted} of the plan's {slices} slices are over the run's shard limit and never ran. {next}, or split the landing"
        ));
    }
    let head_line = format!(
        "mutants record: {base}..{head} planned {count} mutants, {selected} chosen, in {slices} slice(s), {} run: {measured} measured, {partial} in part, {none} NOT MEASURED{}; {} tested, {} caught, {} MISSED, {} timed out, {} unviable; {} s in the gates{}",
        jobs.len(),
        if unstarted > 0 {
            format!(", {unstarted} never run")
        } else {
            String::new()
        },
        sum.tested,
        sum.caught,
        sum.missed,
        sum.timeouts,
        sum.unviable,
        sum.secs,
        sum.secs
            .checked_div(sum.tested)
            .map_or(String::new(), |per| format!(", {per} s a mutant with each shard's setup"))
    );
    let level = if none + partial + unstarted > 0 || count > selected {
        "::warning::"
    } else {
        "::notice::"
    };
    let mut text = format!("{level}{head_line}\n");
    for l in lines {
        text.push_str(&l);
        text.push('\n');
    }
    text
}

/// `mutants record`: reads the shard jobs of run `run` and prints [`record_text`] (and appends it to `GITHUB_STEP_SUMMARY`). Exit 0
/// whatever it finds (report-only, ER-41); jobs it cannot read are NOT MEASURED, with the cause, never a clean record.
pub fn command_record(
    run: Option<u64>,
    base: &str,
    head: &str,
    count: usize,
    selected: usize,
    slices: usize,
    shards: &str,
) -> Result<paddock_cli::Report, paddock_cli::CliError> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let planned: Vec<String> = serde_json::from_str(shards)
        .map_err(|e| paddock_cli::CliError::usage(format!("--shards `{shards}` is not the plan's JSON list of k/n: {e}")))?;
    let run = run
        .or_else(|| env("GITHUB_RUN_ID").and_then(|v| v.parse().ok()))
        .ok_or_else(|| paddock_cli::CliError::usage("no run: pass --run or set GITHUB_RUN_ID"))?;
    let server = env("GITHUB_SERVER_URL").unwrap_or_else(|| crate::ci_runs::SERVER.into());
    let repo = env("GITHUB_REPOSITORY").unwrap_or_else(|| "decryp7/paddock".into());
    let token = env("PLAN_TOKEN").or_else(|| env("GITEA_TOKEN")).unwrap_or_default();
    let mut http = crate::wait::CurlHttp;
    let mut client = crate::ci_runs::client::Client::new(&mut http, &server, &repo, &token);
    let jobs = read_jobs(&mut client, run, &planned);
    let text = record_text(base, head, count, selected, slices, &jobs);
    if let Some(path) = env("GITHUB_STEP_SUMMARY") {
        use std::io::Write;
        let _ = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .and_then(|mut f| f.write_all(text.as_bytes()));
    }
    Ok(paddock_cli::Report::new(json!({"run": run, "shards": planned.len()}), text))
}

/// The planned shards' jobs of `run` with their logs; a shard whose job or log cannot be read gets the cause as its outcome.
fn read_jobs(client: &mut crate::ci_runs::client::Client<'_>, run: u64, planned: &[String]) -> Vec<ShardJob> {
    let listed = client.timed_jobs(run);
    planned
        .iter()
        // per-item: one log per shard job, at most MAX_SHARDS of them (the plan's limit); the API has no bulk log read.
        .map(|shard| {
            let found = match &listed {
                Ok(Some(jobs)) => jobs.iter().find(|j| j.name == job_name(shard)).cloned(),
                _ => None,
            };
            let Some(j) = found else {
                let outcome = match &listed {
                    Ok(Some(_)) => "no job of that name".to_string(),
                    Ok(None) => format!("run {run} has no job list"),
                    Err(e) => format!("the jobs cannot be read: {e}"),
                };
                return ShardJob {
                    shard: shard.clone(),
                    outcome,
                    log: None,
                };
            };
            let outcome = format!("{:?}", j.outcome).to_lowercase();
            let log = if j.never_ran() { None } else { client.log(j.id).ok().flatten() };
            ShardJob {
                shard: shard.clone(),
                outcome,
                log,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(shard: &str, outcome: &str, log: Option<String>) -> ShardJob {
        ShardJob {
            shard: shard.into(),
            outcome: outcome.into(),
            log,
        }
    }

    fn summary(tested: usize, missed: usize, over_cap: usize, not_measured: &[&str]) -> Summary {
        Summary {
            tested,
            caught: tested - missed,
            missed,
            over_cap,
            not_measured: not_measured.iter().map(|s| s.to_string()).collect(),
            ..Summary::default()
        }
    }

    #[test]
    fn a_result_line_reads_back_through_the_json_parser_with_a_runner_timestamp_before_it() {
        let line = result_line("2/5", &summary(4, 1, 0, &[]), 240);
        let log = format!("2026-11-28T10:00:00.1Z noise\n2026-11-28T10:00:01.2Z {line}\n");
        let r = parse_result(&log, "2/5").unwrap().unwrap();
        assert_eq!((r.tested, r.caught, r.missed, r.over_cap, r.secs), (4, 3, 1, 0, 240));
        assert!(parse_result(&log, "3/5").is_none(), "another shard's line is not this one's");
        assert!(parse_result("mutants shard result: {\"shard\":\"2/5\"", "2/5").unwrap().is_err());
    }

    /// The silent losses of runs 9393 and 9400: a shard that yielded, one cancelled before it started, one that refused its slice for the
    /// cap and one cut short are each NOT MEASURED (or in part) in the record, and the record is a warning, never a clean notice.
    #[test]
    fn every_shard_that_did_not_measure_all_of_its_slice_is_named_not_measured() {
        let skip = format!("{}train/x in CI (runs ci.yml 1)", crate::ci_runs::schedule_skip::SKIP_NOTICE);
        let jobs = vec![
            job("0/5", "success", Some(result_line("0/5", &summary(6, 1, 0, &[]), 600))),
            job("1/5", "success", Some(format!("2026-11-28T10:00:00.1Z {skip}\n"))),
            job("2/5", "cancelled", None),
            job(
                "3/5",
                "success",
                Some(result_line("3/5", &summary(0, 0, 20, &["the cap of 16 per shard"]), 30)),
            ),
            job(
                "4/5",
                "success",
                Some(result_line("4/5", &summary(3, 0, 0, &["cut short after 3 of 6 mutants"]), 3300)),
            ),
        ];
        let text = record_text("aaa", "bbb", 30, 30, 5, &jobs);
        let first = text.lines().next().unwrap();
        assert!(
            first.starts_with("::warning::mutants record: aaa..bbb planned 30 mutants"),
            "{text}"
        );
        assert!(
            first.contains("1 measured, 1 in part, 3 NOT MEASURED") && first.contains("9 tested"),
            "{text}"
        );
        assert!(first.contains("3900 s in the gates, 433 s a mutant"), "{text}");
        for (shard, why) in [("1/5", "yielded to a train"), ("2/5", "no log"), ("3/5", "cap of 16")] {
            assert!(
                text.lines()
                    .any(|l| l.contains(&format!("NOT MEASURED: shard {shard}")) && l.contains(why)),
                "{shard}: {text}"
            );
        }
        assert!(
            text.contains("NOT MEASURED in part: shard 4/5") && text.contains("base set to aaa"),
            "{text}"
        );
    }

    #[test]
    fn a_run_whose_every_shard_measured_its_slice_is_a_notice_and_slices_over_the_limit_are_named() {
        let jobs = vec![job("0/1", "success", Some(result_line("0/1", &summary(2, 0, 0, &[]), 100)))];
        let text = record_text("aaa", "bbb", 2, 2, 1, &jobs);
        assert!(text.starts_with("::notice::") && text.lines().count() == 1, "{text}");
        let text = record_text("aaa", "bbb", 300, 30, 3, &jobs);
        assert!(text.contains("NOT MEASURED: 2 of the plan's 3 slices"), "{text}");
    }

    /// The mutants over the per-landing cap are NOT MEASURED in the record by number, with the base, and make it a warning.
    #[test]
    fn mutants_over_the_landing_cap_are_named_not_measured_with_the_base() {
        let jobs = vec![job("0/1", "success", Some(result_line("0/1", &summary(2, 0, 0, &[]), 100)))];
        let text = record_text("aaa", "bbb", 2, 2, 1, &jobs);
        assert!(!text.contains("per-landing cap"), "{text}");
        let text = record_text("aaa", "bbb", 80, 60, 1, &jobs);
        assert!(text.starts_with("::warning::"), "{text}");
        assert!(
            text.lines().any(
                |l| l.contains("NOT MEASURED: 20 of the 80 mutants are over the per-landing cap of 60")
                    && l.contains("base set to a commit between aaa and bbb")
            ),
            "{text}"
        );
    }

    #[test]
    fn the_job_name_is_the_one_mutants_yml_gives_its_shards() {
        let yml = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.gitea/workflows/mutants.yml")).unwrap();
        let docs = yaml_rust2::YamlLoader::load_from_str(&yml).unwrap();
        let name = docs[0]["jobs"]["shards"]["name"].as_str().unwrap().to_string();
        assert_eq!(name.replace("${{ matrix.shard }}", "3/7"), job_name("3/7"));
    }
}
