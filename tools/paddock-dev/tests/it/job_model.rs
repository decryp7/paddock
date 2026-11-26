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
