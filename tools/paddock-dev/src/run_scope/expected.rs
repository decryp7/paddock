//! The display names a run is expected to have (ADR 0123 §3): the planner renders every job of the job model over the legs it emits
//! and writes `{display name: {job, leg}}` into the run's scope line (version 3), so every reader looks a name up instead of
//! inverting a `name:` template.

#[cfg(test)]
use super::Scope;
use paddock_ci_config::expr::{self, Lookup, Tri, Val};
use paddock_ci_config::jobs::{self, Declared as ModelDeclared, Job, JobModel, Leg, Legs, PROJECT_OUTPUTS};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The leg keys the line keeps (a leg value that is empty is left out).
pub const LEG_KEYS: [&str; 7] = ["os", "app", "e2e_leg", "part_label", "test_packages", "part_extra", "shard"];

/// A job a run is expected to have, by display name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Expected {
    /// The workflow job id (`crate`).
    pub job: String,
    /// The leg it runs: only the non-empty keys of [`LEG_KEYS`] (empty for a job without a matrix).
    pub leg: BTreeMap<String, String>,
    /// The projects the job runs for (the job model's `projects`), so a reader maps a job to its projects without a table of its own.
    pub projects: BTreeSet<String>,
}

impl Expected {
    /// The project a failure of this job is filed under: the first project of the job model's order ([`PROJECT_OUTPUTS`]) it runs for,
    /// else the first other one that names a project (not `everything` or `apps`). `None` for a job that runs for none.
    pub fn primary_project(&self) -> Option<&str> {
        PROJECT_OUTPUTS
            .iter()
            .map(|(_, project, _)| *project)
            .find(|p| self.projects.contains(*p))
            .or_else(|| {
                self.projects
                    .iter()
                    .map(String::as_str)
                    .find(|p| !matches!(*p, "everything" | jobs::APPS_PROJECT))
            })
    }
}

/// The expected jobs of a run by display name.
pub type ExpectedMap = BTreeMap<String, Expected>;

/// The legs the planner emitted for the three planned matrices of ci.yml.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlannedLegs<'a> {
    /// `crate_legs`.
    pub crate_legs: &'a [Value],
    /// `paddock_dev_legs`.
    pub paddock_dev_legs: &'a [Value],
    /// `apps`.
    pub apps: &'a [Value],
}

/// The job model of a workflow text with the declared projects of the inputs file text.
pub fn load_model_from(workflow_text: &str, inputs_text: &str) -> Result<JobModel, String> {
    let projects = crate::repo_inputs::ci_jobs(inputs_text)?;
    jobs::model(
        workflow_text,
        &ModelDeclared {
            projects,
            ..ModelDeclared::default()
        },
    )
    .map_err(|p| {
        let lines: Vec<String> = p
            .iter()
            .map(|p| format!("{} (line {}): {}", p.job.as_deref().unwrap_or("-"), p.line, p.message))
            .collect();
        format!("ci.yml does not model: {}", lines.join("; "))
    })
}

/// The job model of the repository at `root` (`.gitea/workflows/ci.yml` and `docs/repo-graph.toml`).
pub fn load_model(root: &std::path::Path) -> Result<JobModel, String> {
    let read = |rel: &str| {
        std::fs::read_to_string(root.join(rel))
            .map(|t| t.replace("\r\n", "\n"))
            .map_err(|e| format!("{rel} cannot be read ({e}); the scope line names the jobs it expects from it"))
    };
    load_model_from(&read(".gitea/workflows/ci.yml")?, &read(crate::repo_inputs::FILE)?)
}

/// The job name without the calling workflow's job: a run called from another workflow (the nightly's) lists its jobs as
/// `<caller> / <job>` (ADR 0123 §3). The one place the prefix is stripped.
pub fn strip_caller(name: &str) -> &str {
    name.rsplit(" / ").next().unwrap_or(name)
}

/// A job found in a run's scope line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found<'a> {
    /// The workflow job id.
    pub job: String,
    /// The leg the line gives it; `None` when only the job's own log named it (the leg is not measured).
    pub leg: Option<&'a BTreeMap<String, String>>,
    /// The projects the line gives it; `None` when only the job's own log named it.
    pub projects: Option<&'a BTreeSet<String>>,
}

