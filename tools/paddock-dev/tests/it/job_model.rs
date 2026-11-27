//! The job model of ci.yml (`paddock-ci-config`, ADR 0123) against this repository's real workflow, the declared
//! `[ci.jobs]` table and the planner's job table, which stays until ADR 0117's remaining deletion (D12).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_ci_config::jobs::{Declared, JobModel, model};
use paddock_dev::plan_gitea::CI_JOBS;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    crate::full_tree(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap())
}

fn ci_text() -> String {
    std::fs::read_to_string(root().join(".gitea/workflows/ci.yml"))
        .unwrap()
        .replace("\r\n", "\n")
}

fn declared() -> Declared {
    Declared {
        projects: paddock_dev::repo_inputs::load_ci_jobs(&root()).unwrap(),
        ..Declared::default()
    }
}

fn real_model() -> JobModel {
    model(&ci_text(), &declared()).unwrap_or_else(|p| panic!("ci.yml models: {p:#?}"))
}

/// The bridge to ADR 0117 D12: while `CI_JOBS` exists, the model's projects of every job equal it, and both name the same jobs.
#[test]
fn the_models_projects_equal_the_planners_job_table_for_every_job() {
    let m = real_model();
    let table: BTreeSet<&str> = CI_JOBS.iter().map(|(j, _)| *j).collect();
    let modelled: BTreeSet<&str> = m.jobs.iter().map(|j| j.id.as_str()).collect();
    assert_eq!(modelled, table, "ci.yml's jobs and plan_gitea::CI_JOBS name the same jobs");
    assert!(modelled.len() >= 10, "a positive control: ci.yml's jobs were read: {modelled:?}");
    for (job, projects) in CI_JOBS {
        let want: BTreeSet<String> = projects.iter().map(|p| p.to_string()).collect();
        assert_eq!(m.job(job).unwrap().projects, want, "the projects of job {job}");
    }
}

/// A positive control (ER-33): a table that differs from the model is seen.
#[test]
fn a_job_table_that_differs_from_the_model_is_seen() {
    let m = real_model();
    let ui = m.job("ui").unwrap();
    assert_ne!(
        ui.projects,
        ["ui".to_string()].into_iter().collect::<BTreeSet<_>>(),
        "ui also runs for eslint and icons"
    );
}

#[test]
fn every_job_but_the_plan_needs_the_plan_and_asks_rerun_jobs_for_itself() {
    let m = real_model();
    for job in &m.jobs {
        if job.id == "plan" {
            continue;
        }
        assert_eq!(job.needs, ["plan"], "job {}", job.id);
        assert!(job.asks_rerun_jobs, "job {} asks rerun_jobs for itself", job.id);
    }
}

#[test]
fn the_inputs_file_declares_what_no_condition_shows() {
    let d = declared();
    assert_eq!(d.projects["plan"], ["everything"]);
    assert!(d.projects.contains_key("gitleaks") && d.projects.contains_key("test-cache-shadow") && d.projects.contains_key("quick"));
}

/// The probe that the check sees what it checks (ER-33): on the real tree it reads ci.yml's jobs and the action files, the model builds
/// and the dev tool's restore is found in them, not in a hand list.
#[test]
fn the_check_reads_the_real_tree_and_finds_no_model_problem() {
    let found = paddock_dev::ci_job_model::findings_at(&root()).unwrap();
    let unreadable: Vec<_> = found
        .iter()
        .filter(|f| f.message.contains("does not model") || f.message.contains("cannot be read"))
        .collect();
    assert!(unreadable.is_empty(), "{unreadable:#?}");
    let actions = std::fs::read_dir(root().join(".gitea/actions"))
        .unwrap()
        .filter_map(|e| {
            let e = e.unwrap();
            std::fs::read_to_string(e.path().join("action.yml"))
                .ok()
                .map(|t| (e.file_name().to_string_lossy().into_owned(), t))
        })
        .collect();
    let derived = paddock_ci_config::jobs::tool_cache_actions(&actions).unwrap();
    assert!(
        derived.contains("dev-tool-cache") && derived.contains("setup-rust"),
        "the actions that restore the dev tool are read from the action files: {derived:?}"
    );
}

/// Removal tests (ER-33): the check runs in ci.yml's plan job and in the member gate, and a change that deletes either line fails here.
#[test]
fn the_plan_job_runs_the_check_on_every_run_and_the_member_gate_has_its_row() {
    let ci = ci_text();
    let doc = paddock_ci_config::yaml::parse(&ci).unwrap();
    let plan = doc.get("jobs").unwrap().get("plan").unwrap();
    let step = plan
        .get("steps")
        .unwrap()
        .items()
        .iter()
        .find(|s| s.get_text("run").is_some_and(|r| r.trim() == "./dev ci job-model check"))
        .expect("the plan job has a step that runs `./dev ci job-model check`");
    assert!(
        step.get("if").is_none(),
        "the step runs on every event, whatever the change (ER-41)"
    );
    assert!(
        paddock_dev::pre_push::gates::ROW_SET.contains(&"job-model"),
        "the member gate's job-model row (pre_push/gates.rs)"
    );
}
