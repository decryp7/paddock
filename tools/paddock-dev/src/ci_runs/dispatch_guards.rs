// ci-runs-reader: ci dispatch's guards read a train's full runs and failed-only rounds, and main's newest nightly and push run
//! The guards `./dev ci dispatch` runs before a train dispatch (user, 2026-11-24).
//!
//! - **failed-first** (ER-10): on a `train/*` ref, a full dispatch of a workflow that takes `since_run` is refused while the train's
//!   last full run of that workflow is red and no failed-only round is green on the branch's current head. The refusal prints the
//!   exact failed-only command (`--input since_run=<the newest red run>`). The coordinator's override is `--full-anyway "<reason>"`,
//!   which is logged.
//! - **green-main**: a train dispatch (a failed-only round of a train already in CI excepted) reads the newest finished nightly on main
//!   and the newest finished push run of ci.yml on main and sorts their failed jobs with [`active_apps::classify_nightly`] under the
//!   active set of each run's commit: a failure of the foundation or an active app refuses (the jobs and the next step are named); a
//!   failure of an inactive app is printed as that app's fix item and does not refuse. The override is `--red-main-anyway "<reason>"`
//!   (a train that carries the fix), logged.
//!
//! Both start report-only (ER-41): they print what they would refuse as a `::warning::` and the dispatch goes on, until their first
//! green use is recorded by run id in [`REGISTER`] (`./dev ci guard-record`, which switches the guard to blocking in the same step).
//! Anything unreadable is "not measured", never a pass (ER-33): report-only prints it, blocking refuses it.
//!
//! A run's kind is read from its dispatch token ([`DispatchTag`], in the title through the workflow's `run-name`); a run without one
//! (dispatched before the token, or by another tool) is read from its plan job's mode line ([`read_marker`]): `full=true` is a full
//! run, anything else is not a full run and never counts as a failed-only round.

use super::client::{Client, RunInfo, TimedJob, encode_path};
use super::dispatch::{DispatchTag, ReadWorkflow, Request, RunKind, Target, is_train};
use crate::active_apps;
use crate::plan_since::read_marker;
use crate::wait::Http;
use std::collections::{BTreeMap, BTreeSet};

/// Where each guard's first green use is recorded (ER-41), relative to the repository root.
pub const REGISTER: &str = "docs/ci-dispatch-guards.toml";

/// A guard of `ci dispatch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    /// A failed-only round before a full run (ER-10).
    FailedFirst,
    /// Main green in the foundation and the active apps before a train.
    GreenMain,
}

impl Guard {
    /// Its name in [`REGISTER`] and the log.
    pub fn name(self) -> &'static str {
        match self {
            Guard::FailedFirst => "failed-first",
            Guard::GreenMain => "green-main",
        }
    }

    /// The guard of a name.
    pub fn parse(name: &str) -> Option<Guard> {
        [Guard::FailedFirst, Guard::GreenMain].into_iter().find(|g| g.name() == name)
    }

    /// Its override flag.
    pub fn override_flag(self) -> &'static str {
        match self {
            Guard::FailedFirst => "--full-anyway",
            Guard::GreenMain => "--red-main-anyway",
        }
    }
}

/// Whether a guard refuses or only reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Report-only (ER-41): prints what it would refuse, the dispatch goes on. The text says why (no record yet, or an unreadable
    /// register).
    ReportOnly(String),
    /// Blocking since its first green use.
    Blocking {
        /// The run of that use.
        first_green: u64,
    },
}

/// The run id recorded for `guard` in the register text; `Err` for a text that is not the register's shape.
pub fn recorded(text: &str, guard: Guard) -> Result<Option<u64>, String> {
    let v: toml::Table = text.parse().map_err(|e| format!("{REGISTER} is not TOML: {e}"))?;
    let table = v
        .get("first_green")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("{REGISTER} has no [first_green] table"))?;
    if let Some(k) = table.keys().find(|k| Guard::parse(k).is_none()) {
        return Err(format!("{REGISTER}: first_green.{k} is not a guard (failed-first, green-main)"));
    }
    match table.get(guard.name()) {
        None => Ok(None),
        Some(id) => id
            .as_integer()
            .filter(|n| *n > 0)
            .map(|n| Some(n as u64))
            .ok_or_else(|| format!("{REGISTER}: first_green.{} is not a run id", guard.name())),
    }
}

