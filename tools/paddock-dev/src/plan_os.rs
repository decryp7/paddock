//! The `os` input of ci.yml (2026-11-21, user: member trials use spare lanes): a run that does only one operating system's part of
//! the plan. The member gate of `merge check --gates` dispatches `ci.yml` with `os=linux` at once, on Linux lanes the train is not
//! using, and `os=windows` after the train's reservation (the Mac runner's jobs are not judged by the gate, so no run asks for them).
//!
//! It is the same planner input every other narrowing uses, not a second copy of "what runs": the plan job (`dev plan --ci` with
//! `PLAN_OS`) filters the matrix legs it writes (apps, crates, paddock-dev) to the OS and writes `rerun_jobs`, the list every job's
//! `if` already reads (an empty list means "all"), as the jobs of ci.yml that can run on the OS. A job runs on an OS when its
//! `runs-on` names it, or is an expression over its matrix (its legs were filtered). `plan` itself always runs.

use serde_json::Value;
use yaml_rust2::{Yaml, YamlLoader};

/// The operating system part a run is restricted to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsPart {
    /// `ubuntu-latest`.
    Linux,
    /// `windows-latest`.
    Windows,
    /// `macos-latest`.
    Macos,
}

impl OsPart {
    /// The value of the `os` input.
    pub fn input(self) -> &'static str {
        match self {
            OsPart::Linux => "linux",
            OsPart::Windows => "windows",
            OsPart::Macos => "macos",
        }
    }

    /// The runner label of a matrix leg or a job.
    pub fn label(self) -> &'static str {
        match self {
            OsPart::Linux => "ubuntu-latest",
            OsPart::Windows => "windows-latest",
            OsPart::Macos => "macos-latest",
        }
    }

    fn matches(self, label: &str) -> bool {
        let l = label.to_ascii_lowercase();
        match self {
            OsPart::Linux => l.contains("ubuntu") || l.contains("linux"),
            OsPart::Windows => l.contains("windows"),
            OsPart::Macos => l.contains("macos"),
        }
    }
}

/// The `os` input: empty means every OS (`None`); anything but `linux`, `windows` or `macos` is an error naming the choices.
pub fn parse(value: &str) -> Result<Option<OsPart>, String> {
    match value.trim() {
        "" => Ok(None),
        "linux" => Ok(Some(OsPart::Linux)),
        "windows" => Ok(Some(OsPart::Windows)),
        "macos" => Ok(Some(OsPart::Macos)),
        other => Err(format!(
            "the os input is {other:?}: use linux, windows or macos (or leave it empty for all)"
        )),
    }
}

/// The legs (objects with an `os` label) that run on `os`.
pub fn filter_legs(legs: Vec<Value>, os: OsPart) -> Vec<Value> {
    legs.into_iter()
        .filter(|l| l["os"].as_str().is_some_and(|label| os.matches(label)))
        .collect()
}

/// The keys of the jobs of a workflow that can run on `os` (see the module doc), sorted; `plan` is not among them. `Err` when the
/// workflow is not valid YAML.
pub fn jobs_on(workflow_yaml: &str, os: OsPart) -> Result<Vec<String>, String> {
    let docs = YamlLoader::load_from_str(workflow_yaml).map_err(|e| format!("the workflow is not valid YAML: {e}"))?;
    let mut keys: Vec<String> = Vec::new();
    for doc in &docs {
        let Some(jobs) = doc["jobs"].as_hash() else { continue };
        for (key, job) in jobs {
            let Some(key) = key.as_str().filter(|k| *k != "plan") else {
                continue;
            };
            let mut labels = Vec::new();
            collect(&job["runs-on"], &mut labels);
            let on_os = labels.iter().any(|l| if l.contains("${{") { true } else { os.matches(l) });
            if on_os {
                keys.push(key.to_string());
            }
        }
    }
    keys.sort();
    keys.dedup();
    Ok(keys)
}

fn collect(y: &Yaml, out: &mut Vec<String>) {
    match y {
        Yaml::String(s) => out.push(s.clone()),
        Yaml::Array(a) => a.iter().for_each(|v| collect(v, out)),
        Yaml::Hash(h) => h.values().for_each(|v| collect(v, out)),
        _ => {}
    }
}

