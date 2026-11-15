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

use super::client::{Client, RunInfo};
use crate::wait::{Clock, Http, MAX_LIMIT, MIN_INTERVAL, Outcome, Poll, Probe, SystemClock, fmt_duration, parse_duration, wait_for};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::Write;
use std::time::Duration;
use yaml_rust2::{Yaml, YamlLoader};

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
/// labels (a `${{ matrix.x }}` label stands for every value of the job's matrix) and never from other text of the file: a step
/// that mentions macOS does not make a Linux job a Mac job. `Err` when the file cannot be read or parsed.
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
        let docs = match YamlLoader::load_from_str(&text) {
            Ok(d) => d,
            Err(e) if name == workflow => return Err(format!("workflow file {workflow} is not valid YAML: {e}")),
            Err(_) => continue,
        };
        for doc in &docs {
            let Some(jobs) = doc["jobs"].as_hash() else { continue };
            for (_, job) in jobs {
                if let Some(called) = job["uses"].as_str().and_then(called_workflow) {
                    todo.push(called);
                }
                let mut labels = Vec::new();
                collect_strings(&job["runs-on"], &mut labels);
                let mut from_matrix = Vec::new();
                collect_strings(&job["strategy"]["matrix"], &mut from_matrix);
                for label in labels {
                    if label.contains("${{") {
                        classify_all(&from_matrix, &mut classes);
                    } else {
                        classify_all(&[label], &mut classes);
                    }
                }
            }
        }
    }
    Ok(classes)
}

/// Every string scalar under `y`, whatever the nesting.
fn collect_strings(y: &Yaml, out: &mut Vec<String>) {
    match y {
        Yaml::String(s) => out.push(s.clone()),
        Yaml::Array(a) => a.iter().for_each(|v| collect_strings(v, out)),
        Yaml::Hash(h) => h.values().for_each(|v| collect_strings(v, out)),
        _ => {}
    }
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
}

/// How many pages of 50 runs are read at most for the unfinished ones (the newest first).
const BUSY_PAGES: u32 = 4;

