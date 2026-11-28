//! `dev ci job-model check` (ADR 0123 §7): the one check at the source of the CI job layout. It reads ci.yml, the declared `[ci.jobs]`
//! table and the action files, and fails on:
//!
//! - any problem of the job model (`paddock_ci_config::jobs::model`);
//! - a job without `needs: [plan]`, without the `rerun_jobs` gate, or that calls `./dev` before the dev tool is restored (the actions
//!   that restore it are read from the action files, [`paddock_ci_config::jobs::tool_cache_actions`], ER-42);
//! - a `render` that is not injective over the legs the planner emits ([`shapes`]: whole and partitioned Linux, each OS alone, apps
//!   with their end-to-end legs, no legs), or a name that reads a leg key the scope line does not keep (the rerun legs are written from
//!   the line's leg, so such a key would rename the job);
//! - while `plan_gitea::CI_JOBS` exists: a job whose derived projects differ from it (the bridge to ADR 0117 D12).
//!
//! **Report-only (ER-41).** [`BLOCKING`] is `false`: a finding is a `::warning::` and the exit is 0. It becomes `true` after the first
//! green run on the real tree, recorded in [`BLOCKING_RECORD`]; the review date is [`REVIEW_BY`]. Two places run it: a step of ci.yml's
//! plan job and the member gate's `job-model` row (`pre_push/gates.rs`); a test fails when either is removed (ER-33).

use crate::plan_ci::{self, LINUX_PARTITIONS};
use crate::plan_gitea::CI_JOBS;
use crate::run_scope::{LEG_KEYS, PlannedLegs, expected_of};
use clap::{Args, Subcommand};
use paddock_ci_config::jobs::{self, Declared, JobModel};
use paddock_cli::{CliError, Exit, Report};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Whether a finding fails the command. Report-only until the first green run on the real tree is recorded in [`BLOCKING_RECORD`].
pub const BLOCKING: bool = false;

/// Where the first green run that makes the check blocking is recorded: empty while it is report-only (set the constant above and this
/// to the run id and date together; ER-41).
pub const BLOCKING_RECORD: &str = "";

/// The check's review date (ER-41): three months after it starts reporting (2026-11-27). A check that caught nothing real by then is
/// deleted; one that did is made blocking (a test fails once the date has passed with the check still report-only and unrecorded).
pub const REVIEW_BY: &str = "2027-02-27";

/// The workflow and the inputs file the check reads.
const WORKFLOW: &str = ".gitea/workflows/ci.yml";
const ACTIONS_DIR: &str = ".gitea/actions";

/// Arguments of `dev ci job-model`.
#[derive(Args, Debug)]
pub struct JobModelArgs {
    /// The command to run.
    #[command(subcommand)]
    pub command: JobModelCommand,
}

/// The commands of `dev ci job-model`.
#[derive(Subcommand, Debug)]
pub enum JobModelCommand {
    /// Check ci.yml's job model: it builds, every job keeps the conventions, `render` is injective over the legs the planner emits.
    /// Report-only (`::warning::` lines, exit 0) until the first green run on the real tree is recorded; then a finding exits 5.
    Check(CheckArgs),
}

/// Arguments of `dev ci job-model check`.
#[derive(Args, Debug)]
pub struct CheckArgs {
    /// The repository root (default: this checkout's root).
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Fail on a finding whatever [`BLOCKING`] says (the positive control and a local strict run).
    #[arg(long)]
    pub strict: bool,
}

/// One thing the check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The job, when the finding belongs to one.
    pub job: Option<String>,
    /// What is wrong and what to do.
    pub message: String,
}

fn finding(job: Option<&str>, message: impl Into<String>) -> Finding {
    Finding {
        job: job.map(str::to_string),
        message: message.into(),
    }
}

/// A set of planned legs: a name for the report and the three matrices ci.yml reads from the plan job.
pub struct Shape {
    /// What the planner is doing in this shape.
    pub name: &'static str,
    /// `crate_legs`.
    pub crate_legs: Vec<Value>,
    /// `paddock_dev_legs`.
    pub paddock_dev_legs: Vec<Value>,
    /// `apps`.
    pub apps: Vec<Value>,
}

