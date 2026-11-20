//! The fix ladder of `train release`: after a red CI run, the cheapest evidence first, widened one rung at a time, and only the last
//! rung lets a train land.
//!
//! 1. **Failed tests only**: the tests of the red run's `ci triage` rows, on the OS and in the job where they failed (the
//!    `only_tests` input of the workflow, read by its plan job together with `since_run`; `plan_only`). Jobs not named do not run.
//!    Repeated after each fix push; the round is counted from the server (the runs of the train ref newer than the red run), at most
//!    `--max-test-rounds`; past the bound the ladder starts at rung 2.
//! 2. **Their whole jobs**: the `since_run` round the sequence always had, but only the jobs rung 1 did not run green (`only_jobs`);
//!    when rung 1 covered every failing job, rung 2 is skipped. Its test jobs must show tests that ran (ER-33).
//! 3. **Stage 1 on every OS**: the fast checks and unit tests, when the workflow has that layout (`ci/staged-fast-then-long`);
//!    without it a logged note and rung 4. Rung 4 is dispatched alongside rung 3 and cancelled when rung 3 goes red.
//! 4. **The full run on the exact head**: the shared reader (`ci verdict`, or `Landable`) in [`Seq::full`]. Nothing else lets a train
//!    land: this module never pushes a `Source` the landing reads, and [`super::release`] returns `Done` only after rung 4.
//!
//! A rung that ran no test is NOT MEASURED, never green (ER-33): rung 1 reads the counts of `ci triage --ran` and stops when a named
//! job ran fewer tests than it was given ids, or none. A rung that has nothing to run (no named failing test a job can take) is
//! skipped with its reason, and the next rung starts: skipping is not a pass and appears as such in the report.

use super::{RunEnd, Seq, Stop, short};
use crate::ci_runs::verdict::{RunResult, Verdict};
use crate::plan_only::{Kind, MAX_IDS, OnlyTests, id_kind};
use crate::train::verdict::verdict;
use crate::wait::fmt_duration;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The workflow input that selects stage 1 of a staged `ci.yml` (`ci/staged-fast-then-long`), and its value. The one place the
/// ladder names the layout: when that branch lands with other names, they change here and in the test that reads the workflow.
pub const STAGE_INPUT: (&str, &str) = ("stage", "fast");

/// The rungs, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rung {
    /// The failed tests only.
    Tests,
    /// Their whole jobs (`since_run`).
    Jobs,
    /// Stage 1 on every OS.
    Stage1,
    /// The full run on the exact head.
    Full,
}

impl Rung {
    /// The rung's number (1 to 4).
    pub fn number(self) -> u8 {
        match self {
            Rung::Tests => 1,
            Rung::Jobs => 2,
            Rung::Stage1 => 3,
            Rung::Full => 4,
        }
    }

    /// The name logs and reports use.
    pub fn name(self) -> &'static str {
        match self {
            Rung::Tests => "failed tests only",
            Rung::Jobs => "their whole jobs",
            Rung::Stage1 => "stage 1 on every OS",
            Rung::Full => "the full run on the exact head",
        }
    }
}

/// What a rung came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RungOutcome {
    /// It ran and was green (rung 4: accepted by the shared reader); this run.
    Green(u64),
    /// It did not run, and why. Never a pass: the next rung ran in its place.
    Skipped(String),
}

/// The configuration of rung 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ladder {
    /// The most rounds of rung 1; 0 turns the rung off.
    pub max_rounds: u32,
}

impl Ladder {
    /// Rung 1 off: the sequence starts at rung 2, as it did before the ladder.
    pub fn off() -> Ladder {
        Ladder { max_rounds: 0 }
    }
}

/// The failing tests of a triage document that rung 1 can run, by job, and what was left to the next rung.
#[derive(Debug, PartialEq, Eq)]
pub struct Picked {
    /// Job display name to the ids of its failing tests.
    pub tests: BTreeMap<String, Vec<String>>,
    /// What was left out: the job and why, one entry per row (a whole-job failure, an unread row, a job with no test list). The
    /// jobs of these rows are the ones rung 2 reruns.
    pub left: Vec<(String, String)>,
}