/// The mode of `guard` from the register text (`None`: the file is missing or unreadable, with the reason): report-only unless a run
/// is recorded. An unreadable register never switches a guard on or off by accident: it stays report-only, with the reason.
pub fn mode_of(text: Result<&str, &str>, guard: Guard) -> Mode {
    match text.map_err(str::to_string).and_then(|t| recorded(t, guard)) {
        Ok(Some(first_green)) => Mode::Blocking { first_green },
        Ok(None) => Mode::ReportOnly(format!("no first green use is recorded in {REGISTER}")),
        Err(e) => Mode::ReportOnly(format!("{REGISTER} cannot be read ({e})")),
    }
}

/// The register text with `run` recorded for `guard`; refuses a guard already recorded (the first green use is recorded once).
pub fn with_first_green(text: &str, guard: Guard, run: u64) -> Result<String, String> {
    if let Some(id) = recorded(text, guard)? {
        return Err(format!(
            "{} is already recorded at run {id}: the first green use is recorded once",
            guard.name()
        ));
    }
    let mut out = text.trim_end().to_string();
    out.push_str(&format!("\n\"{}\" = {run}\n", guard.name()));
    match recorded(&out, guard)? {
        Some(id) if id == run => Ok(out),
        _ => Err(format!("{REGISTER} could not take the record for {}", guard.name())),
    }
}

/// What a guard found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// Nothing to refuse; the text says what was seen.
    Pass(String),
    /// The dispatch should not go out.
    Refuse {
        /// Why.
        why: String,
        /// The next step, with the exact command where there is one.
        next: String,
    },
    /// The guard could not look (ER-33): never a pass.
    NotMeasured(String),
}

/// What the dispatch does with a guard's finding.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// The dispatch goes on; the JSON row of the guard for the report.
    Go(serde_json::Value),
    /// The dispatch stops: blocking mode and a refusal (`not_measured: false`) or a finding that could not look (`true`).
    Stop {
        /// Whether the guard could not look (exit 6) rather than refused (exit 4).
        not_measured: bool,
        /// The message, the next step included.
        message: String,
    },
}

/// Applies a guard's mode and the coordinator's override (a non-empty reason) to its finding, and logs it: a pass as a plain line, a
/// refusal or not-measured as a `::warning::` (report-only, or overridden with the reason) or an `::error::` (blocking).
pub fn decide(guard: Guard, mode: &Mode, finding: &Finding, override_reason: Option<&str>, log: &mut dyn std::io::Write) -> Decision {
    let name = guard.name();
    let (mode_name, mode_why) = match mode {
        Mode::ReportOnly(why) => ("report-only", why.clone()),
        Mode::Blocking { first_green } => ("blocking", format!("blocking since run {first_green}")),
    };
    let row = |result: &str, detail: &str, overridden: Option<&str>| serde_json::json!({"guard": name, "mode": mode_name, "result": result, "detail": detail, "overridden": overridden});
    let (result, why, next) = match finding {
        Finding::Pass(seen) => {
            let _ = writeln!(log, "dispatch guard {name}: pass: {seen}");
            return Decision::Go(row("pass", seen, None));
        }
        Finding::Refuse { why, next } => ("refuse", why.clone(), next.clone()),
        Finding::NotMeasured(why) => (
            "not-measured",
            format!("not measured, never a pass (ER-33): {why}"),
            "make the runs readable (GITEA_TOKEN, the server) and dispatch again".to_string(),
        ),
    };
    if let Some(reason) = override_reason {
        let _ = writeln!(
            log,
            "::warning::dispatch guard {name}: overridden by the coordinator ({} \"{reason}\"): {why}",
            guard.override_flag()
        );
        return Decision::Go(row(result, &why, Some(reason)));
    }
    match mode {
        Mode::ReportOnly(_) => {
            let _ = writeln!(
                log,
                "::warning::dispatch guard {name} (report-only, ER-41: {mode_why}; the dispatch goes on) would refuse: {why}. Next: {next}."
            );
            Decision::Go(row(result, &why, None))
        }
        Mode::Blocking { .. } => {
            let message = format!("dispatch: refused by guard {name} ({mode_why}): {why}. Next: {next}.");
            let _ = writeln!(log, "::error::{message}");
            Decision::Stop {
                not_measured: matches!(finding, Finding::NotMeasured(_)),
                message,
            }
        }
    }
}

/// The coordinator's overrides, each with its logged reason (the queue has none).
#[derive(Debug, Clone, Copy, Default)]
pub struct Overrides<'a> {
    /// `--full-anyway`: the failed-first guard.
    pub full_anyway: Option<&'a str>,
    /// `--red-main-anyway`: the green-main guard.
    pub red_main_anyway: Option<&'a str>,
}

