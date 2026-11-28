// ci-runs-reader: ci dispatch: lists unfinished runs on a shared runner class before dispatching; no verdict
//! `ci dispatch <workflow> --ref <ref>`: start a workflow only when no run on the same single-runner class is going.
//!
//! The Mac (and Windows) runner is one machine: two runs on it at once make each other's timing-sensitive tests time out (CI runs
//! 7960 with 7961, and 7969-7971 with 7967, ER-36). So before it dispatches, the command lists the Gitea runs that are not
//! finished, reads each one's job list (status and runner labels) and waits while one of them has an unfinished job on a class the
//! workflow to start uses (user decision 2026-11-15, D-1: a run whose Mac jobs are done no longer holds the Mac runner while its
//! Linux jobs run; a run without a job list is judged by the runner classes of its workflow file). The wait is like
//! `dev wait --ci-run`, bounded by the required `--max`. Linux jobs run on container runners that may overlap, so a Linux-only
//! workflow dispatches at once.
//!
//! A class is read from the jobs' `runs-on` labels (a matrix expression stands for the job's matrix values), following job-level
//! `uses: ./.gitea/workflows/<file>` calls; other text of the file (steps, `runner.os` tests) does not count. The token is read
//! from `GITEA_TOKEN`, travels in a header only and is printed nowhere.
//!
//! **A train waits in its own run** (2026-11-15, the review's common-practice option): a waiting dispatcher had nothing queued, so
//! `ci schedule-skip` did not see the train and each new scheduled run took the runner first (2026-11-14 11:0x, 2026-11-15 05:03
//! to 07:07 behind runs 8112 to 8114). A `train/*` dispatch of a workflow whose first job runs the `runner-wait` action is sent at
//! once: the queued run is what schedule-skip already yields to, and its first job ([`runner_wait`], on a Linux runner) waits
//! until no OLDER run has an unfinished job (queued, waiting or running) on its single-machine class; an older run
//! whose Mac and Windows jobs are done does not hold it, whatever Linux jobs it still runs. Older only: two runs of one train never wait for each other
//! (the oldest goes first), so there is no deadlock. Any other dispatch waits here as before.
//!
//! **Trials yield to trains** (2026-11-27, the "trains first" rule): a dispatch that is not a train's (a trial, a member, a manual run)
//! first waits while a `train/*` run of the same workflow is queued or running ([`TrainFirstProbe`]), bounded by `--max`, whatever
//! runner classes the workflow uses: trial 9247 took all six Linux slots of d-gtr9 while train run 9248 queued. A train never waits
//! for a trial.

use super::client::{Client, RunInfo};
use crate::health::{Health, Ready, wait_held};
use crate::http::Http;
use crate::wait::{Clock, MAX_LIMIT, MIN_INTERVAL, Outcome, Poll, Probe, SystemClock, fmt_duration, parse_duration, wait_for};
use paddock_ci_config::jobs::{MatrixValues, RunnerJob, runner_jobs, triggers};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::Write;
use std::time::Duration;

/// A runner class with a single machine behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RunnerClass {
    /// The Mac runner.
    Macos,
    /// The Windows runner.
    Windows,
}

impl RunnerClass {
    fn name(self) -> &'static str {
        match self {
            RunnerClass::Macos => "macOS",
            RunnerClass::Windows => "Windows",
        }
    }
}

/// Reads a workflow file of the repository by its file name (`dyno.yml`); `None` when it is not there.
pub type ReadWorkflow<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The single-machine classes `workflow` can run on, including the workflows it calls. They come from each job's `runs-on`
/// labels as the ADR 0123 job model reads them (a `${{ matrix.x }}` label stands for every value of the job's matrix; a planned
/// matrix, written by the planner or a caller at run time, stands for every runner the planner can emit, [`crate::plan_ci::OS_LEGS`]: fail closed) and
/// never from other text of the file: a step that mentions macOS does not make a Linux job a Mac job. `Err` when the file cannot
/// be read or the model cannot read it.
pub fn classes_of(workflow: &str, read: ReadWorkflow<'_>) -> Result<BTreeSet<RunnerClass>, String> {
    let mut classes = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut todo = vec![workflow.to_string()];
    while let Some(name) = todo.pop() {
        if !seen.insert(name.clone()) || seen.len() > 20 {
            continue;
        }
        let Some(text) = read(&name) else {
            if name == workflow {
                return Err(format!("workflow file .gitea/workflows/{workflow} is not in this checkout"));
            }
            continue;
        };
        let jobs = runner_jobs(&text).map_err(|problems| {
            let why: Vec<String> = problems.iter().map(|p| p.message.clone()).collect();
            format!("workflow file {name} is not read by the CI job model: {}", why.join("; "))
        })?;
        for job in jobs {
            if let Some(called) = job.uses.as_deref().and_then(called_workflow) {
                todo.push(called);
            }
            for label in &job.runs_on {
                if !label.contains("${{") {
                    classify_all(std::slice::from_ref(label), &mut classes);
                    continue;
                }
                match &job.matrix {
                    MatrixValues::None => {}
                    MatrixValues::Literal(values) => classify_all(values, &mut classes),
                    MatrixValues::Dynamic => classify_all(&crate::plan_ci::os_legs(), &mut classes),
                }
            }
        }
    }
    Ok(classes)
}

fn classify_all(labels: &[String], classes: &mut BTreeSet<RunnerClass>) {
    for l in labels {
        let l = l.to_ascii_lowercase();
        if l.contains("macos") {
            classes.insert(RunnerClass::Macos);
        }
        if l.contains("windows") {
            classes.insert(RunnerClass::Windows);
        }
    }
}

/// Whether a job label names a known runner class: macOS, Windows, or a Linux one (`ubuntu-*`, `linux`). An expression, an
/// empty label or another name is unknown.
fn known_label(l: &str) -> bool {
    let l = l.to_ascii_lowercase();
    !l.contains("${{") && (l.contains("macos") || l.contains("windows") || l.contains("ubuntu") || l.contains("linux"))
}

/// The local workflow a job's `uses: ./.gitea/workflows/<file>` calls.
fn called_workflow(uses: &str) -> Option<String> {
    let rest = uses.trim().strip_prefix("./.gitea/workflows/")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        .collect();
    (!name.is_empty()).then_some(name)
}

/// Waits while a run that shares a class with the workflow to start is not finished.
struct BusyProbe<'a, 'b> {
    client: &'a mut Client<'b>,
    read: ReadWorkflow<'a>,
    wanted: BTreeSet<RunnerClass>,
    /// Only runs older than this one count (a run waiting for its own runner, [`runner_wait`]); `None`: every run.
    older_than: Option<u64>,
    /// The waiting run's train ([`train_identity`]): runs of the same train (same `train/*` branch and head commit) are one
    /// reservation and never hold each other up; every other run still does. `None`: not a train run.
    own_train: Option<(String, String)>,
    /// The workflow file this probe waits for: another run of the same workflow at the same head is a duplicate, not a sibling.
    own_workflow: String,
    /// The same-train runs the last look ignored (ER-27: the log says whom this run shares the reservation with).
    joined: Vec<String>,
    /// A dispatch of this train branch: its head commit is read again on every look and right before sending (rule 53: an
    /// identity read earlier is verified before it is acted on), so a train that moved is not mistaken for its old head.
    train_branch: Option<String>,
    /// Why the dispatch waits for every run instead of sharing its train's reservation (the branch head could not be read).
    strict_why: Option<String>,
}

/// How many pages of 50 runs are read at most for the unfinished ones (the newest first).
const BUSY_PAGES: u32 = 4;

/// The unfinished runs, newest first: pages until a short page, a page whose newest run is older than a day (a run the
/// server never finished is not taken as busy for longer, as in schedule-skip), or [`BUSY_PAGES`].
fn unfinished_runs(client: &mut Client<'_>) -> Result<Vec<RunInfo>, String> {
    let now_ms = super::time::now_secs() as f64 * 1000.0;
    let mut out = Vec::new();
    for page in 1..=BUSY_PAGES {
        let p = client.runs_page(page)?;
        let n = p.runs.len();
        let old = p
            .runs
            .first()
            .and_then(|r| super::time::ts(&r.created_at))
            .is_some_and(|c| now_ms - c > super::schedule_skip::STALE_MS);
        out.extend(p.runs.into_iter().filter(|r| !r.finished()));
        if n < 50 || old {
            break;
        }
    }
    Ok(out)
}

/// Waits while a `train/*` run of the same workflow is queued or running: a trial, a member or a manual run yields to the train
/// (2026-11-27, the "trains first" rule: trial 9247 held d-gtr9's six slots and two of the Windows runner's three while train run 9248
/// queued, 117 s or more on its critical path). Only a non-train dispatch asks; a train never waits for a trial.
struct TrainFirstProbe<'a, 'b> {
    client: &'a mut Client<'b>,
    workflow: String,
}

impl Probe for TrainFirstProbe<'_, '_> {
    fn describe(&self) -> String {
        format!("the train run of {} to finish", self.workflow)
    }
    fn poll(&mut self) -> Result<Poll, String> {
        let runs = unfinished_runs(self.client)?;
        Ok(match runs.iter().find(|r| is_train(r.git_ref()) && r.workflow() == self.workflow) {
            None => Poll::Done(format!("no train run of {} is queued or running", self.workflow)),
            Some(r) => Poll::Pending(format!(
                "train run {} ({}, {}) on {} goes first",
                r.id,
                r.workflow(),
                r.status,
                r.git_ref().strip_prefix("refs/heads/").unwrap_or(r.git_ref())
            )),
        })
    }
    fn next_step(&self) -> String {
        "wait for that train run (`./dev wait --ci-run <id> --max <duration>`), then dispatch again; the train always goes first".into()
    }
}

impl BusyProbe<'_, '_> {
    /// The unfinished runs, newest first ([`unfinished_runs`]).
    fn unfinished(&mut self) -> Result<Vec<RunInfo>, String> {
        unfinished_runs(self.client)
    }

    /// Reads the branch head again (dispatch of a train branch only) and sets the train identity from it; unreadable: none
    /// (strict: every run counts) and the reason is kept for the log.
    fn refresh_identity(&mut self) {
        let Some(branch) = self.train_branch.clone() else { return };
        match self.client.branch_head(&branch) {
            Ok(Some(sha)) => {
                self.own_train = Some((branch, sha));
                self.strict_why = None;
            }
            Ok(None) => {
                self.own_train = None;
                self.strict_why = Some(format!("branch {branch} not found (HTTP 404)"));
            }
            Err(e) => {
                self.own_train = None;
                self.strict_why = Some(format!("branch {branch} unreadable: {e}"));
            }
        }
    }

    /// Whether the train identity changed since the last look: read right before the dispatch is sent.
    fn head_moved(&mut self) -> bool {
        let before = self.own_train.clone();
        self.refresh_identity();
        before != self.own_train
    }

    /// The unfinished runs that share a class, as text, or `None` when there are none.
    fn busy(&mut self) -> Result<Option<String>, String> {
        let runs = self.unfinished()?;
        let older_than = self.older_than;
        let own_train = self.own_train.clone();
        self.joined.clear();
        for run in runs.iter().filter(|r| older_than.is_none_or(|own| r.id < own)) {
            if own_train.is_some() && train_identity(run) == own_train && run.workflow() != self.own_workflow {
                self.joined.push(format!("{} ({})", run.id, run.workflow()));
                continue;
            }
            if let Some(why) = self.job_clash(run)? {
                return Ok(Some(why));
            }
        }
        Ok(None)
    }

