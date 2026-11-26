//! The job model of one workflow file (ADR NEW-ci-job-model §2).

use crate::expr::{self, Lookup, Tri, Val};
use crate::yaml::{self, Node};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

/// A leg of a matrix: its values by key (`os`, `part_label`, ...).
pub type Leg = BTreeMap<String, String>;

/// The ci.yml job outputs that say whether a project runs: (output name, project id, plan key). The one table: the planner writes
/// them, and [`model`] reads a job's `if:` through it to know which project the job runs for.
pub const PROJECT_OUTPUTS: [(&str, &str, &str); 8] = [
    ("crates", "crates", "crates"),
    ("ui", "ui", "ui"),
    ("eslint", "eslint", "eslint"),
    ("conformance", "conformance", "conformance"),
    ("c99", "c99", "c99"),
    ("tools", "tools", "tools"),
    ("icons", "icons", "icons"),
    ("paddock_dev", "paddock-dev", "paddockDev"),
];

/// The project a job that runs for the apps carries (every app leg).
pub const APPS_PROJECT: &str = "apps";

/// What the model cannot take from the workflow text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Declared {
    /// Per job id: the projects it runs for by choice, which no `if:` shows (`[ci.jobs]` of docs/repo-graph.toml).
    pub projects: BTreeMap<String, Vec<String>>,
    /// The local actions whose first step restores the dev tool before any `./dev` call (the edge reads the action files).
    pub tool_cache_actions: BTreeSet<String>,
}

/// How a job's matrix is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Legs {
    /// No matrix: the job runs once.
    None,
    /// Written in the file: one leg per combination.
    Static(Vec<Leg>),
    /// Written by the planner: the plan job output that holds the legs.
    Planned(String),
}

/// One job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// The key under `jobs:`.
    pub id: String,
    /// The raw `name:` template (the id when absent).
    pub name: String,
    /// The matrix.
    pub legs: Legs,
    /// The jobs it needs.
    pub needs: Vec<String>,
    /// Whether its `if:` runs it when `rerun_jobs` names it and not when `rerun_jobs` names only another job.
    pub asks_rerun_jobs: bool,
    /// Whether a step that restores the dev tool comes before the first step that runs `./dev`.
    pub tool_cache_before_first_dev: bool,
    /// The projects it runs for: derived from the plan outputs its conditions read, plus the declared ones.
    pub projects: BTreeSet<String>,
}

/// Something the model could not tell, with its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The job, when the problem belongs to one.
    pub job: Option<String>,
    /// The 1-based line in the workflow file (0 when none).
    pub line: usize,
    /// What is wrong and what to do.
    pub message: String,
}

/// The jobs of one workflow, in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobModel {
    /// The jobs.
    pub jobs: Vec<Job>,
}

impl JobModel {
    /// The job with this id.
    pub fn job(&self, id: &str) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }
}

/// The local action that restores the dev tool; a step that uses it counts as the restore whatever [`Declared`] says.
const TOOL_CACHE_ACTION: &str = "dev-tool-cache";

/// The prefix of a plan job output in an expression.
const PLAN_OUTPUT: &str = "needs.plan.outputs.";

/// What kind of output of the plan job a name is.
enum Output {
    /// A project flag of the plan (one of [`PROJECT_OUTPUTS`]): the project id.
    Project(&'static str),
    /// An app output (`any_app`, `apps`): every app.
    Apps,
    /// An output that shapes legs, filters or the rerun and adds no project.
    Shape,
    /// Not known: a layout change the table has not seen.
    Unknown,
}

fn classify(output: &str) -> Output {
    if let Some((_, project, _)) = PROJECT_OUTPUTS.iter().find(|(name, _, _)| *name == output) {
        return Output::Project(project);
    }
    match output {
        "any_app" | "apps" => Output::Apps,
        "rerun_jobs" | "webkit" | "selection" | "partial" => Output::Shape,
        o if o.ends_with("_legs") || o.ends_with("_filter") => Output::Shape,
        _ => Output::Unknown,
    }
}

fn problem(job: Option<&str>, line: usize, message: String) -> Problem {
    Problem {
        job: job.map(str::to_string),
        line,
        message,
    }
}

/// Records every plan output an expression asks for; each is a plan-job output "that runs".
#[derive(Default)]
struct Reads(RefCell<Vec<String>>);

impl Lookup for Reads {
    fn get(&self, name: &str) -> Val {
        match name.strip_prefix(PLAN_OUTPUT) {
            Some(output) => {
                self.0.borrow_mut().push(output.to_string());
                Val::Planned(output.to_string())
            }
            None => Val::Unknown,
        }
    }
}

/// `rerun_jobs` is `list`; every other plan output reads as set.
struct RerunAs<'a>(&'a str);

impl Lookup for RerunAs<'_> {
    fn get(&self, name: &str) -> Val {
        match name.strip_prefix(PLAN_OUTPUT) {
            Some("rerun_jobs") => Val::Str(self.0.to_string()),
            Some(output) => Val::Planned(output.to_string()),
            None => Val::Unknown,
        }
    }
}