/// A blocking guard stopped the dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stopped {
    /// Whether the guard could not look (exit 6) rather than refused (exit 4).
    pub not_measured: bool,
    /// The message, the next step included (also logged as an `::error::`).
    pub message: String,
}

/// The guards of one dispatch, the one entry point of `ci dispatch` and the landing queue: nothing for a ref that is not a train's;
/// failed-first for a full run of a workflow that takes `since_run`; green-main for every train dispatch but a failed-only round. Each
/// is decided by its mode in [`REGISTER`] under `root` and its override. The JSON rows of the guards that let it go, or the stop.
#[allow(clippy::too_many_arguments)]
pub fn check_train(
    http: &mut dyn Http,
    target: &Target<'_>,
    root: &std::path::Path,
    read: ReadWorkflow<'_>,
    req: &Request,
    overrides: Overrides<'_>,
    now_ms: f64,
    log: &mut dyn std::io::Write,
) -> Result<Vec<serde_json::Value>, Stopped> {
    if !is_train(&req.git_ref) {
        return Ok(Vec::new());
    }
    let register = std::fs::read_to_string(root.join(REGISTER)).map_err(|e| e.to_string());
    let mode = |g: Guard| mode_of(register.as_deref().map_err(String::as_str), g);
    let kind = RunKind::of(&req.inputs);
    let branch = req.git_ref.strip_prefix("refs/heads/").unwrap_or(&req.git_ref).to_string();
    let Target { server, repo, token } = *target;
    let mut rows = Vec::new();
    let mut go = |d: Decision| match d {
        Decision::Go(row) => {
            rows.push(row);
            Ok(())
        }
        Decision::Stop { not_measured, message } => Err(Stopped { not_measured, message }),
    };
    if kind == RunKind::Full && takes_since_run(&req.workflow, read) {
        let mut client = Client::new(http, server, repo, token);
        let finding = match client.branch_head(&branch) {
            Ok(Some(head)) => match read_train(&mut client, &req.workflow, &branch) {
                Ok(runs) => failed_first(&runs, &head, &req.workflow, &branch),
                Err(e) => Finding::NotMeasured(format!("the runs of {} on {branch} cannot be read: {e}", req.workflow)),
            },
            Ok(None) => Finding::NotMeasured(format!("branch {branch} is not on the server")),
            Err(e) => Finding::NotMeasured(format!("the head of {branch} cannot be read: {e}")),
        };
        go(decide(
            Guard::FailedFirst,
            &mode(Guard::FailedFirst),
            &finding,
            overrides.full_anyway,
            log,
        ))?;
    }
    if !matches!(kind, RunKind::Since(_)) {
        let apps = apps_in(root);
        let main = {
            let mut client = Client::new(http, server, repo, token);
            read_main(&mut client)
        };
        let green = match main {
            Ok((nightly, push)) => green_main(nightly.as_ref(), push.as_ref(), &apps, now_ms, &mut |sha| {
                active_at_remote(http, server, repo, token, sha, &apps)
            }),
            Err(e) => GreenMain {
                finding: Finding::NotMeasured(format!("main's runs cannot be read: {e}")),
                fix_items: BTreeMap::new(),
            },
        };
        for (app, items) in &green.fix_items {
            for item in items {
                let _ = writeln!(
                    log,
                    "::notice::green-main: fix item of {app} (inactive, does not stop the train): {item}"
                );
            }
        }
        go(decide(
            Guard::GreenMain,
            &mode(Guard::GreenMain),
            &green.finding,
            overrides.red_main_anyway,
            log,
        ))?;
    }
    Ok(rows)
}

/// One run of a train's workflow, newest first, as the failed-first guard needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainRun {
    /// The run's id.
    pub id: u64,
    /// The commit it ran on.
    pub head: String,
    /// Whether it has finished.
    pub finished: bool,
    /// `success`, `failure`, `cancelled`... (empty while it runs).
    pub conclusion: String,
    /// Its kind; `None` when it cannot be told (no token and no readable mode line).
    pub kind: Option<RunKind>,
}

fn short(sha: &str) -> &str {
    sha.get(..10).unwrap_or(sha)
}