impl Picked {
    /// The jobs rung 1 did not cover (each once, sorted): rung 2 reruns exactly these.
    pub fn left_jobs(&self) -> Vec<String> {
        self.left
            .iter()
            .map(|(j, _)| j.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

/// Picks the rows of `ci triage --json` that rung 1 runs: a NEW, FAILED, FLAKY or ALREADY-FAILING row that names one test of a
/// runner and sits in a job whose runner takes a test list (`kind_of` says which). A test that fails on main too is run as well: it
/// blocks the landing like any other (user rule 2026-11-20). A whole-job failure, an unread row and a job with no test list rerun
/// whole in rung 2.
pub fn pick(doc: &Value, kind_of: &mut dyn FnMut(&str) -> Option<Kind>) -> Picked {
    let mut out = Picked {
        tests: BTreeMap::new(),
        left: Vec::new(),
    };
    for row in doc.get("rows").and_then(Value::as_array).into_iter().flatten() {
        let job = row.get("job").and_then(Value::as_str).unwrap_or("");
        let status = row.get("status").and_then(Value::as_str).unwrap_or("");
        let test = row.get("test").and_then(Value::as_str);
        let whole = row.get("whole_job").is_some_and(|w| !matches!(w, Value::Null | Value::Bool(false)));
        let label = test.unwrap_or("(the whole job)");
        let reason = match (test, whole, status) {
            (_, true, _) | (None, _, _) => Some("a whole-job failure"),
            (_, _, "unknown") => Some("not known (UNKNOWN)"),
            (_, _, "new" | "failed" | "flaky" | "already-failing") => None,
            _ => Some("a status this ladder does not know"),
        };
        if let Some(r) = reason {
            out.left.push((job.to_string(), format!("{job}: {label}: {r}")));
            continue;
        }
        let id = test.unwrap_or_default();
        match (kind_of(job), id_kind(id)) {
            (Some(want), Some(have)) if want == have => out.tests.entry(job.to_string()).or_default().push(id.to_string()),
            (None, _) => out
                .left
                .push((job.to_string(), format!("{job}: {label}: the job's runner takes no test list"))),
            _ => out
                .left
                .push((job.to_string(), format!("{job}: {label}: not a test id its job's runner takes"))),
        }
    }
    for ids in out.tests.values_mut() {
        ids.sort();
        ids.dedup();
    }
    out
}

fn count_of(jobs: &[Value], name: &str) -> Option<u64> {
    jobs.iter()
        .find(|j| j.get("job").and_then(Value::as_str) == Some(name))
        .and_then(|j| j.get("tests_run").and_then(Value::as_u64))
}

fn ran_jobs(doc: &Value) -> Result<&Vec<Value>, String> {
    doc.get("ran").and_then(|r| r.get("jobs")).and_then(Value::as_array).ok_or_else(|| {
        match doc.get("ran").and_then(|r| r.get("error")).and_then(Value::as_str) {
            Some(e) => format!("the tests the jobs ran could not be counted ({e})"),
            None => "the triage has no `ran` counts: the named tests cannot be shown to have run".to_string(),
        }
    })
}

/// Whether the counts of `ci triage --ran` show the named tests ran: every named job is in the run and ran at least as many tests as
/// it was given ids (a Playwright location can run in two projects, never in fewer than one test per id). `Err` is NOT MEASURED. The
/// warnings name a job that was not named and ran tests anyway (the input was not honoured there).
pub fn check_ran(only: &OnlyTests, doc: &Value) -> Result<Vec<String>, String> {
    let jobs = ran_jobs(doc)?;
    for (job, ids) in only.jobs() {
        match count_of(jobs, job) {
            None => {
                return Err(format!(
                    "job {job:?} is not in the run: its {} named test(s) did not run",
                    ids.len()
                ));
            }
            Some(0) => {
                return Err(format!(
                    "job {job:?} ran no test: its filter for {} named test(s) matched nothing",
                    ids.len()
                ));
            }
            Some(n) if (n as usize) < ids.len() => {
                return Err(format!(
                    "job {job:?} ran {n} test(s) but {} were named: some did not run",
                    ids.len()
                ));
            }
            Some(_) => {}
        }
    }
    let warnings = jobs
        .iter()
        .filter_map(|j| Some((j.get("job")?.as_str()?, j.get("tests_run")?.as_u64()?)))
        .filter(|(name, n)| *n > 0 && !only.jobs().contains_key(*name))
        .map(|(name, n)| format!("job {name:?} ran {n} test(s) although only_tests did not name it"))
        .collect();
    Ok(warnings)
}

/// Whether the counts of a rung 2 run show that each of its test jobs (`jobs`: the jobs whose runner takes a test list) ran tests: a
/// whole-job rerun of a test job that ran none is NOT MEASURED, never green (ER-33). `Err` is that reason.
pub fn check_jobs_ran(jobs: &[String], doc: &Value) -> Result<(), String> {
    let counts = ran_jobs(doc)?;
    for job in jobs {
        match count_of(counts, job) {
            None => return Err(format!("job {job:?} is not in the run: it did not rerun")),
            Some(0) => return Err(format!("job {job:?} ran no test")),
            Some(_) => {}
        }
    }
    Ok(())
}

/// What rung 1 came to, for [`Seq::ladder`].
enum Tests {
    /// Green, with the run.
    Green(u64),
    /// Not run, and why.
    Skipped(String),
}

impl Seq<'_> {
    /// Rungs 1 to 3 (and the first full run of rung 4 when it went alongside rung 3); `true` when that first full run is already
    /// judged, so the caller does not dispatch it again.
    pub(super) fn ladder(&mut self) -> Result<bool, Stop> {
        let mut only_jobs: Option<Vec<String>> = None;
        let mut skip_rung2 = None;
        if let super::Round::Dispatch(since) = self.plan.round {
            self.since_facts(since)?;
            match self.rung_tests(since)? {
                (Tests::Green(run), left) => {
                    self.rungs.push((Rung::Tests, RungOutcome::Green(run)));
                    if left.is_empty() {
                        skip_rung2 = Some("rung 1 ran every failing test green and left no job".to_string());
                    } else {
                        only_jobs = Some(left);
                    }
                }
                (Tests::Skipped(why), _) => {
                    let _ = writeln!(self.log, "train: rung 1 ({}): skipped: {why}; rung 2 starts", Rung::Tests.name());
                    self.rungs.push((Rung::Tests, RungOutcome::Skipped(why)));
                }
            }
        } else {
            let why = "the fix round already ran (passed:<run>): it is judged now".to_string();
            self.rungs.push((Rung::Tests, RungOutcome::Skipped(why)));
        }
        match skip_rung2 {
            Some(why) => {
                let _ = writeln!(self.log, "train: rung 2 ({}): skipped: {why}", Rung::Jobs.name());
                self.rungs.push((Rung::Jobs, RungOutcome::Skipped(why)));
            }
            None => {
                if let Some(jobs) = &only_jobs {
                    let _ = writeln!(
                        self.log,
                        "train: rung 2 ({}): only the jobs rung 1 did not cover: {}",
                        Rung::Jobs.name(),
                        jobs.join("; ")
                    );
                }
                self.fix_round(only_jobs.as_deref())?;
                if let Some(run) = self.steps.last().map(|(_, s)| s.run()) {
                    let _ = writeln!(self.log, "train: rung 2 ({}): green, run {run}", Rung::Jobs.name());
                    self.rungs.push((Rung::Jobs, RungOutcome::Green(run)));
                }
            }
        }
        self.rung_stage1_and_full()
    }

    /// The triage of the red run (read once): its rows name the jobs that failed (`since_jobs`) and the tests rung 1 can name.
    fn red_doc(&mut self, since: u64) -> Result<Value, Stop> {
        let doc = self.ports.triage(since).map_err(|e| {
            Stop::new(
                format!("the triage of the red run {since} failed: {e}"),
                format!("run `./dev ci triage {since}` by hand and read why"),
            )
        })?;
        let failed_jobs: BTreeSet<String> = doc
            .get("rows")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|r| r.get("job").and_then(Value::as_str).map(str::to_string))
            .collect();
        self.since_jobs = failed_jobs.into_iter().collect();
        Ok(doc)
    }

    /// The count of the tests the jobs of a rung 2 run ran: each test job it reran must show tests that ran (ER-33). Not run for a
    /// round that already ran (`passed:<run>`), which has no counts of its own to read.
    pub(super) fn rung2_ran(&mut self, run: u64, only_jobs: Option<&[String]>) -> Result<(), Stop> {
        let wf = self.plan.workflow.file();
        let named: Vec<String> = match only_jobs {
            Some(j) => j.to_vec(),
            None => {
                if self.since_jobs.is_empty()
                    && let super::Round::Dispatch(since) = self.plan.round
                {
                    self.red_doc(since)?;
                }
                self.since_jobs.clone()
            }
        };
        let ports = &mut *self.ports;
        let test_jobs: Vec<String> = named.into_iter().filter(|j| ports.job_kind(wf, j).is_some()).collect();
        if test_jobs.is_empty() {
            let _ = writeln!(
                self.log,
                "train: rung 2: run {run}: no job of it runs a test list: nothing to count"
            );
            return Ok(());
        }
        let doc = self.ports.triage_ran(run, Some(&test_jobs)).map_err(|e| {
            Stop::new(
                format!("rung 2 run {run}: its triage with the test counts failed: {e}"),
                format!("run `./dev ci triage {run} --ran` by hand; a rung that cannot show its tests ran is not measured"),
            )
        })?;
        check_jobs_ran(&test_jobs, &doc).map_err(|why| {
            Stop::new(
                format!("rung 2 run {run} is NOT MEASURED: {why}"),
                "read the run: a job that ran no test is not green; then run the sequence again",
            )
        })?;
        let _ = writeln!(
            self.log,
            "train: rung 2: run {run}: tests ran in {} job(s): {}",
            test_jobs.len(),
            test_jobs.join("; ")
        );
        Ok(())
    }

    /// Rung 1: the failed tests of `since` only, in the jobs where they failed. The second value is the jobs it did not cover.
    fn rung_tests(&mut self, since: u64) -> Result<(Tests, Vec<String>), Stop> {
        let wf = self.plan.workflow.file();
        let ladder = self.plan.ladder;
        if ladder.max_rounds == 0 {
            return Ok((Tests::Skipped("turned off (--max-test-rounds 0)".into()), Vec::new()));
        }
        let doc = &self.red_doc(since)?;
        // The round is counted from the server: the commits of the runs of this workflow on the train ref newer than the red run,
        // each other than the head being a push of a fix. An argument would be trusted; the runs are not.
        let heads = self.ports.runs_after(wf, since).map_err(|e| {
            Stop::new(
                format!("rung 1: the earlier runs of {wf} on the train ref could not be read: {e}"),
                "check the network and GITEA_TOKEN, then run it again",
            )
        })?;
        let sha = self.plan.sha.clone();
        let earlier: BTreeSet<&String> = heads.iter().filter(|h| **h != sha).collect();
        let round = earlier.len() as u32 + 1;
        if round > ladder.max_rounds {
            return Ok((
                Tests::Skipped(format!(
                    "round {round} (the server shows {} earlier head(s) with runs after run {since}) is past the bound of {} rounds (--max-test-rounds)",
                    earlier.len(),
                    ladder.max_rounds
                )),
                Vec::new(),
            ));
        }
        let ports = &mut *self.ports;
        let picked = pick(doc, &mut |job| ports.job_kind(wf, job));
        for (_, l) in &picked.left {
            let _ = writeln!(self.log, "train: rung 1: left to rung 2: {l}");
        }
        let left = picked.left_jobs();
        if picked.tests.is_empty() {
            return Ok((
                Tests::Skipped(format!("run {since} has no named failing test that a job can run alone")),
                Vec::new(),
            ));
        }
        let only = OnlyTests::new(picked.tests);
        if only.count() > MAX_IDS {
            return Ok((
                Tests::Skipped(format!(
                    "{} failing tests are more than the {MAX_IDS} one only_tests input carries",
                    only.count()
                )),
                Vec::new(),
            ));
        }
        let t0 = self.ports.elapsed();
        let _ = writeln!(
            self.log,
            "train: rung 1 ({}), round {round}/{}: {} test(s) in {} job(s) of run {since}, on {}",
            Rung::Tests.name(),
            ladder.max_rounds,
            only.count(),
            only.jobs().len(),
            short(&self.plan.sha)
        );
        for (job, ids) in only.jobs() {
            for id in ids {
                let _ = writeln!(self.log, "train: rung 1:   {job}: {id}");
            }
        }
        let encoded = only.encode();
        let input = self.plan.workflow.input();
        let (run, end) = self.dispatch_and_wait(wf, &[input, ("only_tests", &encoded)], Some(since))?;
        if let RunEnd::Unfinished(why) = end {
            return Err(super::unfinished(wf, run, &why));
        }
        let again = format!(
            "fix the failing tests in their owners' branches, push through the gate, and run the sequence again with --since-run {since} (the round is counted from the server)"
        );
        let named_jobs: Vec<String> = only.jobs().keys().cloned().collect();
        let ran = self.ports.triage_ran(run, Some(&named_jobs)).map_err(|e| {
            Stop::new(
                format!("rung 1 run {run}: its triage with the test counts failed: {e}"),
                format!("run `./dev ci triage {run} --ran` by hand; a rung that cannot show its tests ran is not measured"),
            )
        })?;
        match self.result_of(run)? {
            RunResult::Success => {}
            RunResult::Failure => {
                if let Err(reasons) = verdict(&ran, wf, &self.plan.accepted) {
                    return Err(Stop::new(format!("rung 1 run {run} is red: {}", reasons.join("; ")), again));
                }
                let _ = writeln!(self.log, "train: rung 1: run {run}: only accepted FLAKY tests failed");
            }
            other => return Err(super::not_a_result(wf, run, &other)),
        }
        match check_ran(&only, &ran) {
            Ok(warnings) => {
                for w in warnings {
                    let _ = writeln!(self.log, "::warning::train: rung 1 run {run}: {w}");
                }
            }
            Err(why) => {
                return Err(Stop::new(
                    format!("rung 1 run {run} is NOT MEASURED: {why}"),
                    format!(
                        "read the run's plan job log (the only_tests input is honoured by ./dev plan --ci); a green run of no test is not green. {again}"
                    ),
                ));
            }
        }
        let _ = writeln!(
            self.log,
            "train: rung 1 ({}): green, run {run}: {} named test(s) ran, after {}",
            Rung::Tests.name(),
            only.count(),
            fmt_duration(self.ports.elapsed().saturating_sub(t0))
        );
        self.steps.push((wf.to_string(), super::Source::TestsOnly(run)));
        Ok((Tests::Green(run), left))
    }

    /// Rung 3: stage 1 on every OS, when the workflow has that layout; otherwise a logged fall back to rung 4. For `ci.yml` the first
    /// full run (rung 4) is dispatched alongside it and cancelled when stage 1 goes red; `true` when that full run is judged here.
    fn rung_stage1_and_full(&mut self) -> Result<bool, Stop> {
        let wf = self.plan.workflow.file();
        let sha = self.plan.sha.clone();
        let inputs = match self.ports.stage1(wf) {
            Err(e) => {
                return Err(Stop::new(
                    format!("rung 3: whether {wf} has a stage 1 could not be read: {e}"),
                    "check that the workflow file is readable at the train head, then run the sequence again",
                ));
            }
            Ok(None) => {
                let why = format!(
                    "{wf} at {} has no `{}` input (the staged layout of ci/staged-fast-then-long is not on this train)",
                    short(&sha),
                    STAGE_INPUT.0
                );
                let _ = writeln!(
                    self.log,
                    "::notice::train: rung 3 ({}): skipped: {why}; falling back to rung 4, the full run",
                    Rung::Stage1.name()
                );
                self.rungs.push((Rung::Stage1, RungOutcome::Skipped(why)));
                return Ok(false);
            }
            Ok(Some(i)) => i,
        };
        // A full run of the head that is acceptable already makes rung 3 unnecessary.
        if wf == "ci.yml"
            && let Some(source) = self.accepted_full(wf, None)?
        {
            let why = "an acceptable full run of the head exists already".to_string();
            let _ = writeln!(self.log, "train: rung 3 ({}): skipped: {why}", Rung::Stage1.name());
            self.rungs.push((Rung::Stage1, RungOutcome::Skipped(why)));
            self.steps.push((wf.to_string(), source));
            return Ok(true);
        }
        let mut all: Vec<(&str, &str)> = inputs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        all.push(self.plan.workflow.input());
        let _ = writeln!(
            self.log,
            "train: rung 3 ({}): {wf} on {} with {inputs:?}",
            Rung::Stage1.name(),
            short(&sha)
        );
        let (run3, t3) = self.dispatch_only(wf, &all, None)?;
        // Rung 4 alongside: the same head's full run starts now and is cancelled when rung 3 goes red.
        let full = if wf == "ci.yml" {
            let (run4, t4) = self.dispatch_only("ci.yml", &[("webkit", "true")], None)?;
            let _ = writeln!(self.log, "train: rung 4 alongside rung 3: ci.yml full run {run4}");
            Some((run4, t4))
        } else {
            None
        };
        let outcome = self.judge_stage1(wf, run3, t3);
        if let Err(stop) = outcome {
            if let Some((run4, _)) = full {
                match self.ports.cancel_run(run4) {
                    Ok(()) => {
                        let _ = writeln!(self.log, "train: rung 4: run {run4} cancelled: rung 3 is red");
                    }
                    Err(e) => {
                        let _ = writeln!(
                            self.log,
                            "::warning::train: rung 4: run {run4} could not be cancelled ({e}); cancel it by hand, it only wastes runner time"
                        );
                    }
                }
            }
            return Err(stop);
        }
        match full {
            Some((run4, t4)) => {
                let end = self.wait_dispatched("ci.yml", run4, t4);
                self.accept_dispatched_full("ci.yml", run4, end)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Waits for rung 3's run and judges it: green, a stage 1 that is not counted as a full run, and tests that ran.
    fn judge_stage1(&mut self, wf: &str, run: u64, t0: std::time::Duration) -> Result<(), Stop> {
        let sha = self.plan.sha.clone();
        let end = self.wait_dispatched(wf, run, t0);
        if let RunEnd::Unfinished(why) = end {
            return Err(super::unfinished(wf, run, &why));
        }
        match self.result_of(run)? {
            RunResult::Success => {}
            RunResult::Failure => {
                return Err(Stop::new(
                    format!("rung 3 run {run} ({wf}, stage 1) is red"),
                    format!("read it with `./dev ci triage {run}`, fix the cause in its owner's branch, then run the sequence again"),
                ));
            }
            other => return Err(super::not_a_result(wf, run, &other)),
        }
        // A stage 1 run is not a full run. If the shared reader counts it as one, the landing could pass on it: stop here.
        if let Verdict::Green { run: counted } = self.ports.ci_verdict(wf, &sha)
            && counted == run
        {
            return Err(Stop::new(
                format!(
                    "the shared reader counts the stage 1 run {run} of {wf} as a full green run of {}",
                    short(&sha)
                ),
                "fix the plan job so that a stage 1 run never prints `plan verdict: complete=true` (plan_since::COMPLETE_LINE); no rung but 4 lets a train land",
            ));
        }
        let doc = self.ports.triage_ran(run, None).map_err(|e| {
            Stop::new(
                format!("rung 3 run {run}: the tests it ran could not be counted: {e}"),
                format!("run `./dev ci triage {run} --ran` by hand; a rung that cannot show its tests ran is not measured"),
            )
        })?;
        let total = doc.get("ran").and_then(|r| r.get("total")).and_then(Value::as_u64);
        match total {
            Some(n) if n > 0 => {
                let _ = writeln!(
                    self.log,
                    "train: rung 3 ({}): green, run {run}, {n} test(s) ran",
                    Rung::Stage1.name()
                );
            }
            _ => {
                return Err(Stop::new(
                    format!("rung 3 run {run} is NOT MEASURED: it shows no test that ran (stage 1 runs the unit tests)"),
                    "read the run: a stage 1 that ran nothing is not green; then run the sequence again",
                ));
            }
        }
        self.steps.push((wf.to_string(), super::Source::Stage1(run)));
        self.rungs.push((Rung::Stage1, RungOutcome::Green(run)));
        Ok(())
    }
}
