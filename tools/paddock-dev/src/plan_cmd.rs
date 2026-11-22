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
//! options: --explain
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
    BaseInput, LintInput, MatrixOpts, USAGE, choose_base, ci_outputs, commit_lint_range, matrix, member_run, only_app, package_filters,
    parse_args, summary, wants_webkit,
};
use crate::plan_os;
use crate::plan_repo::git_text;
use crate::plan_since::{APP_WORKFLOWS, GiteaRuns, SincePlan, since_run};
use crate::run_scope::{DEFAULT_OS_LEGS, Scope, ci_scope, scope_of};
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

/// The base of a push to main: the last first-parent commit whose outcome is complete and green, with or without fix items, by the one
/// definition of green (`ci_runs::green::find_base`, ADR 0117 WP-4: the same reader as `ci verdict`, `ci reuse` and the landing
/// queue). The runs are read once, in pages, and matched to commits in memory. An unreadable list warns and plans everything.
pub fn green_base(
    http: &mut dyn Http,
    env: &dyn Fn(&str) -> Option<String>,
    root: &Path,
    warn: &mut dyn FnMut(&str),
) -> Result<Option<(String, u64)>, String> {
    use crate::ci_runs::client::Client;
    let server = env("GITHUB_SERVER_URL").unwrap_or_else(|| "https://dev.decryptology.net".into());
    let repo = env("GITHUB_REPOSITORY").unwrap_or_else(|| "decryp7/paddock".into());
    let token = env("PLAN_TOKEN").unwrap_or_default();
    let mut client = Client::new(http, &server, &repo, &token);
    green_base_in(&mut client, root, warn)
}

