// ci-runs-reader: train release: finds the run it dispatched and waits for its end; a red round is judged by ci triage, never by its conclusion alone
//! The release sequence of a train (the port of the integrator's `seq-release.sh`): an optional failed-only fix round of
//! `ci.yml` or `debrief.yml`, judged by [`super::verdict`]; after a `ci.yml` round `debrief.yml` with bundles; then the release
//! workflows. It stops at the first run that is not green, and says why in one [`Stop`].
//!
//! Everything it touches goes through [`Ports`]: the real ones are in `real.rs`, the tests use a table. Every wait is bounded:
//! the whole sequence by `--max`, one run by [`RUN_CAP`] (ER-32). Each step logs its phase and how long it took (ER-27).

use super::verdict::{Accepted, verdict};
use crate::ci_runs::client::RunInfo;
use crate::wait::fmt_duration;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::time::Duration;

/// The longest one run is waited for (the script's 6 h; the slowest full run is well inside it).
pub const RUN_CAP: Duration = Duration::from_secs(6 * 3600);

/// The release workflows, in order, with their inputs. The four apps run without their gates (the train's own CI ran them).
pub const RELEASE_WORKFLOWS: [(&str, &[(&str, &str)]); 6] = [
    ("dyno-release.yml", &[("gates", "false")]),
    ("pitcrew-release.yml", &[("gates", "false")]),
    ("pitwall-release.yml", &[("gates", "false")]),
    ("spotter-release.yml", &[("gates", "false")]),
    ("shell-demo-release.yml", &[]),
    ("workbench-demo-release.yml", &[]),
];

/// The workflow of the failed-only fix round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixWorkflow {
    /// `ci.yml` (with WebKit); `debrief.yml` with bundles follows.
    Ci,
    /// `debrief.yml` (with bundles); nothing follows it but the releases.
    Debrief,
}

impl FixWorkflow {
    /// Parses the `--workflow` value.
    pub fn parse(text: &str) -> Result<FixWorkflow, String> {
        match text {
            "ci.yml" => Ok(FixWorkflow::Ci),
            "debrief.yml" => Ok(FixWorkflow::Debrief),
            other => Err(format!("--workflow {other} is not one of ci.yml, debrief.yml")),
        }
    }

    /// The workflow file.
    pub fn file(self) -> &'static str {
        match self {
            FixWorkflow::Ci => "ci.yml",
            FixWorkflow::Debrief => "debrief.yml",
        }
    }

    fn input(self) -> (&'static str, &'static str) {
        match self {
            FixWorkflow::Ci => ("webkit", "true"),
            FixWorkflow::Debrief => ("bundles", "true"),
        }
    }
}

/// Where the fix round starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Round {
    /// Dispatch the workflow with `since_run=<id>`: only the failed jobs of that run again.
    Dispatch(u64),
    /// That round already ran as this run: it is not dispatched again, but it is judged now, like a run just finished.
    Passed(u64),
}

impl Round {
    /// Parses `--since-run`: `8300`, or `passed:8307`.
    pub fn parse(text: &str) -> Result<Round, String> {
        let bad = || format!("--since-run {text:?} is not a run id or passed:<run id>");
        match text.strip_prefix("passed:") {
            Some(id) => id.parse().map(Round::Passed).map_err(|_| bad()),
            None => text.parse().map(Round::Dispatch).map_err(|_| bad()),
        }
    }
}

/// What to run.
#[derive(Debug)]
pub struct Plan {
    /// The train branch (`train/2026-11-19-16`): the ref every run is dispatched on.
    pub branch: String,
    /// Its head: what every run must have tested.
    pub sha: String,
    /// The workflow of the fix round.
    pub workflow: FixWorkflow,
    /// Where the fix round starts.
    pub round: Round,
    /// The flaky tests that do not stop the round.
    pub accepted: Vec<Accepted>,
    /// A green run to use instead of dispatching a release workflow again (the tree is unchanged for that app).
    pub reuse: BTreeMap<String, u64>,
    /// The longest the whole sequence takes.
    pub max: Duration,
}

