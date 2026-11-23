// ci-runs-reader: plan: the base of a diff is the last green push run of main (what to test, not whether to release)
//! `dev plan`: the command line of the CI planner, formerly the entry points of the retired `scripts/affected.mjs`:
//!
//! ```text
//! dev plan <base> [<head>]       the plan as JSON on stdout
//! dev plan --files a,b,c         the same for an explicit list of changed files
//! dev plan --all                 everything
//! dev plan --ci                  CI: picks the base from the event, writes GITHUB_OUTPUT and GITHUB_STEP_SUMMARY
//! dev plan --ci --app <app>      `true` or `false` on stdout: whether the change reaches that app
//! dev plan --commit-range        the commit lint's range (or `skip`) from EVENT, REF, BEFORE, SHA, PLAN_BASE, BASE_REF, INPUT_APP
//! dev plan --since-run <run>     what of a finished CI run reruns: its failed jobs, or only their failed tests (JSON)
//! options: --explain, --compare (the repository model's answer beside the rule tables', ADR 0117 §8)
//! ```
//!
//! `--ci` with `PLAN_SINCE_RUN` set (ci.yml's `since_run` input) writes the outputs of such a rerun instead: only the failed
//! jobs run, a narrowed one only its failed tests ([`crate::plan_since`]). The run is read with `PLAN_TOKEN` or `GITEA_TOKEN`.
//! `--ci --app <app>` with `PLAN_SINCE_RUN` (the `since_run` input of an app's own workflow, `debrief.yml`) does the same for
//! a run of that workflow: `true` on stdout and the rerun's outputs ([`SincePlan::app_outputs`]) in `GITHUB_OUTPUT`;
//! `PLAN_FULL` (`true` for a full manual run) must be set when the earlier run was full and a full-run job reruns.
//!
//! Log lines of `--ci` go to standard error (the script printed them to standard output unless `--app` was given); the job log
//! shows both streams.

use crate::plan::{Request, plan_with_graph};
use crate::plan_ci::{
    BaseInput, LintInput, MatrixOpts, USAGE, choose_base, ci_outputs, commit_lint_range, is_ci_yml, matrix, member_run, only_app,
    package_filters, parse_args, summary, wants_webkit,
};
use crate::plan_repo::git_text;
use crate::plan_since::{APP_WORKFLOWS, GiteaRuns, SincePlan, since_run};
use crate::wait::{CurlHttp, Http};
use paddock_cli::{CliError, Report};
use paddock_repo_graph::GitHistory;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(120);

/// A planner failure. In a workflow (`GITHUB_ACTIONS`) an `::error::` annotation with the cause and the next step goes first (ER-27).
fn fail(message: impl Into<String>) -> CliError {
    let message = message.into();
    if std::env::var("GITHUB_ACTIONS").is_ok_and(|v| v == "true") {
        eprintln!(
            "::error::dev plan: {message}. Run `./dev plan` with the same arguments and environment locally to see it again; the usage is in the log below."
        );
    }
    CliError::failure(format!("affected: {message}\n{USAGE}"))
}

/// The newest ci.yml runs on main (Gitea API), or `None` with a warning when they cannot be read.
pub fn ci_runs(http: &mut dyn Http, env: &dyn Fn(&str) -> Option<String>, warn: &mut dyn FnMut(&str)) -> Option<Vec<Value>> {
    let server = env("GITHUB_SERVER_URL").unwrap_or_else(|| "https://dev.decryptology.net".into());
    let repo = env("GITHUB_REPOSITORY").unwrap_or_else(|| "decryp7/paddock".into());
    let token = env("PLAN_TOKEN").unwrap_or_default();
    let api = format!("{}/api/v1/repos/{repo}", server.trim_end_matches('/'));
    let mut out: Vec<Value> = Vec::new();
    for page in 1..=4 {
        let url = format!("{api}/actions/runs?branch=main&event=push&limit=50&page={page}");
        let body = match http.get_json(&url, &token) {
            Ok(b) => b,
            Err(e) => {
                warn(&format!(
                    "::warning::the CI runs of main could not be read ({e}); this run tests everything"
                ));
                return None;
            }
        };
        let runs = body["workflow_runs"].as_array().cloned().unwrap_or_default();
        let count = runs.len();
        out.extend(runs);
        if count < 50
            || out
                .iter()
                .any(|r| r["conclusion"] == "success" && is_ci_yml(r["path"].as_str().unwrap_or("")))
        {
            break;
        }
    }
    Some(out)
}

