//! The display names a run is expected to have (ADR 0123 §3): the planner renders every job of the job model over the legs it emits
//! and writes `{display name: {job, leg}}` into the run's scope line (version 3), so every reader looks a name up instead of
//! inverting a `name:` template.

use super::Scope;
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

/// Whether the run ran the job: a key of `ran.jobs` or `ran.apps` names it, by its own id or by a project it runs for.
fn ran_job(job: &Job, ran: &Scope) -> bool {
    let projects_ran = PROJECT_OUTPUTS
        .iter()
        .filter(|(_, project, plan_key)| job.projects.contains(*project) && (ran.jobs.contains(*plan_key) || ran.jobs.contains(*project)))
        .count()
        > 0;
    let named = ran.jobs.iter().any(|k| k.split('@').next() == Some(job.id.as_str()));
    let app = job.projects.contains(jobs::APPS_PROJECT) && !ran.apps.is_empty();
    projects_ran || named || app
}

/// The legs of a job the run ran: the model's own, or the planner's for a planned matrix.
fn legs_ran(job: &Job, ran: &Scope, planned: &PlannedLegs) -> Result<Vec<Leg>, String> {
    let from_values = |legs: &[Value], keep: &dyn Fn(&Leg) -> bool| legs.iter().map(text_leg).filter(|l| keep(l)).collect();
    Ok(match &job.legs {
        Legs::None => vec![Leg::new()],
        Legs::Static(legs) => legs.clone(),
        Legs::Planned(output) => match (job.id.as_str(), output.as_str()) {
            ("crate", "crate_legs") | ("paddock-dev", "paddock_dev_legs") => {
                let legs = if job.id == "crate" {
                    planned.crate_legs
                } else {
                    planned.paddock_dev_legs
                };
                from_values(legs, &|l| ran.jobs.contains(&format!("{}@{}", job.id, l["os"])))
            }
            ("app", "apps") => from_values(planned.apps, &|_| true),
            _ => return Err(format!("job {} has planned legs `{output}` that the planner does not name", job.id)),
        },
    })
}

/// The expected jobs of a run: every job of the model the run ran, rendered over its legs. A name two legs share is refused: a lookup
/// by name would be ambiguous (the check of ADR 0123 §7 is this, made at the writer).
pub fn expected_of(model: &JobModel, ran: &Scope, planned: &PlannedLegs) -> Result<ExpectedMap, String> {
    let mut out = ExpectedMap::new();
    for job in model.jobs.iter().filter(|j| ran_job(j, ran)) {
        for leg in legs_ran(job, ran, planned)? {
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
        let map = expected_of(&model(), &all_crate_keys(), &planned).unwrap();
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
        let map = expected_of(&model(), &scope, &planned).unwrap();
        assert_eq!(map["app dyno (ubuntu-latest, e2e-1)"].leg["e2e_leg"], "e2e-1");
        assert_eq!(map["app dyno (macos-latest)"].leg["app"], "dyno");
        assert!(
            !map["app dyno (macos-latest)"].leg.contains_key("e2e_leg"),
            "an empty value is not stored"
        );
    }

    #[test]
    fn only_the_jobs_of_the_projects_the_run_ran_are_expected() {
        let map = expected_of(&model(), &ran(&["conformance"]), &PlannedLegs::default()).unwrap();
        assert!(map.contains_key("conformance (checks, release scripts)"));
        assert!(
            map.keys().any(|n| n.starts_with("quick checks (")),
            "quick runs for conformance: {map:?}"
        );
        assert!(!map.keys().any(|n| n.starts_with("crate") || n.starts_with("ui (")), "{map:?}");
    }

    #[test]
    fn a_job_with_a_static_matrix_is_expected_once_per_shard() {
        let map = expected_of(&model(), &ran(&["e2e"]), &PlannedLegs::default()).unwrap();
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
        let err = expected_of(&model(), &ran(&["crates", "crate@windows-latest"]), &planned).unwrap_err();
        assert!(err.contains("crate (windows-latest)") && err.contains("twice"), "{err}");
    }

    #[test]
    fn a_leg_whose_runner_the_run_did_not_run_is_not_expected() {
        let planned = PlannedLegs {
            crate_legs: &crate_legs(),
            ..PlannedLegs::default()
        };
        let map = expected_of(&model(), &ran(&["crates", "crate@windows-latest"]), &planned).unwrap();
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
        let map = expected_of(&model(), &ran(&["conformance"]), &PlannedLegs::default()).unwrap();
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