/// The plan outputs a condition reads (in any order, repeats kept), or `None` when the evaluator could not read all of it.
fn reads(condition: &str) -> Option<Vec<String>> {
    let lookup = Reads::default();
    let _ = expr::condition(condition, &lookup);
    let found = lookup.0.into_inner();
    (found.len() >= condition.matches(PLAN_OUTPUT).count()).then_some(found)
}

fn asks_rerun_jobs(id: &str, condition: Option<&str>) -> bool {
    let Some(c) = condition else { return false };
    let runs = |list: &str| expr::condition(c, &RerunAs(list)).0;
    runs(&format!("[\"{id}\"]")) == Tri::Yes && runs("[\"\u{1}another job\"]") == Tri::No
}

fn text_list(n: Option<&Node>) -> Vec<String> {
    n.map(|n| match n.text() {
        Some(t) => vec![t.to_string()],
        None => n
            .items()
            .iter()
            .filter_map(|i| i.text().map(str::to_string))
            .collect(),
    })
    .unwrap_or_default()
}

/// The words of a shell text that are `./dev` (comment lines left out).
fn runs_dev(run: &str) -> bool {
    run.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .flat_map(|l| l.split(|c: char| c.is_whitespace() || ";&|()\"'`".contains(c)))
        .any(|w| w == "./dev")
}

fn tool_cache_before_first_dev(steps: &[Node], declared: &Declared) -> bool {
    for step in steps {
        if let Some(action) = step
            .get_text("uses")
            .and_then(|u| u.strip_prefix("./.gitea/actions/"))
            && (action == TOOL_CACHE_ACTION || declared.tool_cache_actions.contains(action))
        {
            return true;
        }
        if step.get_text("run").is_some_and(runs_dev) {
            return false;
        }
    }
    true
}