/// The entry of a job's display name in a run's `expected` map: the name as listed, then without its caller prefix.
pub fn lookup_entry<'a>(expected: &'a ExpectedMap, name: &str) -> Option<&'a Expected> {
    expected.get(name).or_else(|| expected.get(strip_caller(name)))
}

/// Looks a job's display name up in a run's `expected` map: the name as listed, then without its caller prefix. A name the line does
/// not hold falls back to the `Job:` line of the job's log (`log`), which gives the id and no leg. `None` when neither knows it.
pub fn lookup<'a>(expected: &'a ExpectedMap, name: &str, log: Option<&str>) -> Option<Found<'a>> {
    if let Some(e) = lookup_entry(expected, name) {
        return Some(Found {
            job: e.job.clone(),
            leg: Some(&e.leg),
            projects: Some(&e.projects),
        });
    }
    crate::plan_since::job_key(log?).map(|job| Found {
        job,
        leg: None,
        projects: None,
    })
}

/// A scalar of a planned leg as the runner shows it in a name (`2` of a number, `true` of a boolean); anything else is empty.
fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn text_leg(leg: &Value) -> Leg {
    LEG_KEYS.iter().map(|k| (k.to_string(), scalar(&leg[*k]))).collect()
}

/// Every leg of every key a planned leg object holds, as text (an app workflow's shards carry their own keys).
fn whole_leg(leg: &Value) -> Leg {
    leg.as_object()
        .map(|o| o.iter().map(|(k, v)| (k.clone(), scalar(v))).collect())
        .unwrap_or_default()
}

/// What an app's own workflow can show as job names (ADR 0123 §3, `./dev ci scope`): every job of the workflow text over its matrix,
/// the planned matrices taken from `planned` (the plan job's outputs by name). A job whose planned legs the caller did not give is left
/// out, so a reader finds no leg for it and reports it as not measured. The workflow's names, not only the ones this run ran: a
/// reader needs the leg of whichever job failed.
pub fn app_expected(workflow_text: &str, planned: &BTreeMap<String, Vec<Value>>) -> Result<ExpectedMap, String> {
    let named = jobs::named_jobs(workflow_text).map_err(|p| {
        let lines: Vec<String> = p.iter().map(|p| format!("line {}: {}", p.line, p.message)).collect();
        format!("the workflow does not model: {}", lines.join("; "))
    })?;
    let mut out = ExpectedMap::new();
    for job in named {
        let legs: Vec<Leg> = match &job.legs {
            Legs::None => vec![Leg::new()],
            Legs::Static(legs) => legs.clone(),
            Legs::Planned(output) => planned.get(output).map(|l| l.iter().map(whole_leg).collect()).unwrap_or_default(),
        };
        for leg in legs {
            let name = jobs::render_name(&job.name, &leg);
            let kept = leg
                .into_iter()
                .filter(|(k, v)| LEG_KEYS.contains(&k.as_str()) && !v.is_empty())
                .collect();
            let entry = Expected {
                job: job.id.clone(),
                leg: kept,
                // an app workflow has no project model: the job is the app's own
                projects: BTreeSet::new(),
            };
            if out.insert(name.clone(), entry).is_some() {
                return Err(format!(
                    "two legs of job {} render the name {name:?} twice: the run's jobs could not be told apart",
                    job.id
                ));
            }
        }
    }
    Ok(out)
}

/// The prefix of a plan job output in an `if:`.
const PLAN_OUTPUT: &str = "needs.plan.outputs.";

/// The plan job's outputs as the planner is about to emit them: the names an `if:` reads, with the text the runner would hand over. An
/// output the planner emits no value for reads as empty, as the runner reads an unset output.
struct Outputs<'a>(&'a [(String, String)]);

impl Lookup for Outputs<'_> {
    fn get(&self, name: &str) -> Val {
        match name.strip_prefix(PLAN_OUTPUT) {
            Some(output) => Val::Str(
                self.0
                    .iter()
                    .rev()
                    .find(|(k, _)| k == output)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default(),
            ),
            None => Val::Unknown,
        }
    }
}