fn packages() -> Vec<String> {
    ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn app_leg(app: &str, os: &str, e2e_leg: &str) -> Value {
    json!({ "app": app, "os": os, "e2e_leg": e2e_leg })
}

/// The legs the planner can emit, made by the planner's own functions where it has them ([`plan_ci::workspace_legs`],
/// [`plan_ci::partition_linux_legs`]); the app legs have the keys `plan_ci::matrix` gives a leg (`app`, `os`, `e2e_leg`), with its e2e
/// shards and the WebKit leg.
pub fn shapes() -> Vec<Shape> {
    let all = plan_ci::os_legs();
    let whole = || plan_ci::workspace_legs(&all, None, "", None);
    let partitioned = || plan_ci::partition_linux_legs(whole(), &packages(), LINUX_PARTITIONS);
    let apps_full = || {
        vec![
            app_leg("x", "ubuntu-latest", ""),
            app_leg("x", "macos-latest", ""),
            app_leg("x", "ubuntu-latest", "e2e-1"),
            app_leg("x", "ubuntu-latest", "e2e-2"),
            app_leg("x", "ubuntu-latest", "e2e-webkit"),
            app_leg("z", "ubuntu-latest", ""),
        ]
    };
    let mut out = vec![
        Shape {
            name: "whole Linux legs and the other two OS",
            crate_legs: whole(),
            paddock_dev_legs: whole(),
            apps: apps_full(),
        },
        Shape {
            name: "Linux split into partitions",
            crate_legs: partitioned(),
            paddock_dev_legs: partitioned(),
            apps: apps_full(),
        },
        Shape {
            name: "no planned leg",
            crate_legs: Vec::new(),
            paddock_dev_legs: Vec::new(),
            apps: Vec::new(),
        },
    ];
    for os in plan_ci::OS_LEGS {
        let one = || plan_ci::workspace_legs(&[os.to_string()], None, "", None);
        out.push(Shape {
            name: match os {
                "ubuntu-latest" => "the Linux leg alone",
                "windows-latest" => "the Windows leg alone",
                _ => "the macOS leg alone",
            },
            crate_legs: one(),
            paddock_dev_legs: one(),
            apps: vec![app_leg("x", os, "")],
        });
    }
    out
}

/// The plan outputs of a run that wants everything: every project, WebKit and the apps.
fn all_outputs() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = jobs::PROJECT_OUTPUTS
        .iter()
        .map(|(output, _, _)| (output.to_string(), "true".to_string()))
        .collect();
    for o in ["webkit", "any_app", "apps"] {
        out.push((o.to_string(), "true".to_string()));
    }
    out
}

/// A job's conventions: the plan is its need, `rerun_jobs` its gate, the dev tool restored before its first `./dev`.
fn conventions(model: &JobModel, out: &mut Vec<Finding>) {
    for job in &model.jobs {
        if job.id == "plan" {
            continue;
        }
        if job.needs != ["plan"] {
            out.push(finding(
                Some(&job.id),
                format!(
                    "job {} must `needs: [plan]` (it needs {:?}): the plan's outputs and `rerun_jobs` come from it",
                    job.id, job.needs
                ),
            ));
        }
        if !job.asks_rerun_jobs {
            out.push(finding(
                Some(&job.id),
                format!(
                    "job {}'s `if:` must run it when `rerun_jobs` names it and not when `rerun_jobs` names only another job: a failed-only round would run it whole or never (copy the `contains(fromJSON(needs.plan.outputs.rerun_jobs ...), '{}')` clause of a sibling job)",
                    job.id, job.id
                ),
            ));
        }
        if !job.tool_cache_before_first_dev {
            out.push(finding(
                Some(&job.id),
                format!(
                    "job {} runs `./dev` before the dev tool is restored: put `uses: ./.gitea/actions/dev-tool-cache` (or an action that starts with it) before its first `./dev` step",
                    job.id
                ),
            ));
        }
    }
}

/// The rendered names over every shape (injective), and the leg keys the names read.
fn names(model: &JobModel, out: &mut Vec<Finding>) {
    for shape in shapes() {
        let planned = PlannedLegs {
            crate_legs: &shape.crate_legs,
            paddock_dev_legs: &shape.paddock_dev_legs,
            apps: &shape.apps,
        };
        if let Err(e) = expected_of(model, &all_outputs(), &planned) {
            out.push(finding(
                None,
                format!("render is not injective or a name cannot be listed (legs: {}): {e}", shape.name),
            ));
        }
    }
    // A name that reads a leg key the scope line does not keep renames a rerun leg (the rerun legs are written from the line's leg).
    for job in &model.jobs {
        for key in matrix_keys(&job.name) {
            if !LEG_KEYS.contains(&key.as_str()) {
                out.push(finding(
                    Some(&job.id),
                    format!("the name of job {} reads matrix.{key}, which the scope line does not keep (run_scope::LEG_KEYS): a rerun leg would rename the job; add the key there", job.id),
                ));
            }
        }
    }
}

