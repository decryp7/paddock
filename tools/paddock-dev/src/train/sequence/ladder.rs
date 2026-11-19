//! The fix ladder of `train release`: after a red CI run, the cheapest evidence first, widened one rung at a time, and only the last
//! rung lets a train land.
//!
//! 1. **Failed tests only**: the tests of the red run's `ci triage` rows, on the OS and in the job where they failed (the
//!    `only_tests` input of the workflow, read by its plan job together with `since_run`; `plan_only`). Jobs not named do not run.
//!    Repeated after each fix push, `--test-round n` of at most `--max-test-rounds`; past the bound the ladder starts at rung 2.
//! 2. **Their whole jobs**: the `since_run` round the sequence always had.
//! 3. **Stage 1 on every OS**: the fast checks and unit tests, when the workflow has that layout (`ci/staged-fast-then-long`);
//!    without it a logged note and rung 4.
//! 4. **The full run on the exact head**: the shared reader (`ci verdict`, or `Landable`) in [`Seq::full`]. Nothing else lets a train
//!    land: this module never pushes a `Source` the landing reads, and [`super::release`] returns `Done` only after rung 4.
//!
//! A rung that ran no test is NOT MEASURED, never green (ER-33): rung 1 reads the counts of `ci triage --ran` and stops when a named
//! job ran fewer tests than it was given ids, or none. A rung that has nothing to run (no named failing test a job can take) is
//! skipped with its reason, and the next rung starts: skipping is not a pass and appears as such in the report.

use super::{Seq, Stop, short};
use crate::plan_only::{Kind, MAX_IDS, OnlyTests, id_kind};
use crate::train::verdict::verdict;
use crate::ci_runs::verdict::{RunResult, Verdict};
use crate::wait::fmt_duration;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;

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
    /// Which round of rung 1 this is (1 for the first run on a red run; one more after each fix push).
    pub round: u32,
    /// The most rounds of rung 1; 0 turns the rung off.
    pub max_rounds: u32,
}

impl Ladder {
    /// Rung 1 off: the sequence starts at rung 2, as it did before the ladder.
    pub fn off() -> Ladder {
        Ladder { round: 1, max_rounds: 0 }
    }
}

/// The failing tests of a triage document that rung 1 can run, by job, and what was left to the next rung.
#[derive(Debug, PartialEq, Eq)]
pub struct Picked {
    /// Job display name to the ids of its failing tests.
    pub tests: BTreeMap<String, Vec<String>>,
    /// Why the rest was left out, one line each (a whole-job failure, an unread row, a job with no test list, a failure that is
    /// already on main).
    pub left: Vec<String>,
}

/// Picks the rows of `ci triage --json` that rung 1 runs: a NEW, FAILED or FLAKY row that names one test of a runner and sits in a job
/// whose runner takes a test list (`kind_of` says which). A failure already on main is left to the full run (it is not this
/// train's); a whole-job failure, an unread row and a job with no test list rerun whole in rung 2.
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
            (_, _, "already-failing") => Some("failing on main already"),
            (_, _, "unknown") => Some("not known (UNKNOWN)"),
            (_, _, "new" | "failed" | "flaky") => None,
            _ => Some("a status this ladder does not know"),
        };
        if let Some(r) = reason {
            out.left.push(format!("{job}: {label}: {r}"));
            continue;
        }
        let id = test.unwrap_or_default();
        match (kind_of(job), id_kind(id)) {
            (Some(want), Some(have)) if want == have => out.tests.entry(job.to_string()).or_default().push(id.to_string()),
            (None, _) => out.left.push(format!("{job}: {label}: the job's runner takes no test list")),
            _ => out.left.push(format!("{job}: {label}: not a test id its job's runner takes")),
        }
    }
    for ids in out.tests.values_mut() {
        ids.sort();
        ids.dedup();
    }
    out
}