    /// A run blocks only while one of its jobs on a shared class is not finished (queued, waiting or running), never for its
    /// Linux jobs: for every dispatch (user decision 2026-11-15, D-1) and for a waiting train run. A run whose job list is
    /// missing (404) is judged by its workflow file.
    fn job_clash(&mut self, run: &RunInfo) -> Result<Option<String>, String> {
        let Some(jobs) = self.client.timed_jobs(run.id)? else {
            return Ok(self.clash(run));
        };
        for j in jobs.iter().filter(|j| j.status != "completed") {
            // In a busy check, unknown counts as busy: a label not resolved yet (`${{ matrix.os }}` before the plan ran), no
            // label (a reusable-workflow caller) or one of no known class judges the run by its workflow file, as before D-1.
            if !j.labels.iter().all(|l| known_label(l)) || j.labels.is_empty() {
                if let Some(why) = self.clash(run) {
                    return Ok(Some(format!(
                        "{why} (job {} has the label {:?}, read by the workflow file)",
                        j.name, j.labels
                    )));
                }
                continue;
            }
            let mut classes = BTreeSet::new();
            classify_all(&j.labels, &mut classes);
            let shared: Vec<&str> = classes.intersection(&self.wanted).map(|c| c.name()).collect();
            if !shared.is_empty() {
                return Ok(Some(format!(
                    "run {} ({}) has job {} ({}) on {}",
                    run.id,
                    run.workflow(),
                    j.name,
                    if j.status.is_empty() { "unfinished" } else { &j.status },
                    shared.join(" and ")
                )));
            }
        }
        Ok(None)
    }

    /// A workflow file that is not in this checkout cannot be classified and does not block.
    fn clash(&self, run: &RunInfo) -> Option<String> {
        match classes_of(run.workflow(), self.read) {
            Ok(theirs) => {
                let shared: Vec<&str> = theirs.intersection(&self.wanted).map(|c| c.name()).collect();
                (!shared.is_empty()).then(|| format!("run {} ({}, {}) is on {}", run.id, run.workflow(), run.status, shared.join(" and ")))
            }
            Err(_) => None,
        }
    }
}

impl Probe for BusyProbe<'_, '_> {
    fn describe(&self) -> String {
        let names: Vec<&str> = self.wanted.iter().map(|c| c.name()).collect();
        format!("the {} runner to be free of other runs", names.join(" and "))
    }
    fn poll(&mut self) -> Result<Poll, String> {
        self.refresh_identity();
        let busy = self.busy()?;
        let mut joint = if self.joined.is_empty() {
            String::new()
        } else {
            format!(
                "; joint reservation: not waiting for run(s) {} of the same train",
                self.joined.join(", ")
            )
        };
        if let Some(why) = &self.strict_why {
            joint.push_str(&format!("; waiting for every run, as the train's head commit is unknown ({why})"));
        }
        Ok(match busy {
            None => Poll::Done(format!("no other run is on the same runner{joint}")),
            Some(why) => Poll::Pending(format!("{why}{joint}")),
        })
    }
    fn next_step(&self) -> String {
        "wait for that run (`./dev wait --ci-run <id> --max <duration>`), or cancel it if it is stale, then dispatch again".into()
    }
}

/// What to dispatch.
#[derive(Debug)]
pub struct Request {
    /// The workflow file name, `dyno-release.yml`.
    pub workflow: String,
    /// The branch or tag to run it on.
    pub git_ref: String,
    /// `key=value` workflow inputs.
    pub inputs: Vec<(String, String)>,
    /// The longest to wait for a busy runner.
    pub max: Duration,
    /// How often to look.
    pub interval: Duration,
    /// At most this many train runs may wait for their runner at once (the Linux slots less one, [`train_waiters`]); 0 when
    /// unknown, and then a train dispatch waits here like any other.
    pub train_waiters: usize,
}

/// Parses `--max` and `--input k=v`.
pub fn parse_request(workflow: &str, git_ref: &str, inputs: &[String], max: &str, interval: Option<&str>) -> Result<Request, String> {
    let max = parse_duration(max)?;
    if max.is_zero() || max > MAX_LIMIT {
        return Err(format!("--max {} is out of range: use 1s to 24h.", fmt_duration(max)));
    }
    let interval = match interval {
        Some(i) => parse_duration(i)?,
        None => Duration::from_secs(15),
    }
    .max(MIN_INTERVAL);
    let inputs = inputs
        .iter()
        .map(|i| {
            i.split_once('=')
                .filter(|(k, _)| !k.is_empty())
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| format!("--input {i:?} is not key=value"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if workflow.contains('/') || workflow.is_empty() {
        return Err("the workflow is a file name in .gitea/workflows, like dyno-release.yml".into());
    }
    Ok(Request {
        workflow: workflow.to_string(),
        git_ref: git_ref.to_string(),
        inputs,
        max,
        interval,
        train_waiters: 0,
    })
}

/// How a dispatch ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Dispatched {
    /// Dispatched after waiting `waited` (zero when the runner was free at once).
    Yes {
        /// How long the runner was busy.
        waited: Duration,
    },
    /// The runner stayed busy for `--max`; nothing was dispatched.
    StillBusy {
        /// The run that held it at the end.
        last: String,
    },
    /// Gitea stayed down for `--max` ([`crate::health`]); nothing was dispatched.
    ServiceDown {
        /// What was down at the end, with the cause.
        last: String,
    },
}

/// The Gitea repository and the token to ask it with.
#[derive(Clone, Copy)]
pub struct Target<'a> {
    /// `https://dev.decryptology.net`.
    pub server: &'a str,
    /// `decryp7/paddock`.
    pub repo: &'a str,
    /// The API token; sent in a header only.
    pub token: &'a str,
}

/// Waits for a free runner class, then dispatches. Errors are texts for `::error::`.
pub fn run(
    http: &mut dyn Http,
    target: &Target<'_>,
    req: &Request,
    read: ReadWorkflow<'_>,
    clock: &mut dyn Clock,
    log: &mut dyn Write,
    mut health: Option<&mut Health<'_>>,
) -> Result<Dispatched, String> {
    let Target { server, repo, token } = *target;
    let wanted = classes_of(&req.workflow, read)?;
    let mut waited = Duration::ZERO;
    // Hold while Gitea is down: before anything is read or sent, and again inside each wait below. The hold shares the
    // clock, so it counts against the same `--max`.
    if health.is_some() {
        let held = wait_held(
            &mut Ready("the CI server to answer"),
            health.as_deref_mut(),
            clock,
            req.max,
            req.interval,
            log,
        );
        match held {
            Outcome::TimedOut { .. } => return Ok(service_down(&req.workflow, health.as_deref(), req.max, log)),
            // An address that can never work (a wrong --server): no hold will help.
            Outcome::Failed { detail, .. } => return Err(detail),
            _ => {}
        }
    }
    if !is_train(&req.git_ref) {
        match yield_to_trains(http, target, req, clock, log, health.as_deref_mut())? {
            Ok(yielded) => waited = yielded,
            Err(ended) => return Ok(ended),
        }
    }
    if wanted.is_empty() {
        let _ = writeln!(
            log,
            "dispatch: {} uses no single-machine runner (Linux only); not waiting",
            req.workflow
        );
    } else {
        if req.train_waiters > 0 && is_train(&req.git_ref) && waits_itself(&req.workflow, read) {
            // Staggered: each queued train run holds a Linux slot in its runner-wait job, so at most `train_waiters` wait at
            // once and a Linux slot stays free for the older runs' own Linux jobs.
            let mut client = Client::new(http, server, repo, token);
            let mut probe = WaitersProbe {
                client: &mut client,
                read,
                cap: req.train_waiters,
            };
            let waited = match wait_held(&mut probe, health.as_deref_mut(), clock, req.max, req.interval, log) {
                Outcome::Done { elapsed, .. } => elapsed,
                Outcome::TimedOut { .. } if health.as_deref().is_some_and(|h| !h.down_now().is_empty()) => {
                    return Ok(service_down(&req.workflow, health.as_deref(), req.max, log));
                }
                Outcome::TimedOut { last, max } => {
                    let _ = writeln!(
                        log,
                        "::error::dispatch: not dispatching {}: {last} after {} (--max). The Mac or Windows runner may be offline (those runs wait for it): check the runners in Gitea's Actions settings. Next: wait for one of those train runs to leave its runner-wait job, then dispatch again.",
                        req.workflow,
                        fmt_duration(max)
                    );
                    return Ok(Dispatched::StillBusy { last });
                }
                Outcome::Broken { error, .. } => return Err(format!("the run list could not be read: {error}")),
                Outcome::Failed { detail, .. } => return Err(detail),
            };
            let _ = writeln!(
                log,
                "dispatch: {} on {} waits for its {} runner in its own first job (runner-wait); dispatching at once so scheduled runs see the train",
                req.workflow,
                req.git_ref,
                wanted.iter().map(|c| c.name()).collect::<Vec<_>>().join(" and ")
            );
            return send(http, target, req, waited, log);
        }
        // A train's workflow does not wait for its own train's other workflows: its identity is the ref's head commit, read
        // from the branch on every look and again right before sending (unreadable: strict, it waits for every run).
        let train_branch = is_train(&req.git_ref).then(|| req.git_ref.strip_prefix("refs/heads/").unwrap_or(&req.git_ref).to_string());
        let mut client = Client::new(http, server, repo, token);
        let mut probe = BusyProbe {
            client: &mut client,
            read,
            wanted,
            older_than: None,
            own_train: None,
            own_workflow: req.workflow.clone(),
            joined: Vec::new(),
            train_branch,
            strict_why: None,
        };
        let mut rounds = 0;
        loop {
            match wait_held(&mut probe, health.as_deref_mut(), clock, req.max, req.interval, log) {
                Outcome::Done { elapsed, .. } => waited = elapsed,
                Outcome::TimedOut { .. } if health.as_deref().is_some_and(|h| !h.down_now().is_empty()) => {
                    return Ok(service_down(&req.workflow, health.as_deref(), req.max, log));
                }
                Outcome::TimedOut { last, max } => {
                    let _ = writeln!(
                        log,
                        "::error::dispatch: not dispatching {}: still busy after {} (--max); {last}. Next: {}.",
                        req.workflow,
                        fmt_duration(max),
                        probe.next_step()
                    );
                    return Ok(Dispatched::StillBusy { last });
                }
                Outcome::Broken { error, .. } => return Err(format!("the run list could not be read: {error}")),
                Outcome::Failed { detail, .. } => return Err(detail),
            }
            if !probe.head_moved() {
                break;
            }
            rounds += 1;
            if rounds >= 5 {
                return Err(format!(
                    "the head of {} moved five times while {} waited: dispatch again when the train has settled",
                    req.git_ref, req.workflow
                ));
            }
            let _ = writeln!(
                log,
                "dispatch: the head of {} moved while waiting; looking again with the new head",
                req.git_ref
            );
        }
    }
    send(http, target, req, waited, log)
}

/// Waits while a `train/*` run of `req.workflow` is queued or running ([`TrainFirstProbe`]). `Ok(Ok(waited))`: none is left, after
/// `waited`; `Ok(Err(ended))`: the dispatch ends without sending (a train held it for `--max`, or Gitea was down), the `::error::`
/// already logged; `Err`: the run list could not be read. `./dev ci dispatch --trial` asks this before it pushes the trial ref, so a
/// trial that never gets its turn leaves nothing on origin; [`run`] asks again right before it sends (a train may have come since).
pub fn yield_to_trains(
    http: &mut dyn Http,
    target: &Target<'_>,
    req: &Request,
    clock: &mut dyn Clock,
    log: &mut dyn Write,
    mut health: Option<&mut Health<'_>>,
) -> Result<Result<Duration, Dispatched>, String> {
    let before = clock.elapsed();
    let mut client = Client::new(http, target.server, target.repo, target.token);
    let mut probe = TrainFirstProbe {
        client: &mut client,
        workflow: req.workflow.clone(),
    };
    match wait_held(&mut probe, health.as_deref_mut(), clock, req.max, req.interval, log) {
        // The wait's own time (the clock also counts a health hold before it); a class wait later reports the clock's total.
        Outcome::Done { .. } => Ok(Ok(clock.elapsed() - before)),
        Outcome::TimedOut { .. } if health.as_deref().is_some_and(|h| !h.down_now().is_empty()) => {
            Ok(Err(service_down(&req.workflow, health.as_deref(), req.max, log)))
        }
        Outcome::TimedOut { last, max } => {
            let _ = writeln!(
                log,
                "::error::dispatch: not dispatching {} on {}: {last} after {} (--max). Next: {}.",
                req.workflow,
                req.git_ref,
                fmt_duration(max),
                probe.next_step()
            );
            Ok(Err(Dispatched::StillBusy { last }))
        }
        Outcome::Broken { error, .. } => Err(format!("the run list could not be read: {error}")),
        Outcome::Failed { detail, .. } => Err(detail),
    }
}

/// The `::error::` of a dispatch that held for `--max` while a service was down, and its outcome.
fn service_down(workflow: &str, health: Option<&Health<'_>>, max: Duration, log: &mut dyn Write) -> Dispatched {
    let last = health.map(|h| h.down_now().join("; ")).unwrap_or_default();
    let next = health.and_then(Health::next_step).unwrap_or_else(|| "run the command again".into());
    let _ = writeln!(
        log,
        "::error::dispatch: not dispatching {workflow}: {last} for the whole --max ({}). Next: {next}.",
        fmt_duration(max)
    );
    Dispatched::ServiceDown { last }
}

/// Sends the dispatch.
fn send(http: &mut dyn Http, target: &Target<'_>, req: &Request, waited: Duration, log: &mut dyn Write) -> Result<Dispatched, String> {
    let url = format!(
        "{}/api/v1/repos/{}/actions/workflows/{}/dispatches",
        target.server.trim_end_matches('/'),
        target.repo,
        req.workflow
    );
    let inputs: serde_json::Map<String, Value> = req.inputs.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    http.post_json(&url, target.token, &json!({"ref": req.git_ref, "inputs": inputs}))?;
    let _ = writeln!(
        log,
        "dispatch: dispatched {} on {} after waiting {}",
        req.workflow,
        req.git_ref,
        fmt_duration(waited)
    );
    Ok(Dispatched::Yes { waited })
}

/// The file that declares how many jobs each runner class runs at once (`{"linux": 3}`), relative to the repository root.
pub const RUNNER_SLOTS: &str = "conformance/ci/runner-slots.json";

/// How many train runs may wait in their runner-wait job at once: the Linux slots of [`RUNNER_SLOTS`] less one, so one slot
/// stays free for the older runs' own Linux jobs (a Mac job that needs them would otherwise never start, and every waiter would
/// stall to its limit). `Err` when the file cannot be read or holds no `linux` count of at least 2.
pub fn train_waiters(root: &std::path::Path) -> Result<usize, String> {
    let path = root.join(RUNNER_SLOTS);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{} cannot be read: {e}", path.display()))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{} is not JSON: {e}", path.display()))?;
    match v["linux"].as_u64() {
        Some(n) if n >= 2 => Ok(n as usize - 1),
        _ => Err(format!("{} has no `linux` slot count of at least 2", path.display())),
    }
}