/// The legs of a job: the model's own, or the planner's for a planned matrix.
fn legs_of(job: &Job, planned: &PlannedLegs) -> Result<Vec<Leg>, String> {
    Ok(match &job.legs {
        Legs::None => vec![Leg::new()],
        Legs::Static(legs) => legs.clone(),
        Legs::Planned(output) => {
            let legs = match (job.id.as_str(), output.as_str()) {
                ("crate", "crate_legs") => planned.crate_legs,
                ("paddock-dev", "paddock_dev_legs") => planned.paddock_dev_legs,
                ("app", "apps") => planned.apps,
                _ => return Err(format!("job {} has planned legs `{output}` that the planner does not name", job.id)),
            };
            legs.iter().map(text_leg).collect()
        }
    })
}

/// The expected jobs of a run (ADR 0123 §3): every job whose `if:` the plan's `outputs` make true, rendered over the legs the planner
/// emitted. A job without an `if:` always runs and is listed; the plan job itself, which writes the line, is not. An `if:` the evaluator cannot
/// settle is an error naming the job, never a guess in either direction (ER-33). A name two legs share is refused: a lookup by name
/// would be ambiguous (the check of ADR 0123 §7 is this, made at the writer).
pub fn expected_of(model: &JobModel, outputs: &[(String, String)], planned: &PlannedLegs) -> Result<ExpectedMap, String> {
    let lookup = Outputs(outputs);
    let mut out = ExpectedMap::new();
    for job in &model.jobs {
        // The plan job writes the line and is not one of the run's jobs it lists; the model marks it by running for `everything`.
        if job.projects.contains("everything") {
            continue;
        }
        // A job without an `if:` always runs.
        if let Some(condition) = &job.condition {
            match expr::condition(condition, &lookup).0 {
                Tri::Yes => {}
                Tri::No => continue,
                Tri::Unknown => {
                    return Err(format!(
                        "the `if:` of job {} cannot be evaluated against the plan's outputs ({condition}): the run's jobs cannot be listed",
                        job.id
                    ));
                }
            }
        }
        for leg in legs_of(job, planned)? {
            let name = jobs::render(job, &leg);
            let kept = leg
                .into_iter()
                .filter(|(k, v)| LEG_KEYS.contains(&k.as_str()) && !v.is_empty())
                .collect();
            let entry = Expected {
                job: job.id.clone(),
                leg: kept,
                projects: job.projects.clone(),
            };
            if out.insert(name.clone(), entry).is_some() {
                return Err(format!(
                    "two legs of job {} render the name {name:?} twice: the run's jobs could not be told apart",
                    job.id
                ));
            }
        }
    }
    Ok(out)
}

