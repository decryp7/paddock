// ci-runs-reader: escapes: the previous green nightly's commit and the nightly's own failed jobs; the monitor judges plans, not a release
//! The edges of the escape monitor: the record a CI plan run saves and the nightly's `dev plan --escapes` (ADR 0117 WP-9). The
//! judgement itself is [`crate::escape::judge`]; this file reads the tree, the run and the arguments.

use super::artifact::{ArtifactStore, FileSink};
use crate::escape::{Failure, Record, Sel, WHOLE_JOB, register};
use crate::plan::{RepoGraph, Request};
use crate::plan_ci::LINUX;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What the rule tables' plan runs, project by project (the record of a plan when the switch says `rules`): the halves of each project
/// on the OS legs of its contract.
pub fn rules_selected(plan: &Value, inputs: &paddock_repo_graph::model::inputs::Inputs, graph: &RepoGraph) -> BTreeMap<String, Sel> {
    let mut selected = BTreeMap::new();
    for (id, runs) in crate::repo_model::old_runs(plan, inputs, graph) {
        let os = match id.strip_prefix("app:") {
            Some(app) => {
                let o: Vec<String> = graph.config(app)["os"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect();
                if o.is_empty() { vec![LINUX.to_string()] } else { o }
            }
            None => vec![LINUX.to_string()],
        };
        selected.insert(id, Sel { os, runs, targets: None });
    }
    selected
}

/// The record of a plan: what ran (`selected`: the graph's selection, what CI runs since the wiring, or the rule tables' answer when
/// the switch says `rules`) and what the change could reach (the model's reach, the conservative one: every project when a file is
/// unowned).
pub fn record_for(
    commit: &str,
    base: Option<&str>,
    at: i64,
    selected: BTreeMap<String, Sel>,
    reached: &paddock_repo_graph::model::Reached,
    projects: &[(String, String)],
) -> Record {
    let mut reach: BTreeMap<String, Vec<String>> = reached
        .nodes
        .iter()
        .filter(|(id, _)| id.as_str() != "docs")
        .map(|(id, n)| (id.clone(), n.why.clone()))
        .collect();
    if reached.everything {
        let why = format!("an unowned file ({}) makes the plan answer everything", reached.unowned.join(", "));
        for (id, _) in projects {
            reach.entry(id.clone()).or_insert_with(|| vec![why.clone()]);
        }
    }
    Record {
        commit: commit.to_string(),
        base: base.map(str::to_string),
        at,
        reach,
        selected,
    }
}

/// The commit time of `rev`.
fn commit_time(root: &Path, rev: &str) -> Result<i64, String> {
    let mut cmd = crate::git_env::git();
    cmd.arg("-C").arg(root).args(["log", "-1", "--format=%ct", rev, "--"]);
    let o = cmd.output().map_err(|e| format!("git log: {e}"))?;
    String::from_utf8_lossy(&o.stdout)
        .trim()
        .parse()
        .map_err(|_| format!("git log {rev}: no commit time ({})", String::from_utf8_lossy(&o.stderr).trim()))
}

/// The model's reach of a commit with every project id and folder (what [`save_ci`] records beside the selection).
pub type ReachOf<'a> = dyn FnMut(&str) -> Result<(paddock_repo_graph::model::Reached, Vec<(String, String)>), String> + 'a;

/// The env variable that names the file the plan job writes its record to; the job's upload step publishes that folder as the run's
/// artifact `plan-record` (ci.yml stays read-only: nothing pushes a ref, ADR 0117 D13).
pub const RECORD_FILE_ENV: &str = "PLAN_RECORD_FILE";

/// What `dev plan --ci` does after planning in a workflow run: writes the selection record (`selected`, what this run's plan runs) with
/// the model's reach to `PLAN_RECORD_FILE`. Never fails the plan: a record that is not saved makes the nightly `not measured` for that
/// commit, which says so (ER-27); the cause is a `::warning::`. `reach` gives the model's reach (the graph planner has it already).
pub fn save_ci(root: &Path, req: &Request, selected: BTreeMap<String, Sel>, reach: &mut ReachOf, log: &mut dyn FnMut(&str)) {
    // Only a workflow run saves a record: a local `dev plan --ci` (and the golden harness, which removes GITHUB_*) writes no file, and a
    // workflow that names no file (every one but ci.yml) keeps none.
    if !std::env::var("GITHUB_ACTIONS").is_ok_and(|v| v == "true") {
        return;
    }
    let Some(file) = std::env::var_os(RECORD_FILE_ENV).filter(|f| !f.is_empty()) else {
        return;
    };
    let mut run = || -> Result<String, String> {
        let mut head = crate::git_env::git();
        head.arg("-C")
            .arg(root)
            .args(["rev-parse", "--verify", &format!("{}^{{commit}}", req.head)]);
        let o = head.output().map_err(|e| format!("git rev-parse: {e}"))?;
        let commit = String::from_utf8_lossy(&o.stdout).trim().to_string();
        let base = match &req.base {
            Some(b) => {
                let mut c = crate::git_env::git();
                c.arg("-C").arg(root).args(["rev-parse", "--verify", &format!("{b}^{{commit}}")]);
                let o = c.output().map_err(|e| format!("git rev-parse: {e}"))?;
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| crate::escape::valid_commit(s))
            }
            None => None,
        };
        let (reached, projects) = reach(&commit)?;
        let rec = record_for(
            &commit,
            base.as_deref(),
            commit_time(root, &commit)?,
            selected.clone(),
            &reached,
            &projects,
        );
        crate::escape::save(&FileSink { path: file.clone().into() }, &rec)?;
        Ok(commit)
    };
    match run() {
        Ok(c) => log(&format!("selection record saved for {}", &c[..c.len().min(9)])),
        Err(e) => log(&format!(
            "::warning::the selection record of this plan was not saved ({e}); the next nightly counts this commit as not measured. Next: fix the cause above; nothing else is needed."
        )),
    }
}