/// The `matrix.<key>` names a `name:` template reads.
fn matrix_keys(template: &str) -> Vec<String> {
    template
        .split("matrix.")
        .skip(1)
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect::<String>()
        })
        .filter(|k| !k.is_empty())
        .collect()
}

/// The findings of a workflow text, the inputs file text and the action files by folder name. Pure: nothing is read here.
pub fn findings(workflow: &str, inputs: &str, actions: &BTreeMap<String, String>) -> Vec<Finding> {
    let mut out = Vec::new();
    let tool_cache_actions = match jobs::tool_cache_actions(actions) {
        Ok(t) => t,
        Err(e) => return vec![finding(None, format!("the action files cannot be read: {e}"))],
    };
    let projects = match crate::repo_inputs::ci_jobs(inputs) {
        Ok(p) => p,
        Err(e) => return vec![finding(None, e)],
    };
    let model = match jobs::model(
        workflow,
        &Declared {
            projects,
            tool_cache_actions,
        },
    ) {
        Ok(m) => m,
        Err(problems) => {
            return problems
                .iter()
                .map(|p| finding(p.job.as_deref(), format!("ci.yml does not model (line {}): {}", p.line, p.message)))
                .collect();
        }
    };
    conventions(&model, &mut out);
    names(&model, &mut out);
    bridge(&model, &mut out);
    out
}

/// While `CI_JOBS` exists: the model's projects of every job equal it.
fn bridge(model: &JobModel, out: &mut Vec<Finding>) {
    let table: BTreeSet<&str> = CI_JOBS.iter().map(|(j, _)| *j).collect();
    let modelled: BTreeSet<&str> = model.jobs.iter().map(|j| j.id.as_str()).collect();
    for j in modelled.difference(&table) {
        out.push(finding(
            Some(j),
            format!("job {j} is in ci.yml but not in plan_gitea::CI_JOBS (until ADR 0117 D12 both name the jobs)"),
        ));
    }
    for j in table.difference(&modelled) {
        out.push(finding(
            Some(j),
            format!("plan_gitea::CI_JOBS names job {j}, which ci.yml does not have"),
        ));
    }
    for (job, want) in CI_JOBS {
        let Some(m) = model.job(job) else { continue };
        let want: BTreeSet<String> = want.iter().map(|p| p.to_string()).collect();
        if m.projects != want {
            out.push(finding(
                Some(job),
                format!("job {job} runs for {:?} by ci.yml's conditions and [ci.jobs], and for {want:?} by plan_gitea::CI_JOBS: make them equal", m.projects),
            ));
        }
    }
}

/// The action files of a checkout: folder name to `action.yml` text. A folder without one is left out.
fn read_actions(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let dir = root.join(ACTIONS_DIR);
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("{ACTIONS_DIR} cannot be read: {e}"))? {
        let entry = entry.map_err(|e| format!("{ACTIONS_DIR}: {e}"))?;
        let file = entry.path().join("action.yml");
        if file.is_file() {
            let text = std::fs::read_to_string(&file).map_err(|e| format!("{} cannot be read: {e}", file.display()))?;
            out.insert(entry.file_name().to_string_lossy().into_owned(), text.replace("\r\n", "\n"));
        }
    }
    Ok(out)
}

/// The findings of the repository at `root`; `Err` when a file cannot be read (a check that cannot see its input is not a pass, ER-33).
pub fn findings_at(root: &Path) -> Result<Vec<Finding>, String> {
    let read = |rel: &str| {
        std::fs::read_to_string(root.join(rel))
            .map(|t| t.replace("\r\n", "\n"))
            .map_err(|e| format!("{rel} cannot be read: {e}"))
    };
    Ok(findings(&read(WORKFLOW)?, &read(crate::repo_inputs::FILE)?, &read_actions(root)?))
}