impl BusyProbe<'_, '_> {
    /// The unfinished runs, newest first: pages until a short page, a page whose newest run is older than a day (a run the
    /// server never finished is not taken as busy for longer, as in schedule-skip), or [`BUSY_PAGES`].
    fn unfinished(&mut self) -> Result<Vec<RunInfo>, String> {
        let now_ms = super::time::now_secs() as f64 * 1000.0;
        let mut out = Vec::new();
        for page in 1..=BUSY_PAGES {
            let p = self.client.runs_page(page)?;
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

    /// The unfinished runs that share a class, as text, or `None` when there are none.
    fn busy(&mut self) -> Result<Option<String>, String> {
        let runs = self.unfinished()?;
        let older_than = self.older_than;
        for run in runs.iter().filter(|r| older_than.is_none_or(|own| r.id < own)) {
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
        Ok(match self.busy()? {
            None => Poll::Done("no other run is on the same runner".into()),
            Some(why) => Poll::Pending(why),
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
) -> Result<Dispatched, String> {
    let Target { server, repo, token } = *target;
    let wanted = classes_of(&req.workflow, read)?;
    let mut waited = Duration::ZERO;
    if wanted.is_empty() {
        let _ = writeln!(
            log,
            "dispatch: {} uses no single-machine runner (Linux only); not waiting",
            req.workflow
        );
    } else {
        if is_train(&req.git_ref) && waits_itself(&req.workflow, read) {
            // Staggered: each queued train run holds a Linux slot in its runner-wait job, so at most MAX_TRAIN_WAITERS wait at
            // once and a Linux slot stays free for the older runs' own Linux jobs.
            let mut client = Client::new(http, server, repo, token);
            client.backoff_ms = 0;
            let mut probe = WaitersProbe { client: &mut client, read };
            let waited = match wait_for(&mut probe, clock, req.max, req.interval, log) {
                Outcome::Done { elapsed, .. } => elapsed,
                Outcome::TimedOut { last, max } => {
                    let _ = writeln!(
                        log,
                        "::error::dispatch: not dispatching {}: {last} after {} (--max). Next: wait for one of those train runs to start its Mac or Windows jobs, then dispatch again.",
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
        let mut client = Client::new(http, server, repo, token);
        client.backoff_ms = 0;
        let mut probe = BusyProbe {
            client: &mut client,
            read,
            wanted,
            older_than: None,
        };
        match wait_for(&mut probe, clock, req.max, req.interval, log) {
            Outcome::Done { elapsed, .. } => waited = elapsed,
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
    }
    send(http, target, req, waited, log)
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

/// The Linux runner slots (three: docs/ci-schedules.md, canary.yml). Each queued train run takes one in its runner-wait job.
pub const LINUX_SLOTS: usize = 3;

/// At most this many train runs wait in their runner-wait job at once: one Linux slot stays free for the older runs' Linux jobs
/// (a Mac job that needs them would otherwise never start, and every waiter would stall to its limit).
pub const MAX_TRAIN_WAITERS: usize = LINUX_SLOTS - 1;

/// Waits while [`MAX_TRAIN_WAITERS`] or more train runs (unfinished `workflow_dispatch` runs on `train/*` of a workflow that
/// waits itself) have not started a Mac or Windows job yet: they are, or will be, in their runner-wait job.
struct WaitersProbe<'a, 'b> {
    client: &'a mut Client<'b>,
    read: ReadWorkflow<'a>,
}

impl WaitersProbe<'_, '_> {
    fn waiting(&mut self) -> Result<Vec<u64>, String> {
        let page = self.client.runs_page_where(1, "event=workflow_dispatch")?;
        let mut ids = Vec::new();
        for r in page
            .runs
            .iter()
            .filter(|r| !r.finished() && is_train(r.git_ref()) && waits_itself(r.workflow(), self.read))
        {
            let started = self.client.timed_jobs(r.id)?.unwrap_or_default().iter().any(|j| {
                let mut c = BTreeSet::new();
                classify_all(&j.labels, &mut c);
                !c.is_empty() && j.status != "waiting" && j.status != "blocked" && !j.status.is_empty()
            });
            if !started {
                ids.push(r.id);
            }
        }
        Ok(ids)
    }
}

impl Probe for WaitersProbe<'_, '_> {
    fn describe(&self) -> String {
        format!("fewer than {MAX_TRAIN_WAITERS} train runs waiting for their runner ({LINUX_SLOTS} Linux slots)")
    }
    fn poll(&mut self) -> Result<Poll, String> {
        let w = self.waiting()?;
        Ok(if w.len() < MAX_TRAIN_WAITERS {
            Poll::Done(format!("{} train run(s) waiting", w.len()))
        } else {
            Poll::Pending(format!("train runs {w:?} are waiting for their runner"))
        })
    }
    fn next_step(&self) -> String {
        "wait for one of those train runs to start its Mac or Windows jobs, then dispatch again".into()
    }
}

/// The composite action a workflow's first job runs to wait for its single-machine runner inside the run.
pub const RUNNER_WAIT_ACTION: &str = "./.gitea/actions/runner-wait";

/// Whether a ref is a train's (`train/...`, also spelled `refs/heads/train/...`).
pub fn is_train(git_ref: &str) -> bool {
    git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref).starts_with("train/")
}

/// Whether `workflow` waits for its runner itself: a job with no `needs` (a first job) has a step that uses [`RUNNER_WAIT_ACTION`].
/// Conformance (ci-actions.test.mjs) checks that every single-machine job of such a workflow needs that job.
pub fn waits_itself(workflow: &str, read: ReadWorkflow<'_>) -> bool {
    let Some(text) = read(workflow) else { return false };
    let Ok(docs) = YamlLoader::load_from_str(&text) else { return false };
    docs.iter().any(|doc| {
        doc["jobs"].as_hash().is_some_and(|jobs| {
            jobs.values().any(|job| {
                job["needs"].is_badvalue()
                    && job["steps"]
                        .as_vec()
                        .is_some_and(|steps| steps.iter().any(|s| s["uses"].as_str() == Some(RUNNER_WAIT_ACTION)))
            })
        })
    })
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

/// How many times `ci runner-wait` reads its own run before it gives up and goes on (each read already retries in the client).
pub const OWN_RUN_READS: u32 = 5;

/// `ci runner-wait`: the first job of a train run waits, on a Linux runner, until no older run has an unfinished job on one of
/// its workflow's single-machine runner classes (at most `max`). `own` is this run's id; its workflow file is read from the run
/// itself (`ci.yml@refs/heads/train/...`), retried [`OWN_RUN_READS`] times `interval` apart before it fails open. `backoff_ms`
/// is the client's wait between its own retries (tests: 0).
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
    backoff_ms: u64,
) -> Result<RunnerWait, String> {
    let mut client = Client::new(http, target.server, target.repo, target.token);
    client.backoff_ms = backoff_ms;
    let mut workflow = None;
    let mut last = String::new();
    for attempt in 1..=OWN_RUN_READS {
        match client.run(own) {
            Ok(Some(r)) => {
                workflow = Some(r.workflow().to_string());
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
                return Ok(json!({"id": id, "path": path, "status": "in_progress", "event": "workflow_dispatch"}));
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
            "comment.yml" => Some("# runs-on: macos-latest\njobs:\n  t:\n    runs-on: ubuntu-latest\n".into()),
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
        );
        (r, String::from_utf8(log).unwrap(), clock.0)
    }

    #[test]
    fn busy_then_free_dispatches_after_the_wait() {
        let mut http = Script::new(vec![
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
        let req = parse_request(wf, git_ref, &[], "10m", Some("15s")).unwrap();
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
        let waiting = || vec![jobs(&[("plan", "ubuntu-latest", "in_progress"), ("t", "macos-latest", "waiting")])];
        http.jobs = vec![
            (
                8130,
                VecDeque::from(vec![
                    jobs(&[("plan", "ubuntu-latest", "in_progress"), ("t", "macos-latest", "waiting")]),
                    jobs(&[("plan", "ubuntu-latest", "completed"), ("t", "macos-latest", "queued")]),
                ]),
            ),
            (8131, waiting().into()),
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
        assert_eq!(MAX_TRAIN_WAITERS, LINUX_SLOTS - 1);
    }

    #[test]
    fn any_other_dispatch_still_waits_here() {
        for (wf, git_ref) in [
            ("waits.yml", "main"),
            ("mac.yml", "train/2026-11-15-12"),
            ("waits-late.yml", "train/t"),
        ] {
            let mut http = Script::new(vec![
                runs(&[(8112, "mac.yml", "in_progress")]),
                runs(&[(8112, "mac.yml", "completed")]),
            ]);
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
            0,
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
        // The client retries each read 4 times itself; 5 failures outlast one read, so the second read comes 15 s later.
        http.own_errors = 5;
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

    #[test]
    fn another_runner_class_busy_dispatches_at_once() {
        let mut http = Script::new(vec![runs(&[(7967, "mac.yml", "in_progress"), (7968, "matrix.yml", "queued")])]);
        let (r, _log, t) = go(&mut http, "linux.yml", "1m");
        assert_eq!(r.unwrap(), Dispatched::Yes { waited: Duration::ZERO });
        assert_eq!(t, Duration::ZERO);
        assert_eq!(http.gets, 0, "a Linux-only workflow needs no run list");
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
    fn a_bad_request_is_refused_with_the_fix() {
        assert!(
            parse_request("a.yml", "main", &["novalue".into()], "1m", None)
                .unwrap_err()
                .contains("key=value")
        );
        assert!(parse_request("a.yml", "main", &[], "0s", None).is_err());
        assert!(parse_request("../a.yml", "main", &[], "1m", None).is_err());
    }
}
