//! `dev plan`: the command line of the CI planner, the port of the entry points of `scripts/affected.mjs`:
//!
//! ```text
//! dev plan <base> [<head>]       the plan as JSON on stdout
//! dev plan --files a,b,c         the same for an explicit list of changed files
//! dev plan --all                 everything
//! dev plan --ci                  CI: picks the base from the event, writes GITHUB_OUTPUT and GITHUB_STEP_SUMMARY
//! dev plan --ci --app <app>      `true` or `false` on stdout: whether the change reaches that app
//! dev plan --commit-range        the commit lint's range (or `skip`) from EVENT, REF, BEFORE, SHA, PLAN_BASE, BASE_REF, INPUT_APP
//! options: --explain
//! ```
//!
//! Log lines of `--ci` go to standard error (the script printed them to standard output unless `--app` was given); the job log
//! shows both streams.

use crate::plan::{Request, plan_with_graph};
use crate::plan_ci::{
    BaseInput, LintInput, MatrixOpts, USAGE, choose_base, ci_outputs, commit_lint_range, is_ci_yml, matrix, only_app, parse_args, summary,
    wants_webkit,
};
use crate::plan_repo::git_text;
use crate::wait::{CurlHttp, Http};
use paddock_cli::{CliError, Report};
use paddock_repo_graph::GitHistory;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(120);

fn fail(message: impl Into<String>) -> CliError {
    CliError::failure(format!("affected: {}\n{USAGE}", message.into()))
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
    if args.first().is_some_and(|a| a == "--commit-range") {
        let lint = |name: &str| env(name).unwrap_or_default();
        let (event, reference, before, sha, base, base_ref, app) = (
            lint("EVENT"),
            lint("REF"),
            lint("BEFORE"),
            lint("SHA"),
            lint("PLAN_BASE"),
            lint("BASE_REF"),
            lint("INPUT_APP"),
        );
        let known = |c: &str| git_text(&root, &["cat-file", "-e", &format!("{c}^{{commit}}")]).is_some();
        let range = commit_lint_range(
            &LintInput {
                event: &event,
                reference: &reference,
                before: &before,
                sha: &sha,
                base: &base,
                base_ref: &base_ref,
                app: &app,
            },
            &known,
        );
        let text = range.unwrap_or_else(|| "skip".into());
        return Ok(Report::new(serde_json::json!({ "range": text }), text));
    }
    let a = parse_args(args).map_err(fail)?;
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
    let log = |l: &str| eprintln!("{l}");
    let contains = |sha: &str| git_text(root, &["merge-base", "--is-ancestor", sha, "HEAD"]).is_some();
    let merge_base = |b: &str| git_text(root, &["merge-base", "HEAD", &format!("origin/{b}")]).map(|s| s.trim().to_string());
    let event = env("GITHUB_EVENT_NAME").unwrap_or_default();
    let reference = env("GITHUB_REF").unwrap_or_default();
    let plan_app = env("PLAN_APP").filter(|a| !a.is_empty());
    let mut http = CurlHttp;
    let runs = if event == "push" && reference == "refs/heads/main" {
        ci_runs(&mut http, env, &mut |w| log(w))
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
    let legs = matrix(
        &plan,
        &graph,
        MatrixOpts {
            narrow: event == "pull_request" && plan_app.is_none(),
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
    let outputs = ci_outputs(&plan, &legs, if plan_app.is_some() { None } else { base.as_deref() });
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