/// The failed-first guard over a train's runs of `workflow` (newest first) and the branch's current head.
pub fn failed_first(runs: &[TrainRun], head: &str, workflow: &str, git_ref: &str) -> Finding {
    let counted = |r: &TrainRun| r.finished && !matches!(r.conclusion.as_str(), "cancelled" | "skipped" | "");
    let mut last_full = None;
    for r in runs {
        match &r.kind {
            Some(RunKind::Full) if counted(r) => {
                last_full = Some(r);
                break;
            }
            None if counted(r) => {
                return Finding::NotMeasured(format!(
                    "run {} of {workflow} on {git_ref} has no dispatch token and no readable plan mode line: it may be a newer full run",
                    r.id
                ));
            }
            _ => {}
        }
    }
    let Some(full) = last_full else {
        return Finding::Pass(format!("no finished full run of {workflow} on {git_ref} yet"));
    };
    if full.conclusion == "success" {
        return Finding::Pass(format!("the last full run {} of {workflow} on {git_ref} is green", full.id));
    }
    let rounds: Vec<&TrainRun> = runs
        .iter()
        .filter(|r| r.id > full.id && matches!(r.kind, Some(RunKind::Since(_))))
        .collect();
    if let Some(g) = rounds.iter().find(|r| r.finished && r.conclusion == "success" && r.head == head) {
        return Finding::Pass(format!(
            "the last full run {} is {}, and failed-only round {} is green on the head {}",
            full.id,
            full.conclusion,
            g.id,
            short(head)
        ));
    }
    let newest_red = rounds
        .iter()
        .find(|r| r.finished && r.conclusion == "failure")
        .map_or(full.id, |r| r.id);
    let going: Vec<String> = rounds.iter().filter(|r| !r.finished).map(|r| r.id.to_string()).collect();
    let mut next =
        format!("run the failed-only round first: ./dev ci dispatch {workflow} --ref {git_ref} --input since_run={newest_red} --max 2h");
    if !going.is_empty() {
        next = format!(
            "wait for failed-only round(s) {} (`./dev wait --ci-run <id> --max 2h`), or {next}",
            going.join(", ")
        );
    }
    next.push_str(
        "; when that round is green on this head, dispatch the full run again (a coordinator who must skip it: --full-anyway \"<reason>\")",
    );
    Finding::Refuse {
        why: format!(
            "the last full run {} of {workflow} on {git_ref} is {} and no failed-only round after it is green on the head {} (ER-10)",
            full.id,
            full.conclusion,
            short(head)
        ),
        next,
    }
}

/// One run of main as the green-main guard reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct MainRun {
    /// The run's id.
    pub id: u64,
    /// Its workflow file (`nightly.yml`, `ci.yml`).
    pub workflow: String,
    /// The commit it tested.
    pub sha: String,
    /// The jobs that failed (a cancelled one says so in its name: it was not measured).
    pub failed: Vec<String>,
    /// When it was created, milliseconds since the epoch (`None`: not given).
    pub created_ms: Option<f64>,
}

/// The oldest a nightly may be and still speak for main: the nightly runs every night and a skip never leaves main unmeasured for
/// more than 36 h (nightly.yml); 48 h leaves a late night room. An older one means the schedule is off or failing to start.
pub const NIGHTLY_MAX_AGE_MS: f64 = 48.0 * 3600.0 * 1000.0;

/// What the green-main guard found, with the inactive apps' fix items it printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GreenMain {
    /// The verdict.
    pub finding: Finding,
    /// The failed jobs of each inactive app (`workflow: job`): its fix items, never a refusal.
    pub fix_items: BTreeMap<String, Vec<String>>,
}