/// The plan job outputs of an `os` run besides the filtered `apps` legs: `rerun_jobs`, and `crate_legs` and `paddock_dev_legs`
/// narrowed from `default_legs` (the workflow's `PLAN_OS_LEGS`, a JSON array of `{"os": ...}`).
pub fn outputs(workflow_yaml: &str, os: OsPart, default_legs: &str) -> Result<Vec<(String, String)>, String> {
    let jobs = jobs_on(workflow_yaml, os)?;
    let legs: Vec<Value> = serde_json::from_str(default_legs).map_err(|e| format!("PLAN_OS_LEGS is not a JSON array: {e}"))?;
    let legs = serde_json::to_string(&filter_legs(legs, os)).map_err(|e| e.to_string())?;
    Ok(vec![
        ("rerun_jobs".into(), serde_json::to_string(&jobs).map_err(|e| e.to_string())?),
        ("crate_legs".into(), legs.clone()),
        ("paddock_dev_legs".into(), legs),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const WF: &str = r#"
jobs:
  plan:
    runs-on: ubuntu-latest
  ui:
    runs-on: ubuntu-latest
  tools-macos:
    runs-on: macos-latest
  crate:
    runs-on: ${{ matrix.os }}
  e2e-webkit:
    runs-on: macos-latest
"#;

    #[test]
    fn the_os_input_is_one_of_three_or_empty() {
        assert_eq!(parse("").unwrap(), None);
        assert_eq!(parse("linux").unwrap(), Some(OsPart::Linux));
        assert!(parse("freebsd").unwrap_err().contains("use linux, windows or macos"));
    }

    #[test]
    fn a_job_runs_on_an_os_when_it_names_it_or_is_a_matrix_over_filtered_legs() {
        assert_eq!(jobs_on(WF, OsPart::Linux).unwrap(), vec!["crate", "ui"]);
        assert_eq!(jobs_on(WF, OsPart::Windows).unwrap(), vec!["crate"]);
        assert_eq!(jobs_on(WF, OsPart::Macos).unwrap(), vec!["crate", "e2e-webkit", "tools-macos"]);
    }

    #[test]
    fn the_legs_and_outputs_are_narrowed_to_the_os() {
        let legs = vec![
            json!({"os": "ubuntu-latest"}),
            json!({"os": "windows-latest"}),
            json!({"os": "macos-latest"}),
        ];
        assert_eq!(filter_legs(legs, OsPart::Windows), vec![json!({"os": "windows-latest"})]);
        let o = outputs(
            WF,
            OsPart::Windows,
            r#"[{"os":"ubuntu-latest"},{"os":"windows-latest"},{"os":"macos-latest"}]"#,
        )
        .unwrap();
        let get = |k: &str| o.iter().find(|(n, _)| n == k).unwrap().1.clone();
        assert_eq!(get("rerun_jobs"), r#"["crate"]"#);
        assert_eq!(get("crate_legs"), r#"[{"os":"windows-latest"}]"#);
        assert_eq!(get("paddock_dev_legs"), r#"[{"os":"windows-latest"}]"#);
    }

    /// The real ci.yml: every job of it is listed for Linux or for Windows' matrix jobs, so an `os=windows` run asks for nothing but
    /// the matrix jobs and an `os=linux` run for no macOS job (positive control: the two macOS jobs are in the macOS list only).
    #[test]
    fn the_real_ci_yml_splits_by_os_with_no_hand_list() {
        let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.gitea/workflows/ci.yml")).unwrap();
        let linux = jobs_on(&text, OsPart::Linux).unwrap();
        let windows = jobs_on(&text, OsPart::Windows).unwrap();
        let macos = jobs_on(&text, OsPart::Macos).unwrap();
        assert!(
            !linux.contains(&"tools-macos".to_string()) && macos.contains(&"tools-macos".to_string()),
            "{linux:?} {macos:?}"
        );
        assert!(
            linux.contains(&"conformance".to_string()) && !windows.contains(&"conformance".to_string()),
            "{windows:?}"
        );
        for matrix_job in ["crate", "app", "paddock-dev"] {
            assert!(windows.contains(&matrix_job.to_string()), "{matrix_job}: {windows:?}");
        }
        assert_eq!(windows.len(), 3, "{windows:?}");
    }
}