fn repo_root(root: Option<PathBuf>) -> Result<PathBuf, CliError> {
    if let Some(r) = root {
        return Ok(r);
    }
    let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
    let out = crate::input_hash::Exec::run(&crate::input_hash::Real, "git", &["rev-parse", "--show-toplevel"], &cwd, None)?;
    Ok(PathBuf::from(out.trim()))
}

fn append(path: &str, text: &str) -> Result<(), CliError> {
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .map_err(|e| CliError::failure(format!("{path} cannot be opened for appending: {e}.")))?;
    f.write_all(text.as_bytes())
        .map_err(|e| CliError::failure(format!("{path} cannot be written: {e}.")))
}

fn env_var(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

/// `dev plan`: `args` are the arguments after `plan`.
pub fn command(args: &[String], root: Option<PathBuf>) -> Result<Report, CliError> {
    let root = repo_root(root)?;
    let env = env_var;
    // A sparse checkout makes the planner read a tree with folders missing and answer for it; say so first (local runs only).
    if !matches!(args.first().map(String::as_str), Some("--commit-range" | "--commit-flags"))
        && let Some(line) = crate::gates::edge::sparse_problem(&root)
    {
        return Err(CliError::failure(line));
    }
    if let Some(mode) = args.first().filter(|a| *a == "--commit-range" || *a == "--commit-flags") {
        let lint = |name: &str| env(name).unwrap_or_default();
        let (event, reference, before, base, base_ref, app) = (
            lint("EVENT"),
            lint("REF"),
            lint("BEFORE"),
            lint("PLAN_BASE"),
            lint("BASE_REF"),
            lint("INPUT_APP"),
        );
        let sha = env("SHA");
        let known = |c: &str| git_text(&root, &["cat-file", "-e", &format!("{c}^{{commit}}")]).is_some();
        let range = commit_lint_range(
            &LintInput {
                event: &event,
                reference: &reference,
                before: &before,
                sha: sha.as_deref(),
                base: &base,
                base_ref: &base_ref,
                app: &app,
            },
            &known,
        )
        .map_err(fail)?;
        let text = if mode == "--commit-flags" {
            range.map(|r| r.flags.join(" ")).unwrap_or_default()
        } else {
            range.map(|r| r.range).unwrap_or_else(|| "skip".into())
        };
        return Ok(Report::new(serde_json::json!({ "output": text }), text));
    }
    if args.first().is_some_and(|a| a == "--replay") {
        return replay(
            &root,
            args.get(1).map(String::as_str),
            args.get(2).filter(|a| *a == "--out").and_then(|_| args.get(3)).map(PathBuf::from),
        );
    }
    if args.first().is_some_and(|a| a == "--hash") {
        return hash(&root, args.get(1).map(String::as_str));
    }
    let a = parse_args(args).map_err(fail)?;
    if let Some(id) = a.since_run {
        let plan = read_since_run(&root, &env, id).map_err(fail)?;
        let v = plan.to_json();
        return Ok(Report::new(v.clone(), serde_json::to_string_pretty(&v).unwrap_or_default()));
    }
    if a.ci {
        return ci(&root, &env, a.app.as_deref());
    }
    if a.files.is_none() && a.base.is_none() && !a.all {
        return Err(fail("no base, --files or --all given"));
    }
    let history = GitHistory::new(&root, TIMEOUT);
    let req = Request {
        files: a.files.unwrap_or_default(),
        all: a.all,
        explain: a.explain,
        root: None,
        base: a.base,
        head: a.head,
    };
    let compare = a.compare;
    let req = Request {
        explain: req.explain || compare,
        ..req
    };
    let (graph, plan) = plan_with_graph(&root, &history, &req).map_err(|e| fail(e.to_string()))?;
    if compare {
        // The migration net (ADR 0117 §8): the model's answer beside the rule tables', which still decide the plan.
        let world = crate::repo_model::build_world(&root, &history, &graph, req.base.as_ref().map(|_| req.head.as_str())).map_err(fail)?;
        let changed = crate::repo_model::changed_files(&history, &req).map_err(fail)?;
        let reached = crate::repo_model::reach_in(&world, &root, &req, &changed, &graph);
        let v = crate::repo_model::compare_targets(&plan, &reached, &graph, &world.model);
        return Ok(Report::new(v.clone(), serde_json::to_string_pretty(&v).unwrap_or_default()));
    }
    Ok(Report::new(plan.clone(), plan.to_string()))
}

/// `dev plan --replay <from>..<to>`: every non-merge commit of the range planned against its parent by the rule tables and by the
/// model, the differences with their category (ADR 0117 §8). Exit 5 when a narrowing no category explains.
fn replay(root: &Path, range: Option<&str>, out: Option<PathBuf>) -> Result<Report, CliError> {
    let range = range.ok_or_else(|| fail("--replay needs a range <from>..<to>".to_string()))?;
    let history = GitHistory::new(root, TIMEOUT);
    let members = crate::repo_model::replay::members(root, range).map_err(fail)?;
    let graph = crate::plan::read_graph(root, &history).map_err(|e| fail(e.to_string()))?;
    let world = crate::repo_model::build_world(root, &history, &graph, None).map_err(fail)?;
    let subject = |sha: &str| {
        git_text(root, &["log", "-1", "--format=%s", sha])
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    let replayed = crate::repo_model::replay::replay(root, &history, &graph, &world, &members, &subject);
    let bad = crate::repo_model::replay::unlisted(&replayed);
    let v = crate::repo_model::replay::to_json(&replayed);
    if let Some(path) = out {
        std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap_or_default())
            .map_err(|e| fail(format!("{} cannot be written: {e}", path.display())))?;
    }
    let (old, new) = replayed
        .iter()
        .fold((0.0, 0.0), |a, r| (a.0 + r.took.0.as_secs_f64(), a.1 + r.took.1.as_secs_f64()));
    eprintln!("replay: {} commits, rule tables {old:.1} s, model {new:.1} s", replayed.len());
    let text = format!(
        "replayed {} commits of {range}; {} unlisted\n{}",
        replayed.len(),
        bad.len(),
        bad.iter().map(|l| format!("::error::{l}")).collect::<Vec<_>>().join("\n")
    );
    let report = Report::new(v, text);
    Ok(if bad.is_empty() {
        report
    } else {
        report.with_exit(paddock_cli::Exit::Findings)
    })
}

/// `dev plan --hash <node>[,<node>...]`: the input hash of model nodes (ADR 0117 §5, WP-7), one bulk read for all of them.
fn hash(root: &Path, nodes: Option<&str>) -> Result<Report, CliError> {
    let nodes: Vec<String> = nodes
        .ok_or_else(|| fail("--hash needs node ids, comma separated (for example crate:crates/paddock-proc)".to_string()))?
        .split(',')
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .collect();
    let history = GitHistory::new(root, TIMEOUT);
    let graph = crate::plan::read_graph(root, &history).map_err(|e| fail(e.to_string()))?;
    let world = crate::repo_model::build_world(root, &history, &graph, None).map_err(fail)?;
    let text = |f: &str| std::fs::read_to_string(root.join(f)).ok();
    let tree = crate::repo_model::hash::Tree {
        files: &world.files,
        text: &text,
    };
    let vars: Vec<(String, String)> = crate::process_env::vars().collect();
    let ctx = crate::repo_model::hash::HashEnv {
        env: &env_var,
        vars: &vars,
        os: std::env::consts::OS,
    };
    let hashes = crate::repo_model::hash::node_hashes(&crate::input_hash::Real, &ctx, root, &world, &tree, &nodes)?;
    let lines: Vec<String> = hashes
        .values()
        .map(|h| format!("{} {}{}", h.hash, h.node, if h.dirty { " (uncommitted changes)" } else { "" }))
        .collect();
    let data: serde_json::Map<String, serde_json::Value> = hashes
        .values()
        .map(|h| (h.node.clone(), serde_json::json!({"hash": h.hash, "dirty": h.dirty})))
        .collect();
    Ok(Report::new(serde_json::Value::Object(data), lines.join("\n")))
}

/// CI: the plan for this event. Without `app`, the ci.yml job outputs and summary; with `app` (an app's own workflow),
/// `true` or `false` on stdout for whether the change reaches that app.
fn ci(root: &Path, env: &dyn Fn(&str) -> Option<String>, app: Option<&str>) -> Result<Report, CliError> {
    if let Some(run) = env("PLAN_SINCE_RUN").filter(|r| !r.trim().is_empty()) {
        return match app {
            None => since_ci(root, env, run.trim()),
            Some(app) => since_app(root, env, app, run.trim()),
        };
    }
    let log = |l: &str| eprintln!("{l}");
    let contains = |sha: &str| git_text(root, &["merge-base", "--is-ancestor", sha, "HEAD"]).is_some();
    let merge_base = |b: &str| git_text(root, &["merge-base", "HEAD", &format!("origin/{b}")]).map(|s| s.trim().to_string());
    let event = env("GITHUB_EVENT_NAME").unwrap_or_default();
    let reference = env("GITHUB_REF").unwrap_or_default();
    let plan_app = env("PLAN_APP").filter(|a| !a.is_empty());
    let mut http = CurlHttp;
    let runs = if event == "push" && reference == "refs/heads/main" {
        // A workflow command is read from standard output; with `--app` standard output is the answer, so it goes to the log.
        ci_runs(&mut http, env, &mut |w| if app.is_some() { log(w) } else { println!("{w}") })
    } else {
        None
    };
    let (base, reason) = choose_base(&BaseInput {
        event: &event,
        reference: &reference,
        input_base: &env("PLAN_BASE").unwrap_or_default(),
        base_ref: &env("GITHUB_BASE_REF").unwrap_or_default(),
        runs: runs.as_deref(),
        contains: &contains,
        merge_base: &merge_base,
    });
    if base.as_deref().is_some_and(|b| b.starts_with('-')) {
        return Err(fail(format!(
            "the base {:?} is not a revision (it starts with '-')",
            base.unwrap_or_default()
        )));
    }
    let history = GitHistory::new(root, TIMEOUT);
    let req = Request {
        files: Vec::new(),
        all: plan_app.is_some() || base.is_none(),
        explain: true,
        root: None,
        base: if plan_app.is_some() { None } else { base.clone() },
        head: "HEAD".into(),
    };
    let (graph, planned) = plan_with_graph(root, &history, &req).map_err(|e| fail(e.to_string()))?;
    if let Some(p) = &plan_app
        && !graph.apps.contains(p)
    {
        return Err(fail(format!("unknown app {p} (no apps/{p}/paddock.ci.json)")));
    }
    let plan = match &plan_app {
        Some(p) => only_app(&planned, p),
        None => planned,
    };
    let narrow = (event == "pull_request" && plan_app.is_none())
        || member_run(
            &event,
            &env("PLAN_BASE").unwrap_or_default(),
            &env("PLAN_SINCE_RUN").unwrap_or_default(),
            plan_app.as_deref(),
        );
    let legs = matrix(
        &plan,
        &graph,
        MatrixOpts {
            narrow,
            webkit: wants_webkit(env),
        },
    )
    .map_err(fail)?;
    match &plan_app {
        Some(p) => log(&format!("base: only {p}, every leg (a tag's gates)")),
        None => log(&format!("base: {reason}")),
    }
    log(&serde_json::to_string_pretty(&plan).unwrap_or_default());
    if let Some(app) = app {
        if !graph.apps.contains(app) {
            return Err(fail(format!("unknown app {app} (no apps/{app}/paddock.ci.json)")));
        }
        let runs_it = plan["apps"].as_array().is_some_and(|a| a.iter().any(|x| x == app));
        let kinds: Vec<&str> = plan["kinds"][app]
            .as_array()
            .map(|k| k.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        log(&if runs_it {
            format!("{app}: affected ({})", kinds.join(", "))
        } else {
            format!("{app}: not affected by this change")
        });
        if let Some(path) = env("GITHUB_STEP_SUMMARY") {
            let verdict = if runs_it {
                "Affected: every job runs."
            } else {
                "Not affected: nothing to run."
            };
            append(&path, &format!("### {app}\n\nBase: {reason}\n\n{verdict}\n"))?;
        }
        return Ok(Report::new(serde_json::json!({ "affected": runs_it }), runs_it.to_string()));
    }
    let mut plan = plan;
    plan["filters"] = package_filters(&plan, &graph, narrow);
    let outputs = ci_outputs(&plan, &legs, if plan_app.is_some() { None } else { base.as_deref() });
    if narrow {
        log("narrowed to the packages the change reaches (a member run; the final head's full run tests the rest)");
    }
    if let Some(path) = env("GITHUB_OUTPUT") {
        append(&path, &outputs.iter().map(|(k, v)| format!("{k}={v}\n")).collect::<String>())?;
    }
    if let Some(path) = env("GITHUB_STEP_SUMMARY") {
        append(&path, &summary(&plan, &reason, &legs))?;
    }
    Ok(Report::new(
        serde_json::json!({ "base": base, "reason": reason, "apps": legs.len() }),
        String::new(),
    ))
}

/// The workflow file a run names (`ci.yml`), from this checkout; `None` for a name that is not a plain file name.
fn workflow_text(root: &Path, file: &str) -> Option<String> {
    let plain = !file.is_empty() && file.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    plain
        .then(|| std::fs::read_to_string(root.join(".gitea/workflows").join(file)).ok())
        .flatten()
}

/// Reads a finished run of this repository and decides what of it reruns; its phases go to the log (standard error).
fn read_since_run(root: &Path, env: &dyn Fn(&str) -> Option<String>, id: u64) -> Result<SincePlan, String> {
    let server = env("GITHUB_SERVER_URL").unwrap_or_else(|| "https://dev.decryptology.net".into());
    let repo = env("GITHUB_REPOSITORY").unwrap_or_else(|| "decryp7/paddock".into());
    let token = env("PLAN_TOKEN")
        .filter(|t| !t.is_empty())
        .or_else(|| env("GITEA_TOKEN"))
        .unwrap_or_default();
    let mut http = CurlHttp;
    let mut runs = GiteaRuns::new(&mut http, &server, &repo, token);
    since_run(&mut runs, id, &|f| workflow_text(root, f), &mut |l| eprintln!("{l}"))
}

/// Appends the outputs and the summary of a `--ci` run to the files the workflow names.
fn write_ci(env: &dyn Fn(&str) -> Option<String>, outputs: &[(String, String)], md: &str) -> Result<(), CliError> {
    if let Some(path) = env("GITHUB_OUTPUT") {
        append(&path, &outputs.iter().map(|(k, v)| format!("{k}={v}\n")).collect::<String>())?;
    }
    if let Some(path) = env("GITHUB_STEP_SUMMARY") {
        append(&path, md)?;
    }
    Ok(())
}

/// CI with `PLAN_SINCE_RUN`: the outputs that rerun only what failed in that run. A run that cannot be read or has not finished
/// stops the plan with an error (a rerun must not quietly become a full run); a run whose failures cannot be told job by job
/// plans everything with a warning (a rerun never runs less than failed).
fn since_ci(root: &Path, env: &dyn Fn(&str) -> Option<String>, run: &str) -> Result<Report, CliError> {
    let id: u64 = run
        .parse()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| fail(format!("PLAN_SINCE_RUN must be a run number, not {run:?}")))?;
    since_ci_with(root, env, id, read_since_run(root, env, id))
}

/// The workflow file of an app's own workflow (`workflow` of `apps/<app>/paddock.ci.json`), `None` for a leg of ci.yml.
fn app_workflow(root: &Path, app: &str) -> Result<Option<String>, CliError> {
    let path = root.join("apps").join(app).join("paddock.ci.json");
    let text = std::fs::read_to_string(&path).map_err(|e| fail(format!("{} cannot be read: {e}", path.display())))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| fail(format!("{} is not JSON: {e}", path.display())))?;
    Ok(v["workflow"].as_str().map(str::to_string))
}