/// The green-main guard over main's newest finished nightly and push run at `now_ms`. `active_at(sha)` is the active set at a commit.
/// A nightly older than [`NIGHTLY_MAX_AGE_MS`] (or of unknown age) speaks for an older main: not measured.
pub fn green_main(
    nightly: Option<&MainRun>,
    push: Option<&MainRun>,
    apps: &BTreeSet<String>,
    now_ms: f64,
    active_at: &mut dyn FnMut(&str) -> Result<BTreeSet<String>, String>,
) -> GreenMain {
    let mut fix_items: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let (Some(nightly), Some(push)) = (nightly, push) else {
        let missing = if nightly.is_none() { "nightly" } else { "push run of ci.yml" };
        return GreenMain {
            finding: Finding::NotMeasured(format!(
                "no finished {missing} on main among the newest runs (is the nightly schedule enabled?)"
            )),
            fix_items,
        };
    };
    if nightly.created_ms.is_none_or(|c| now_ms - c > NIGHTLY_MAX_AGE_MS) {
        let age = nightly
            .created_ms
            .map_or("of unknown age".to_string(), |c| format!("{:.0} h old", (now_ms - c) / 3_600_000.0));
        return GreenMain {
            finding: Finding::NotMeasured(format!(
                "the newest finished nightly on main (run {}) is {age}: the nightly schedule is off or not starting, so main is not measured",
                nightly.id
            )),
            fix_items,
        };
    }
    let mut blocking = Vec::new();
    for run in [nightly, push] {
        let active = match active_at(&run.sha) {
            Ok(a) => a,
            Err(e) => {
                return GreenMain {
                    finding: Finding::NotMeasured(format!(
                        "the active apps at {} (run {}) cannot be read: {e}",
                        short(&run.sha),
                        run.id
                    )),
                    fix_items,
                };
            }
        };
        let pairs: Vec<(String, String)> = run.failed.iter().map(|j| (run.workflow.clone(), j.clone())).collect();
        let red = active_apps::classify_nightly(&pairs, &active, apps);
        blocking.extend(
            red.blocking
                .into_iter()
                .map(|j| format!("run {} ({}): {j}", run.id, short(&run.sha))),
        );
        for (app, items) in red.fix_items {
            fix_items.entry(app).or_default().extend(items);
        }
    }
    let seen = format!(
        "nightly run {} ({}) and push run {} ({})",
        nightly.id,
        short(&nightly.sha),
        push.id,
        short(&push.sha)
    );
    let finding = if blocking.is_empty() {
        Finding::Pass(format!("{seen}: green in the foundation and the active apps"))
    } else {
        Finding::Refuse {
            why: format!(
                "main is red in the foundation or an active app ({seen}): {}",
                blocking.join("; ")
            ),
            next: "fix those jobs on main first (a fix branch, its train landed); a train that carries the fix dispatches with --red-main-anyway \"<the fix it carries>\"".into(),
        }
    };
    GreenMain { finding, fix_items }
}

/// Jobs whose failure only restates other jobs' (the nightly's installers verdict): left out when another job failed too.
const AGGREGATES: &[&str] = &["installers verdict"];

/// The failed jobs of a finished run's job list: failures, and cancelled jobs marked as not measured; a reusable workflow's caller
/// (it spans its children, which are listed) and an aggregate whose inputs also failed are left out.
pub fn failed_jobs(jobs: &[TimedJob]) -> Vec<String> {
    let mut out: Vec<(bool, String)> = jobs
        .iter()
        .filter(|j| !j.is_caller())
        .filter_map(|j| match j.conclusion.as_str() {
            "failure" => Some((AGGREGATES.iter().any(|a| j.name.starts_with(a)), j.name.clone())),
            "cancelled" => Some((false, format!("{} (cancelled: not measured)", j.name))),
            _ => None,
        })
        .collect();
    if out.iter().any(|(agg, _)| !agg) {
        out.retain(|(agg, _)| !agg);
    }
    out.into_iter().map(|(_, n)| n).collect()
}

/// Whether a finished nightly measured anything: a job other than its gates ran to a success or a failure (a night the gate skipped
/// measured nothing: main unchanged since the last green nightly, or a train in CI).
fn measured(jobs: &[TimedJob]) -> bool {
    jobs.iter()
        .filter(|j| !j.name.starts_with("gate") && !j.name.starts_with("installers gate"))
        .any(|j| matches!(j.conclusion.as_str(), "success" | "failure"))
}

/// How many pages of 50 runs the readers look through at most.
const PAGES: u32 = 4;
/// How many finished nightlies that measured nothing are passed over at most.
const SKIPPED_NIGHTS: usize = 10;

fn on_branch(r: &RunInfo, branch: &str) -> bool {
    r.git_ref().strip_prefix("refs/heads/").unwrap_or(r.git_ref()) == branch
}