/// The failures of a nightly's run, from its failed jobs: a job names its project (the run's scope line, or the leg's app) and its OS. A failed
/// job that cannot be mapped is returned as a line: not measured, never dropped.
pub fn failures_of(plan: &crate::plan_since::SincePlan) -> (Vec<Failure>, Vec<String>) {
    let (mut out, mut unmapped) = (Vec::new(), Vec::new());
    for r in &plan.reruns {
        let key = r.key.as_deref().unwrap_or("");
        if key == "plan" {
            continue;
        }
        let project = if key == "app" {
            r.leg.app.as_ref().map(|a| format!("app:{a}"))
        } else {
            crate::run_scope::lookup_entry(&plan.expected, &r.name)
                .and_then(|e| e.primary_project())
                .map(str::to_string)
        };
        match (
            project,
            r.leg
                .os
                .clone()
                .or_else(|| (key != "app" && key != "crate" && key != "paddock-dev").then(|| LINUX.to_string())),
        ) {
            (Some(project), Some(os)) => out.push(Failure {
                project,
                os,
                target: WHOLE_JOB.to_string(),
            }),
            _ => unmapped.push(format!("job {:?} (key {key:?}) names no project or OS", r.name)),
        }
    }
    out.sort();
    out.dedup();
    (out, unmapped)
}

/// A failures file: `[{"project": "ui", "os": "ubuntu-latest", "target": "*"}]` (the target may be left out for the whole job).
pub fn read_failures(text: &str) -> Result<Vec<Failure>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    v.as_array()
        .ok_or("not a list")?
        .iter()
        .map(|f| {
            let s = |k: &str| f[k].as_str().map(str::to_string);
            Ok(Failure {
                project: s("project").ok_or("a failure has no project")?,
                os: s("os").ok_or("a failure has no os")?,
                target: s("target").unwrap_or_else(|| WHOLE_JOB.to_string()),
            })
        })
        .collect()
}

/// The commit of the newest finished, green run of the nightly workflow other than `current` (ADR 0117 §8: the span runs from the last
/// green nightly, across a weekend and a paused night). `Err` says what to pass instead.
pub fn previous_green(http: &mut dyn crate::http::Http, env: &dyn Fn(&str) -> Option<String>, current: u64) -> Result<String, String> {
    let server = env("GITHUB_SERVER_URL").unwrap_or_else(|| "https://dev.decryptology.net".into());
    let repo = env("GITHUB_REPOSITORY").unwrap_or_else(|| "decryp7/paddock".into());
    let token = env("PLAN_TOKEN").unwrap_or_default();
    let url = format!(
        "{}/api/v1/repos/{repo}/actions/runs?event=schedule&limit=50",
        server.trim_end_matches('/')
    );
    let body = http
        .get_json(&url, &token)
        .map_err(|e| format!("the nightly runs could not be read ({e}); pass --since <the previous green nightly's commit>"))?;
    pick_green(&body, current)
        .ok_or_else(|| "no earlier green nightly run was found in the last 50 scheduled runs; pass --since <commit>".to_string())
}

/// The head commit of the newest green nightly run in an API answer, apart from `current`.
pub fn pick_green(body: &Value, current: u64) -> Option<String> {
    body["workflow_runs"]
        .as_array()?
        .iter()
        .filter(|r| r["path"].as_str().is_some_and(|p| p.split('@').next() == Some("nightly.yml")))
        .filter(|r| r["conclusion"] == "success" && r["run_number"].as_u64() != Some(current) && r["id"].as_u64() != Some(current))
        .find_map(|r| r["head_sha"].as_str().map(str::to_string))
}