/// [`green_base`] over a client already made.
fn green_base_in(
    client: &mut crate::ci_runs::client::Client,
    root: &Path,
    warn: &mut dyn FnMut(&str),
) -> Result<Option<(String, u64)>, String> {
    use crate::ci_runs::{green, verdict};
    let first_parent = green::first_parent_before(root, "HEAD");
    let scope = verdict::scope_decisive("ci.yml");
    let found = green::find_base(client, "ci.yml", &first_parent, scope, &mut |s| green::plan_touched(root, s));
    match found {
        Ok(base) => Ok(base.map(|b| (b.sha, b.run))),
        Err(e) => {
            warn(&format!(
                "::warning::the CI runs of main could not be read ({e}); this run tests everything"
            ));
            Err(e)
        }
    }
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
    let (_, plan) = plan_with_graph(&root, &history, &req).map_err(|e| fail(e.to_string()))?;
    Ok(Report::new(plan.clone(), plan.to_string()))
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
    let merge_base = |b: &str| git_text(root, &["merge-base", "HEAD", &format!("origin/{b}")]).map(|s| s.trim().to_string());
    let event = env("GITHUB_EVENT_NAME").unwrap_or_default();
    let reference = env("GITHUB_REF").unwrap_or_default();
    let plan_app = env("PLAN_APP").filter(|a| !a.is_empty());
    let mut http = CurlHttp;
    let green = if event == "push" && reference == "refs/heads/main" {
        // A workflow command is read from standard output; with `--app` standard output is the answer, so it goes to the log.
        Some(green_base(&mut http, env, root, &mut |w| {
            if app.is_some() { log(w) } else { println!("{w}") }
        }))
    } else {
        None
    };
    let (base, reason) = choose_base(&BaseInput {
        event: &event,
        reference: &reference,
        input_base: &env("PLAN_BASE").unwrap_or_default(),
        base_ref: &env("GITHUB_BASE_REF").unwrap_or_default(),
        green: green.as_ref(),
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
        None => planned.clone(),
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
    plan["webkit"] = serde_json::json!(wants_webkit(env));
    // The `os` input (ci.yml `os`, PLAN_OS): only that operating system's part of the plan (src/plan_os.rs).
    let os_part = plan_os::parse(&env("PLAN_OS").unwrap_or_default()).map_err(fail)?;
    let legs = match os_part {
        Some(os) => plan_os::filter_legs(legs, os),
        None => legs,
    };
    let mut outputs = ci_outputs(&plan, &legs, if plan_app.is_some() { None } else { base.as_deref() });
    if let Some(os) = os_part {
        let text =
            workflow_text(root, "ci.yml").ok_or_else(|| fail("the os input needs .gitea/workflows/ci.yml in this checkout".to_string()))?;
        let default_legs = env("PLAN_OS_LEGS")
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| serde_json::to_string(&[serde_json::json!({"os": os.label()})]).unwrap_or_default());
        outputs.extend(plan_os::outputs(&text, os, &default_legs).map_err(fail)?);
        log(&format!("os: only {} (rerun_jobs and the legs are narrowed to it)", os.input()));
    }
    // ADR 0113: the run declares its scope as data (what it ran, what a push to main does for this tree); `ci verdict` compares them.
    // A reused run (`PLAN_REUSE`, ci.yml's reuse step found a complete green run of this commit) runs no job: no job reads an empty
    // `rerun_jobs` as its own, and it declares that it ran nothing.
    let reused = env("PLAN_REUSE").as_deref() == Some("true");
    if reused {
        outputs.push(("rerun_jobs".into(), "[]".into()));
    }
    let os_legs = os_legs(env);
    // An `os` run ran only that operating system's crate and paddock-dev jobs; what a push to main needs stays every runner.
    let ran_os_legs: Vec<String> = match os_part {
        Some(os) => os_legs
            .iter()
            .filter(|l| plan_os::filter_legs(vec![serde_json::json!({ "os": l })], os).len() == 1)
            .cloned()
            .collect(),
        None => os_legs.clone(),
    };
    let filter_of = |p: &Value, k: &str| p["filters"][k].as_str().unwrap_or("").to_string();
    let ran = if reused {
        Scope::default()
    } else {
        scope_of(
            &plan,
            &legs,
            wants_webkit(env),
            &ran_os_legs,
            &[filter_of(&plan, "crates").as_str(), filter_of(&plan, "paddockDev").as_str()],
        )
    };
    // A push to main is its own measure (its plan is the diff from the last green main); every other run is measured against the
    // whole tree, which is what a run with a base, an app only or a rerun is not.
    let need_plan = if event == "push" && plan_app.is_none() {
        plan.clone()
    } else if req.all {
        planned.clone()
    } else {
        let all = Request {
            files: Vec::new(),
            all: true,
            explain: true,
            root: None,
            base: None,
            head: "HEAD".into(),
        };
        plan_with_graph(root, &history, &all).map_err(|e| fail(e.to_string()))?.1
    };
    let need_legs = matrix(
        &need_plan,
        &graph,
        MatrixOpts {
            narrow: false,
            webkit: true,
        },
    )
    .map_err(fail)?;
    let need = scope_of(&need_plan, &need_legs, true, &os_legs, &[]);
    log(&ci_scope(ran, false, need).with_os(os_part.map(|o| o.input())).line());
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

/// The runners of the `crate` and `paddock-dev` jobs: the `os` of each entry of `PLAN_OS_LEGS` (ci.yml's own default list), or the
/// three runners when it is not set or not readable.
fn os_legs(env: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    env("PLAN_OS_LEGS")
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| {
            v.as_array()
                .map(|a| a.iter().filter_map(|e| e["os"].as_str().map(str::to_string)).collect::<Vec<_>>())
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_OS_LEGS.iter().map(|s| s.to_string()).collect())
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
    // ADR 0113: a rerun of failures is never complete; it declares the scope a push to main needs and that it ran none of it.
    let need_legs = matrix(
        &everything,
        &graph,
        MatrixOpts {
            narrow: false,
            webkit: true,
        },
    )
    .map_err(fail)?;
    let need = scope_of(&everything, &need_legs, true, &os_legs(env), &[]);
    eprintln!("{}", ci_scope(Scope::default(), true, need).line());
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

    fn no_env(_: &str) -> Option<String> {
        None
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
            pages: (0..4).map(|_| Err("HTTP 500".to_string())).collect(),
            urls: vec![],
        };
        let mut warned = String::new();
        let root = tempfile::tempdir().unwrap();
        let mut client = crate::ci_runs::client::Client::new(&mut http, "https://gitea.invalid", "decryp7/paddock", "t");
        client.backoff_ms = 0;
        let got = green_base_in(&mut client, root.path(), &mut |w| warned = w.to_string());
        assert!(got.is_err(), "{got:?}");
        assert!(warned.starts_with("::warning::the CI runs of main could not be read ("), "{warned}");
        assert!(warned.ends_with("this run tests everything"), "{warned}");
        assert!(warned.contains("HTTP 500"), "{warned}");
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