/// The display name of a workflow's runner-wait job (the first job whose steps use [`RUNNER_WAIT_ACTION`]): its `name`, else its id.
fn waiter_job_name(workflow: &str, read: ReadWorkflow<'_>) -> Option<String> {
    let jobs = runner_jobs(&read(workflow)?).ok()?;
    jobs.into_iter().find(is_waiter).map(|j| j.name)
}

/// A first job (no `needs`) with a step that uses [`RUNNER_WAIT_ACTION`].
fn is_waiter(job: &RunnerJob) -> bool {
    job.needs.is_empty() && job.step_uses.iter().any(|u| u == RUNNER_WAIT_ACTION)
}

/// Waits while `cap` or more train runs (unfinished `workflow_dispatch` runs on `train/*` of a workflow that waits itself) are
/// still in their runner-wait step: that job is not finished and its runner-wait step has not ended. A run past it holds no
/// Linux slot for the wait.
struct WaitersProbe<'a, 'b> {
    client: &'a mut Client<'b>,
    read: ReadWorkflow<'a>,
    cap: usize,
}

impl WaitersProbe<'_, '_> {
    fn waiting(&mut self) -> Result<Vec<u64>, String> {
        let page = self.client.runs_page_where(1, "event=workflow_dispatch")?;
        let mut ids = Vec::new();
        for r in page.runs.iter().filter(|r| !r.finished() && is_train(r.git_ref())) {
            let Some(name) = waiter_job_name(r.workflow(), self.read) else {
                continue;
            };
            let jobs = self.client.timed_jobs(r.id)?.unwrap_or_default();
            // No job list yet, or the waiter job not listed yet: the run is about to wait.
            let still = match jobs.iter().find(|j| j.name == name) {
                None => true,
                Some(j) => {
                    let step_done = j.steps.iter().any(|(s, secs)| s.contains("actions/runner-wait") && secs.is_some());
                    j.status != "completed" && !step_done
                }
            };
            if still {
                ids.push(r.id);
            }
        }
        Ok(ids)
    }
}

impl Probe for WaitersProbe<'_, '_> {
    fn describe(&self) -> String {
        format!(
            "fewer than {} train runs waiting for their runner ({} Linux slots, {RUNNER_SLOTS})",
            self.cap,
            self.cap + 1
        )
    }
    fn poll(&mut self) -> Result<Poll, String> {
        let w = self.waiting()?;
        Ok(if w.len() < self.cap {
            Poll::Done(format!("{} train run(s) waiting", w.len()))
        } else {
            Poll::Pending(format!("train runs {w:?} are waiting for their runner"))
        })
    }
    fn next_step(&self) -> String {
        "wait for one of those train runs to leave its runner-wait job (or check that the Mac and Windows runners are online), then dispatch again".into()
    }
}

/// The composite action a workflow's first job runs to wait for its single-machine runner inside the run.
pub const RUNNER_WAIT_ACTION: &str = "./.gitea/actions/runner-wait";

/// The train a run belongs to: its `train/*` ref and head commit. Two runs with the same identity are one reservation of the
/// runners (ER-24 allows one train's CI at a time; its workflows run together). Read from the run record the Gitea API
/// gives, never from workflow names. `None` for a run that is not a dispatched train run, or whose commit is unknown (then it
/// is never taken for a sibling: unknown waits).
pub fn train_identity(r: &RunInfo) -> Option<(String, String)> {
    let git_ref = r.git_ref();
    let branch = git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref);
    // The record's own branch must agree with the workflow path's ref; a disagreement is not a train run we can name.
    let agrees = r.head_branch.is_empty() || r.head_branch == branch;
    (r.event == "workflow_dispatch" && is_train(git_ref) && !r.head_sha.is_empty() && agrees)
        .then(|| (branch.to_string(), r.head_sha.clone()))
}

/// Whether a ref is a train's (`train/...`, also spelled `refs/heads/train/...`).
pub fn is_train(git_ref: &str) -> bool {
    git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref).starts_with("train/")
}

/// Whether `workflow` waits for its runner itself: a job with no `needs` (a first job) has a step that uses [`RUNNER_WAIT_ACTION`].
/// Conformance (ci-actions.test.mjs) checks that every single-machine job of such a workflow needs that job.
pub fn waits_itself(workflow: &str, read: ReadWorkflow<'_>) -> bool {
    read(workflow)
        .and_then(|text| runner_jobs(&text).ok())
        .is_some_and(|jobs| jobs.iter().any(is_waiter))
}

/// What `ci runner-wait` found.
#[derive(Debug, PartialEq, Eq)]
pub enum RunnerWait {
    /// No older run holds the runner (or the workflow needs none): the run goes on after `waited`.
    Free {
        /// How long it waited.
        waited: Duration,
    },
    /// An older run still held it at `--max`.
    StillBusy {
        /// The run that held it.
        last: String,
    },
    /// The runs could not be read: the run goes on (fails open), with the reason.
    Unreadable(String),
}

/// How many times `ci runner-wait` reads its own run before it gives up and goes on (each read already retries in the shared HTTP layer).
pub const OWN_RUN_READS: u32 = 5;

/// `ci runner-wait`: the first job of a train run waits, on a Linux runner, until no older run has an unfinished job on one of
/// its workflow's single-machine runner classes (at most `max`). `own` is this run's id; its workflow file is read from the run
/// itself (`ci.yml@refs/heads/train/...`), retried [`OWN_RUN_READS`] times `interval` apart before it fails open.
#[allow(clippy::too_many_arguments)]
pub fn runner_wait(
    http: &mut dyn Http,
    target: &Target<'_>,
    own: u64,
    max: Duration,
    interval: Duration,
    read: ReadWorkflow<'_>,
    clock: &mut dyn Clock,
    log: &mut dyn Write,
) -> Result<RunnerWait, String> {
    let mut client = Client::new(http, target.server, target.repo, target.token);
    let mut workflow = None;
    let mut own_train = None;
    let mut last = String::new();
    for attempt in 1..=OWN_RUN_READS {
        match client.run(own) {
            Ok(Some(r)) => {
                workflow = Some(r.workflow().to_string());
                own_train = train_identity(&r);
                break;
            }
            Ok(None) => {
                last = format!("the server has no run {own}");
                break;
            }
            Err(e) => {
                last = e;
                if attempt < OWN_RUN_READS {
                    clock.sleep(interval);
                }
            }
        }
    }
    let Some(workflow) = workflow.filter(|w| !w.is_empty()) else {
        let _ = writeln!(
            log,
            "::warning::runner-wait: run {own} could not be read ({last}); it goes on without waiting (fail open). Check that the job token can read the repository's Actions runs."
        );
        return Ok(RunnerWait::Unreadable(last));
    };
    let workflow = workflow.as_str();
    let wanted = classes_of(workflow, read)?;
    if wanted.is_empty() {
        let _ = writeln!(
            log,
            "runner-wait: {workflow} uses no single-machine runner (Linux only); not waiting"
        );
        return Ok(RunnerWait::Free { waited: Duration::ZERO });
    }
    let mut probe = BusyProbe {
        client: &mut client,
        read,
        wanted,
        older_than: Some(own),
        own_train: own_train.clone(),
        own_workflow: workflow.to_string(),
        joined: Vec::new(),
        train_branch: None,
        strict_why: None,
    };
    Ok(match wait_for(&mut probe, clock, max, interval, log) {
        Outcome::Done { elapsed, .. } => {
            let _ = writeln!(
                log,
                "runner-wait: run {own} ({workflow}) goes on after waiting {}",
                fmt_duration(elapsed)
            );
            RunnerWait::Free { waited: elapsed }
        }
        Outcome::TimedOut { last, max } => {
            let _ = writeln!(
                log,
                "::error::runner-wait: run {own} ({workflow}) still waits after {} (--max); {last}. Next step: wait for that run (`./dev wait --ci-run <id> --max <duration>`) or cancel it if it is stale, then rerun this one.",
                fmt_duration(max)
            );
            RunnerWait::StillBusy { last }
        }
        Outcome::Broken { error, .. } => {
            let _ = writeln!(
                log,
                "::warning::runner-wait: the runs could not be read ({error}); run {own} goes on without waiting (fail open). Check that the job token can read the repository's Actions runs."
            );
            RunnerWait::Unreadable(error)
        }
        Outcome::Failed { detail, .. } => return Err(detail),
    })
}

/// The workflow input that carries a dispatch's own token (guard A6, ER-51): the workflow's `run-name` shows it as
/// `<name> [dispatch <token>]`, so the dispatcher finds the one run it made by that exact token, never "the newest run".
pub const DISPATCH_ID: &str = "dispatch_id";

/// What a dispatch asks a workflow to run, carried in its token (after the last `.`) so a reader of the run list tells a full run from
/// a failed-only round without reading logs (guard 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunKind {
    /// No `base`, `since_run`, `os` or `app`: the whole plan of the ref (a train's landing run).
    Full,
    /// A failed-only round (`since_run=<run>`, ER-10).
    Since(u64),
    /// A member run (`base`): narrowed, never complete.
    Member,
    /// One part only (`os`, `app`, or a `since_run` that is not a run id).
    Part,
}

impl RunKind {
    /// The kind of a dispatch from its inputs.
    pub fn of(inputs: &[(String, String)]) -> RunKind {
        let get = |k: &str| {
            inputs
                .iter()
                .find(|(key, v)| key == k && !v.trim().is_empty())
                .map(|(_, v)| v.trim())
        };
        if let Some(s) = get("since_run") {
            return s.parse().map_or(RunKind::Part, RunKind::Since);
        }
        if get("base").is_some() {
            return RunKind::Member;
        }
        if get("os").is_some() || get("app").is_some() {
            return RunKind::Part;
        }
        RunKind::Full
    }