/// `dev plan --escapes --head <sha> --since <sha> (--results <file> | --run <id>) [--bisect-cmd <script>] [--register <run> <date>]`.
/// Exit 5 (findings) when an escape, an unmeasured failure or an unreadable record is found.
pub fn command(root: &Path, args: &[String], env: &dyn Fn(&str) -> Option<String>) -> Result<(Value, String, bool), String> {
    let mut a = args.iter();
    let (mut head, mut since, mut results, mut run, mut bisect) = (None, None, None, None, None);
    let mut register_at: Option<(u64, String)> = None;
    while let Some(k) = a.next() {
        let mut val = || a.next().cloned().ok_or(format!("{k} needs a value"));
        match k.as_str() {
            "--head" => head = Some(val()?),
            "--since" => since = Some(val()?),
            "--results" => results = Some(PathBuf::from(val()?)),
            "--run" => run = Some(val()?.parse::<u64>().map_err(|_| "--run needs a run number".to_string())?),
            "--bisect-cmd" => bisect = Some(PathBuf::from(val()?)),
            "--register" => {
                let id = val()?.parse::<u64>().map_err(|_| "--register needs <run> <date>".to_string())?;
                register_at = Some((id, val()?));
            }
            other => return Err(format!("dev plan --escapes: unknown argument {other:?}")),
        }
    }
    let head = head.unwrap_or_else(|| "HEAD".to_string());
    let since = match (since, run) {
        (Some(s), _) => s,
        (None, Some(id)) => {
            let mut http = crate::wait::CurlHttp;
            previous_green(&mut http, env, id)?
        }
        (None, None) => return Err("--since <the previous green nightly's commit> is needed".into()),
    };
    for c in [&head, &since] {
        if c.starts_with('-') {
            return Err(format!("{c:?} is not a revision"));
        }
    }
    let rev = |r: &str| -> Result<String, String> {
        let mut c = crate::git_env::git();
        c.arg("-C").arg(root).args(["rev-parse", "--verify", &format!("{r}^{{commit}}")]);
        let o = c.output().map_err(|e| format!("git rev-parse: {e}"))?;
        let t = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if o.status.success() && crate::escape::valid_commit(&t) {
            Ok(t)
        } else {
            Err(format!("{r} is not a commit here"))
        }
    };
    let (head, since) = (rev(&head)?, rev(&since)?);
    let (failures, unmapped) = match (&results, run) {
        (Some(p), None) => (
            read_failures(&std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?)?,
            Vec::new(),
        ),
        (None, Some(id)) => failures_of(&crate::plan_cmd::read_run(root, env, id, true)?),
        _ => return Err("exactly one of --results <file> and --run <id> is needed".into()),
    };
    let server = env("GITHUB_SERVER_URL").unwrap_or_else(|| "https://dev.decryptology.net".into());
    let repo = env("GITHUB_REPOSITORY").unwrap_or_else(|| "decryp7/paddock".into());
    let token = env("PLAN_TOKEN").unwrap_or_default();
    let mut http = crate::wait::CurlHttp;
    let store = ArtifactStore::new(crate::ci_runs::client::Client::new(&mut http, &server, &repo, &token));
    let reached = |c: &str| {
        c == since || {
            let mut cmd = crate::git_env::git();
            cmd.arg("-C").arg(root).args(["merge-base", "--is-ancestor", c, &since]);
            cmd.status().is_ok_and(|s| s.success())
        }
    };
    let commits = crate::escape::chain(&store, &head, &reached);
    let mut runner: Box<dyn crate::escape::Runner> = match &bisect {
        Some(p) => Box::new(crate::escape::CommandRunner { program: p, root }),
        None => Box::new(crate::escape::NoRunner),
    };
    let mut judged = crate::escape::judge(&commits, &store, &failures, runner.as_mut());
    for u in &unmapped {
        judged.outcomes.push(crate::escape::Outcome::NotMeasured(
            Failure {
                project: "?".into(),
                os: "?".into(),
                target: WHOLE_JOB.into(),
            },
            u.clone(),
        ));
    }
    let mut lines = judged.lines();
    lines.push(format!(
        "escape monitor: {} commits judged, {} failures, {} escapes, {} not measured, {} records unreadable, {} bisect runs; {}",
        commits.len(),
        failures.len(),
        judged.escapes().len(),
        judged.not_measured(),
        judged.unreadable.len(),
        judged.runs_used,
        if judged.clean() { "clean" } else { "NOT clean" }
    ));
    let row = register::Row {
        run: register_at.as_ref().map_or(run.unwrap_or(0), |r| r.0),
        date: register_at.as_ref().map_or_else(String::new, |r| r.1.clone()),
        head: head.clone(),
        commits: commits.len() as u64,
        escapes: judged.escapes().len() as u64,
        not_measured: judged.not_measured() as u64,
        unreadable: judged.unreadable.len() as u64,
    };
    if register_at.is_some() {
        register::append(root, &row)?;
        lines.push(format!("recorded run {} in {}", row.run, register::FILE));
    } else {
        lines.push(format!("row for {}:{}", register::FILE, register::row_text(&row)));
    }
    let data = serde_json::json!({"commits": commits, "escapes": judged.escapes().len(), "notMeasured": judged.not_measured(),
        "unreadable": judged.unreadable.len(), "clean": judged.clean(), "runsUsed": judged.runs_used});
    Ok((data, lines.join("\n"), judged.clean()))
}