impl Plan {
    /// Refuses a `--reuse` of a workflow that is not a release workflow (a typo would be reused by nothing, silently).
    pub fn check(&self) -> Result<(), String> {
        for wf in self.reuse.keys() {
            if !RELEASE_WORKFLOWS.iter().any(|(w, _)| w == wf) {
                let names: Vec<&str> = RELEASE_WORKFLOWS.iter().map(|(w, _)| *w).collect();
                return Err(format!("--reuse {wf}: not a release workflow ({})", names.join(", ")));
            }
        }
        Ok(())
    }
}

/// How a waited-for run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEnd {
    /// Finished with success.
    Green,
    /// Finished and did not succeed.
    Red,
    /// Not finished (the bound, or the run list could not be read): nothing is known about the run.
    Unfinished(String),
}

/// What the sequence needs from the outside.
pub trait Ports {
    /// How long the sequence has run.
    fn elapsed(&self) -> Duration;
    /// The disk check before a dispatch; `Err` is the cause, for the one `::error::`.
    fn disk_check(&mut self) -> Result<(), String>;
    /// Dispatches `workflow` on the train branch (waiting at most `wait` for a busy runner) and returns the new run's id.
    fn dispatch(&mut self, workflow: &str, inputs: &[(String, String)], wait: Duration) -> Result<u64, String>;
    /// Waits, at most `max`, for the run to finish.
    fn wait_run(&mut self, run: u64, max: Duration) -> RunEnd;
    /// One run by id (`None`: the server has none).
    fn run_info(&mut self, run: u64) -> Result<Option<RunInfo>, String>;
    /// `ci triage <run>` as the JSON `ci triage --json` prints, `ok` included. `Err` when it could not be made.
    fn triage(&mut self, run: u64) -> Result<Value, String>;
}

/// Why the sequence stopped, and what to do next.
#[derive(Debug, PartialEq, Eq)]
pub struct Stop {
    /// The cause.
    pub cause: String,
    /// The next step.
    pub next: String,
}

impl Stop {
    pub(super) fn new(cause: impl Into<String>, next: impl Into<String>) -> Stop {
        Stop {
            cause: cause.into(),
            next: next.into(),
        }
    }

    /// The one annotation line.
    pub fn annotation(&self) -> String {
        format!(
            "::error::train release: {}. Next step: {}.",
            self.cause.trim_end_matches('.'),
            self.next
        )
    }
}

/// How each workflow of a green sequence was covered.
#[derive(Debug, PartialEq, Eq)]
pub enum Source {
    /// Dispatched and finished green, as this run.
    Ran(u64),
    /// A run that was judged and passed (the fix round of `passed:<run>`, or accepted flaky tests only).
    Judged(u64),
    /// A green run of an unchanged tree, used instead of a dispatch.
    Reused(u64),
}

/// A green sequence: the workflows in order and what covered each.
#[derive(Debug, PartialEq, Eq)]
pub struct Done {
    /// `(workflow, source)` in order.
    pub steps: Vec<(String, Source)>,
}

struct Seq<'a> {
    plan: &'a Plan,
    ports: &'a mut dyn Ports,
    log: &'a mut dyn Write,
    steps: Vec<(String, Source)>,
}

/// Runs the sequence; stops at the first run that is not green.
pub fn release(plan: &Plan, ports: &mut dyn Ports, log: &mut dyn Write) -> Result<Done, Stop> {
    plan.check().map_err(|e| Stop::new(e, "fix the arguments and run it again"))?;
    let mut s = Seq {
        plan,
        ports,
        log,
        steps: Vec::new(),
    };
    s.fix_round()?;
    if plan.workflow == FixWorkflow::Ci {
        s.green("debrief.yml", &[("bundles", "true")])?;
    }
    for (wf, inputs) in RELEASE_WORKFLOWS {
        match plan.reuse.get(wf) {
            Some(&run) => s.reuse(wf, run)?,
            None => s.green(wf, inputs)?,
        }
    }
    let _ = writeln!(
        s.log,
        "train: release sequence green on {} after {}",
        short(&plan.sha),
        fmt_duration(s.ports.elapsed())
    );
    Ok(Done { steps: s.steps })
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(9)]
}