    fn tag(&self) -> String {
        match self {
            RunKind::Full => "full".into(),
            RunKind::Since(n) => format!("since-{n}"),
            RunKind::Member => "member".into(),
            RunKind::Part => "part".into(),
        }
    }

    fn parse(s: &str) -> Option<RunKind> {
        match s {
            "full" => Some(RunKind::Full),
            "member" => Some(RunKind::Member),
            "part" => Some(RunKind::Part),
            _ => s.strip_prefix("since-")?.parse().ok().map(RunKind::Since),
        }
    }
}

/// The dispatch token a run's title carries (`ci [dispatch 1a2b3c4d5e6f7a8b.full]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchTag {
    /// The whole token, as sent in [`DISPATCH_ID`].
    pub id: String,
    /// The kind of run it asked for.
    pub kind: RunKind,
}

impl DispatchTag {
    /// The tag of a run title; `None` for a run that was not dispatched through `./dev ci dispatch` (a push, a schedule, a run from
    /// before the `run-name` line, or a title that does not hold one well-formed tag).
    pub fn of_title(title: &str) -> Option<DispatchTag> {
        let start = title.find("[dispatch ")? + "[dispatch ".len();
        let rest = &title[start..];
        let id = &rest[..rest.find(']')?];
        let (nonce, kind) = id.rsplit_once('.')?;
        (nonce.len() >= 12 && nonce.chars().all(|c| c.is_ascii_hexdigit())).then_some(())?;
        Some(DispatchTag {
            id: id.to_string(),
            kind: RunKind::parse(kind)?,
        })
    }
}