/// `${{ fromJSON(needs.plan.outputs.<name>) }}`: the name.
fn planned_output(text: &str) -> Option<String> {
    let inner = text.trim().strip_prefix("${{")?.strip_suffix("}}")?.trim();
    let name = inner
        .strip_prefix("fromJSON(")?
        .strip_suffix(')')?
        .trim()
        .strip_prefix(PLAN_OUTPUT)?;
    (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then(|| name.to_string())
}

fn scalar_leg(entry: &Node) -> Option<Leg> {
    entry
        .entries()
        .iter()
        .map(|(k, v)| v.text().map(|t| (k.clone(), t.to_string())))
        .collect()
}

fn legs_of(id: &str, body: &Node) -> Result<Legs, Problem> {
    let Some(matrix) = body.get("strategy").and_then(|s| s.get("matrix")) else {
        return Ok(Legs::None);
    };
    let bad = |message: String| problem(Some(id), matrix.line, message);
    let mut axes: Vec<(&str, Vec<String>)> = Vec::new();
    let mut include: Option<Legs> = None;
    for (key, value) in matrix.entries() {
        match key.as_str() {
            "include" => {
                include = Some(match value.text() {
                    Some(t) => Legs::Planned(planned_output(t).ok_or_else(|| {
                        bad(format!(
                            "matrix include `{t}` is not fromJSON(needs.plan.outputs.<name>)"
                        ))
                    })?),
                    None => Legs::Static(
                        value
                            .items()
                            .iter()
                            .map(|e| {
                                scalar_leg(e).ok_or_else(|| {
                                    bad("a matrix include entry holds a value that is not text"
                                        .to_string())
                                })
                            })
                            .collect::<Result<_, _>>()?,
                    ),
                });
            }
            "exclude" => {
                return Err(bad(
                    "matrix exclude is not expanded by this model: write the legs with include"
                        .to_string(),
                ));
            }
            axis => {
                let values: Option<Vec<String>> = (value.text().is_none()
                    && !value.items().is_empty())
                .then(|| {
                    value
                        .items()
                        .iter()
                        .map(|i| i.text().map(str::to_string))
                        .collect::<Option<_>>()
                })
                .flatten();
                axes.push((
                    axis,
                    values.ok_or_else(|| {
                        bad(format!("matrix axis `{axis}` is not a list of text"))
                    })?,
                ));
            }
        }
    }
    match (axes.is_empty(), include) {
        (true, None) => Ok(Legs::None),
        (true, Some(legs)) => Ok(legs),
        (false, Some(_)) => Err(bad(
            "the matrix mixes axes and include; this model reads one or the other".to_string(),
        )),
        (false, None) => {
            let mut legs = vec![Leg::new()];
            for (axis, values) in axes {
                legs = legs
                    .iter()
                    .flat_map(|leg| {
                        values.iter().map(move |v| {
                            let mut l = leg.clone();
                            l.insert(axis.to_string(), v.clone());
                            l
                        })
                    })
                    .collect();
            }
            Ok(Legs::Static(legs))
        }
    }
}

/// The projects the plan outputs of a condition name; a problem for an output the table does not know.
fn project_reads(
    job: &str,
    node: &Node,
    projects: &mut BTreeSet<String>,
    problems: &mut Vec<Problem>,
) {
    let Some(condition) = node.text() else { return };
    let Some(outputs) = reads(condition) else {
        problems.push(problem(
            Some(job),
            node.line,
            format!("the condition `{condition}` reads a plan output the evaluator cannot read: rewrite it in the supported subset"),
        ));
        return;
    };
    for output in outputs {
        match classify(&output) {
            Output::Project(p) => {
                projects.insert(p.to_string());
            }
            Output::Apps => {
                projects.insert(APPS_PROJECT.to_string());
            }
            Output::Shape => {}
            Output::Unknown => problems.push(problem(
                Some(job),
                node.line,
                format!("the plan output `{output}` is neither a project output, an app output nor a known leg or filter output: add it to the model"),
            )),
        }
    }
}

/// Builds the model of a workflow text; every problem is reported together (ER-33: what it cannot tell is never a pass).
pub fn model(workflow_text: &str, declared: &Declared) -> Result<JobModel, Vec<Problem>> {
    let doc = yaml::parse(workflow_text).map_err(|e| vec![problem(None, 0, e)])?;
    let Some(jobs_node) = doc.get("jobs").filter(|j| !j.entries().is_empty()) else {
        return Err(vec![problem(
            None,
            0,
            "the workflow has no `jobs:` mapping".to_string(),
        )]);
    };
    let mut problems = Vec::new();
    let mut jobs = Vec::new();
    for (id, body) in jobs_node.entries() {
        let steps = body.get("steps").map(Node::items).unwrap_or_default();
        let mut projects: BTreeSet<String> = BTreeSet::new();
        if let Some(c) = body.get("if") {
            project_reads(id, c, &mut projects, &mut problems);
        }
        for step in steps {
            if let Some(c) = step.get("if") {
                project_reads(id, c, &mut projects, &mut problems);
            }
        }
        projects.extend(declared.projects.get(id).into_iter().flatten().cloned());
        if projects.is_empty() {
            problems.push(problem(
                Some(id),
                body.line,
                "the job runs for no project: its conditions read no project output and [ci.jobs] declares none (a job is never `always`)".to_string(),
            ));
        }
        let legs = legs_of(id, body).unwrap_or_else(|p| {
            problems.push(p);
            Legs::None
        });
        jobs.push(Job {
            id: id.clone(),
            name: body.get_text("name").unwrap_or(id).to_string(),
            legs,
            needs: text_list(body.get("needs")),
            asks_rerun_jobs: asks_rerun_jobs(id, body.get_text("if")),
            tool_cache_before_first_dev: tool_cache_before_first_dev(steps, declared),
            projects,
        });
    }
    for job in declared
        .projects
        .keys()
        .filter(|d| !jobs.iter().any(|j| &j.id == *d))
    {
        problems.push(problem(
            None,
            0,
            format!("[ci.jobs] declares the job `{job}`, which the workflow does not have"),
        ));
    }
    if problems.is_empty() {
        Ok(JobModel { jobs })
    } else {
        Err(problems)
    }
}

/// The display name of a job for one leg: the `name:` template evaluated with the leg's values. A value the leg lacks is empty, as
/// on the runner.
pub fn render(job: &Job, leg: &Leg) -> String {
    render_name(&job.name, leg)
}

/// A job's id, `name:` template and matrix, without the project reading of [`Job`]: what a workflow that is not ci.yml (an app's own)
/// offers to a reader that only needs its display names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedJob {
    /// The key under `jobs:`.
    pub id: String,
    /// The raw `name:` template (the id when absent).
    pub name: String,
    /// The matrix.
    pub legs: Legs,
}

/// The jobs of a workflow with their name templates and matrices, in file order; every problem is reported together.
pub fn named_jobs(workflow_text: &str) -> Result<Vec<NamedJob>, Vec<Problem>> {
    let doc = yaml::parse(workflow_text).map_err(|e| vec![problem(None, 0, e)])?;
    let Some(jobs_node) = doc.get("jobs").filter(|j| !j.entries().is_empty()) else {
        return Err(vec![problem(
            None,
            0,
            "the workflow has no `jobs:` mapping".to_string(),
        )]);
    };
    let mut problems = Vec::new();
    let mut out = Vec::new();
    for (id, body) in jobs_node.entries() {
        let legs = legs_of(id, body).unwrap_or_else(|p| {
            problems.push(p);
            Legs::None
        });
        out.push(NamedJob {
            id: id.clone(),
            name: body.get_text("name").unwrap_or(id).to_string(),
            legs,
        });
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(problems)
    }
}

/// [`render`] of a raw `name:` template.
pub fn render_name(template: &str, leg: &Leg) -> String {
    let lookup = |name: &str| match name.strip_prefix("matrix.") {
        Some(key) => leg.get(key).map_or(Val::Null, |v| Val::Str(v.clone())),
        None => Val::Unknown,
    };
    expr::interpolate(template, &lookup).text
}