/// `dev ci job-model check`.
pub fn command(a: &JobModelArgs) -> Result<Report, CliError> {
    let JobModelCommand::Check(c) = &a.command;
    let root = match &c.root {
        Some(r) => r.clone(),
        None => {
            let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("current folder: {e}")))?;
            crate::worktree::git::git_at(&cwd)
                .text(&["rev-parse", "--show-toplevel"])
                .map(|t| PathBuf::from(t.trim()))
                .map_err(|e| CliError::failure(format!("job-model check: {e}")))?
        }
    };
    let found = findings_at(&root).map_err(|why| {
        CliError::new(
            Exit::NotMeasured,
            format!("::error::job-model check NOT MEASURED: {why}. Next step: run it in a full checkout (`./dev worktree add` makes one)."),
        )
    })?;
    let blocking = BLOCKING || c.strict;
    let level = if blocking { "error" } else { "warning" };
    let text: String = found
        .iter()
        .map(|f| {
            format!(
                "::{level}::job-model check: {} Next step: fix ci.yml, docs/repo-graph.toml [ci.jobs] or the planner (ADR 0123 §7).\n",
                f.message
            )
        })
        .collect();
    let data = json!({
        "findings": found.iter().map(|f| json!({"job": f.job, "message": f.message})).collect::<Vec<_>>(),
        "blocking": blocking,
        "review_by": REVIEW_BY,
    });
    let text = if found.is_empty() {
        "job-model check: ci.yml models, every job keeps the conventions and every name is unique over the planner's legs\n".to_string()
    } else if blocking {
        text
    } else {
        format!(
            "{text}job-model check: report-only until its first green run is recorded (review by {REVIEW_BY}); {} finding(s)\n",
            found.len()
        )
    };
    let report = Report::new(data, text);
    Ok(if blocking && !found.is_empty() {
        report.with_exit(Exit::Findings)
    } else {
        report
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const INPUTS: &str = "version = 1\n[[project]]\nid = \"ui\"\n[[project]]\nid = \"crates\"\n[ci.jobs]\nplan = [\"everything\"]\n";
    const RESTORE: &str = "runs:\n  using: composite\n  steps:\n    - run: echo restore\n";

    fn actions() -> BTreeMap<String, String> {
        BTreeMap::from([("dev-tool-cache".to_string(), RESTORE.to_string())])
    }

    /// A workflow in the shape of ci.yml: a plan job, a `ui` job and a `crate` job over the planner's legs.
    fn workflow(crate_name: &str, ui_extra: &str, crate_extra: &str) -> String {
        format!(
            "name: ci\njobs:\n  plan:\n    runs-on: ubuntu-latest\n    steps:\n      - run: ./dev plan --ci\n  ui:\n    needs: [plan]\n    if: ${{{{ needs.plan.outputs.ui == 'true' && contains(fromJSON(needs.plan.outputs.rerun_jobs || '[\"ui\"]'), 'ui') }}}}\n{ui_extra}    runs-on: ubuntu-latest\n    steps:\n      - uses: ./.gitea/actions/dev-tool-cache\n      - run: ./dev ci run ui tests\n  crate:\n    needs: [plan]\n    if: ${{{{ needs.plan.outputs.crates == 'true' && contains(fromJSON(needs.plan.outputs.rerun_jobs || '[\"crate\"]'), 'crate') }}}}\n    name: {crate_name}\n{crate_extra}    strategy:\n      matrix:\n        include: ${{{{ fromJSON(needs.plan.outputs.crate_legs) }}}}\n    runs-on: ${{{{ matrix.os }}}}\n    steps:\n      - uses: ./.gitea/actions/dev-tool-cache\n      - run: ./dev ci run crates tests\n"
        )
    }

    fn good() -> String {
        workflow("crate (${{ matrix.os }}${{ matrix.part_label }})", "", "")
    }

    fn run(workflow: &str, inputs: &str) -> Vec<Finding> {
        // the bridge compares with the real CI_JOBS, which names more jobs than the toy workflow: leave it out of these tests' reading
        findings(workflow, inputs, &actions())
            .into_iter()
            .filter(|f| !f.message.contains("plan_gitea::CI_JOBS"))
            .collect()
    }

    #[test]
    fn a_workflow_that_keeps_every_convention_has_no_finding() {
        assert_eq!(run(&good(), INPUTS), Vec::<Finding>::new());
    }

    /// Positive control (ER-33): a name template without the partition label renders the three Linux partitions alike.
    #[test]
    fn a_name_template_without_part_label_is_not_injective_over_the_partitions() {
        let found = run(&workflow("crate (${{ matrix.os }})", "", ""), INPUTS);
        assert!(
            found
                .iter()
                .any(|f| f.message.contains("twice") && f.message.contains("partitions")),
            "{found:?}"
        );
    }

    /// Positive control: a job without the rerun gate.
    #[test]
    fn a_job_without_the_rerun_gate_is_found() {
        let wf = good().replace(" && contains(fromJSON(needs.plan.outputs.rerun_jobs || '[\"ui\"]'), 'ui')", "");
        let found = run(&wf, INPUTS);
        assert!(
            found
                .iter()
                .any(|f| f.job.as_deref() == Some("ui") && f.message.contains("rerun_jobs")),
            "{found:?}"
        );
    }

    /// Positive control: a job that does not need the plan.
    #[test]
    fn a_job_that_does_not_need_the_plan_is_found() {
        let wf = good().replacen("  ui:\n    needs: [plan]\n", "  ui:\n    needs: []\n", 1);
        let found = run(&wf, INPUTS);
        assert!(
            found
                .iter()
                .any(|f| f.job.as_deref() == Some("ui") && f.message.contains("needs: [plan]")),
            "{found:?}"
        );
    }

    /// Positive control: a `./dev` call before the tool is restored; and the same job passes through an action the files show restores it.
    #[test]
    fn a_dev_call_before_the_tool_cache_is_found_and_an_action_that_restores_it_counts() {
        let late = good().replacen(
            "      - uses: ./.gitea/actions/dev-tool-cache\n      - run: ./dev ci run ui tests\n",
            "      - run: ./dev ci run ui tests\n",
            1,
        );
        let found = run(&late, INPUTS);
        assert!(
            found
                .iter()
                .any(|f| f.job.as_deref() == Some("ui") && f.message.contains("dev-tool-cache")),
            "{found:?}"
        );
        let through = good().replacen(
            "      - uses: ./.gitea/actions/dev-tool-cache\n      - run: ./dev ci run ui tests\n",
            "      - uses: ./.gitea/actions/node-caches\n      - run: ./dev ci run ui tests\n",
            1,
        );
        let mut files = actions();
        files.insert(
            "node-caches".into(),
            "runs:\n  using: composite\n  steps:\n    - uses: ./.gitea/actions/dev-tool-cache\n".into(),
        );
        let found: Vec<Finding> = findings(&through, INPUTS, &files)
            .into_iter()
            .filter(|f| !f.message.contains("CI_JOBS"))
            .collect();
        assert_eq!(
            found,
            Vec::<Finding>::new(),
            "an action that starts with the restore counts, read from its file"
        );
        // without the action file the same workflow is found: the set is derived from the files, not declared
        let found = run(&through, INPUTS);
        assert!(found.iter().any(|f| f.message.contains("dev-tool-cache")), "{found:?}");
    }

    /// Positive control: a workflow the model cannot build (a job that runs for no project) is a finding, never a pass.
    #[test]
    fn a_workflow_that_does_not_model_is_a_finding() {
        let found = run(&good(), "version = 1\n[ci.jobs]\n");
        assert!(found.iter().any(|f| f.message.contains("does not model")), "{found:?}");
    }

    /// A leg the scope line records must render the name it was recorded under: a key the name uses and the line drops renames a rerun.
    #[test]
    fn a_name_that_uses_a_key_the_scope_line_does_not_keep_is_found() {
        let found = run(
            &workflow("crate (${{ matrix.os }}${{ matrix.part_label }}${{ matrix.part }})", "", ""),
            INPUTS,
        );
        assert!(found.iter().any(|f| f.message.contains("would rename")), "{found:?}");
    }

    /// The review date has not passed with the check still report-only and unrecorded (ER-41).
    #[test]
    fn a_check_past_its_review_date_is_blocking_or_recorded() {
        let today = jiff::Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date().to_string();
        assert!(
            today.as_str() <= REVIEW_BY || BLOCKING || !BLOCKING_RECORD.is_empty(),
            "the job-model check is past its review date {REVIEW_BY}: make it blocking or delete it (ER-41)"
        );
    }
}