/// A new dispatch token for `kind`: 16 hex digits from the time, the process and a counter (unique per call), then the kind.
pub fn new_dispatch_id(kind: &RunKind) -> String {
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut h = Sha256::new();
    h.update(format!(
        "{nanos}:{}:{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let hex: String = h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("{hex}.{}", kind.tag())
}

/// Whether `workflow` declares the [`DISPATCH_ID`] input of `workflow_dispatch` (only then is it sent: a server may refuse an
/// input the workflow does not declare).
pub fn declares_dispatch_id(workflow: &str, read: ReadWorkflow<'_>) -> bool {
    read(workflow)
        .and_then(|text| triggers(&text).ok())
        .is_some_and(|t| t.takes_input(DISPATCH_ID))
}

/// The inputs with a new [`DISPATCH_ID`] added when the workflow declares it (a caller's own non-empty one is kept); the token, if any.
pub fn with_dispatch_id(
    workflow: &str,
    read: ReadWorkflow<'_>,
    mut inputs: Vec<(String, String)>,
) -> (Vec<(String, String)>, Option<String>) {
    if let Some(id) = inputs
        .iter()
        .find(|(k, v)| k == DISPATCH_ID && !v.is_empty())
        .map(|(_, v)| v.clone())
    {
        return (inputs, Some(id));
    }
    if !declares_dispatch_id(workflow, read) {
        return (inputs, None);
    }
    let id = new_dispatch_id(&RunKind::of(&inputs));
    inputs.retain(|(k, _)| k != DISPATCH_ID);
    inputs.push((DISPATCH_ID.into(), id.clone()));
    (inputs, Some(id))
}

/// How long [`find_run`] looks for the run a dispatch made: the server lists a dispatched run within seconds.
pub const FIND_MAX: Duration = Duration::from_secs(120);
/// How often [`find_run`] looks.
pub const FIND_INTERVAL: Duration = Duration::from_secs(5);

/// Looks for the run whose title carries exactly `id` among the newest dispatch runs of `workflow`.
struct RunFinder<'a, 'b> {
    client: &'a mut Client<'b>,
    workflow: &'a str,
    id: &'a str,
    found: Option<u64>,
}

impl Probe for RunFinder<'_, '_> {
    fn describe(&self) -> String {
        format!("the run of {} with dispatch id {}", self.workflow, self.id)
    }
    fn poll(&mut self) -> Result<Poll, String> {
        let page = self.client.runs_page_where(1, "event=workflow_dispatch")?;
        let ids: Vec<u64> = page
            .runs
            .iter()
            .filter(|r| r.workflow() == self.workflow && DispatchTag::of_title(&r.title).is_some_and(|t| t.id == self.id))
            .map(|r| r.id)
            .collect();
        Ok(match ids.as_slice() {
            [] => Poll::Pending(format!("no run of {} shows dispatch id {} yet", self.workflow, self.id)),
            [one] => {
                self.found = Some(*one);
                Poll::Done(format!("run {one}"))
            }
            many => Poll::Failed(format!("runs {many:?} all show dispatch id {}: the token is not unique", self.id)),
        })
    }
    fn next_step(&self) -> String {
        format!(
            "find the run in Gitea's Actions list by its title `[dispatch {}]`; do not dispatch again (the dispatch was sent)",
            self.id
        )
    }
}

/// The run a dispatch made, found by its exact token (guard A6): `Ok(Some(id))`, `Ok(None)` when no listed run showed it within
/// `max` (the dispatch was sent; it is never sent again), `Err` when the list could not be read or two runs show the token.
#[allow(clippy::too_many_arguments)]
pub fn find_run(
    http: &mut dyn Http,
    target: &Target<'_>,
    workflow: &str,
    id: &str,
    clock: &mut dyn Clock,
    max: Duration,
    interval: Duration,
    log: &mut dyn Write,
) -> Result<Option<u64>, String> {
    let mut client = Client::new(http, target.server, target.repo, target.token);
    let mut probe = RunFinder {
        client: &mut client,
        workflow,
        id,
        found: None,
    };
    match wait_for(&mut probe, clock, max, interval, log) {
        Outcome::Done { .. } => Ok(probe.found),
        Outcome::TimedOut { .. } => Ok(None),
        Outcome::Broken { error, .. } => Err(format!("the run list could not be read: {error}")),
        Outcome::Failed { detail, .. } => Err(detail),
    }
}

/// The real clock, for the command.
pub fn system_clock() -> SystemClock {
    SystemClock::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    const SERVER: &str = "https://gitea.invalid";
    const REPO: &str = "decryp7/paddock";

    /// Run lists in order (the last one repeats), and the posts it got.
    struct Script {
        lists: VecDeque<Value>,
        posts: Vec<(String, Value)>,
        gets: usize,
        /// `GET .../runs/<id>`: the run's workflow path; `own_errors` reads fail first.
        own: Vec<(u64, String)>,
        own_errors: usize,
        /// The head commit of the own run (`GET .../runs/<id>`); empty: none given.
        own_sha: String,
        /// The head commit `GET .../branches/<branch>` answers; empty: 404.
        branch_sha: String,
        /// Heads in order for `GET .../branches/<branch>` (the last repeats); used before `branch_sha`.
        branch_seq: VecDeque<String>,
        /// `GET .../runs/<id>/jobs`: the job lists in order per run (the last one repeats).
        jobs: Vec<(u64, VecDeque<Value>)>,
    }

    impl Script {
        fn new(lists: Vec<Value>) -> Script {
            Script {
                lists: lists.into(),
                posts: Vec::new(),
                gets: 0,
                own: Vec::new(),
                own_errors: 0,
                own_sha: String::new(),
                branch_sha: String::new(),
                branch_seq: VecDeque::new(),
                jobs: Vec::new(),
            }
        }
    }

    impl Http for Script {
        fn get_json(&mut self, url: &str, token: &str) -> Result<Value, String> {
            assert!(!token.is_empty());
            if let Some(rest) = url.split("/actions/runs/").nth(1) {
                let id: u64 = rest.split(['/', '?']).next().unwrap().parse().unwrap();
                if rest.contains("/jobs") {
                    let Some((_, list)) = self.jobs.iter_mut().find(|(r, _)| *r == id) else {
                        return Err("HTTP 404".into());
                    };
                    return Ok(if list.len() > 1 {
                        list.pop_front().unwrap()
                    } else {
                        list[0].clone()
                    });
                }
                if self.own_errors > 0 {
                    self.own_errors -= 1;
                    return Err("curl: (28) timed out".into());
                }
                let path = &self.own.iter().find(|(r, _)| *r == id).expect("the run").1;
                return Ok(
                    json!({"id": id, "path": path, "status": "in_progress", "event": "workflow_dispatch", "head_sha": self.own_sha}),
                );
            }
            if url.contains("/branches/") {
                if self.branch_seq.len() > 1 {
                    self.branch_sha = self.branch_seq.pop_front().unwrap();
                } else if let Some(last) = self.branch_seq.front() {
                    self.branch_sha = last.clone();
                }
                return if self.branch_sha.is_empty() {
                    Err("HTTP 404".into())
                } else {
                    Ok(json!({"commit": {"id": self.branch_sha}}))
                };
            }
            assert!(url.contains("/actions/runs?"), "{url}");
            self.gets += 1;
            Ok(if self.lists.len() > 1 {
                self.lists.pop_front().unwrap()
            } else {
                self.lists[0].clone()
            })
        }
        fn post_json(&mut self, url: &str, _token: &str, body: &Value) -> Result<Value, String> {
            self.posts.push((url.to_string(), body.clone()));
            Ok(Value::Null)
        }
    }

    struct FakeClock(Duration);
    impl Clock for FakeClock {
        fn elapsed(&self) -> Duration {
            self.0
        }
        fn sleep(&mut self, d: Duration) {
            self.0 += d;
        }
    }

    fn runs(list: &[(u64, &str, &str)]) -> Value {
        json!({"total_count": list.len(), "workflow_runs": list.iter().map(|(id, wf, st)| json!({
            "id": id, "path": format!("{wf}@refs/heads/main"), "status": st, "event": "push"
        })).collect::<Vec<_>>()})
    }

    fn repo_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn files(name: &str) -> Option<String> {
        match name {
            "mac.yml" => Some("jobs:\n  t:\n    runs-on: macos-latest\n".into()),
            "mac2.yml" => Some("jobs:\n  t:\n    runs-on: [self-hosted, macos]\n".into()),
            "linux.yml" => Some("jobs:\n  t:\n    runs-on: ubuntu-latest\n".into()),
            "matrix.yml" => Some(
                "jobs:\n  t:\n    strategy:\n      matrix:\n        os: [ubuntu-latest, windows-latest]\n    runs-on: ${{ matrix.os }}\n"
                    .into(),
            ),
            "caller.yml" => Some("jobs:\n  c:\n    uses: ./.gitea/workflows/mac.yml\n".into()),
            "waits.yml" => Some(
                "jobs:\n  plan:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - uses: ./.gitea/actions/runner-wait\n  t:\n    needs: [plan]\n    runs-on: macos-latest\n"
                    .into(),
            ),
            "waits-late.yml" => Some(
                "jobs:\n  a:\n    runs-on: ubuntu-latest\n  plan:\n    needs: [a]\n    runs-on: ubuntu-latest\n    steps:\n      - uses: ./.gitea/actions/runner-wait\n  t:\n    needs: [plan]\n    runs-on: macos-latest\n"
                    .into(),
            ),
            "planned.yml" => Some(
                "jobs:\n  t:\n    needs: [plan]\n    strategy:\n      matrix:\n        include: \"${{ fromJSON(needs.plan.outputs.crate_legs) }}\"\n    runs-on: \"${{ matrix.os }}\"\n"
                    .into(),
            ),
            "comment.yml" => Some("# runs-on: macos-latest\njobs:\n  t:\n    runs-on: ubuntu-latest\n".into()),
            "tagged.yml" => Some(
                "on:\n  workflow_dispatch:\n    inputs:\n      dispatch_id:\n        default: \"\"\njobs:\n  t:\n    runs-on: ubuntu-latest\n".into(),
            ),
            "step-text.yml" => Some(
                "jobs:\n  t:\n    runs-on: ubuntu-latest\n    steps:\n      - if: runner.os != 'Windows'\n        run: echo macOS\n".into(),
            ),
            "bench.yml" => Some(include_str!("../../../../.gitea/workflows/debrief-bench.yml").into()),
            "debrief.yml" => Some(include_str!("../../../../.gitea/workflows/debrief.yml").into()),
            _ => None,
        }
    }

    fn request(wf: &str, max: &str) -> Request {
        parse_request(wf, "main", &["gates=on".to_string()], max, Some("15s")).unwrap()
    }

    fn go(http: &mut Script, wf: &str, max: &str) -> (Result<Dispatched, String>, String, Duration) {
        let (mut clock, mut log) = (FakeClock(Duration::ZERO), Vec::new());
        #[expect(clippy::disallowed_methods, reason = "the tests of the raw dispatch itself")]
        let r = run(
            http,
            &Target {
                server: SERVER,
                repo: REPO,
                token: "secret-token",
            },
            &request(wf, max),
            &files,
            &mut clock,
            &mut log,
            None,
        );
        (r, String::from_utf8(log).unwrap(), clock.0)
    }

    fn go_held(
        http: &mut Script,
        wf: &str,
        max: &str,
        reach: &mut crate::health::tests::Scripted,
    ) -> (Result<Dispatched, String>, String, Duration) {
        let (mut clock, mut log) = (FakeClock(Duration::ZERO), Vec::new());
        let mut health = Health::new(crate::health::tests::both(), reach);
        #[expect(clippy::disallowed_methods, reason = "the tests of the raw dispatch itself")]
        let r = run(
            http,
            &Target {
                server: SERVER,
                repo: REPO,
                token: "secret-token",
            },
            &request(wf, max),
            &files,
            &mut clock,
            &mut log,
            Some(&mut health),
        );
        (r, String::from_utf8(log).unwrap(), clock.0)
    }

    #[test]
    fn a_dispatch_holds_while_gitea_or_rustfs_is_down_and_sends_nothing_until_both_answer() {
        use crate::health::Reach;
        // Linux only: no runner wait, so the hold is the only wait. Gitea answers 502 twice, RustFS refuses once, then both are up.
        let mut reach = crate::health::tests::Scripted::new()
            .script(
                "api/v1/version",
                vec![Reach::Answered(502), Reach::Answered(502), Reach::Answered(200)],
            )
            .script("bucket-x", vec![Reach::Down("connection refused".into()), Reach::Answered(403)]);
        let mut http = Script::new(vec![runs(&[])]);
        let (r, log, t) = go_held(&mut http, "linux.yml", "10m", &mut reach);
        assert_eq!(r.unwrap(), Dispatched::Yes { waited: Duration::ZERO }, "{log}");
        assert_eq!(http.posts.len(), 1);
        assert_eq!(t, Duration::from_secs(30), "held for two intervals: {log}");
        assert!(log.contains("Gitea is down") && log.contains("RustFS is down"), "{log}");
        assert!(log.contains("Gitea answers again") && log.contains("RustFS answers again"), "{log}");
        // The 403 of the bucket is an answer: RustFS came back at the 403, not later.
        assert!(!log.contains("RustFS is down (HTTP 403)"), "{log}");
    }

    #[test]
    fn a_dispatch_that_holds_for_the_whole_max_ends_with_the_named_error_and_sends_nothing() {
        use crate::health::Reach;
        let mut reach = crate::health::tests::Scripted::new().script("api/v1/version", vec![Reach::Answered(502)]);
        let mut http = Script::new(vec![runs(&[])]);
        let (r, log, _) = go_held(&mut http, "linux.yml", "1m", &mut reach);
        let Ok(Dispatched::ServiceDown { last }) = r else {
            panic!("{r:?}\n{log}")
        };
        assert!(last.contains("Gitea") && last.contains("HTTP 502"), "{last}");
        assert!(http.posts.is_empty() && http.gets == 0, "nothing was sent or read: {log}");
        assert!(
            log.contains("::error::dispatch: not dispatching linux.yml") && log.contains("Next: Gitea runs on the NAS"),
            "{log}"
        );
        // Positive control: the same dispatch with no health gate goes out at once, so the gate is what held it.
        let mut http = Script::new(vec![runs(&[])]);
        let (r, _, _) = go(&mut http, "linux.yml", "1m");
        assert!(matches!(r.unwrap(), Dispatched::Yes { .. }));
        assert_eq!(http.posts.len(), 1);
    }

    #[test]
    fn a_service_that_goes_down_while_the_runner_is_busy_holds_the_busy_wait_too() {
        use crate::health::Reach;
        // Mac busy (run 7960 in progress), and Gitea is down for the whole --max: the end is the service's error, not "busy".
        let mut reach = crate::health::tests::Scripted::new().script(
            "api/v1/version",
            vec![Reach::Answered(200), Reach::Answered(200), Reach::Answered(503)],
        );
        let mut http = Script::new(vec![runs(&[(7960, "mac.yml", "in_progress")])]);
        let (r, log, _) = go_held(&mut http, "mac.yml", "2m", &mut reach);
        assert!(matches!(r, Ok(Dispatched::ServiceDown { .. })), "{r:?}\n{log}");
        assert!(http.posts.is_empty());
    }

    #[test]
    fn a_dispatch_to_an_address_that_cannot_resolve_fails_at_once_instead_of_holding() {
        use crate::health::Reach;
        let mut reach = crate::health::tests::Scripted::new().script(
            "api/v1/version",
            vec![Reach::Invalid("curl: (6) Could not resolve host: typo.invalid".into())],
        );
        let mut http = Script::new(vec![runs(&[])]);
        let (r, _, t) = go_held(&mut http, "linux.yml", "10m", &mut reach);
        let e = r.unwrap_err();
        assert!(e.contains("Gitea") && e.contains("typo.invalid") && e.contains("--server"), "{e}");
        assert_eq!(t, Duration::ZERO, "no hold");
        assert!(http.posts.is_empty());
    }

    #[test]
    fn busy_then_free_dispatches_after_the_wait() {
        let mut http = Script::new(vec![
            runs(&[]), // the look for a train run that goes first
            runs(&[(7960, "mac.yml", "in_progress")]),
            runs(&[(7960, "mac2.yml", "in_progress")]),
            runs(&[(7960, "mac.yml", "completed")]),
        ]);
        let (r, log, t) = go(&mut http, "mac.yml", "10m");
        assert_eq!(
            r.unwrap(),
            Dispatched::Yes {
                waited: Duration::from_secs(30)
            }
        );
        assert_eq!(t, Duration::from_secs(30));
        assert!(
            log.contains("run 7960 (mac.yml, in_progress) is on macOS") || log.contains("waiting for the macOS runner"),
            "{log}"
        );
        assert!(!log.contains("secret-token"));
        assert_eq!(http.posts.len(), 1);
        let (url, body) = &http.posts[0];
        assert!(url.ends_with("/actions/workflows/mac.yml/dispatches"), "{url}");
        assert_eq!(body, &json!({"ref": "main", "inputs": {"gates": "on"}}));
    }

    fn go_ref(http: &mut Script, wf: &str, git_ref: &str) -> (Result<Dispatched, String>, String) {
        let (mut clock, mut log) = (FakeClock(Duration::ZERO), Vec::new());
        let mut req = parse_request(wf, git_ref, &[], "10m", Some("15s")).unwrap();
        req.train_waiters = train_waiters(&repo_root()).unwrap();
        #[expect(clippy::disallowed_methods, reason = "the tests of the raw dispatch itself")]
        let r = run(
            http,
            &Target {
                server: SERVER,
                repo: REPO,
                token: "secret-token",
            },
            &req,
            &files,
            &mut clock,
            &mut log,
            None,
        );
        (r, String::from_utf8(log).unwrap())
    }

    #[test]
    fn a_train_whose_first_job_waits_itself_is_dispatched_at_once_so_scheduled_runs_see_it() {
        // 2026-11-15 05:03-07:07: the train's dispatcher waited behind scheduled runs 8112-8114 with nothing queued.
        let mut http = Script::new(vec![runs(&[(8112, "mac.yml", "in_progress")])]);
        let (r, log) = go_ref(&mut http, "waits.yml", "train/2026-11-15-12");
        assert_eq!(r.unwrap(), Dispatched::Yes { waited: Duration::ZERO });
        assert_eq!(
            http.gets, 1,
            "one look at the train runs already waiting, no wait for the Mac: the run waits in its own first job"
        );
        assert_eq!(http.posts.len(), 1);
        assert!(log.contains("waits for its macOS runner in its own first job"), "{log}");
        assert!(is_train("refs/heads/train/x") && !is_train("trial/train/x") && !is_train("main"));
    }

    #[test]
    fn at_most_two_train_runs_wait_for_their_runner_at_once_so_a_linux_slot_stays_free() {
        // Review round 3: each queued train run holds one of the 3 Linux slots in its runner-wait job.
        let train = |id: u64| json!({"id": id, "path": "waits.yml@refs/heads/train/2026-11-15-12", "status": "in_progress", "event": "workflow_dispatch"});
        let two = json!({"workflow_runs": [train(8131), train(8130)]});
        let mut http = Script::new(vec![two]);
        // The waiter job of waits.yml is `plan`; its runner-wait step has no end time while it waits.
        let plan = |status: &str, wait_done: bool| {
            let step = if wait_done {
                json!({"name": "Run ./.gitea/actions/runner-wait", "started_at": "2026-11-15T10:00:00Z", "completed_at": "2026-11-15T10:20:00Z"})
            } else {
                json!({"name": "Run ./.gitea/actions/runner-wait", "started_at": "2026-11-15T10:00:00Z"})
            };
            json!({"jobs": [
                {"id": 1, "name": "plan", "labels": ["ubuntu-latest"], "status": status, "steps": [step]},
                {"id": 2, "name": "t", "labels": ["macos-latest"], "status": "waiting"}
            ]})
        };
        http.jobs = vec![
            // 8130 leaves its runner-wait step while its plan job still runs: it no longer counts.
            (8130, VecDeque::from(vec![plan("in_progress", false), plan("in_progress", true)])),
            (8131, VecDeque::from(vec![plan("in_progress", false)])),
        ];
        let (r, log) = go_ref(&mut http, "waits.yml", "train/2026-11-15-12");
        assert_eq!(
            r.unwrap(),
            Dispatched::Yes {
                waited: Duration::from_secs(15)
            },
            "{log}"
        );
        assert!(
            log.contains("train runs [8131, 8130] are waiting for their runner") || log.contains("fewer than 2 train runs"),
            "{log}"
        );
        assert_eq!(http.posts.len(), 1);
        assert_eq!(
            train_waiters(&repo_root()),
            Ok(2),
            "conformance/ci/runner-slots.json declares 3 Linux slots"
        );
    }

    #[test]
    fn the_slot_count_is_read_from_its_file_and_a_train_without_it_waits_here() {
        let d = tempfile::tempdir().unwrap();
        assert!(train_waiters(d.path()).unwrap_err().contains("cannot be read"));
        std::fs::create_dir_all(d.path().join("conformance/ci")).unwrap();
        std::fs::write(d.path().join(RUNNER_SLOTS), r#"{"linux": 1}"#).unwrap();
        assert!(train_waiters(d.path()).unwrap_err().contains("at least 2"));
        std::fs::write(d.path().join(RUNNER_SLOTS), r#"{"linux": 5}"#).unwrap();
        assert_eq!(train_waiters(d.path()), Ok(4));
        // Unknown (0): the train dispatch waits in the dispatcher, as before runner-wait.
        let mut http = Script::new(vec![
            runs(&[(8112, "mac.yml", "in_progress")]),
            runs(&[(8112, "mac.yml", "completed")]),
        ]);
        let (mut clock, mut log) = (FakeClock(Duration::ZERO), Vec::new());
        let req = parse_request("waits.yml", "train/t", &[], "1h", Some("15s")).unwrap();
        assert_eq!(req.train_waiters, 0);
        #[expect(clippy::disallowed_methods, reason = "the tests of the raw dispatch itself")]
        let r = run(
            &mut http,
            &Target {
                server: SERVER,
                repo: REPO,
                token: "t",
            },
            &req,
            &files,
            &mut clock,
            &mut log,
            None,
        );
        let log = String::from_utf8(log).unwrap();
        assert_eq!(
            r.unwrap(),
            Dispatched::Yes {
                waited: Duration::from_secs(15)
            },
            "{log}"
        );
        assert!(!log.contains("dispatching at once"), "{log}");
    }

    #[test]
    fn a_stagger_that_never_frees_names_the_runner_as_maybe_offline() {
        let train =
            |id: u64| json!({"id": id, "path": "waits.yml@refs/heads/train/t", "status": "in_progress", "event": "workflow_dispatch"});
        let mut http = Script::new(vec![json!({"workflow_runs": [train(8131), train(8130)]})]);
        let (mut clock, mut log) = (FakeClock(Duration::ZERO), Vec::new());
        let mut req = parse_request("waits.yml", "train/t", &[], "1m", Some("15s")).unwrap();
        req.train_waiters = 2;
        #[expect(clippy::disallowed_methods, reason = "the tests of the raw dispatch itself")]
        let r = run(
            &mut http,
            &Target {
                server: SERVER,
                repo: REPO,
                token: "t",
            },
            &req,
            &files,
            &mut clock,
            &mut log,
            None,
        );
        assert!(matches!(r.unwrap(), Dispatched::StillBusy { .. }));
        let log = String::from_utf8(log).unwrap();
        assert!(log.contains("The Mac or Windows runner may be offline"), "{log}");
        assert!(http.posts.is_empty());
    }

    #[test]
    fn any_other_dispatch_still_waits_here() {
        for (wf, git_ref) in [
            ("waits.yml", "main"),
            ("mac.yml", "train/2026-11-15-12"),
            ("waits-late.yml", "train/t"),
        ] {
            let mut lists = vec![runs(&[(8112, "mac.yml", "in_progress")]), runs(&[(8112, "mac.yml", "completed")])];
            if !is_train(git_ref) {
                lists.insert(0, runs(&[])); // the look for a train run that goes first
            }
            let mut http = Script::new(lists);
            let (r, _log) = go_ref(&mut http, wf, git_ref);
            assert_eq!(
                r.unwrap(),
                Dispatched::Yes {
                    waited: Duration::from_secs(15)
                },
                "{wf} on {git_ref}"
            );
            assert!(http.gets >= 2, "{wf} on {git_ref} waited");
        }
    }

    fn wait_here(http: &mut Script, own: u64, max: &str) -> (Result<RunnerWait, String>, String, Duration) {
        let (mut clock, mut log) = (FakeClock(Duration::ZERO), Vec::new());
        let r = runner_wait(
            http,
            &Target {
                server: SERVER,
                repo: REPO,
                token: "job-token",
            },
            own,
            parse_duration(max).unwrap(),
            Duration::from_secs(15),
            &files,
            &mut clock,
            &mut log,
        );
        (r, String::from_utf8(log).unwrap(), clock.0)
    }

    fn jobs(list: &[(&str, &str, &str)]) -> Value {
        json!({"jobs": list.iter().enumerate().map(|(i, (name, label, status))| json!({
            "id": i + 1, "name": name, "labels": [label], "status": status, "conclusion": if *status == "completed" { "success" } else { "" }
        })).collect::<Vec<_>>()})
    }

    /// A train run 8120 of waits.yml, an older run 8112 and a newer one 8121 (both unfinished).
    fn train_world(older_jobs: Vec<Value>) -> Script {
        let mut http = Script::new(vec![runs(&[
            (8121, "mac.yml", "queued"),
            (8120, "waits.yml", "in_progress"),
            (8112, "mac.yml", "in_progress"),
        ])]);
        http.own = vec![(8120, "waits.yml@refs/heads/train/2026-11-15-12".into())];
        http.jobs = vec![
            (8112, older_jobs.into()),
            (8121, vec![jobs(&[("t", "macos-latest", "queued")])].into()),
            (
                8120,
                vec![jobs(&[("plan", "ubuntu-latest", "in_progress"), ("t", "macos-latest", "waiting")])].into(),
            ),
        ];
        http
    }

    #[test]
    fn a_train_run_waits_only_while_an_older_run_has_an_unfinished_mac_job_not_for_its_later_linux_jobs() {
        // Review 2026-11-15: waiting for the whole older run held a Linux slot through that run's later Linux jobs.
        let mut http = train_world(vec![
            jobs(&[
                ("rust (macos)", "macos-latest", "in_progress"),
                ("report", "ubuntu-latest", "waiting"),
            ]),
            jobs(&[
                ("rust (macos)", "macos-latest", "completed"),
                ("report", "ubuntu-latest", "in_progress"),
            ]),
        ]);
        let (r, log, t) = wait_here(&mut http, 8120, "1h");
        assert_eq!(
            r.unwrap(),
            RunnerWait::Free {
                waited: Duration::from_secs(15)
            },
            "{log}"
        );
        assert_eq!(
            t,
            Duration::from_secs(15),
            "neither itself nor the newer run 8121 held it up: {log}"
        );
        assert!(log.contains("waiting for the macOS runner to be free"), "{log}");
        assert!(log.contains("runner-wait: run 8120 (waits.yml) goes on after waiting 15s"), "{log}");
    }

    /// One run of a list: (id, workflow, ref, event, head sha).
    type RunRow<'a> = (u64, &'a str, &'a str, &'a str, &'a str);

    fn runs_full(list: &[RunRow<'_>]) -> Value {
        json!({"total_count": list.len(), "workflow_runs": list.iter().map(|(id, wf, r, ev, sha)| json!({
            "id": id, "path": format!("{wf}@{r}"), "status": "in_progress", "event": ev, "head_sha": sha
        })).collect::<Vec<_>>()})
    }

    /// Train `train/t` at commit `aaa`: the waiting run 8373 (waits.yml, the Debrief role) and an older run 8372 of another
    /// workflow of `other_ref`/`other_event`/`other_sha`, whose macOS job runs.
    fn joint_world(other_ref: &str, other_event: &str, other_sha: &str) -> Script {
        let mut http = Script::new(vec![runs_full(&[
            (8373, "waits.yml", "refs/heads/train/t", "workflow_dispatch", "aaa"),
            (8372, "mac.yml", other_ref, other_event, other_sha),
        ])]);
        http.own = vec![(8373, "waits.yml@refs/heads/train/t".into())];
        http.own_sha = "aaa".into();
        http.jobs = vec![
            (8372, vec![jobs(&[("rust (macos)", "macos-latest", "in_progress")])].into()),
            (
                8373,
                vec![jobs(&[("plan", "ubuntu-latest", "in_progress"), ("t", "macos-latest", "waiting")])].into(),
            ),
        ];
        http
    }

    #[test]
    fn two_workflows_of_one_train_do_not_wait_for_each_other() {
        // 2026-11-20: debrief.yml 8373 waited for ci.yml 8372 of the same train, which held the runner.
        let mut http = joint_world("refs/heads/train/t", "workflow_dispatch", "aaa");
        let (r, log, t) = wait_here(&mut http, 8373, "1m");
        assert_eq!(r.unwrap(), RunnerWait::Free { waited: Duration::ZERO }, "{log}");
        assert_eq!(t, Duration::ZERO, "{log}");
    }

    #[test]
    fn a_run_of_another_train_a_trial_or_a_new_head_of_the_same_branch_still_holds_the_train_run() {
        // Positive control of the test above: the same world, with one thing of the older run's identity changed, waits.
        for (r#ref, event, sha, what) in [
            ("refs/heads/train/other", "workflow_dispatch", "aaa", "another train"),
            ("refs/heads/trial/train/t", "workflow_dispatch", "aaa", "a trial of the same commit"),
            (
                "refs/heads/train/t",
                "workflow_dispatch",
                "bbb",
                "the same branch at another commit",
            ),
            ("refs/heads/train/t", "push", "aaa", "a push run, not a dispatched train run"),
        ] {
            let mut http = joint_world(r#ref, event, sha);
            let (r, log, _) = wait_here(&mut http, 8373, "1m");
            assert!(
                matches!(&r, Ok(RunnerWait::StillBusy { last }) if last.contains("run 8372")),
                "{what}: {r:?}\n{log}"
            );
        }
    }

    #[test]
    fn a_scheduled_run_holding_the_runner_holds_a_train_run_and_a_non_train_dispatch_waits_for_a_train_run() {
        let mut http = joint_world("refs/heads/main", "schedule", "aaa");
        let (r, log, _) = wait_here(&mut http, 8373, "1m");
        assert!(matches!(r, Ok(RunnerWait::StillBusy { .. })), "{log}");
        // A dispatch of main (a trial or member check is not a train) waits for the running train run, even at its commit.
        let mut http = Script::new(vec![runs_full(&[(
            8372,
            "mac.yml",
            "refs/heads/train/t",
            "workflow_dispatch",
            "aaa",
        )])]);
        http.jobs = vec![(8372, vec![jobs(&[("rust (macos)", "macos-latest", "in_progress")])].into())];
        let (r, log, _) = go(&mut http, "mac.yml", "1m");
        assert!(matches!(r, Ok(Dispatched::StillBusy { .. })), "{log}");
        assert!(http.posts.is_empty());
    }

    #[test]
    fn a_foreign_holder_keeps_the_waiter_cap_at_slots_less_one_whatever_the_trains_of_the_waiters() {
        // 3 Linux slots, cap 2. Waiters of one train must not fill the slots while a foreign run holds the Mac (the deadlock
        // the cap prevents, ER-43): each waiting run counts, siblings included.
        let wait_step = || json!({"name": "Run ./.gitea/actions/runner-wait", "started_at": "2026-11-15T10:00:00Z"});
        let plan = vec![
            json!({"jobs": [{"id": 1, "name": "plan", "labels": ["ubuntu-latest"], "status": "in_progress", "steps": [wait_step()]}]}),
        ];
        let mut http = Script::new(vec![runs_full(&[
            (8131, "waits.yml", "refs/heads/train/t", "workflow_dispatch", "aaa"),
            (8130, "waits.yml", "refs/heads/train/t", "workflow_dispatch", "aaa"),
        ])]);
        http.jobs = [8131u64, 8130].iter().map(|id| (*id, plan.clone().into())).collect();
        let (r, log) = go_ref(&mut http, "waits.yml", "train/t");
        assert!(matches!(r.unwrap(), Dispatched::StillBusy { .. }), "{log}");
        assert!(log.contains("train runs [8131, 8130] are waiting"), "{log}");
        assert!(http.posts.is_empty());
    }

    #[test]
    fn a_duplicate_dispatch_of_the_same_workflow_at_the_same_head_is_not_a_sibling() {
        let mut http = joint_world("refs/heads/train/t", "workflow_dispatch", "aaa");
        http.own = vec![(8373, "mac.yml@refs/heads/train/t".into())];
        let (r, log, _) = wait_here(&mut http, 8373, "1m");
        assert!(
            matches!(&r, Ok(RunnerWait::StillBusy { last }) if last.contains("run 8372")),
            "{r:?}\n{log}"
        );
    }

    #[test]
    fn the_ignored_siblings_are_named_in_the_log() {
        let mut http = joint_world("refs/heads/train/t", "workflow_dispatch", "aaa");
        let (_, log, _) = wait_here(&mut http, 8373, "1m");
        assert!(
            log.contains("joint reservation: not waiting for run(s) 8372 (mac.yml) of the same train"),
            "{log}"
        );
    }

    #[test]
    fn a_train_that_moves_while_the_dispatch_waits_makes_it_wait_for_the_old_heads_runs() {
        // The head is aaa at the first look (the sibling of aaa is ignored, nothing to wait for), bbb right before sending: the
        // old head's run 8372 is now foreign and holds the Mac until it ends; the dispatch waits for it, then starts bbb.
        let mut http = Script::new(vec![
            runs_full(&[(8372, "mac2.yml", "refs/heads/train/t", "workflow_dispatch", "aaa")]),
            runs_full(&[(8372, "mac2.yml", "refs/heads/train/t", "workflow_dispatch", "aaa")]),
            runs(&[(8372, "mac2.yml", "completed")]),
        ]);
        http.branch_seq = ["aaa", "bbb"].iter().map(|s| s.to_string()).collect();
        http.jobs = vec![(8372, vec![jobs(&[("rust (macos)", "macos-latest", "in_progress")])].into())];
        let (r, log) = go_ref(&mut http, "mac.yml", "train/t");
        assert_eq!(
            r.unwrap(),
            Dispatched::Yes {
                waited: Duration::from_secs(15)
            },
            "{log}"
        );
        assert!(log.contains("moved while waiting"), "{log}");
        assert_eq!(http.posts.len(), 1);
        // Control: a head that stays aaa dispatches at once, so the move is what made it wait.
        let mut http = Script::new(vec![runs_full(&[(
            8372,
            "mac2.yml",
            "refs/heads/train/t",
            "workflow_dispatch",
            "aaa",
        )])]);
        http.branch_sha = "aaa".into();
        http.jobs = vec![(8372, vec![jobs(&[("rust (macos)", "macos-latest", "in_progress")])].into())];
        let (r, log) = go_ref(&mut http, "mac.yml", "train/t");
        assert_eq!(r.unwrap(), Dispatched::Yes { waited: Duration::ZERO }, "{log}");
    }

    #[test]
    fn an_unreadable_branch_is_logged_with_its_cause_and_the_dispatch_stays_strict() {
        let mut http = Script::new(vec![runs_full(&[(
            8372,
            "mac2.yml",
            "refs/heads/train/t",
            "workflow_dispatch",
            "aaa",
        )])]);
        http.jobs = vec![(8372, vec![jobs(&[("rust (macos)", "macos-latest", "in_progress")])].into())];
        let (r, log) = go_ref(&mut http, "mac.yml", "train/t");
        assert!(matches!(r.unwrap(), Dispatched::StillBusy { .. }), "{log}");
        assert!(
            log.contains("head commit is unknown (branch train/t not found (HTTP 404))"),
            "{log}"
        );
    }

    #[test]
    fn a_record_whose_branch_disagrees_with_its_workflow_path_is_not_a_train_run() {
        let mut r = RunInfo {
            id: 1,
            path: "mac.yml@refs/heads/train/t".into(),
            event: "workflow_dispatch".into(),
            status: "in_progress".into(),
            conclusion: String::new(),
            head_sha: "aaa".into(),
            head_branch: "train/t".into(),
            created_at: String::new(),
            run_attempt: 1,
            title: String::new(),
        };
        assert_eq!(train_identity(&r), Some(("train/t".into(), "aaa".into())));
        r.head_branch = "train/other".into();
        assert_eq!(train_identity(&r), None);
    }

    /// A train's workflow that does not wait for itself does not wait for its own train's other workflows, and still waits
    /// for a foreign one.
    #[test]
    fn a_train_dispatch_does_not_wait_for_its_own_trains_other_workflows_but_does_for_a_foreign_one() {
        let jobs_mac = || vec![jobs(&[("rust (macos)", "macos-latest", "in_progress")])];
        let mut http = Script::new(vec![runs_full(&[(
            8372,
            "mac2.yml",
            "refs/heads/train/t",
            "workflow_dispatch",
            "aaa",
        )])]);
        http.branch_sha = "aaa".into();
        http.jobs = vec![(8372, jobs_mac().into())];
        let (r, log) = go_ref(&mut http, "mac.yml", "train/t");
        assert_eq!(r.unwrap(), Dispatched::Yes { waited: Duration::ZERO }, "{log}");
        // Controls: another head, another train, an unreadable branch: all wait.
        for (sibling_ref, branch_sha) in [
            ("refs/heads/train/t", "bbb"),
            ("refs/heads/train/u", "aaa"),
            ("refs/heads/train/t", ""),
        ] {
            let mut http = Script::new(vec![runs_full(&[(8372, "mac2.yml", sibling_ref, "workflow_dispatch", "aaa")])]);
            http.branch_sha = branch_sha.into();
            http.jobs = vec![(8372, jobs_mac().into())];
            let (r, log) = go_ref(&mut http, "mac.yml", "train/t");
            assert!(
                matches!(r.unwrap(), Dispatched::StillBusy { .. }),
                "{sibling_ref} {branch_sha}: {log}"
            );
        }
    }

    #[test]
    fn a_train_run_with_no_known_commit_is_never_taken_for_a_sibling() {
        let mut http = joint_world("refs/heads/train/t", "workflow_dispatch", "");
        let (r, _, _) = wait_here(&mut http, 8373, "1m");
        assert!(matches!(r, Ok(RunnerWait::StillBusy { .. })));
    }

    #[test]
    fn an_older_run_with_only_linux_jobs_left_does_not_hold_the_train_and_a_queued_mac_job_does() {
        let mut http = train_world(vec![jobs(&[
            ("rust (macos)", "macos-latest", "completed"),
            ("e2e", "ubuntu-latest", "in_progress"),
        ])]);
        let (r, _log, t) = wait_here(&mut http, 8120, "1h");
        assert_eq!(r.unwrap(), RunnerWait::Free { waited: Duration::ZERO });
        assert_eq!(t, Duration::ZERO);
        // A Mac job that has not started yet (queued behind the runner, or waiting for its needs) still claims the runner.
        let mut http = train_world(vec![jobs(&[
            ("plan", "ubuntu-latest", "in_progress"),
            ("rust (macos)", "macos-latest", "waiting"),
        ])]);
        let (r, log, _) = wait_here(&mut http, 8120, "1m");
        assert!(
            matches!(r.unwrap(), RunnerWait::StillBusy { last } if last.contains("run 8112 (mac.yml) has job rust (macos) (waiting) on macOS")),
            "{log}"
        );
        assert!(
            log.contains("::error::runner-wait: run 8120 (waits.yml) still waits after 1m"),
            "{log}"
        );
    }

    #[test]
    fn the_own_run_is_read_again_after_an_error_and_only_a_lasting_error_fails_open() {
        let mut http = train_world(vec![jobs(&[("rust (macos)", "macos-latest", "completed")])]);
        // The client does not retry (the shared HTTP layer does); one failure fails one read, so the second read comes 15 s later.
        http.own_errors = 1;
        let (r, log, t) = wait_here(&mut http, 8120, "1h");
        assert_eq!(
            r.unwrap(),
            RunnerWait::Free {
                waited: Duration::from_secs(15)
            },
            "the reading time counts as waited: {log}"
        );
        assert_eq!(
            t,
            Duration::from_secs(15),
            "one read failed after its own retries, the next 15 s later worked"
        );
        assert!(!log.contains("fail open"), "{log}");
        let mut http = train_world(vec![jobs(&[("rust (macos)", "macos-latest", "in_progress")])]);
        http.own_errors = 99;
        let (r, log, _) = wait_here(&mut http, 8120, "1h");
        assert!(matches!(r.unwrap(), RunnerWait::Unreadable(e) if e.contains("timed out")));
        assert!(
            log.contains("::warning::runner-wait: run 8120 could not be read") && log.contains("fail open"),
            "{log}"
        );
    }

    #[test]
    fn every_dispatch_waits_only_for_unfinished_mac_jobs_of_the_runs_before_it() {
        // D-1 (user, 2026-11-15): a run whose Mac jobs are done does not hold the Mac runner while its Linux jobs run.
        let mut http = Script::new(vec![runs(&[(8112, "mac.yml", "in_progress")])]);
        http.jobs = vec![(
            8112,
            vec![
                jobs(&[
                    ("rust (macos)", "macos-latest", "in_progress"),
                    ("report", "ubuntu-latest", "waiting"),
                ]),
                jobs(&[
                    ("rust (macos)", "macos-latest", "completed"),
                    ("report", "ubuntu-latest", "in_progress"),
                ]),
            ]
            .into(),
        )];
        let (r, log, t) = go(&mut http, "mac.yml", "1h");
        assert_eq!(
            r.unwrap(),
            Dispatched::Yes {
                waited: Duration::from_secs(15)
            },
            "{log}"
        );
        assert_eq!(t, Duration::from_secs(15));
        assert_eq!(http.posts.len(), 1, "dispatched while run 8112 still runs its Linux job");
    }

    #[test]
    fn an_unfinished_job_with_an_unresolved_or_empty_label_counts_its_run_by_the_workflow_file() {
        // Review round 3: a matrix job may sit as `${{ matrix.os }}` (or a caller with no labels) until its plan has run; an
        // unknown label is busy, judged by the run's workflow file (mac.yml runs on macOS).
        for label in [json!(["${{ matrix.os }}"]), json!([]), json!(["self-hosted-x"])] {
            let older = json!({"jobs": [
                {"id": 1, "name": "plan", "labels": ["ubuntu-latest"], "status": "in_progress"},
                {"id": 2, "name": "crate", "labels": label, "status": "waiting"}
            ]});
            let mut http = train_world(vec![older]);
            let (r, log, _) = wait_here(&mut http, 8120, "1m");
            assert!(
                matches!(r.as_ref().unwrap(), RunnerWait::StillBusy { last } if last.contains("run 8112")),
                "{label}: {r:?} {log}"
            );
        }
        // A known Linux label alone does not hold it.
        let older = json!({"jobs": [{"id": 1, "name": "plan", "labels": ["ubuntu-latest"], "status": "in_progress"}]});
        let mut http = train_world(vec![older]);
        let (r, _, _) = wait_here(&mut http, 8120, "1m");
        assert_eq!(r.unwrap(), RunnerWait::Free { waited: Duration::ZERO });
    }

    #[test]
    fn a_linux_only_train_run_never_waits() {
        let mut http = train_world(vec![jobs(&[("rust (macos)", "macos-latest", "in_progress")])]);
        http.own = vec![(8120, "linux.yml@refs/heads/train/t".into())];
        let (r, log, _) = wait_here(&mut http, 8120, "1m");
        assert_eq!(r.unwrap(), RunnerWait::Free { waited: Duration::ZERO });
        assert_eq!(http.gets, 0, "{log}");
    }

    #[test]
    fn busy_past_max_gives_the_error_and_dispatches_nothing() {
        let mut http = Script::new(vec![runs(&[(7961, "mac2.yml", "in_progress")])]);
        let (r, log, t) = go(&mut http, "mac.yml", "1m");
        match r.unwrap() {
            Dispatched::StillBusy { last } => assert!(last.contains("run 7961"), "{last}"),
            other => panic!("{other:?}"),
        }
        assert!(http.posts.is_empty());
        assert_eq!(t, Duration::from_secs(60));
        assert!(
            log.contains("::error::dispatch: not dispatching mac.yml: still busy after 1m"),
            "{log}"
        );
        assert!(log.contains("run 7961") && log.contains("Next:"), "{log}");
        assert!(!log.contains("secret-token"));
    }

    fn train_run(id: u64, wf: &str, status: &str) -> Value {
        json!({"total_count": 1, "workflow_runs": [{
            "id": id, "path": format!("{wf}@refs/heads/train/2026-11-27-y"), "status": status, "event": "workflow_dispatch"
        }]})
    }

    #[test]
    fn a_trial_waits_while_a_train_run_of_its_workflow_is_queued() {
        // Run 9247 (a trial) took d-gtr9's slots while train run 9248 queued: the trial yields, then dispatches.
        let mut http = Script::new(vec![train_run(9248, "linux.yml", "queued"), runs(&[])]);
        let (r, log) = go_ref(&mut http, "linux.yml", "refs/heads/trial/fix/x");
        match r.unwrap() {
            Dispatched::Yes { waited } => assert!(waited >= Duration::from_secs(15), "{waited:?}\n{log}"),
            other => panic!("{other:?}\n{log}"),
        }
        assert!(log.contains("waiting for the train run of linux.yml to finish"), "{log}");
        assert_eq!(http.posts.len(), 1);
    }

    #[test]
    fn a_trial_that_never_gets_the_train_out_ends_with_an_error_naming_the_train_run() {
        // Positive control: the train run stays, so the trial is not sent and the error names the run.
        let mut http = Script::new(vec![train_run(9248, "linux.yml", "in_progress")]);
        let (r, log) = go_ref(&mut http, "linux.yml", "refs/heads/trial/fix/x");
        assert!(matches!(r.unwrap(), Dispatched::StillBusy { .. }), "{log}");
        assert!(
            log.contains("::error::dispatch: not dispatching linux.yml") && log.contains("train run 9248"),
            "{log}"
        );
        assert!(http.posts.is_empty(), "nothing is dispatched while the train runs");
    }

    #[test]
    fn the_train_wait_alone_sends_nothing_so_a_trial_can_wait_before_it_pushes_its_ref() {
        let req = parse_request("linux.yml", "trial/fix/x", &[], "1m", Some("15s")).unwrap();
        let target = Target {
            server: SERVER,
            repo: REPO,
            token: "secret-token",
        };
        let ask = |http: &mut Script| {
            let (mut clock, mut log) = (FakeClock(Duration::ZERO), Vec::new());
            let r = yield_to_trains(http, &target, &req, &mut clock, &mut log, None);
            (r, String::from_utf8(log).unwrap())
        };
        let mut held = Script::new(vec![train_run(9248, "linux.yml", "queued")]);
        let (r, log) = ask(&mut held);
        assert!(matches!(r, Ok(Err(Dispatched::StillBusy { .. }))), "{log}");
        assert!(log.contains("train run 9248") && held.posts.is_empty(), "{log}");
        // Positive control: with the train run gone it is free at once and still sends nothing.
        let mut free = Script::new(vec![runs(&[])]);
        let (r, _) = ask(&mut free);
        assert!(matches!(r, Ok(Ok(d)) if d.is_zero()));
        assert!(free.posts.is_empty());
    }

    #[test]
    fn a_train_never_waits_for_a_trial_and_a_trial_ignores_other_workflows() {
        // The train dispatches at once whatever non-train runs are going (positive control of the one-way rule) ...
        let trial = json!({"total_count": 1, "workflow_runs": [{
            "id": 9247, "path": "linux.yml@refs/heads/sync/m/abc", "status": "in_progress", "event": "workflow_dispatch"
        }]});
        let mut http = Script::new(vec![trial]);
        let (r, log) = go_ref(&mut http, "linux.yml", "train/2026-11-27-z");
        assert_eq!(r.unwrap(), Dispatched::Yes { waited: Duration::ZERO }, "{log}");
        assert_eq!(http.gets, 0, "a train asks nothing about trials: {log}");
        // ... and a train run of ANOTHER workflow does not hold a trial.
        let mut http = Script::new(vec![train_run(9249, "mac.yml", "queued")]);
        let (r, log) = go_ref(&mut http, "linux.yml", "refs/heads/trial/fix/x");
        assert_eq!(r.unwrap(), Dispatched::Yes { waited: Duration::ZERO }, "{log}");
    }

    #[test]
    fn another_runner_class_busy_dispatches_at_once() {
        let mut http = Script::new(vec![runs(&[(7967, "mac.yml", "in_progress"), (7968, "matrix.yml", "queued")])]);
        let (r, _log, t) = go(&mut http, "linux.yml", "1m");
        assert_eq!(r.unwrap(), Dispatched::Yes { waited: Duration::ZERO });
        assert_eq!(t, Duration::ZERO);
        assert_eq!(
            http.gets, 1,
            "a Linux-only workflow needs no run list for a class, only the one look for a train run"
        );
        assert_eq!(http.posts.len(), 1);
    }

    #[test]
    fn a_finished_run_or_a_run_of_another_class_does_not_block() {
        let mut http = Script::new(vec![runs(&[(1, "mac.yml", "completed"), (2, "linux.yml", "in_progress")])]);
        let (r, _l, t) = go(&mut http, "mac.yml", "1m");
        assert_eq!(r.unwrap(), Dispatched::Yes { waited: Duration::ZERO });
        assert_eq!(t, Duration::ZERO);
    }

    #[test]
    fn classes_follow_matrices_calls_and_ignore_comments() {
        let c = |w| classes_of(w, &files).unwrap();
        assert_eq!(c("matrix.yml"), BTreeSet::from([RunnerClass::Windows]));
        assert_eq!(c("caller.yml"), BTreeSet::from([RunnerClass::Macos]));
        assert!(c("comment.yml").is_empty());
        // A planned matrix is written by the planner at run time: it counts as every runner the planner can emit (ADR 0123 row 54).
        assert_eq!(c("planned.yml"), BTreeSet::from([RunnerClass::Macos, RunnerClass::Windows]));
        assert!(classes_of("missing.yml", &files).unwrap_err().contains("not in this checkout"));
    }

    #[test]
    fn classes_come_from_runs_on_not_from_other_text() {
        let c = |w| classes_of(w, &files).unwrap();
        assert!(
            c("step-text.yml").is_empty(),
            "a step that mentions macOS or Windows is not a Mac job"
        );
        // debrief-bench.yml is all ubuntu-latest and its steps mention macOS and Windows.
        assert!(c("bench.yml").is_empty());
        // debrief.yml has a matrix with all three OS and a macos-latest job.
        assert_eq!(c("debrief.yml"), BTreeSet::from([RunnerClass::Macos, RunnerClass::Windows]));
    }

    #[test]
    fn every_workflow_of_the_repository_is_read_by_the_job_model() {
        // ci.yml's planned `crate` and `paddock-dev` legs reach the Mac and Windows runners (run 9248: no class was found).
        let read = |n: &str| std::fs::read_to_string(repo_root().join(".gitea/workflows").join(n)).ok();
        assert_eq!(
            classes_of("ci.yml", &read).unwrap(),
            BTreeSet::from([RunnerClass::Macos, RunnerClass::Windows])
        );
        for e in std::fs::read_dir(repo_root().join(".gitea/workflows")).unwrap() {
            let name = e.unwrap().file_name().to_string_lossy().to_string();
            if name.ends_with(".yml") {
                classes_of(&name, &read).unwrap_or_else(|e| panic!("{name}: {e}"));
            }
        }
    }

    #[test]
    fn a_bad_request_is_refused_with_the_fix() {
        assert!(
            parse_request("a.yml", "main", &["novalue".into()], "1m", None)
                .unwrap_err()
                .contains("key=value")
        );
        assert!(parse_request("a.yml", "main", &[], "0s", None).is_err());
        assert!(parse_request("../a.yml", "main", &[], "1m", None).is_err());
    }

    fn titled(list: &[(u64, &str, &str)]) -> Value {
        json!({"total_count": list.len(), "workflow_runs": list.iter().map(|(id, wf, title)| json!({
            "id": id, "path": format!("{wf}@refs/heads/train/t"), "status": "queued", "event": "workflow_dispatch",
            "display_title": title
        })).collect::<Vec<_>>()})
    }

    fn find(http: &mut Script, id: &str) -> (Result<Option<u64>, String>, Duration) {
        let mut clock = FakeClock(Duration::ZERO);
        let target = Target {
            server: SERVER,
            repo: REPO,
            token: "secret-token",
        };
        let r = find_run(http, &target, "ci.yml", id, &mut clock, FIND_MAX, FIND_INTERVAL, &mut Vec::new());
        (r, clock.0)
    }

    #[test]
    fn a_dispatch_finds_its_own_run_by_its_token_never_the_newest_run() {
        let mine = "0123456789abcdef.full";
        let other = "fedcba9876543210.full";
        // First look: only another dispatch's run (the newest). Second: ours appears, older than a newer stranger.
        let mut http = Script::new(vec![
            titled(&[(9001, "ci.yml", &format!("ci [dispatch {other}]"))]),
            titled(&[
                (9002, "ci.yml", "chore(train): merge x"),
                (9001, "ci.yml", &format!("ci [dispatch {other}]")),
                (9000, "ci.yml", &format!("ci [dispatch {mine}]")),
            ]),
        ]);
        let (r, t) = find(&mut http, mine);
        assert_eq!(r.unwrap(), Some(9000));
        assert_eq!(t, FIND_INTERVAL, "found on the second look");
        // Positive control: when no run shows the token the finder never takes the newest run; it ends at its limit with none.
        let mut http = Script::new(vec![titled(&[(9002, "ci.yml", &format!("ci [dispatch {other}]"))])]);
        let (r, t) = find(&mut http, mine);
        assert_eq!(r.unwrap(), None);
        assert!(t >= FIND_MAX);
        // Another workflow's run with the token does not count, and two runs with it are an error, not a pick.
        let mut http = Script::new(vec![titled(&[(9003, "debrief.yml", &format!("debrief [dispatch {mine}]"))])]);
        assert_eq!(find(&mut http, mine).0.unwrap(), None);
        let mut http = Script::new(vec![titled(&[
            (9005, "ci.yml", &format!("ci [dispatch {mine}]")),
            (9004, "ci.yml", &format!("ci [dispatch {mine}]")),
        ])]);
        assert!(find(&mut http, mine).0.unwrap_err().contains("not unique"));
    }

    #[test]
    fn a_dispatch_token_carries_the_run_kind_and_reads_back_from_a_title() {
        let kv = |i: &[(&str, &str)]| i.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>();
        assert_eq!(RunKind::of(&kv(&[("webkit", "true")])), RunKind::Full);
        assert_eq!(RunKind::of(&kv(&[("base", ""), ("landing_base", "abc")])), RunKind::Full);
        assert_eq!(RunKind::of(&kv(&[("since_run", "8868")])), RunKind::Since(8868));
        assert_eq!(RunKind::of(&kv(&[("base", "abc")])), RunKind::Member);
        assert_eq!(RunKind::of(&kv(&[("os", "linux")])), RunKind::Part);
        for kind in [RunKind::Full, RunKind::Since(8868), RunKind::Member, RunKind::Part] {
            let id = new_dispatch_id(&kind);
            let tag = DispatchTag::of_title(&format!("ci [dispatch {id}]")).unwrap();
            assert_eq!((tag.id.as_str(), &tag.kind), (id.as_str(), &kind));
        }
        assert_ne!(
            new_dispatch_id(&RunKind::Full),
            new_dispatch_id(&RunKind::Full),
            "each dispatch has its own token"
        );
        // Positive controls: a commit subject, a cut tag or an unknown kind is no tag.
        for title in [
            "chore(train): merge feat/x",
            "ci [dispatch 0123456789abcdef.full",
            "ci [dispatch 0123.full]",
            "ci [dispatch 0123456789abcdef.later]",
            "ci [dispatch zz23456789abcdef.full]",
        ] {
            assert_eq!(DispatchTag::of_title(title), None, "{title}");
        }
    }

    #[test]
    fn the_token_is_sent_only_to_a_workflow_that_declares_it() {
        let (inputs, id) = with_dispatch_id("tagged.yml", &files, vec![("since_run".into(), "8868".into())]);
        let id = id.unwrap();
        assert!(id.ends_with(".since-8868"), "{id}");
        assert_eq!(inputs.last(), Some(&(DISPATCH_ID.to_string(), id)));
        // Positive control: a workflow without the input gets nothing it would refuse.
        let (inputs, id) = with_dispatch_id("linux.yml", &files, vec![]);
        assert!(inputs.is_empty() && id.is_none());
        // A caller's own token is kept.
        let (_, id) = with_dispatch_id("tagged.yml", &files, vec![(DISPATCH_ID.into(), "0123456789abcdef.full".into())]);
        assert_eq!(id.as_deref(), Some("0123456789abcdef.full"));
    }

    #[test]
    fn every_workflow_dispatched_through_ci_dispatch_declares_the_token_and_shows_it_in_its_run_name() {
        let read = |name: &str| std::fs::read_to_string(repo_root().join(".gitea/workflows").join(name)).ok();
        for wf in [
            "ci.yml",
            "debrief.yml",
            "dyno-release.yml",
            "pitcrew-release.yml",
            "pitwall-release.yml",
            "spotter-release.yml",
            "shell-demo-release.yml",
            "workbench-demo-release.yml",
        ] {
            assert!(declares_dispatch_id(wf, &read), "{wf} declares no {DISPATCH_ID} input");
            let doc = paddock_ci_config::yaml::parse(&read(wf).unwrap()).unwrap();
            let run_name = doc.get_text("run-name").unwrap_or_default().to_string();
            assert!(
                run_name.contains("[dispatch {1}]") && run_name.contains("github.event.inputs.dispatch_id"),
                "{wf}: run-name {run_name:?} does not show the token as `[dispatch <id>]`"
            );
        }
        // Positive control: a workflow without it is seen.
        assert!(!declares_dispatch_id("nightly.yml", &read));
    }
}