/// `--ci --app <app>` with `PLAN_SINCE_RUN`: a rerun of what failed in a run of the app's own workflow. `true` on stdout (the
/// run is needed), the outputs and the summary to the files the workflow names; the log to standard error (standard output
/// is the answer). Errors as [`since_ci`]: an unreadable or unfinished run, a run of another workflow, an app without its own
/// workflow, or a full run's failure rerun in a run that is not full.
fn since_app(root: &Path, env: &dyn Fn(&str) -> Option<String>, app: &str, run: &str) -> Result<Report, CliError> {
    let id: u64 = run
        .parse()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| fail(format!("PLAN_SINCE_RUN must be a run number, not {run:?}")))?;
    let workflow = app_workflow(root, app)?.ok_or_else(|| {
        fail(format!(
            "{app} is a leg of ci.yml, not its own workflow: rerun ci.yml with since_run"
        ))
    })?;
    since_app_with(env, app, &workflow, id, read_since_run(root, env, id))
}

fn since_app_with(
    env: &dyn Fn(&str) -> Option<String>,
    app: &str,
    workflow: &str,
    id: u64,
    read: Result<SincePlan, String>,
) -> Result<Report, CliError> {
    let profile = APP_WORKFLOWS.iter().find(|a| a.file == workflow).ok_or_else(|| {
        fail(format!(
            "{workflow} ({app}) has no rerun of failures yet (plan_since::APP_WORKFLOWS)"
        ))
    })?;
    let plan = read.map_err(|e| fail(format!("the rerun of run {id} cannot be planned: {e}")))?;
    if plan.workflow != workflow {
        return Err(fail(format!(
            "run {id} is a run of {}; {workflow} reruns only its own runs (ci.yml takes its own since_run)",
            plan.workflow
        )));
    }
    if let Some(why) = &plan.everything {
        eprintln!("::warning::dev plan --since-run {id}: {why}; this run tests everything");
        write_ci(
            env,
            &[],
            &format!("### Rerun of run {id} ({workflow})\n\nEverything runs: the rerun cannot be narrowed ({why}).\n"),
        )?;
        return Ok(Report::new(
            serde_json::json!({ "sinceRun": id, "everything": why }),
            "true".to_string(),
        ));
    }
    if plan.needs_full() && env("PLAN_FULL").as_deref() != Some("true") {
        return Err(fail(format!(
            "run {id} was a full run and a job only a full run has, or one it changes, failed; dispatch the rerun with {}: true as well",
            profile.full_input
        )));
    }
    eprintln!("{}", serde_json::to_string_pretty(&plan.to_json()).unwrap_or_default());
    write_ci(env, &plan.app_outputs(), &plan.summary())?;
    Ok(Report::new(plan.to_json(), "true".to_string()))
}