/// Whether the counts of `ci triage --ran` show the named tests ran: every named job is in the run and ran at least as many tests as
/// it was given ids (a Playwright location can run in two projects, never in fewer than one test per id). `Err` is NOT MEASURED. The
/// warnings name a job that was not named and ran tests anyway (the input was not honoured there).
pub fn check_ran(only: &OnlyTests, doc: &Value) -> Result<Vec<String>, String> {
    let jobs = doc
        .get("ran")
        .and_then(|r| r.get("jobs"))
        .and_then(Value::as_array)
        .ok_or_else(|| match doc.get("ran").and_then(|r| r.get("error")).and_then(Value::as_str) {
            Some(e) => format!("the tests the jobs ran could not be counted ({e})"),
            None => "the triage has no `ran` counts: the named tests cannot be shown to have run".to_string(),
        })?;
    let count_of = |name: &str| {
        jobs.iter()
            .find(|j| j.get("job").and_then(Value::as_str) == Some(name))
            .and_then(|j| j.get("tests_run").and_then(Value::as_u64))
    };
    for (job, ids) in only.jobs() {
        match count_of(job) {
            None => return Err(format!("job {job:?} is not in the run: its {} named test(s) did not run", ids.len())),
            Some(0) => return Err(format!("job {job:?} ran no test: its filter for {} named test(s) matched nothing", ids.len())),
            Some(n) if (n as usize) < ids.len() => {
                return Err(format!("job {job:?} ran {n} test(s) but {} were named: some did not run", ids.len()));
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

/// What rung 1 came to, for [`Seq::ladder`].
enum Tests {
    /// Green, with the run.
    Green(u64),
    /// Not run, and why.
    Skipped(String),
}

impl Seq<'_> {
    /// Rungs 1 to 3; rung 4 is [`Seq::full`], which the caller runs next.
    pub(super) fn ladder(&mut self) -> Result<(), Stop> {
        if let super::Round::Dispatch(since) = self.plan.round {
            match self.rung_tests(since)? {
                Tests::Green(run) => {
                    self.rungs.push((Rung::Tests, RungOutcome::Green(run)));
                }
                Tests::Skipped(why) => {
                    let _ = writeln!(self.log, "train: rung 1 ({}): skipped: {why}; rung 2 starts", Rung::Tests.name());
                    self.rungs.push((Rung::Tests, RungOutcome::Skipped(why)));
                }
            }
        } else {
            let why = "the fix round already ran (passed:<run>): it is judged now".to_string();
            self.rungs.push((Rung::Tests, RungOutcome::Skipped(why)));
        }
        self.fix_round()?;
        let jobs = self.steps.last().map(|(_, s)| s.run());
        if let Some(run) = jobs {
            let _ = writeln!(self.log, "train: rung 2 ({}): green, run {run}", Rung::Jobs.name());
            self.rungs.push((Rung::Jobs, RungOutcome::Green(run)));
        }
        self.rung_stage1()
    }

    /// Rung 1: the failed tests of `since` only, in the jobs where they failed.
    fn rung_tests(&mut self, since: u64) -> Result<Tests, Stop> {
        let wf = self.plan.workflow.file();
        let ladder = self.plan.ladder;
        if ladder.max_rounds == 0 {
            return Ok(Tests::Skipped("turned off (--max-test-rounds 0)".into()));
        }
        if ladder.round > ladder.max_rounds {
            return Ok(Tests::Skipped(format!(
                "round {} is past the bound of {} rounds (--max-test-rounds)",
                ladder.round, ladder.max_rounds
            )));
        }
        // The same checks of the red run as rung 2 makes, before anything is dispatched.
        self.since_facts(since)?;
        let doc = self.ports.triage(since).map_err(|e| {
            Stop::new(
                format!("rung 1: the triage of the red run {since} failed: {e}"),
                format!("run `./dev ci triage {since}` by hand and read why"),
            )
        })?;
        let ports = &mut *self.ports;
        let picked = pick(&doc, &mut |job| ports.job_kind(wf, job));
        for l in &picked.left {
            let _ = writeln!(self.log, "train: rung 1: left to rung 2: {l}");
        }
        if picked.tests.is_empty() {
            return Ok(Tests::Skipped(format!("run {since} has no named failing test that a job can run alone")));
        }
        let only = OnlyTests::new(picked.tests);
        if only.count() > MAX_IDS {
            return Ok(Tests::Skipped(format!(
                "{} failing tests are more than the {MAX_IDS} one only_tests input carries",
                only.count()
            )));
        }
        let t0 = self.ports.elapsed();
        let _ = writeln!(
            self.log,
            "train: rung 1 ({}), round {}/{}: {} test(s) in {} job(s) of run {since}, on {}",
            Rung::Tests.name(),
            ladder.round,
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
        if let super::RunEnd::Unfinished(why) = end {
            return Err(super::unfinished(wf, run, &why));
        }
        let took = |s: &Self| fmt_duration(s.ports.elapsed().saturating_sub(t0));
        let again = format!(
            "fix the failing tests in their owners' branches, push through the gate, and run the sequence again with --since-run {since} --test-round {}",
            ladder.round + 1
        );
        let doc = self.ports.triage_ran(run).map_err(|e| {
            Stop::new(
                format!("rung 1 run {run}: its triage with the test counts failed: {e}"),
                format!("run `./dev ci triage {run} --ran` by hand; a rung that cannot show its tests ran is not measured"),
            )
        })?;
        match self.result_of(run)? {
            RunResult::Success => {}
            RunResult::Failure => {
                if let Err(reasons) = verdict(&doc, wf, &self.plan.accepted) {
                    return Err(Stop::new(
                        format!("rung 1 run {run} is red: {}", reasons.join("; ")),
                        again,
                    ));
                }
                let _ = writeln!(self.log, "train: rung 1: run {run}: only accepted FLAKY tests failed");
            }
            other => return Err(super::not_a_result(wf, run, &other)),
        }
        match check_ran(&only, &doc) {
            Ok(warnings) => {
                for w in warnings {
                    let _ = writeln!(self.log, "::warning::train: rung 1 run {run}: {w}");
                }
            }
            Err(why) => {
                return Err(Stop::new(
                    format!("rung 1 run {run} is NOT MEASURED: {why}"),
                    format!("read the run's plan job log (the only_tests input is honoured by ./dev plan --ci); a green run of no test is not green. {again}"),
                ));
            }
        }
        let _ = writeln!(
            self.log,
            "train: rung 1 ({}): green, run {run}: {} named test(s) ran, after {}",
            Rung::Tests.name(),
            only.count(),
            took(self)
        );
        self.steps.push((wf.to_string(), super::Source::TestsOnly(run)));
        Ok(Tests::Green(run))
    }

    /// Rung 3: stage 1 on every OS, when the workflow has that layout; otherwise a logged fall back to rung 4.
    fn rung_stage1(&mut self) -> Result<(), Stop> {
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
                return Ok(());
            }
            Ok(Some(i)) => i,
        };
        let mut all: Vec<(&str, &str)> = inputs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        all.push(self.plan.workflow.input());
        let t0 = self.ports.elapsed();
        let _ = writeln!(self.log, "train: rung 3 ({}): {wf} on {} with {inputs:?}", Rung::Stage1.name(), short(&sha));
        let (run, end) = self.dispatch_and_wait(wf, &all, None)?;
        if let super::RunEnd::Unfinished(why) = end {
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
                format!("the shared reader counts the stage 1 run {run} of {wf} as a full green run of {}", short(&sha)),
                "fix the plan job so that a stage 1 run never prints `plan verdict: complete=true` (plan_since::COMPLETE_LINE); no rung but 4 lets a train land",
            ));
        }
        let doc = self.ports.triage_ran(run).map_err(|e| {
            Stop::new(
                format!("rung 3 run {run}: the tests it ran could not be counted: {e}"),
                format!("run `./dev ci triage {run} --ran` by hand; a rung that cannot show its tests ran is not measured"),
            )
        })?;
        let total = doc.get("ran").and_then(|r| r.get("total")).and_then(Value::as_u64);
        match total {
            Some(n) if n > 0 => {}
            _ => {
                return Err(Stop::new(
                    format!("rung 3 run {run} is NOT MEASURED: it shows no test that ran (stage 1 runs the unit tests)"),
                    "read the run: a stage 1 that ran nothing is not green; then run the sequence again",
                ));
            }
        }
        let _ = writeln!(
            self.log,
            "train: rung 3 ({}): green, run {run}, {} test(s) ran, after {}",
            Rung::Stage1.name(),
            total.unwrap_or(0),
            fmt_duration(self.ports.elapsed().saturating_sub(t0))
        );
        self.steps.push((wf.to_string(), super::Source::Stage1(run)));
        self.rungs.push((Rung::Stage1, RungOutcome::Green(run)));
        Ok(())
    }
}