/// The plan outputs a test's `ran` scope stands for: each project flag true when its plan key ran (`ui` also when a reference-app job
/// did), `webkit` when a reference-app job did, `any_app` when an app leg did; no `rerun_jobs` (a full run).
#[cfg(test)]
pub fn test_outputs(ran: &Scope) -> Vec<(String, String)> {
    let webkit = ran.jobs.contains("e2e") || ran.jobs.contains("e2e-webkit");
    let mut out: Vec<(String, String)> = PROJECT_OUTPUTS
        .iter()
        .map(|(output, project, key)| {
            // The reference-app jobs run for a UI.
            let on = ran.jobs.contains(*key) || ran.jobs.contains(*project) || (webkit && *project == "ui");
            (output.to_string(), on.to_string())
        })
        .collect();
    out.push(("webkit".into(), webkit.to_string()));
    out.push(("any_app".into(), (!ran.apps.is_empty()).to_string()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn model() -> JobModel {
        load_model_from(
            include_str!("../../../../.gitea/workflows/ci.yml"),
            include_str!("../../../../docs/repo-graph.toml"),
        )
        .unwrap_or_else(|e| panic!("ci.yml models: {e}"))
    }

    fn ran(keys: &[&str]) -> Scope {
        Scope {
            jobs: keys.iter().map(|k| k.to_string()).collect(),
            ..Scope::default()
        }
    }

    fn crate_legs() -> Vec<Value> {
        let mut legs: Vec<Value> = (1..=3)
            .map(|k| json!({"os": "ubuntu-latest", "part_label": format!(", {k}/3"), "test_packages": format!("pkg{k}")}))
            .collect();
        legs.push(json!({"os": "windows-latest"}));
        legs.push(json!({"os": "macos-latest"}));
        legs
    }

    fn all_crate_keys() -> Scope {
        ran(&["crates", "crate@ubuntu-latest", "crate@windows-latest", "crate@macos-latest"])
    }

    #[test]
    fn the_linux_crate_job_is_expected_once_per_partition_and_each_name_carries_its_leg() {
        let planned = PlannedLegs {
            crate_legs: &crate_legs(),
            ..PlannedLegs::default()
        };
        let map = expected_of(&model(), &test_outputs(&all_crate_keys()), &planned).unwrap();
        for k in 1..=3 {
            let e = &map[&format!("crate (ubuntu-latest, {k}/3)")];
            assert_eq!(e.job, "crate");
            assert_eq!(e.leg["os"], "ubuntu-latest");
            assert_eq!(e.leg["part_label"], format!(", {k}/3"));
            assert_eq!(e.leg["test_packages"], format!("pkg{k}"));
        }
        assert!(map.contains_key("crate (windows-latest)") && map.contains_key("crate (macos-latest)"));
        assert!(
            !map.contains_key("crate (ubuntu-latest)"),
            "a partitioned leg has no whole-leg name"
        );
    }

    fn outs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn jobs_of(map: &ExpectedMap) -> BTreeSet<&str> {
        map.values().map(|e| e.job.as_str()).collect()
    }

    #[test]
    fn a_ui_run_that_selected_no_reference_app_job_does_not_expect_one() {
        // Run 9167's shape: the UI ran (a train's full run) but the plan emitted `webkit=false`, so the reference-app jobs are skipped.
        let map = expected_of(
            &model(),
            &outs(&[
                ("ui", "true"),
                ("eslint", "true"),
                ("icons", "true"),
                ("conformance", "true"),
                ("webkit", "false"),
            ]),
            &PlannedLegs::default(),
        )
        .unwrap();
        let jobs = jobs_of(&map);
        assert!(jobs.contains("ui") && jobs.contains("conformance"), "{jobs:?}");
        assert!(!jobs.contains("e2e") && !jobs.contains("e2e-webkit"), "{jobs:?}");
    }

    #[test]
    fn a_webkit_run_expects_the_reference_app_jobs_with_their_shards() {
        let map = expected_of(&model(), &outs(&[("ui", "true"), ("webkit", "true")]), &PlannedLegs::default()).unwrap();
        assert!(
            map.contains_key("reference apps (shard 1/2)") && map.contains_key("reference apps (shard 2/2)"),
            "{map:?}"
        );
        assert!(jobs_of(&map).contains("e2e-webkit"), "{map:?}");
        // Positive control: without a UI the reference-app jobs are not expected even when WebKit is wanted.
        let no_ui = expected_of(&model(), &outs(&[("ui", "false"), ("webkit", "true")]), &PlannedLegs::default()).unwrap();
        assert!(!jobs_of(&no_ui).contains("e2e"), "{no_ui:?}");
    }

    #[test]
    fn a_rerun_round_expects_only_the_jobs_it_names() {
        let planned = PlannedLegs {
            crate_legs: &crate_legs(),
            ..PlannedLegs::default()
        };
        let map = expected_of(
            &model(),
            &outs(&[("crates", "true"), ("conformance", "true"), ("rerun_jobs", r#"["crate"]"#)]),
            &planned,
        )
        .unwrap();
        assert_eq!(jobs_of(&map), BTreeSet::from(["crate"]), "{map:?}");
        // A reused run (the plan emits `rerun_jobs=[]`) expects nothing.
        let reused = expected_of(&model(), &outs(&[("crates", "true"), ("rerun_jobs", "[]")]), &planned).unwrap();
        assert!(reused.is_empty(), "{reused:?}");
    }

    /// The local actions whose steps use the dev-tool cache action, read through the job model's own YAML parser (ER-42).
    fn tool_cache_actions(root: &std::path::Path) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for e in std::fs::read_dir(root.join(".gitea/actions")).unwrap() {
            let dir = e.unwrap().path();
            let Ok(text) = std::fs::read_to_string(dir.join("action.yml")) else {
                continue;
            };
            let doc = paddock_ci_config::yaml::parse(&text).unwrap();
            let steps = doc
                .get("runs")
                .and_then(|r| r.get("steps"))
                .map(|s| s.items().to_vec())
                .unwrap_or_default();
            if steps
                .iter()
                .any(|st| st.get_text("uses") == Some("./.gitea/actions/dev-tool-cache"))
            {
                out.insert(dir.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
        out
    }

    /// User decision 109: one job of ci.yml writes the ./dev runner store and the Actions cache. Train y (run 9243) had none: the plan
    /// job's first tool-cache use, inside runner-wait, was a consumer and every later use did nothing, so no job ever published.
    #[test]
    fn exactly_one_job_of_ci_yml_produces_the_dev_tool_and_its_action_hands_the_input_on() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let read = |rel: &str| std::fs::read_to_string(root.join(rel)).unwrap().replace("\r\n", "\n");
        let actions = tool_cache_actions(&root);
        assert!(
            actions.contains("runner-wait"),
            "the finder sees the action the plan job starts with: {actions:?}"
        );
        let declared = ModelDeclared {
            projects: crate::repo_inputs::ci_jobs(&read("docs/repo-graph.toml")).unwrap(),
            tool_cache_actions: actions,
        };
        let model = jobs::model(&read(".gitea/workflows/ci.yml"), &declared).unwrap();
        let producers: Vec<&str> = model.jobs.iter().filter(|j| j.tool_cache_producer).map(|j| j.id.as_str()).collect();
        assert_eq!(producers, ["plan"], "the plan job is ci.yml's one producer");
        // The action the plan job's producer input goes to hands it on to its own dev-tool-cache use.
        let wait = paddock_ci_config::yaml::parse(&read(".gitea/actions/runner-wait/action.yml")).unwrap();
        let handed = wait
            .get("runs")
            .and_then(|r| r.get("steps"))
            .map(|s| s.items().to_vec())
            .unwrap_or_default()
            .into_iter()
            .find(|st| st.get_text("uses") == Some("./.gitea/actions/dev-tool-cache"))
            .and_then(|st| st.get("with").and_then(|w| w.get_text("producer")).map(str::to_string));
        assert_eq!(handed.as_deref(), Some("${{ inputs.producer }}"));
        // Control: without the input on the plan job, ci.yml has no producer and the check sees it.
        let without = read(".gitea/workflows/ci.yml").replacen("          producer: \"true\"\n", "", 1);
        let model = jobs::model(&without, &declared).unwrap();
        assert!(model.jobs.iter().all(|j| !j.tool_cache_producer));
    }

    #[test]
    fn a_job_without_an_if_always_runs_and_the_plan_job_is_not_listed() {
        let wf = "jobs:\n  plan:\n    runs-on: x\n  always:\n    name: always runs\n    needs: [plan]\n    runs-on: x\n    steps:\n      - run: x\n";
        let declared = ModelDeclared {
            projects: BTreeMap::from([
                ("plan".to_string(), vec!["everything".to_string()]),
                ("always".to_string(), vec!["conformance".to_string()]),
            ]),
            ..ModelDeclared::default()
        };
        let model = jobs::model(wf, &declared).unwrap();
        let map = expected_of(&model, &[], &PlannedLegs::default()).unwrap();
        assert_eq!(map.keys().map(String::as_str).collect::<Vec<_>>(), ["always runs"], "{map:?}");
    }

    #[test]
    fn an_if_the_evaluator_cannot_settle_is_an_error_naming_the_job_never_a_guess() {
        let wf = "jobs:\n  plan:\n    runs-on: x\n  odd:\n    needs: [plan]\n    if: ${{ github.actor == 'me' }}\n    runs-on: x\n    steps:\n      - run: x\n";
        let declared = ModelDeclared {
            projects: BTreeMap::from([
                ("plan".to_string(), vec!["everything".to_string()]),
                ("odd".to_string(), vec!["conformance".to_string()]),
            ]),
            ..ModelDeclared::default()
        };
        let model = jobs::model(wf, &declared).unwrap();
        let err = expected_of(&model, &[], &PlannedLegs::default()).unwrap_err();
        assert!(err.contains("odd") && err.contains("cannot be evaluated"), "{err}");
    }

    #[test]
    fn an_app_leg_with_an_e2e_leg_renders_the_format_call_and_a_main_leg_does_not() {
        let apps = vec![
            json!({"app": "dyno", "os": "ubuntu-latest", "e2e_leg": "e2e-1"}),
            json!({"app": "dyno", "os": "macos-latest", "e2e_leg": ""}),
        ];
        let planned = PlannedLegs {
            apps: &apps,
            ..PlannedLegs::default()
        };
        let scope = Scope {
            apps: ["dyno@ubuntu-latest:e2e-1".to_string(), "dyno@macos-latest".to_string()].into(),
            ..Scope::default()
        };
        let map = expected_of(&model(), &test_outputs(&scope), &planned).unwrap();
        assert_eq!(map["app dyno (ubuntu-latest, e2e-1)"].leg["e2e_leg"], "e2e-1");
        assert_eq!(map["app dyno (macos-latest)"].leg["app"], "dyno");
        assert!(
            !map["app dyno (macos-latest)"].leg.contains_key("e2e_leg"),
            "an empty value is not stored"
        );
    }

    #[test]
    fn only_the_jobs_whose_if_the_plans_outputs_make_true_are_expected() {
        let map = expected_of(&model(), &test_outputs(&ran(&["conformance"])), &PlannedLegs::default()).unwrap();
        assert!(map.contains_key("conformance (checks, release scripts)"));
        assert!(
            map.keys().any(|n| n.starts_with("quick checks (")),
            "quick runs for conformance: {map:?}"
        );
        // `ui` and the test-cache shadow have no project gate of their own (their steps are gated), so a full run lists them; the jobs
        // gated by a project output the plan did not set are not listed.
        assert!(
            map.keys().any(|n| n.starts_with("ui (")) && map.keys().any(|n| n.starts_with("test cache")),
            "{map:?}"
        );
        assert!(
            !map.keys()
                .any(|n| n.starts_with("crate") || n.starts_with("reference apps") || n.starts_with("app ")),
            "{map:?}"
        );
    }

    #[test]
    fn a_job_with_a_static_matrix_is_expected_once_per_shard() {
        let map = expected_of(&model(), &test_outputs(&ran(&["e2e"])), &PlannedLegs::default()).unwrap();
        assert!(
            map.contains_key("reference apps (shard 1/2)") && map.contains_key("reference apps (shard 2/2)"),
            "{map:?}"
        );
        assert_eq!(map["reference apps (shard 2/2)"].leg["shard"], "2");
    }

    #[test]
    fn two_legs_that_render_one_name_are_refused_because_a_lookup_would_be_ambiguous() {
        let legs = vec![json!({"os": "windows-latest"}), json!({"os": "windows-latest"})];
        let planned = PlannedLegs {
            crate_legs: &legs,
            ..PlannedLegs::default()
        };
        let err = expected_of(&model(), &test_outputs(&ran(&["crates", "crate@windows-latest"])), &planned).unwrap_err();
        assert!(err.contains("crate (windows-latest)") && err.contains("twice"), "{err}");
    }

    #[test]
    fn a_leg_the_planner_did_not_emit_is_not_expected() {
        // The planner emitted the Windows leg only (the others dropped by the OS split).
        let windows = vec![json!({"os": "windows-latest"})];
        let planned = PlannedLegs {
            crate_legs: &windows,
            ..PlannedLegs::default()
        };
        let map = expected_of(&model(), &test_outputs(&ran(&["crates", "crate@windows-latest"])), &planned).unwrap();
        assert!(
            map.contains_key("crate (windows-latest)") && !map.contains_key("crate (macos-latest)"),
            "{map:?}"
        );
    }

    const APP_WF: &str = "jobs:\n  plan:\n    name: plan\n  e2e:\n    name: end-to-end (shard ${{ matrix.shard }}/4)\n    strategy:\n      matrix:\n        include: ${{ fromJSON(needs.plan.outputs.e2e_shards) }}\n  smoke:\n    name: real-app smoke (${{ matrix.os }})\n    strategy:\n      matrix:\n        os: [ubuntu-latest, windows-latest]\n";

    #[test]
    fn an_app_workflow_is_expected_over_its_static_matrix_and_the_planned_shards_it_was_given() {
        let shards = BTreeMap::from([("e2e_shards".to_string(), vec![json!({"shard": 1}), json!({"shard": 4})])]);
        let map = app_expected(APP_WF, &shards).unwrap();
        assert_eq!(map["end-to-end (shard 4/4)"].leg["shard"], "4", "{map:?}");
        assert_eq!(map["real-app smoke (windows-latest)"].leg["os"], "windows-latest");
        assert_eq!(map["plan"].job, "plan");
        assert!(map["plan"].leg.is_empty());
        // Planned legs the caller did not give leave that job out: a reader reports it as not measured, never a guess.
        let none = app_expected(APP_WF, &BTreeMap::new()).unwrap();
        assert!(!none.keys().any(|n| n.starts_with("end-to-end")), "{none:?}");
        assert!(none.contains_key("real-app smoke (ubuntu-latest)"));
        assert!(app_expected("name: x\n", &BTreeMap::new()).is_err());
    }

    #[test]
    fn each_expected_job_carries_the_projects_of_its_model_job() {
        let map = expected_of(&model(), &test_outputs(&ran(&["conformance"])), &PlannedLegs::default()).unwrap();
        let e = &map["conformance (checks, release scripts)"];
        assert!(e.projects.contains("conformance"), "{e:?}");
        let jobs = model();
        let job = jobs.jobs.iter().find(|j| j.id == "conformance").unwrap();
        assert_eq!(e.projects, job.projects);
    }

    fn map() -> ExpectedMap {
        let part = |k: u8| {
            (
                format!("crate (ubuntu-latest, {k}/3)"),
                Expected {
                    job: "crate".into(),
                    leg: BTreeMap::from([
                        ("os".to_string(), "ubuntu-latest".to_string()),
                        ("part_label".to_string(), format!(", {k}/3")),
                    ]),
                    ..Default::default()
                },
            )
        };
        ExpectedMap::from([part(1), part(2), part(3)])
    }

    #[test]
    fn a_name_is_looked_up_as_listed_then_without_its_caller_prefix_then_by_the_jobs_own_log() {
        let m = map();
        let by_name = lookup(&m, "crate (ubuntu-latest, 3/3)", None).unwrap();
        assert_eq!(
            (by_name.job.as_str(), by_name.leg.unwrap()["part_label"].as_str()),
            ("crate", ", 3/3")
        );
        // A caller's prefix is stripped here and nowhere else.
        let called = lookup(&m, "ci / crate (ubuntu-latest, 2/3)", None).unwrap();
        assert_eq!(called.leg.unwrap()["part_label"], ", 2/3");
        // A name the line does not hold: the log's `Job:` line gives the job and no leg.
        let by_log = lookup(&m, "crate (ubuntu-latest, 4/4)", Some("2026-11-26T00:00:00.0000000Z Job: crate\n")).unwrap();
        assert_eq!((by_log.job.as_str(), by_log.leg), ("crate", None));
        // Neither knows it: nothing, never a guess from the name's shape.
        assert_eq!(lookup(&m, "crate (ubuntu-latest, 4/4)", None), None);
        assert_eq!(lookup(&m, "crate (ubuntu-latest, 4/4)", Some("no header\n")), None);
        assert_eq!(strip_caller("a / b / c"), "c");
        assert_eq!(strip_caller("plain"), "plain");
    }
}