impl Seq<'_> {
    fn remaining(&self) -> Duration {
        self.plan.max.saturating_sub(self.ports.elapsed())
    }

    /// Dispatches a workflow and waits for it; the run id and how it ended.
    fn dispatch_and_wait(&mut self, wf: &str, inputs: &[(&str, &str)], extra: Option<u64>) -> Result<(u64, RunEnd), Stop> {
        let left = self.remaining();
        if left.is_zero() {
            return Err(Stop::new(
                format!("--max {} is used up before {wf} was dispatched", fmt_duration(self.plan.max)),
                "run it again with a larger --max (the greens so far are logged above; use --reuse for release runs that are green)",
            ));
        }
        if let Err(why) = self.ports.disk_check() {
            return Err(Stop::new(
                format!("the disk check refused the dispatch of {wf}: {why}"),
                "free disk (./dev disk report, then ./dev disk clean --dry-run), then run it again",
            ));
        }
        let mut all: Vec<(String, String)> = inputs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        if let Some(since) = extra {
            all.push(("since_run".into(), since.to_string()));
        }
        let t0 = self.ports.elapsed();
        let run = self.ports.dispatch(wf, &all, left).map_err(|e| {
            Stop::new(
                format!("{wf} was not dispatched: {e}"),
                "check GITEA_TOKEN, the workflow name and the runners, then run it again",
            )
        })?;
        let _ = writeln!(
            self.log,
            "train: {wf}: run {run} dispatched on {} (inputs {all:?})",
            short(&self.plan.sha)
        );
        let cap = self.remaining().min(RUN_CAP);
        let end = self.ports.wait_run(run, cap);
        let _ = writeln!(
            self.log,
            "train: {wf}: run {run} {:?} after {}",
            end,
            fmt_duration(self.ports.elapsed().saturating_sub(t0))
        );
        Ok((run, end))
    }

    /// A workflow that must finish green.
    fn green(&mut self, wf: &str, inputs: &[(&str, &str)]) -> Result<(), Stop> {
        let (run, end) = self.dispatch_and_wait(wf, inputs, None)?;
        match end {
            RunEnd::Green => {
                self.steps.push((wf.to_string(), Source::Ran(run)));
                Ok(())
            }
            RunEnd::Red => Err(Stop::new(
                format!("{wf} run {run} is red"),
                format!("read it with `./dev ci triage {run}`, fix the cause in its owner's branch, then run the sequence again"),
            )),
            RunEnd::Unfinished(why) => Err(unfinished(wf, run, &why)),
        }
    }

    /// A green run of an unchanged tree, used again: it must exist, be a run of that workflow and have succeeded.
    fn reuse(&mut self, wf: &str, run: u64) -> Result<(), Stop> {
        let bad = |why: String| {
            Stop::new(
                format!("--reuse {wf}={run}: {why}"),
                "name a green run of that workflow, or leave --reuse out to dispatch it",
            )
        };
        let info = self
            .ports
            .run_info(run)
            .map_err(|e| bad(format!("the run could not be read ({e})")))?
            .ok_or_else(|| bad("the server has no such run".into()))?;
        if info.workflow() != wf {
            return Err(bad(format!("it is a run of {}", info.workflow())));
        }
        if !info.finished() || info.conclusion != "success" {
            return Err(bad(format!(
                "it is {} ({})",
                info.status,
                if info.conclusion.is_empty() {
                    "no conclusion"
                } else {
                    &info.conclusion
                }
            )));
        }
        let _ = writeln!(self.log, "train: {wf}: reusing green run {run} (tree unchanged for its app)");
        self.steps.push((wf.to_string(), Source::Reused(run)));
        Ok(())
    }

    /// The fix round: green, or red with only accepted flaky tests.
    fn fix_round(&mut self) -> Result<(), Stop> {
        let wf = self.plan.workflow.file();
        let run = match self.plan.round {
            Round::Dispatch(since) => {
                let (run, end) = self.dispatch_and_wait(wf, &[self.plan.workflow.input()], Some(since))?;
                match end {
                    RunEnd::Green => {
                        self.steps.push((wf.to_string(), Source::Ran(run)));
                        return Ok(());
                    }
                    RunEnd::Red => run,
                    RunEnd::Unfinished(why) => return Err(unfinished(wf, run, &why)),
                }
            }
            Round::Passed(run) => {
                let info = self
                    .ports
                    .run_info(run)
                    .map_err(|e| {
                        Stop::new(
                            format!("passed:{run}: the run could not be read ({e})"),
                            "check the network and GITEA_TOKEN",
                        )
                    })?
                    .ok_or_else(|| Stop::new(format!("passed:{run}: the server has no such run"), "name the run of the fix round"))?;
                let wrong = |why: String| {
                    Stop::new(
                        format!("passed:{run}: {why}"),
                        "name the finished fix round of this train head, or give --since-run <failed run> to dispatch it",
                    )
                };
                if info.workflow() != wf {
                    return Err(wrong(format!("it is a run of {}, not of {wf}", info.workflow())));
                }
                if info.head_sha != self.plan.sha {
                    return Err(wrong(format!(
                        "it tested {}, the train head is {}",
                        short(&info.head_sha),
                        short(&self.plan.sha)
                    )));
                }
                if !info.finished() {
                    return Err(wrong(format!("it is still {}", info.status)));
                }
                let _ = writeln!(
                    self.log,
                    "train: {wf}: fix round already ran as run {run}; judged now, not dispatched again"
                );
                if info.conclusion == "success" {
                    self.steps.push((wf.to_string(), Source::Judged(run)));
                    return Ok(());
                }
                run
            }
        };
        self.judge(wf, run)
    }

    /// The verdict of a red run, from its triage (only [`verdict`] can pass it).
    fn judge(&mut self, wf: &str, run: u64) -> Result<(), Stop> {
        let t0 = self.ports.elapsed();
        let doc = self.ports.triage(run).map_err(|e| {
            Stop::new(
                format!("{wf} run {run} is red and its triage failed: {e}"),
                format!("run `./dev ci triage {run}` by hand and read why"),
            )
        })?;
        let result = verdict(&doc, &self.plan.accepted);
        let took = fmt_duration(self.ports.elapsed().saturating_sub(t0));
        match result {
            Ok(pass) => {
                let _ = writeln!(
                    self.log,
                    "train: {wf}: run {run}: PASS: {} failing row(s), each an accepted FLAKY test (triage took {took})",
                    pass.rows
                );
                self.steps.push((wf.to_string(), Source::Judged(run)));
                Ok(())
            }
            Err(reasons) => Err(Stop::new(
                format!("{wf} run {run} is red and not accepted: {}", reasons.join("; ")),
                format!(
                    "fix the new failures in their owners' branches, or accept a flaky test with --accept-flaky after its fix item is recorded (ER-12); `./dev ci triage {run}` has the details"
                ),
            )),
        }
    }
}

fn unfinished(wf: &str, run: u64, why: &str) -> Stop {
    Stop::new(
        format!("{wf} run {run} did not finish: {why}"),
        format!(
            "`./dev wait --ci-run {run} --max <duration>` when it is still running, then run the sequence again with --since-run passed:{run} or the failed run"
        ),
    )
}

#[cfg(test)]
mod tests;