/// The newest finished nightly on main that measured something, and the newest finished push run of ci.yml on main, with their failed
/// jobs. A cancelled run is passed over (it measured nothing whole).
pub fn read_main(client: &mut Client) -> Result<(Option<MainRun>, Option<MainRun>), String> {
    let mut nightly = None;
    let mut push = None;
    let mut skipped = 0;
    'pages: for page in 1..=PAGES {
        let p = client.runs_page_where(page, "branch=main")?;
        let n = p.runs.len();
        for r in p.runs {
            if !on_branch(&r, "main") || !r.finished() || matches!(r.conclusion.as_str(), "cancelled" | "skipped") {
                continue;
            }
            let want_nightly = nightly.is_none() && r.workflow() == "nightly.yml" && skipped < SKIPPED_NIGHTS;
            let want_push = push.is_none() && r.workflow() == "ci.yml" && r.event == "push";
            if !want_nightly && !want_push {
                continue;
            }
            let jobs = client.timed_jobs(r.id)?.ok_or_else(|| format!("run {} has no job list", r.id))?;
            if want_nightly && !measured(&jobs) {
                skipped += 1;
                continue;
            }
            let run = MainRun {
                id: r.id,
                workflow: r.workflow().to_string(),
                sha: r.head_sha.clone(),
                failed: failed_jobs(&jobs),
                created_ms: super::time::ts(&r.created_at),
            };
            if want_nightly {
                nightly = Some(run);
            } else {
                push = Some(run);
            }
            if nightly.is_some() && push.is_some() {
                break 'pages;
            }
        }
        if n < 50 {
            break;
        }
    }
    Ok((nightly, push))
}

/// The active apps at `sha`, read from the server (`raw/docs/ci-active-apps.toml?ref=<sha>`, a 404 is "no file": every app).
pub fn active_at_remote(
    http: &mut dyn Http,
    server: &str,
    repo: &str,
    token: &str,
    sha: &str,
    apps: &BTreeSet<String>,
) -> Result<BTreeSet<String>, String> {
    let url = format!(
        "{}/api/v1/repos/{repo}/raw/{}?ref={sha}",
        server.trim_end_matches('/'),
        encode_path(active_apps::FILE)
    );
    let text = match http.get_text(&url, token) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.contains("error: 404") || e.contains("HTTP 404") => Ok(None),
        Err(e) => Err(format!("{} at {} cannot be read: {e}", active_apps::FILE, short(sha))),
    };
    active_apps::active_at(apps, &|_| text.clone())
}

/// The train's runs of `workflow` on `branch`, newest first, down to and including its last finished full run; each run's kind read
/// from its token, else from its plan job's mode line.
pub fn read_train(client: &mut Client, workflow: &str, branch: &str) -> Result<Vec<TrainRun>, String> {
    let mut out = Vec::new();
    let query = format!("event=workflow_dispatch&branch={}", encode_path(branch));
    for page in 1..=PAGES {
        let p = client.runs_page_where(page, &query)?;
        let n = p.runs.len();
        for r in p.runs.into_iter().filter(|r| r.workflow() == workflow && on_branch(r, branch)) {
            let kind = match DispatchTag::of_title(&r.title) {
                Some(t) => Some(t.kind),
                None if !r.finished() => Some(RunKind::Part),
                None => kind_from_plan_log(client, r.id),
            };
            let full = kind == Some(RunKind::Full) && r.finished() && !matches!(r.conclusion.as_str(), "cancelled" | "skipped");
            let unknown = kind.is_none() && r.finished();
            out.push(TrainRun {
                id: r.id,
                head: r.head_sha.clone(),
                finished: r.finished(),
                conclusion: r.conclusion.clone(),
                kind,
            });
            if full || unknown {
                return Ok(out);
            }
        }
        if n < 50 {
            break;
        }
    }
    Ok(out)
}

/// A run's kind from its plan job's `plan mode: full=` line: `full=true` a full run, `false` a narrowed one; `None` when the job or
/// its log cannot be read or has no such line.
fn kind_from_plan_log(client: &mut Client, run: u64) -> Option<RunKind> {
    let jobs = client.timed_jobs(run).ok()??;
    let plan = jobs.iter().find(|j| j.name.starts_with("plan"))?;
    let log = client.log(plan.id).ok()??;
    match read_marker(&log)? {
        Ok(true) => Some(RunKind::Full),
        Ok(false) => Some(RunKind::Part),
        Err(_) => None,
    }
}

/// Whether `workflow` takes the `since_run` input (its failures can be rerun alone): only such a workflow has the failed-first guard.
pub fn takes_since_run(workflow: &str, read: ReadWorkflow<'_>) -> bool {
    let Some(text) = read(workflow) else { return false };
    let Ok(docs) = yaml_rust2::YamlLoader::load_from_str(&text) else {
        return false;
    };
    docs.iter()
        .any(|d| !d["on"]["workflow_dispatch"]["inputs"]["since_run"].is_badvalue())
}

/// The apps of the checkout at `root` (`apps/<app>/paddock.ci.json`).
pub fn apps_in(root: &std::path::Path) -> BTreeSet<String> {
    std::fs::read_dir(root.join("apps"))
        .map(|d| {
            d.filter_map(Result::ok)
                .filter(|e| e.path().join("paddock.ci.json").is_file())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