fn since_ci_with(root: &Path, env: &dyn Fn(&str) -> Option<String>, id: u64, read: Result<SincePlan, String>) -> Result<Report, CliError> {
    let plan = read.map_err(|e| fail(format!("the rerun of run {id} cannot be planned: {e}")))?;
    if !matches!(plan.workflow.as_str(), "ci.yml" | "ci.yaml") {
        return Err(fail(format!(
            "run {id} is a run of {}; ci.yml reruns only its own runs (rerun that workflow's failed jobs by hand)",
            plan.workflow
        )));
    }
    // The WebKit legs too when a WebKit job or leg failed, or the matrix would have no leg to rerun it in.
    let webkit = wants_webkit(env) || plan.needs_webkit();
    let history = GitHistory::new(root, TIMEOUT);
    let all = Request {
        files: Vec::new(),
        all: true,
        explain: true,
        root: None,
        base: None,
        head: "HEAD".into(),
    };
    let (graph, everything) = plan_with_graph(root, &history, &all).map_err(|e| fail(e.to_string()))?;
    let legs = matrix(&everything, &graph, MatrixOpts { narrow: false, webkit }).map_err(fail)?;
    if let Some(why) = &plan.everything {
        println!("::warning::dev plan --since-run {id}: {why}; this run tests everything");
        let reason = format!("everything: the rerun of run {id} cannot be narrowed ({why})");
        eprintln!("base: {reason}");
        write_ci(env, &ci_outputs(&everything, &legs, None), &summary(&everything, &reason, &legs))?;
        return Ok(Report::new(serde_json::json!({ "sinceRun": id, "everything": why }), String::new()));
    }
    let outputs = plan.ci_outputs(&legs, &mut |w| println!("{w}"));
    eprintln!("{}", serde_json::to_string_pretty(&plan.to_json()).unwrap_or_default());
    write_ci(env, &outputs, &plan.summary())?;
    Ok(Report::new(plan.to_json(), String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        pages: Vec<Result<Value, String>>,
        urls: Vec<String>,
    }

    impl Http for Fake {
        fn get_json(&mut self, url: &str, _token: &str) -> Result<Value, String> {
            self.urls.push(url.to_string());
            if self.pages.is_empty() {
                Ok(serde_json::json!({"workflow_runs": []}))
            } else {
                self.pages.remove(0)
            }
        }
    }

    fn page(n: usize, green: bool) -> Value {
        let run = |i: usize| serde_json::json!({"id": i, "conclusion": if green { "success" } else { "failure" }, "path": "ci.yml@refs/heads/main", "head_sha": "x"});
        serde_json::json!({"workflow_runs": (0..n).map(run).collect::<Vec<_>>()})
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn the_runs_are_read_page_by_page_until_a_short_page_or_a_green_ci_run() {
        let mut http = Fake {
            pages: vec![Ok(page(50, false)), Ok(page(50, false)), Ok(page(3, false))],
            urls: vec![],
        };
        let runs = ci_runs(&mut http, &no_env, &mut |_| {}).unwrap();
        assert_eq!(runs.len(), 103);
        assert_eq!(http.urls.len(), 3);
        assert_eq!(
            http.urls[0],
            "https://dev.decryptology.net/api/v1/repos/decryp7/paddock/actions/runs?branch=main&event=push&limit=50&page=1"
        );
        let mut http = Fake {
            pages: vec![Ok(page(50, true)), Ok(page(50, true))],
            urls: vec![],
        };
        assert_eq!(
            ci_runs(&mut http, &no_env, &mut |_| {}).unwrap().len(),
            50,
            "a green run stops the paging"
        );
        let mut http = Fake {
            pages: (0..6).map(|_| Ok(page(50, false))).collect(),
            urls: vec![],
        };
        assert_eq!(ci_runs(&mut http, &no_env, &mut |_| {}).unwrap().len(), 200, "at most four pages");
    }

    #[test]
    fn an_unreadable_or_unfinished_run_stops_the_rerun_plan_with_an_error() {
        let root = tempfile::tempdir().unwrap();
        let e = since_ci_with(root.path(), &no_env, 7, Err("run 7 is running: wait until it finishes".into()))
            .err()
            .unwrap();
        assert!(e.text().contains("run 7 is running"), "{}", e.text());
    }

    fn debrief_plan(full_ran: bool, key: &str) -> SincePlan {
        use crate::plan_since::{Leg, Rerun, Scope};
        SincePlan {
            run: 7961,
            workflow: "debrief.yml".into(),
            head: "h".into(),
            reruns: vec![Rerun {
                id: 1,
                name: "x".into(),
                key: Some(key.into()),
                leg: Leg::default(),
                scope: Scope::Tests {
                    filter: "(binary_id(=a) & test(=b))".into(),
                    tests: vec![],
                },
                vitest: vec![],
            }],
            not_rerun: 19,
            everything: None,
            full_ran,
        }
    }

    #[test]
    fn an_app_workflow_rerun_writes_its_outputs_and_answers_true() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("out");
        let summary = dir.path().join("summary");
        let (o, m) = (output.display().to_string(), summary.display().to_string());
        let env = |k: &str| match k {
            "GITHUB_OUTPUT" => Some(o.clone()),
            "GITHUB_STEP_SUMMARY" => Some(m.clone()),
            _ => None,
        };
        let r = since_app_with(&env, "debrief", "debrief.yml", 7961, Ok(debrief_plan(false, "rust-test-macos"))).unwrap();
        assert_eq!(r.text, "true");
        let out = std::fs::read_to_string(&output).unwrap();
        assert!(out.contains("rerun_jobs=[\"rust-test-macos\"]\n"), "{out}");
        assert!(out.contains("rust_test_macos_filter=(binary_id(=a) & test(=b))\n"), "{out}");
        assert!(
            std::fs::read_to_string(&summary)
                .unwrap()
                .contains("Rerun of run 7961 (debrief.yml)")
        );
    }

    #[test]
    fn an_app_workflow_rerun_refuses_another_workflows_run_and_a_full_runs_failure_in_a_run_that_is_not_full() {
        let e = since_app_with(
            &no_env,
            "debrief",
            "debrief.yml",
            7,
            Ok({
                let mut p = debrief_plan(false, "crate");
                p.workflow = "ci.yml".into();
                p
            }),
        )
        .err()
        .unwrap();
        assert!(e.text().contains("run 7 is a run of ci.yml"), "{}", e.text());
        let e = since_app_with(&no_env, "debrief", "debrief.yml", 7, Ok(debrief_plan(true, "e2e")))
            .err()
            .unwrap();
        assert!(e.text().contains("dispatch the rerun with bundles: true"), "{}", e.text());
        let full = |k: &str| (k == "PLAN_FULL").then(|| "true".to_string());
        assert!(since_app_with(&full, "debrief", "debrief.yml", 7, Ok(debrief_plan(true, "e2e"))).is_ok());
        assert!(
            since_app_with(&no_env, "debrief", "debrief.yml", 7, Ok(debrief_plan(true, "rust-test"))).is_ok(),
            "a job a full run does not change reruns in any run"
        );
        let e = since_app_with(&no_env, "dyno", "dyno.yml", 7, Ok(debrief_plan(false, "x")))
            .err()
            .unwrap();
        assert!(e.text().contains("has no rerun of failures yet"), "{}", e.text());
        let e = since_app_with(&no_env, "debrief", "debrief.yml", 7, Err("run 7 is running".into()))
            .err()
            .unwrap();
        assert!(e.text().contains("run 7 is running"), "{}", e.text());
    }

    #[test]
    fn an_app_that_is_a_leg_of_ci_yml_is_told_to_rerun_ci_yml() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("apps/dyno")).unwrap();
        std::fs::write(root.path().join("apps/dyno/paddock.ci.json"), r#"{"cargo": "."}"#).unwrap();
        let e = since_app(root.path(), &no_env, "dyno", "7").err().unwrap();
        assert!(e.text().contains("dyno is a leg of ci.yml"), "{}", e.text());
        assert!(
            since_app(root.path(), &no_env, "dyno", "x")
                .err()
                .unwrap()
                .text()
                .contains("must be a run number")
        );
    }

    #[test]
    fn unreadable_runs_warn_and_plan_everything() {
        let mut http = Fake {
            pages: vec![Err("HTTP 500".into())],
            urls: vec![],
        };
        let mut warned = String::new();
        assert!(ci_runs(&mut http, &no_env, &mut |w| warned = w.to_string()).is_none());
        assert_eq!(
            warned,
            "::warning::the CI runs of main could not be read (HTTP 500); this run tests everything"
        );
    }

    #[test]
    fn a_plan_with_nothing_to_plan_or_a_bad_command_line_is_refused_with_the_usage() {
        let root = tempfile::tempdir().unwrap();
        for args in [vec![], vec!["--files".to_string()], vec!["--ci".to_string(), "x".to_string()]] {
            let e = command(&args, Some(root.path().to_path_buf())).err().unwrap();
            assert!(
                e.text().contains("affected: ") && e.text().contains("usage: dev plan"),
                "{}",
                e.text()
            );
        }
    }
}
