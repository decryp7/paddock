//! The CI model of ADR 0060 §2: workflows, jobs, steps and local actions read into one reachability graph, walked from the
//! roots (workflows with a trigger other than `workflow_call`) with bound inputs, environment, working folders, matrices and
//! conditions, yielding every command CI can run ([`RunLine`]) with the levels it runs at.
//!
//! The reader is `yaml-rust2` through [`super::yaml`] (it gives every node its line; MIT/Apache-2.0). The model is an
//! interpretation of the runner's semantics, not the runner: what it cannot tell it marks [`Status::Unresolved`] or
//! [`Status::Conditional`], and no check counts as gated through such a line (ER-33).

use super::expr::{self, Lookup, Tri, Val};
use super::shell;
use super::yaml::{self, Node};
use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};

/// How often a line runs, from the weakest guarantee to the strongest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// Pull requests and branch pushes.
    Branch,
    /// A push to main.
    Main,
    /// The nightly schedule.
    Nightly,
    /// A tag push.
    Release,
    /// A manual run only (`workflow_dispatch`); counts as `Branch` only for the workflows of [`BRANCH_DISPATCH`].
    Manual,
}

impl Level {
    /// The word used in the register and in reports.
    pub fn name(self) -> &'static str {
        match self {
            Level::Branch => "branch",
            Level::Main => "main",
            Level::Nightly => "nightly",
            Level::Release => "release",
            Level::Manual => "manual",
        }
    }
}

/// What a line is worth, worst last (§2.2, §2.4, §2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    /// Resolved, unconditional (or planned), and its failure fails the job.
    Gated,
    /// Its failure does not fail the job (`continue-on-error`, `|| true`, `set +e`).
    ReportOnly,
    /// Runs only under a condition the model cannot decide.
    Conditional,
    /// A value, folder or output the model cannot resolve.
    Unresolved,
}

/// One command CI can run.
#[derive(Debug, Clone, PartialEq)]
pub struct RunLine {
    /// The workflow or action file the command is written in.
    pub file: String,
    /// Its line there.
    pub line: usize,
    /// The root workflow that reaches it.
    pub root: String,
    /// The job id in the root or called workflow.
    pub job: String,
    /// The app of the matrix leg, if any.
    pub app: Option<String>,
    /// The `matrix.os` of the leg, if any.
    pub os: Option<String>,
    /// The folder it runs in (`""` is the repository root); `None` when unknown.
    pub cwd: Option<String>,
    /// The command with its inputs and variables expanded.
    pub text: String,
    /// The environment of the step (job and step `env:`, expanded).
    pub env: BTreeMap<String, String>,
    /// The planner outputs it runs under (`needs.plan.outputs.<name> == 'true'`).
    pub planned: BTreeSet<String>,
    /// The status at each level it can run at.
    pub levels: BTreeMap<Level, Status>,
    /// Where a command list came from, when it did not come from the workflow text (`apps/dyno/paddock.ci.json`).
    pub origin: Option<String>,
}

/// A workflow or action the roots reach, with the status at each level.
pub type Reach = BTreeMap<String, BTreeMap<Level, Status>>;

/// The model.
#[derive(Debug, Default)]
pub struct CiModel {
    /// Every command, merged across roots, matrix legs and levels.
    pub lines: Vec<RunLine>,
    /// Workflows reachable from a root (callees included), by path.
    pub workflows: Reach,
    /// Local actions reachable from a root, by folder (`.gitea/actions/licences`).
    pub actions: Reach,
    /// Workflows reached through a `uses:` of a reachable job (a root that nobody calls is not in here).
    pub called: Reach,
    /// The root workflows.
    pub roots: Vec<String>,
    /// Files the model could not read, `(path, why)`: each is a finding, never a silent skip.
    pub problems: Vec<(String, String)>,
}

/// The files the model reads.
#[derive(Debug, Default, Clone)]
pub struct Sources {
    /// `.gitea/workflows/<name>.yml` by repository path.
    pub workflows: BTreeMap<String, String>,
    /// `.gitea/actions/<name>/action.yml` by the action's folder.
    pub actions: BTreeMap<String, String>,
    /// `apps/<app>/paddock.ci.json` by app name.
    pub contracts: BTreeMap<String, String>,
    /// The register's `[vars]` (repository variables as CI sets them).
    pub vars: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------------------------------------------------
// Parsing

#[derive(Debug, Clone)]
struct Step {
    line: usize,
    cond: Option<String>,
    uses: Option<String>,
    with: Vec<(String, String)>,
    env: Vec<(String, String)>,
    run: Option<(String, usize)>,
    wd: Option<String>,
    shell: Option<String>,
    coe: Option<String>,
}

#[derive(Debug, Clone)]
struct Job {
    id: String,
    cond: Option<String>,
    runs_on: Option<String>,
    uses: Option<String>,
    with: Vec<(String, String)>,
    env: Vec<(String, String)>,
    wd: Option<String>,
    shell: Option<String>,
    matrix: Option<Matrix>,
    steps: Vec<Step>,
    coe: Option<String>,
    outputs: Vec<(String, String)>,
    needs: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct Matrix {
    axes: Vec<(String, Vec<String>)>,
    include: Vec<BTreeMap<String, String>>,
    /// `include: ${{ fromJSON(needs.plan.outputs.apps) }}`: the app matrix of ci.yml (§2.2).
    app_legs: bool,
}

#[derive(Debug, Clone)]
struct Workflow {
    path: String,
    /// Event names, with the `tags` and `branches` filters of a push.
    events: BTreeSet<String>,
    push_tags: Vec<String>,
    push_branches: Option<Vec<String>>,
    dispatch_inputs: BTreeMap<String, String>,
    call_inputs: BTreeMap<String, String>,
    env: Vec<(String, String)>,
    wd: Option<String>,
    shell: Option<String>,
    jobs: Vec<Job>,
}

#[derive(Debug, Clone)]
struct Action {
    inputs: BTreeMap<String, String>,
    steps: Vec<Step>,
}

fn scalar_pairs(node: Option<&Node>) -> Vec<(String, String)> {
    node.map(|n| {
        n.entries()
            .iter()
            .map(|(k, v)| (k.clone(), v.text().unwrap_or("").to_string()))
            .collect()
    })
    .unwrap_or_default()
}

fn parse_steps(node: Option<&Node>) -> Vec<Step> {
    node.map(|n| {
        n.items()
            .iter()
            .map(|s| Step {
                line: s.line,
                cond: s.get_text("if").map(str::to_string),
                uses: s.get_text("uses").map(str::to_string),
                with: scalar_pairs(s.get("with")),
                env: scalar_pairs(s.get("env")),
                run: s.get("run").and_then(|r| r.text().map(|t| (t.to_string(), r.line))),
                wd: s.get_text("working-directory").map(str::to_string),
                shell: s.get_text("shell").map(str::to_string),
                coe: s.get_text("continue-on-error").map(str::to_string),
            })
            .collect()
    })
    .unwrap_or_default()
}

fn list_of_text(n: Option<&Node>) -> Vec<String> {
    n.map(|n| match n.text() {
        Some(t) => vec![t.to_string()],
        None => n.items().iter().filter_map(|i| i.text().map(str::to_string)).collect(),
    })
    .unwrap_or_default()
}

fn parse_matrix(n: &Node) -> Matrix {
    let mut m = Matrix::default();
    for (k, v) in n.entries() {
        match k.as_str() {
            "include" => match v.text() {
                Some(t) if t.contains("fromJSON(needs.plan.outputs.apps)") => m.app_legs = true,
                // The crate and paddock-dev legs of ci.yml: every OS unless a rerun of the earlier run's failures narrows them (the
                // plan step writes PLAN_OS_LEGS otherwise), so the model runs them on all three.
                Some(t)
                    if t.contains("fromJSON(needs.plan.outputs.crate_legs)")
                        || t.contains("fromJSON(needs.plan.outputs.paddock_dev_legs)") =>
                {
                    m.axes.push((
                        "os".to_string(),
                        ["ubuntu-latest", "windows-latest", "macos-latest"].map(String::from).to_vec(),
                    ));
                }
                Some(_) => m.include.push(BTreeMap::from([("__unresolved".to_string(), "true".to_string())])),
                None => m.include.extend(v.items().iter().map(|i| {
                    i.entries()
                        .iter()
                        .map(|(k, v)| (k.clone(), v.text().unwrap_or("").to_string()))
                        .collect()
                })),
            },
            "exclude" => {}
            axis => match v.text() {
                Some(_) => m.include.push(BTreeMap::from([("__unresolved".to_string(), "true".to_string())])),
                None => m.axes.push((axis.to_string(), list_of_text(Some(v)))),
            },
        }
    }
    m
}

/// The events a workflow file listens on (`on:`), read by the same parser as the model.
pub fn workflow_events(path: &str, text: &str) -> Result<BTreeSet<String>, String> {
    parse_workflow(path, text).map(|w| w.events)
}

fn parse_workflow(path: &str, text: &str) -> Result<Workflow, String> {
    let doc = yaml::parse(text)?;
    let on = doc.get("on").or_else(|| doc.get("true"));
    let mut events = BTreeSet::new();
    let (mut push_tags, mut push_branches) = (Vec::new(), None);
    let (mut dispatch_inputs, mut call_inputs) = (BTreeMap::new(), BTreeMap::new());
    if let Some(on) = on {
        match on.text() {
            Some(e) => {
                events.insert(e.to_string());
            }
            None if !on.items().is_empty() => events.extend(on.items().iter().filter_map(|i| i.text().map(str::to_string))),
            None => {
                for (e, v) in on.entries() {
                    events.insert(e.clone());
                    let inputs = |v: &Node| -> BTreeMap<String, String> {
                        v.get("inputs")
                            .map(|i| {
                                i.entries()
                                    .iter()
                                    .map(|(k, d)| (k.clone(), d.get_text("default").unwrap_or("").to_string()))
                                    .collect()
                            })
                            .unwrap_or_default()
                    };
                    match e.as_str() {
                        "push" => {
                            push_tags = list_of_text(v.get("tags"));
                            push_branches = v.get("branches").map(|b| list_of_text(Some(b)));
                        }
                        "workflow_dispatch" => dispatch_inputs = inputs(v),
                        "workflow_call" => call_inputs = inputs(v),
                        _ => {}
                    }
                }
            }
        }
    }
    let jobs = doc
        .get("jobs")
        .map(|j| {
            j.entries()
                .iter()
                .map(|(id, job)| Job {
                    id: id.clone(),
                    cond: job.get_text("if").map(str::to_string),
                    runs_on: job.get_text("runs-on").map(str::to_string),
                    uses: job.get_text("uses").map(str::to_string),
                    with: scalar_pairs(job.get("with")),
                    env: scalar_pairs(job.get("env")),
                    wd: job
                        .get("defaults")
                        .and_then(|d| d.get("run"))
                        .and_then(|r| r.get_text("working-directory"))
                        .map(str::to_string),
                    shell: job
                        .get("defaults")
                        .and_then(|d| d.get("run"))
                        .and_then(|r| r.get_text("shell"))
                        .map(str::to_string),
                    matrix: job.get("strategy").and_then(|s| s.get("matrix")).map(parse_matrix),
                    steps: parse_steps(job.get("steps")),
                    coe: job.get_text("continue-on-error").map(str::to_string),
                    outputs: scalar_pairs(job.get("outputs")),
                    needs: list_of_text(job.get("needs")),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Workflow {
        path: path.to_string(),
        events,
        push_tags,
        push_branches,
        dispatch_inputs,
        call_inputs,
        env: scalar_pairs(doc.get("env")),
        wd: doc
            .get("defaults")
            .and_then(|d| d.get("run"))
            .and_then(|r| r.get_text("working-directory"))
            .map(str::to_string),
        shell: doc
            .get("defaults")
            .and_then(|d| d.get("run"))
            .and_then(|r| r.get_text("shell"))
            .map(str::to_string),
        jobs,
    })
}

fn parse_action(text: &str) -> Result<Action, String> {
    let doc = yaml::parse(text)?;
    Ok(Action {
        inputs: doc
            .get("inputs")
            .map(|i| {
                i.entries()
                    .iter()
                    .map(|(k, d)| (k.clone(), d.get_text("default").unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        steps: parse_steps(doc.get("runs").and_then(|r| r.get("steps"))),
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// The app matrix (§2.2): one leg per app contract and per entry of its `os` list.

/// The keys of one leg of ci.yml's app matrix, as `scripts/affected.mjs` `matrix()` builds them. A test compares this list
/// with that function's text, so a change to the matrix-building step fails here until the binding is updated.
pub const LEG_KEYS: &[&str] = &[
    "app",
    "os",
    "dir",
    "cargo",
    "nextest_config",
    "prepare",
    "checks",
    "rust_checks",
    "host_rust_checks",
    "ui_checks",
    "angular_dir",
    "last_packages",
    "windows_clippy",
    "notices",
    "process_patterns",
    "test_browser_prefix",
    "test_browser_catalogue",
    "test_browser_no_sandbox",
    "display",
    "rust",
    "ui",
    "filter",
    "e2e_checks",
    "e2e_shard",
    "e2e_leg",
    "e2e_artifacts",
];

const LINUX: &str = "ubuntu-latest";

fn join_lines(v: &Json, key: &str) -> String {
    v.get(key)
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(Json::as_str).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

/// The legs of one app: every half the change can reach (the model assumes the planner marks them, §2.4), the end-to-end
/// legs per shard, and the WebKit leg when `webkit`.
fn app_legs(app: &str, contract: &Json, webkit: bool) -> Result<Vec<BTreeMap<String, String>>, String> {
    let has = |k: &str| {
        contract
            .get(k)
            .is_some_and(|v| v.as_array().map_or(!v.is_null(), |a| !a.is_empty()))
    };
    if contract.get("workflow").is_some_and(|w| !w.is_null()) {
        return Ok(Vec::new());
    }
    if !(contract.get("cargo").and_then(Json::as_str).is_some_and(|s| !s.is_empty()) || has("prepare") || has("checks") || has("ui")) {
        return Ok(Vec::new());
    }
    let oses: Vec<String> = match contract.get("os").and_then(Json::as_array) {
        Some(a) if !a.is_empty() => a.iter().filter_map(Json::as_str).map(str::to_string).collect(),
        _ => vec![LINUX.to_string()],
    };
    if !oses.iter().any(|o| o == LINUX) {
        return Err(format!("apps/{app}/paddock.ci.json: \"os\" must include {LINUX}"));
    }
    let dir = format!("apps/{app}");
    let cargo = contract
        .get("cargo")
        .and_then(Json::as_str)
        .filter(|c| !c.is_empty())
        .map(|c| shell::normalise(&format!("{dir}/{c}")).unwrap_or_default())
        .unwrap_or_default();
    let s = |k: &str| contract.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let tb = |k: &str| {
        contract
            .get("testBrowser")
            .and_then(|t| t.get(k))
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string()
    };
    let flag = |b: bool| if b { "true" } else { "false" }.to_string();
    let mut legs: Vec<BTreeMap<String, String>> = Vec::new();
    for os in oses {
        let linux = os == LINUX;
        let m: BTreeMap<String, String> = [
            ("app", app.to_string()),
            ("os", os.clone()),
            ("dir", dir.clone()),
            ("cargo", cargo.clone()),
            ("nextest_config", s("nextestConfig")),
            ("prepare", if linux { join_lines(contract, "prepare") } else { String::new() }),
            ("checks", if linux { join_lines(contract, "checks") } else { String::new() }),
            (
                "rust_checks",
                if linux { join_lines(contract, "rustChecks") } else { String::new() },
            ),
            (
                "host_rust_checks",
                if linux {
                    String::new()
                } else {
                    join_lines(contract, "hostRustChecks")
                },
            ),
            ("ui_checks", if linux { join_lines(contract, "ui") } else { String::new() }),
            (
                "angular_dir",
                if linux && !s("angularDir").is_empty() {
                    format!("{dir}/{}", s("angularDir"))
                } else {
                    String::new()
                },
            ),
            (
                "last_packages",
                contract
                    .get("lastPackages")
                    .and_then(Json::as_array)
                    .map(|a| a.iter().filter_map(Json::as_str).collect::<Vec<_>>().join(" "))
                    .unwrap_or_default(),
            ),
            (
                "windows_clippy",
                flag(contract.get("windowsClippy").and_then(Json::as_bool).unwrap_or(false)),
            ),
            (
                "notices",
                flag(linux && contract.get("notices").is_some_and(|n| !n.is_null() && n != &Json::Bool(false))),
            ),
            ("process_patterns", join_lines(contract, "processPatterns")),
            ("test_browser_prefix", tb("envPrefix")),
            ("test_browser_catalogue", tb("catalogue")),
            (
                "test_browser_no_sandbox",
                flag(
                    linux
                        && contract
                            .get("testBrowser")
                            .and_then(|t| t.get("noSandboxOnLinux"))
                            .and_then(Json::as_bool)
                            .unwrap_or(false),
                ),
            ),
            (
                "display",
                flag(linux && contract.get("display").and_then(Json::as_bool).unwrap_or(false)),
            ),
            ("rust", "true".to_string()),
            ("ui", flag(linux)),
            ("filter", String::new()),
            ("e2e_checks", String::new()),
            ("e2e_shard", String::new()),
            ("e2e_leg", String::new()),
            ("e2e_artifacts", if linux { s("e2eArtifacts") } else { String::new() }),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        legs.push(m);
    }
    let base = legs.iter().find(|l| l["os"] == LINUX).cloned().unwrap_or_default();
    let blank = |mut b: BTreeMap<String, String>, checks: String, shard: String, leg: String| {
        for k in ["prepare", "checks", "ui_checks"] {
            b.insert(k.to_string(), String::new());
        }
        for (k, v) in [("rust", "false"), ("ui", "false"), ("display", "false"), ("notices", "false")] {
            b.insert(k.to_string(), v.to_string());
        }
        b.insert("e2e_checks".into(), checks);
        b.insert("e2e_shard".into(), shard);
        b.insert("e2e_leg".into(), leg);
        b
    };
    if has("e2e") {
        let shards = contract.get("e2eShards").and_then(Json::as_u64).unwrap_or(1).max(1);
        for i in 1..=shards {
            legs.push(blank(
                base.clone(),
                join_lines(contract, "e2e"),
                format!("{i}/{shards}"),
                format!("e2e-{i}"),
            ));
        }
    }
    if webkit && has("e2eWebkit") {
        legs.push(blank(base, join_lines(contract, "e2eWebkit"), String::new(), "e2e-webkit".into()));
    }
    Ok(legs)
}

// ---------------------------------------------------------------------------------------------------------------------
// Walking

#[derive(Clone)]
struct Context {
    event: &'static str,
    git_ref: String,
    level: Level,
    inputs: BTreeMap<String, String>,
}

#[derive(Clone, Default)]
struct Acc {
    unknown: bool,
    unresolved: bool,
    report_only: bool,
    planned: BTreeSet<String>,
}

struct Scope<'a> {
    ctx: &'a Context,
    inputs: &'a BTreeMap<String, Val>,
    env: &'a BTreeMap<String, String>,
    matrix: Option<&'a BTreeMap<String, String>>,
    runner_os: Val,
    action_path: Option<&'a str>,
    job: &'a str,
    vars: &'a BTreeMap<String, String>,
    planners: &'a Planners,
}

/// The planner jobs of a workflow (`plan`, or a job running the `plan` action) and their `outputs:` (§2.4).
type Planners = BTreeMap<String, BTreeMap<String, String>>;

impl Lookup for Scope<'_> {
    fn get(&self, name: &str) -> Val {
        let (head, rest) = name.split_once('.').unwrap_or((name, ""));
        match head {
            "github" => match rest {
                "event_name" => Val::Str(self.ctx.event.to_string()),
                "ref" => Val::Str(self.ctx.git_ref.clone()),
                "workspace" => Val::Str("<ws>".into()),
                "job" => Val::Str(self.job.to_string()),
                "ref_name" => Val::Str(
                    self.ctx
                        .git_ref
                        .rsplit_once("refs/heads/")
                        .or_else(|| self.ctx.git_ref.rsplit_once("refs/tags/"))
                        .map_or(self.ctx.git_ref.clone(), |(_, n)| n.to_string()),
                ),
                "sha" | "run_id" | "run_number" | "run_attempt" | "server_url" | "repository" | "actor" | "token" | "workflow"
                | "event_path" | "api_url" => Val::Opaque(name.to_string()),
                "action_path" => self.action_path.map_or(Val::Unknown, |p| Val::Str(format!("<ws>/{p}"))),
                _ => match rest.strip_prefix("event.inputs.") {
                    Some(i) => self.ctx.inputs.get(i).map_or(Val::Null, |v| Val::Str(v.clone())),
                    None => Val::Unknown,
                },
            },
            "inputs" => self.inputs.get(rest).cloned().unwrap_or(Val::Null),
            "env" => self.env.get(rest).map_or(Val::Null, |v| Val::Str(v.clone())),
            "matrix" => match self.matrix {
                Some(m) => m.get(rest).map_or(Val::Null, |v| Val::Str(v.clone())),
                None => Val::Unknown,
            },
            "needs" => {
                let mut parts = rest.split('.');
                match (parts.next(), parts.next(), parts.next()) {
                    // A needed job is taken to have succeeded: the path on which the dependent job runs.
                    (Some(_), Some("result"), None) => Val::Str("success".into()),
                    (Some(job), Some("outputs"), Some(out)) if !self.planners.contains_key(job) => {
                        let _ = out;
                        Val::Opaque(name.to_string())
                    }
                    // The jobs a manual rerun of an earlier run's failures names (`since_run`): empty on every other run, which is
                    // what the model decides ("a branch run"); each job's `if` then reads its own name from the default list.
                    (Some(_), Some("outputs"), Some("rerun_jobs")) => Val::Null,
                    (Some(job), Some("outputs"), Some(out)) => match self.planners.get(job).and_then(|o| o.get(out)) {
                        // An output computed from the event alone is evaluated; one read from a step is the planner's mark.
                        Some(e) if !e.contains("steps.") => expr::eval(e.trim().trim_start_matches("${{").trim_end_matches("}}"), self).0,
                        Some(_) => Val::Planned(out.to_string()),
                        None => Val::Unknown,
                    },
                    _ => Val::Unknown,
                }
            }
            "steps" if rest.ends_with(".outcome") || rest.ends_with(".conclusion") => Val::Str("success".into()),
            "steps" => Val::Opaque(name.to_string()),
            "secrets" => Val::Opaque(name.to_string()),
            "runner" if rest == "os" => self.runner_os.clone(),
            "runner" => Val::Opaque(name.to_string()),
            "vars" => self.vars.get(rest).map_or(Val::Unknown, |v| Val::Str(v.clone())),
            _ => Val::Unknown,
        }
    }
}

fn runner_os(runs_on: &str) -> Val {
    let r = runs_on.to_ascii_lowercase();
    if r.contains("ubuntu") || r.contains("linux") {
        Val::Str("Linux".into())
    } else if r.contains("windows") {
        Val::Str("Windows".into())
    } else if r.contains("macos") {
        Val::Str("macOS".into())
    } else {
        Val::Unknown
    }
}

fn working_dir(text: &str) -> Option<String> {
    let t = shell::strip_workspace(text);
    let t = t.trim();
    if t.is_empty() || t == "." {
        return Some(String::new());
    }
    if t.contains("${{") || t.contains('\u{2039}') || t.starts_with('/') {
        return None;
    }
    shell::normalise(t)
}

struct Walker<'a> {
    wf: &'a BTreeMap<String, Workflow>,
    actions: &'a BTreeMap<String, Action>,
    contracts: &'a BTreeMap<String, Json>,
    vars: &'a BTreeMap<String, String>,
    root: String,
    lines: BTreeMap<LineKey, RunLine>,
    workflows: Reach,
    action_reach: Reach,
    called: Reach,
    problems: Vec<(String, String)>,
    /// `defaults.run.shell` of the job or workflow being walked.
    default_shell: Option<String>,
}

type LineKey = (String, usize, String, Option<String>, Option<String>, String, Option<String>);

fn worse(a: Status, b: Status) -> Status {
    a.max(b)
}

impl Walker<'_> {
    fn status(acc: &Acc, extra_unresolved: bool, line_report: bool, line_conditional: bool) -> Status {
        if acc.unresolved || extra_unresolved {
            Status::Unresolved
        } else if acc.unknown || line_conditional {
            Status::Conditional
        } else if acc.report_only || line_report {
            Status::ReportOnly
        } else {
            Status::Gated
        }
    }

    fn mark(reach: &mut Reach, path: &str, level: Level, status: Status) {
        let e = reach.entry(path.to_string()).or_default().entry(level).or_insert(status);
        *e = worse(*e, status);
    }

    fn cond(acc: &mut Acc, cond: Option<&str>, scope: &Scope) -> bool {
        let Some(c) = cond else { return true };
        let (t, planned) = expr::condition(c, scope);
        match t {
            Tri::No => false,
            Tri::Yes => {
                acc.planned.extend(planned);
                true
            }
            Tri::Unknown => {
                acc.unknown = true;
                true
            }
        }
    }

    fn coe(acc: &mut Acc, v: Option<&str>, scope: &Scope) {
        if let Some(v) = v {
            let (val, _) = expr::eval(v.trim().trim_start_matches("${{").trim_end_matches("}}"), scope);
            match val {
                Val::Bool(true) => acc.report_only = true,
                Val::Str(ref s) if s == "true" => acc.report_only = true,
                Val::Bool(false) | Val::Null => {}
                Val::Str(ref s) if s == "false" => {}
                _ => acc.report_only = true,
            }
        }
    }

    fn walk_workflow(&mut self, path: &str, ctx: &Context, inputs: &BTreeMap<String, Val>, acc: &Acc, depth: usize) {
        let Some(wf) = self.wf.get(path) else {
            self.problems
                .push((path.to_string(), "a reusable workflow that is not in the checkout".into()));
            return;
        };
        if depth > 8 {
            self.problems
                .push((path.to_string(), "reusable workflows nested deeper than 8".into()));
            return;
        }
        let wf = wf.clone();
        let planners: Planners = wf
            .jobs
            .iter()
            .filter(|j| j.id == "plan" || j.steps.iter().any(|s| s.uses.as_deref() == Some("./.gitea/actions/plan")))
            .map(|j| (j.id.clone(), j.outputs.iter().cloned().collect()))
            .collect();
        let env0: BTreeMap<String, String> = BTreeMap::new();
        let base_scope = Scope {
            ctx,
            inputs,
            env: &env0,
            matrix: None,
            runner_os: Val::Unknown,
            action_path: None,
            job: "",
            vars: self.vars,
            planners: &planners,
        };
        let wf_env: BTreeMap<String, String> = wf
            .env
            .iter()
            .map(|(k, v)| (k.clone(), expr::interpolate(v, &base_scope).text))
            .collect();
        Self::mark(&mut self.workflows, path, ctx.level, Self::status(acc, false, false, false));
        // Jobs that do not run at this level; a job that needs one of them is skipped too, unless its `if:` says `always()`.
        let mut skipped: BTreeSet<String> = BTreeSet::new();
        for job in &wf.jobs {
            self.walk_job(&wf, job, ctx, inputs, &wf_env, acc, depth, &planners, &mut skipped);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_job(
        &mut self,
        wf: &Workflow,
        job: &Job,
        ctx: &Context,
        inputs: &BTreeMap<String, Val>,
        wf_env: &BTreeMap<String, String>,
        outer: &Acc,
        depth: usize,
        planners: &Planners,
        skipped: &mut BTreeSet<String>,
    ) {
        let always = job.cond.as_deref().is_some_and(|c| c.contains("always()"));
        if !always && job.needs.iter().any(|n| skipped.contains(n)) {
            skipped.insert(job.id.clone());
            return;
        }
        // The matrix: every combination, or one empty one.
        let combos = self.combos(job, ctx);
        for combo in combos {
            let mut acc = outer.clone();
            let matrix_ref = combo.as_ref();
            let runs_on_txt = job.runs_on.clone().unwrap_or_default();
            let probe_env = wf_env.clone();
            let pre = Scope {
                ctx,
                inputs,
                env: &probe_env,
                matrix: matrix_ref,
                runner_os: Val::Unknown,
                action_path: None,
                job: &job.id,
                vars: self.vars,
                planners,
            };
            let ros = expr::interpolate(&runs_on_txt, &pre);
            let runner = runner_os(&ros.text);
            let job_env = {
                let mut m = wf_env.clone();
                let sc = Scope {
                    runner_os: runner.clone(),
                    ..base_scope_clone(&pre)
                };
                for (k, v) in &job.env {
                    let i = expr::interpolate(v, &sc);
                    m.insert(k.clone(), i.text);
                }
                m
            };
            let scope = Scope {
                ctx,
                inputs,
                env: &job_env,
                matrix: matrix_ref,
                runner_os: runner.clone(),
                action_path: None,
                job: &job.id,
                vars: self.vars,
                planners,
            };
            if combo.as_ref().is_some_and(|c| c.contains_key("__unresolved")) {
                acc.unresolved = true;
            }
            if !Self::cond(&mut acc, job.cond.as_deref(), &scope) {
                skipped.insert(job.id.clone());
                continue;
            }
            Self::coe(&mut acc, job.coe.as_deref(), &scope);
            if let Some(uses) = &job.uses {
                // `./.paddock/…` is the same repository checked out a second time (app-release.yml): the same file.
                let Some(callee) = uses.strip_prefix("./").map(|d| d.strip_prefix(".paddock/").unwrap_or(d)) else {
                    continue;
                };
                // Bind the callee's inputs from `with:`, over its declared defaults (§2.2).
                let Some(cw) = self.wf.get(callee) else {
                    self.problems
                        .push((format!("{}:{}", wf.path, 0), format!("`uses: {uses}` is not in the checkout")));
                    continue;
                };
                let mut bound: BTreeMap<String, Val> = cw.call_inputs.iter().map(|(k, d)| (k.clone(), Val::Str(d.clone()))).collect();
                for (k, v) in &job.with {
                    let i = expr::interpolate(v, &scope);
                    bound.insert(k.clone(), if i.unresolved { Val::Unknown } else { Val::Str(i.text) });
                }
                Self::mark(&mut self.called, callee, ctx.level, Self::status(&acc, false, false, false));
                self.walk_workflow(callee, ctx, &bound, &acc, depth + 1);
                continue;
            }
            self.default_shell = job.shell.clone().or_else(|| wf.shell.clone());
            let app = combo.as_ref().and_then(|c| c.get("app").cloned());
            let os = combo.as_ref().and_then(|c| c.get("os").cloned());
            let wd = wf.wd.as_deref().or(job.wd.as_deref());
            let wd = job.wd.as_deref().or(wf.wd.as_deref()).or(wd);
            let start = match wd {
                Some(w) => working_dir(&expr::interpolate(w, &scope).text),
                None => Some(String::new()),
            };
            self.walk_steps(
                &wf.path,
                &job.steps,
                &scope,
                &job_env,
                start,
                &acc,
                depth,
                &StepCtx {
                    app,
                    os,
                    job: job.id.clone(),
                },
            );
        }
    }

    fn combos(&mut self, job: &Job, ctx: &Context) -> Vec<Option<BTreeMap<String, String>>> {
        let Some(m) = &job.matrix else { return vec![None] };
        let mut out: Vec<BTreeMap<String, String>> = Vec::new();
        if !m.axes.is_empty() {
            let mut acc: Vec<BTreeMap<String, String>> = vec![BTreeMap::new()];
            for (k, vals) in &m.axes {
                acc = acc
                    .into_iter()
                    .flat_map(|base| {
                        vals.iter().map(move |v| {
                            let mut b = base.clone();
                            b.insert(k.clone(), v.clone());
                            b
                        })
                    })
                    .collect();
            }
            out.extend(acc);
        }
        out.extend(m.include.iter().cloned());
        if m.app_legs {
            let webkit = ctx.level != Level::Branch;
            for (name, c) in self.contracts {
                match app_legs(name, c, webkit) {
                    Ok(legs) => out.extend(legs),
                    Err(e) => self.problems.push((format!("apps/{name}/paddock.ci.json"), e)),
                }
            }
        }
        if out.is_empty() {
            return vec![None];
        }
        out.into_iter().map(Some).collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_steps(
        &mut self,
        file: &str,
        steps: &[Step],
        scope: &Scope,
        env: &BTreeMap<String, String>,
        start: Option<String>,
        outer: &Acc,
        depth: usize,
        sc: &StepCtx,
    ) {
        for step in steps {
            let mut acc = outer.clone();
            if !Self::cond(&mut acc, step.cond.as_deref(), scope) {
                continue;
            }
            Self::coe(&mut acc, step.coe.as_deref(), scope);
            let mut step_env = env.clone();
            for (k, v) in &step.env {
                let i = expr::interpolate(v, scope);
                step_env.insert(k.clone(), i.text);
            }
            let step_scope = Scope {
                env: &step_env,
                ..base_scope_clone(scope)
            };
            if let Some(uses) = &step.uses {
                let Some(dir) = uses.strip_prefix("./").map(|d| d.strip_prefix(".paddock/").unwrap_or(d)) else {
                    continue;
                };
                let Some(action) = self.actions.get(dir) else {
                    if uses.starts_with("./.gitea/actions/") {
                        self.problems
                            .push((format!("{file}:{}", step.line), format!("`uses: {uses}` is not in the checkout")));
                    }
                    continue;
                };
                let action = action.clone();
                let mut inputs: BTreeMap<String, Val> = action.inputs.iter().map(|(k, d)| (k.clone(), Val::Str(d.clone()))).collect();
                for (k, v) in &step.with {
                    let i = expr::interpolate(v, &step_scope);
                    inputs.insert(k.clone(), if i.unresolved { Val::Unknown } else { Val::Str(i.text) });
                }
                Self::mark(
                    &mut self.action_reach,
                    dir,
                    scope.ctx.level,
                    Self::status(&acc, false, false, false),
                );
                if depth > 8 {
                    continue;
                }
                let inner_scope = Scope {
                    ctx: scope.ctx,
                    inputs: &inputs,
                    env: &step_env,
                    matrix: scope.matrix,
                    runner_os: scope.runner_os.clone(),
                    action_path: Some(dir),
                    job: scope.job,
                    vars: scope.vars,
                    planners: scope.planners,
                };
                self.walk_steps(
                    &format!("{dir}/action.yml"),
                    &action.steps,
                    &inner_scope,
                    &step_env,
                    Some(String::new()),
                    &acc,
                    depth + 1,
                    sc,
                );
                continue;
            }
            let Some((run, run_line)) = &step.run else { continue };
            let i = expr::interpolate(run, &step_scope);
            let wd = match &step.wd {
                Some(w) => working_dir(&expr::interpolate(w, &step_scope).text),
                None => start.clone(),
            };
            let text = shell::strip_workspace(&i.text);
            let pipefail = step
                .shell
                .as_deref()
                .or(self.default_shell.as_deref())
                .is_some_and(|sh| sh.starts_with("bash"));
            let cmds = shell::commands_with(
                &text,
                wd,
                shell::Opts {
                    pipefail,
                    lines_checked: false,
                },
            );
            for c in cmds {
                self.emit(
                    file,
                    run_line + c.line,
                    &c.text,
                    c.cwd.clone(),
                    c.report_only,
                    c.conditional,
                    i.unresolved,
                    &acc,
                    &step_env,
                    scope,
                    sc,
                    None,
                    depth,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        file: &str,
        line: usize,
        text: &str,
        cwd: Option<String>,
        report: bool,
        conditional: bool,
        unresolved: bool,
        acc: &Acc,
        env: &BTreeMap<String, String>,
        scope: &Scope,
        sc: &StepCtx,
        origin: Option<String>,
        depth: usize,
    ) {
        let expanded = shell::expand_vars(text, env);
        // `bash …/run-commands.sh`: the lines of $COMMANDS (or the argument) are the commands (§2.2).
        if expanded
            .split_whitespace()
            .any(|w| w.trim_matches('"').ends_with("run-commands.sh"))
            && depth < 12
        {
            let list = env.get("COMMANDS").cloned().unwrap_or_default();
            let origin = sc.app.as_ref().map(|a| format!("apps/{a}/paddock.ci.json"));
            for c in shell::commands(&list, cwd.clone()) {
                self.emit(
                    file,
                    line,
                    &c.text,
                    c.cwd.clone(),
                    report || c.report_only,
                    conditional || c.conditional,
                    unresolved,
                    acc,
                    env,
                    scope,
                    sc,
                    origin.clone(),
                    12,
                );
            }
            return;
        }
        let in_command_position = expanded
            .split_whitespace()
            .next()
            .is_some_and(|w| w.trim_start_matches(['"', '\'']).starts_with('\u{2039}'));
        let status = Self::status(
            acc,
            unresolved || cwd.is_none() || expanded.contains("${{") || in_command_position,
            report,
            conditional,
        );
        let key: LineKey = (
            file.to_string(),
            line,
            expanded.clone(),
            cwd.clone(),
            sc.app.clone(),
            sc.job.clone(),
            sc.os.clone(),
        );
        let level = scope.ctx.level;
        let root = self.root.clone();
        let e = self.lines.entry(key).or_insert_with(|| RunLine {
            file: file.to_string(),
            line,
            root,
            job: sc.job.clone(),
            app: sc.app.clone(),
            os: sc.os.clone(),
            cwd: cwd.clone(),
            text: expanded.clone(),
            env: env.clone(),
            planned: BTreeSet::new(),
            levels: BTreeMap::new(),
            origin,
        });
        e.planned.extend(acc.planned.iter().cloned());
        let slot = e.levels.entry(level).or_insert(status);
        *slot = worse(*slot, status);
    }
}

#[derive(Clone)]
struct StepCtx {
    app: Option<String>,
    os: Option<String>,
    job: String,
}

fn base_scope_clone<'a>(s: &Scope<'a>) -> Scope<'a> {
    Scope {
        ctx: s.ctx,
        inputs: s.inputs,
        env: s.env,
        matrix: s.matrix,
        runner_os: s.runner_os.clone(),
        action_path: s.action_path,
        job: s.job,
        vars: s.vars,
        planners: s.planners,
    }
}

/// The workflows that a branch or a train runs by dispatch. No workflow runs on a pull request (pull requests are the branch
/// ledger, docs/plans/merge-queue.md; the contract test `no_workflow_runs_on_a_pull_request`), so a branch run of these is a dispatch on the branch's ref, and ADR 0060 §2.4
/// counts it at the `branch` level ("pull requests and branch runs"). Every other dispatch stays `manual`. These are the
/// workflows that had the `pull_request` trigger until the ledger; a test checks each one has `workflow_dispatch`.
pub const BRANCH_DISPATCH: &[&str] = &[
    ".gitea/workflows/ci.yml",
    ".gitea/workflows/debrief.yml",
    ".gitea/workflows/debrief-mutants.yml",
];

fn contexts(wf: &Workflow) -> Vec<Context> {
    let mut out = Vec::new();
    let dispatch = wf.dispatch_inputs.clone();
    for e in &wf.events {
        match e.as_str() {
            "workflow_call" => {}
            "pull_request" | "pull_request_target" => out.push(Context {
                event: "pull_request",
                git_ref: "refs/pull/1/merge".into(),
                level: Level::Branch,
                inputs: BTreeMap::new(),
            }),
            "push" => {
                let on_main = match &wf.push_branches {
                    Some(b) => b.iter().any(|x| x == "main" || x == "**" || x == "*"),
                    None => wf.push_tags.is_empty(),
                };
                let any_branch = wf.push_branches.is_none() && wf.push_tags.is_empty();
                if on_main {
                    out.push(Context {
                        event: "push",
                        git_ref: "refs/heads/main".into(),
                        level: Level::Main,
                        inputs: BTreeMap::new(),
                    });
                }
                if any_branch || wf.push_branches.as_ref().is_some_and(|b| b.iter().any(|x| x != "main")) {
                    out.push(Context {
                        event: "push",
                        git_ref: "refs/heads/topic".into(),
                        level: Level::Branch,
                        inputs: BTreeMap::new(),
                    });
                }
                for t in &wf.push_tags {
                    out.push(Context {
                        event: "push",
                        git_ref: format!("refs/tags/{}", t.replace('*', "x")),
                        level: Level::Release,
                        inputs: BTreeMap::new(),
                    });
                }
            }
            "schedule" => out.push(Context {
                event: "schedule",
                git_ref: "refs/heads/main".into(),
                level: Level::Nightly,
                inputs: BTreeMap::new(),
            }),
            "workflow_dispatch" => {
                out.push(Context {
                    event: "workflow_dispatch",
                    git_ref: "refs/heads/main".into(),
                    level: Level::Manual,
                    inputs: dispatch.clone(),
                });
                if BRANCH_DISPATCH.contains(&wf.path.as_str()) {
                    out.push(Context {
                        event: "workflow_dispatch",
                        git_ref: "refs/heads/topic".into(),
                        level: Level::Branch,
                        inputs: dispatch.clone(),
                    });
                }
            }
            _ => out.push(Context {
                event: "other",
                git_ref: "refs/heads/main".into(),
                level: Level::Manual,
                inputs: BTreeMap::new(),
            }),
        }
    }
    out
}

/// Builds the model from file contents.
pub fn build(src: &Sources) -> CiModel {
    let mut model = CiModel::default();
    let mut wf = BTreeMap::new();
    for (p, t) in &src.workflows {
        match parse_workflow(p, t) {
            Ok(w) => {
                wf.insert(p.clone(), w);
            }
            Err(e) => model.problems.push((p.clone(), e)),
        }
    }
    let mut actions = BTreeMap::new();
    for (p, t) in &src.actions {
        match parse_action(t) {
            Ok(a) => {
                actions.insert(p.clone(), a);
            }
            Err(e) => model.problems.push((format!("{p}/action.yml"), e)),
        }
    }
    let mut contracts = BTreeMap::new();
    for (app, t) in &src.contracts {
        match serde_json::from_str::<Json>(t) {
            Ok(j) => {
                contracts.insert(app.clone(), j);
            }
            Err(e) => model
                .problems
                .push((format!("apps/{app}/paddock.ci.json"), format!("not valid JSON: {e}"))),
        }
    }
    let mut lines: BTreeMap<LineKey, RunLine> = BTreeMap::new();
    let mut workflows = Reach::new();
    let mut action_reach = Reach::new();
    let mut called = Reach::new();
    for (path, w) in &wf {
        let ctxs = contexts(w);
        if ctxs.is_empty() {
            continue;
        }
        model.roots.push(path.clone());
        for ctx in ctxs {
            let mut walker = Walker {
                wf: &wf,
                actions: &actions,
                contracts: &contracts,
                vars: &src.vars,
                root: path.clone(),
                lines: BTreeMap::new(),
                workflows: Reach::new(),
                action_reach: Reach::new(),
                called: Reach::new(),
                problems: Vec::new(),
                default_shell: None,
            };
            let inputs: BTreeMap<String, Val> = ctx.inputs.iter().map(|(k, v)| (k.clone(), Val::Str(v.clone()))).collect();
            walker.walk_workflow(path, &ctx, &inputs, &Acc::default(), 0);
            for (k, v) in walker.lines {
                match lines.get_mut(&k) {
                    Some(e) => {
                        e.planned.extend(v.planned);
                        for (l, s) in v.levels {
                            let slot = e.levels.entry(l).or_insert(s);
                            *slot = worse(*slot, s);
                        }
                    }
                    None => {
                        lines.insert(k, v);
                    }
                }
            }
            for (from, into) in [
                (walker.workflows, &mut workflows),
                (walker.action_reach, &mut action_reach),
                (walker.called, &mut called),
            ] {
                for (p, lv) in from {
                    for (l, s) in lv {
                        Walker::mark(into, &p, l, s);
                    }
                }
            }
            model.problems.extend(walker.problems);
        }
    }
    model.problems.sort();
    model.problems.dedup();
    model.lines = lines.into_values().collect();
    model.workflows = workflows;
    model.actions = action_reach;
    model.called = called;
    model
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(files: &[(&str, &str)]) -> Sources {
        let mut s = Sources::default();
        for (p, t) in files {
            if let Some(dir) = p.strip_suffix("/action.yml") {
                s.actions.insert(dir.to_string(), t.to_string());
            } else if let Some(app) = p.strip_prefix("apps/").and_then(|r| r.strip_suffix("/paddock.ci.json")) {
                s.contracts.insert(app.to_string(), t.to_string());
            } else {
                s.workflows.insert(p.to_string(), t.to_string());
            }
        }
        s
    }

    fn find<'a>(m: &'a CiModel, needle: &str) -> Vec<&'a RunLine> {
        m.lines.iter().filter(|l| l.text.contains(needle)).collect()
    }

    #[test]
    fn a_dispatch_is_a_branch_run_only_for_the_branch_dispatch_workflows() {
        let wf = "on:\n  workflow_dispatch:\njobs:\n  c:\n    runs-on: ubuntu-latest\n    steps:\n      - run: node x.mjs\n";
        let m = build(&sources(&[(".gitea/workflows/ci.yml", wf)]));
        assert_eq!(only(&m, "x.mjs").levels.get(&Level::Branch), Some(&Status::Gated));
        let m = build(&sources(&[(".gitea/workflows/ci-token-check.yml", wf)]));
        assert_eq!(only(&m, "x.mjs").levels.get(&Level::Branch), None, "another dispatch stays manual");
        assert_eq!(only(&m, "x.mjs").levels.get(&Level::Manual), Some(&Status::Gated));
    }

    #[test]
    fn every_branch_dispatch_workflow_exists_and_can_be_dispatched() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for p in BRANCH_DISPATCH {
            let text = std::fs::read_to_string(root.join(p)).unwrap_or_else(|e| panic!("{p}: {e}"));
            let w = parse_workflow(p, &text).unwrap();
            assert!(
                w.events.contains("workflow_dispatch"),
                "{p} is a branch-run workflow but cannot be dispatched"
            );
            assert!(!w.events.contains("pull_request"), "{p}: rule 50, no pull_request trigger");
        }
    }

    fn only<'a>(m: &'a CiModel, needle: &str) -> &'a RunLine {
        let v = find(m, needle);
        assert_eq!(v.len(), 1, "{needle}: {:?}", m.lines.iter().map(|l| &l.text).collect::<Vec<_>>());
        v[0]
    }

    const APP_RELEASE: &str = "
on:
  workflow_call:
    inputs:
      bundle-check:
        type: string
        default: ''
jobs:
  installers:
    runs-on: ubuntu-latest
    steps:
      - name: Bundle check
        if: inputs.bundle-check != ''
        env:
          CHECK: ${{ inputs.bundle-check }}
        run: bash \"$CHECK\"
";

    #[test]
    fn a_reusable_workflow_input_binds_a_check_good_and_bad() {
        // dyno-release.yml passes `bundle-check: apps/dyno/scripts/check-bundle.sh` to app-release.yml, which runs it.
        let good = "on:\n  push:\n    tags: [\"dyno-v*\"]\njobs:\n  installers:\n    uses: ./.gitea/workflows/app-release.yml\n    with:\n      bundle-check: apps/dyno/scripts/check-bundle.sh\n";
        let m = build(&sources(&[
            (".gitea/workflows/dyno-release.yml", good),
            (".gitea/workflows/app-release.yml", APP_RELEASE),
        ]));
        let l = only(&m, "check-bundle.sh");
        assert_eq!(l.text, "bash \"apps/dyno/scripts/check-bundle.sh\"");
        assert_eq!(l.levels, BTreeMap::from([(Level::Release, Status::Gated)]));
        // the same caller without the input: the step's condition is false, so nothing runs
        let bad = good.replace("    with:\n      bundle-check: apps/dyno/scripts/check-bundle.sh\n", "");
        let m = build(&sources(&[
            (".gitea/workflows/dyno-release.yml", &bad),
            (".gitea/workflows/app-release.yml", APP_RELEASE),
        ]));
        assert!(m.lines.is_empty());
    }

    #[test]
    fn a_caller_nobody_reaches_is_dead_and_so_are_its_actions() {
        // app-licences.yml has only workflow_call and no caller; its `licences` action is then not used either.
        let dead = "on:\n  workflow_call:\njobs:\n  l:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: ./.gitea/actions/licences\n";
        let action = "runs:\n  using: composite\n  steps:\n    - run: cargo deny check\n      shell: bash\n";
        let m = build(&sources(&[
            (".gitea/workflows/app-licences.yml", dead),
            (".gitea/actions/licences/action.yml", action),
        ]));
        assert!(m.roots.is_empty());
        assert!(m.workflows.is_empty() && m.actions.is_empty() && m.lines.is_empty());
        // a push root that calls it makes both reachable
        let root = "on:\n  pull_request:\njobs:\n  c:\n    uses: ./.gitea/workflows/app-licences.yml\n";
        let m = build(&sources(&[
            (".gitea/workflows/root.yml", root),
            (".gitea/workflows/app-licences.yml", dead),
            (".gitea/actions/licences/action.yml", action),
        ]));
        assert!(m.actions.contains_key(".gitea/actions/licences"));
        assert_eq!(only(&m, "cargo deny").levels, BTreeMap::from([(Level::Branch, Status::Gated)]));
    }

    #[test]
    fn app_gates_commands_come_from_the_contract_with_their_folders() {
        // ci.yml's app job: matrix from the plan, `checks` through app-gates and run-commands.sh
        let plan = "  plan:\n    runs-on: ubuntu-latest\n    outputs:\n      any_app: ${{ steps.plan.outputs.any_app }}\n      apps: ${{ steps.plan.outputs.apps }}\n    steps:\n      - id: plan\n        run: node scripts/affected.mjs --ci\n";
        let ci_without_plan = "on:\n  pull_request:\njobs:\n  app:\n    needs: [plan]\n    if: ${{ needs.plan.outputs.any_app == 'true' }}\n    runs-on: ${{ matrix.os }}\n    strategy:\n      matrix:\n        include: ${{ fromJSON(needs.plan.outputs.apps) }}\n    steps:\n      - uses: ./.gitea/actions/app-gates\n        with:\n          app: ${{ matrix.app }}\n          checks: ${{ matrix.checks }}\n";
        let ci = format!("{ci_without_plan}{plan}");
        let ci = ci.as_str();
        let gates = "inputs:\n  app:\n    required: true\n  checks:\n    default: ''\nruns:\n  using: composite\n  steps:\n    - name: Checks\n      if: inputs.checks != ''\n      shell: bash\n      env:\n        COMMANDS: ${{ inputs.checks }}\n      run: bash \"${{ github.action_path }}/../../../conformance/ci/run-commands.sh\"\n";
        let contract = r#"{"cargo": ".", "os": ["ubuntu-latest", "windows-latest"], "checks": ["cd apps/dyno", "node --test scripts/check-wording.test.mjs"]}"#;
        let m = build(&sources(&[
            (".gitea/workflows/ci.yml", ci),
            (".gitea/actions/app-gates/action.yml", gates),
            ("apps/dyno/paddock.ci.json", contract),
        ]));
        let l = only(&m, "check-wording.test.mjs");
        assert_eq!((l.app.as_deref(), l.cwd.as_deref()), (Some("dyno"), Some("apps/dyno")));
        assert_eq!(l.origin.as_deref(), Some("apps/dyno/paddock.ci.json"));
        assert_eq!(l.levels, BTreeMap::from([(Level::Branch, Status::Gated)]));
        assert_eq!(l.planned, BTreeSet::from(["any_app".to_string()]));
        // Windows legs run no checks (the contract's lists are Linux only), so exactly one line exists
        // bad: a contract without `checks` yields nothing
        let m = build(&sources(&[
            (".gitea/workflows/ci.yml", ci),
            (".gitea/actions/app-gates/action.yml", gates),
            ("apps/dyno/paddock.ci.json", r#"{"cargo": "."}"#),
        ]));
        assert!(m.lines.iter().all(|l| l.app.is_none()), "{:?}", m.lines);
        // bad: with no plan job to mark the app, the job's condition is not decidable, so nothing is gated through it
        let m = build(&sources(&[
            (".gitea/workflows/ci.yml", ci_without_plan),
            (".gitea/actions/app-gates/action.yml", gates),
            ("apps/dyno/paddock.ci.json", contract),
        ]));
        assert_eq!(only(&m, "check-wording.test.mjs").levels[&Level::Branch], Status::Conditional);
    }

    #[test]
    fn the_leg_keys_follow_affected_mjs() {
        // The model binds the matrix by name; a change to scripts/affected.mjs matrix() fails here until LEG_KEYS follows.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/affected.mjs");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("the leg-key test needs scripts/affected.mjs ({}): {e}", path.display()));
        let from = text
            .find("const legs = oses.filter")
            .expect("matrix() changed shape: update gates::ci");
        let to = text[from..].find("// paddock.ci.json `e2e`").expect("matrix() changed shape") + from;
        let body = &text[from..to];
        let obj = &body[body.find("return {").expect("leg object") + 8..];
        let obj = &obj[..obj.find("      };").expect("leg object end")];
        let mut keys: Vec<String> = obj
            .lines()
            .filter_map(|l| {
                // `key: value,` or the shorthand `key,`
                let t = l.trim().trim_end_matches(',');
                let k = t.split(':').next()?;
                (!k.is_empty() && !k.starts_with("//") && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then(|| k.to_string())
            })
            .collect();
        keys.sort();
        let mut want: Vec<String> = LEG_KEYS.iter().map(|k| k.to_string()).collect();
        want.sort();
        assert_eq!(keys, want, "scripts/affected.mjs matrix() legs and gates::ci::LEG_KEYS differ");
    }

    #[test]
    fn working_folders_come_from_defaults_and_later_cd() {
        let wf = "on:\n  pull_request:\ndefaults:\n  run:\n    working-directory: conformance\njobs:\n  c:\n    runs-on: ubuntu-latest\n    steps:\n      - run: npm test\n      - working-directory: .\n        run: |\n          cd tools/paddock-dev\n          cargo test\n      - working-directory: ${{ env.PADDOCK_WORKSPACE || github.workspace }}/tools/paddock-dev\n        run: cargo fmt --check\n";
        let m = build(&sources(&[(".gitea/workflows/ci.yml", wf)]));
        assert_eq!(only(&m, "npm test").cwd.as_deref(), Some("conformance"));
        assert_eq!(only(&m, "cargo test").cwd.as_deref(), Some("tools/paddock-dev"));
        assert_eq!(only(&m, "cargo fmt").cwd.as_deref(), Some("tools/paddock-dev"));
        assert_eq!(only(&m, "cargo test").line, 14);
    }

    #[test]
    fn report_only_shapes_are_not_gated() {
        let wf = "on:\n  pull_request:\njobs:\n  c:\n    runs-on: ubuntu-latest\n    steps:\n      - run: node a.mjs\n        continue-on-error: true\n      - run: node b.mjs || true\n      - run: node c.mjs\n";
        let m = build(&sources(&[(".gitea/workflows/ci.yml", wf)]));
        assert_eq!(only(&m, "a.mjs").levels[&Level::Branch], Status::ReportOnly);
        assert_eq!(only(&m, "b.mjs").levels[&Level::Branch], Status::ReportOnly);
        assert_eq!(only(&m, "c.mjs").levels[&Level::Branch], Status::Gated);
    }

    #[test]
    fn a_pipe_is_a_gate_only_under_bash_with_pipefail_and_swallowing_shapes_are_not() {
        let wf = "on:\n  pull_request:\njobs:\n  c:\n    runs-on: ubuntu-latest\n    steps:\n      - run: node a.mjs | tee log\n      - shell: bash\n        run: node b.mjs | tee log\n      - run: node c.mjs || echo skipped\n      - run: |\n          node d.mjs && echo ok\n          node e.mjs\n";
        let m = build(&sources(&[(".gitea/workflows/ci.yml", wf)]));
        let st = |n: &str| only(&m, n).levels[&Level::Branch];
        assert_eq!(st("a.mjs"), Status::ReportOnly);
        assert_eq!(st("b.mjs"), Status::Gated);
        assert_eq!(st("c.mjs"), Status::ReportOnly);
        assert_eq!(st("d.mjs"), Status::ReportOnly);
        assert_eq!(st("e.mjs"), Status::Gated);
    }

    #[test]
    fn unresolvable_outputs_and_conditions_never_gate() {
        let wf = "on:\n  pull_request:\njobs:\n  a:\n    runs-on: ubuntu-latest\n    outputs:\n      cmd: ${{ steps.s.outputs.cmd }}\n    steps:\n      - id: s\n        run: echo\n  b:\n    needs: [a]\n    runs-on: ubuntu-latest\n    steps:\n      - run: ${{ needs.a.outputs.cmd }}\n      - if: ${{ vars.WITH_X == '1' }}\n        run: node vars.mjs\n      - if: failure()\n        run: node onfail.mjs\n";
        let m = build(&sources(&[(".gitea/workflows/ci.yml", wf)]));
        let l = find(&m, "needs.a.outputs.cmd");
        assert_eq!(l[0].levels[&Level::Branch], Status::Unresolved);
        assert_eq!(only(&m, "vars.mjs").levels[&Level::Branch], Status::Conditional);
        assert_eq!(only(&m, "onfail.mjs").levels[&Level::Branch], Status::Conditional);
        // with the variable in the register's [vars] the condition is decided
        let mut s = sources(&[(".gitea/workflows/ci.yml", wf)]);
        s.vars.insert("WITH_X".into(), "1".into());
        assert_eq!(only(&build(&s), "vars.mjs").levels[&Level::Branch], Status::Gated);
    }

    #[test]
    fn levels_follow_the_triggers() {
        let wf = "on:\n  push:\n    branches: [main]\n    tags: [\"v*\"]\n  schedule:\n    - cron: '0 1 * * *'\n  workflow_dispatch:\n    inputs:\n      webkit:\n        type: boolean\n        default: false\njobs:\n  c:\n    runs-on: ubuntu-latest\n    steps:\n      - run: node always.mjs\n      - if: ${{ github.event_name == 'schedule' }}\n        run: node nightly-only.mjs\n      - if: ${{ inputs.webkit == true || startsWith(github.ref, 'refs/tags/v') }}\n        run: node webkit.mjs\n";
        let m = build(&sources(&[(".gitea/workflows/nightlyish.yml", wf)]));
        let lv = |n: &str| only(&m, n).levels.keys().copied().collect::<Vec<_>>();
        assert_eq!(lv("always.mjs"), vec![Level::Main, Level::Nightly, Level::Release, Level::Manual]);
        assert_eq!(lv("nightly-only.mjs"), vec![Level::Nightly]);
        assert_eq!(lv("webkit.mjs"), vec![Level::Release]);
    }

    #[test]
    fn matrix_axes_give_runner_os_and_one_line_per_leg() {
        let wf = "on:\n  pull_request:\njobs:\n  c:\n    strategy:\n      matrix:\n        os: [ubuntu-latest, windows-latest]\n    runs-on: ${{ matrix.os }}\n    steps:\n      - if: runner.os == 'Windows'\n        run: node win.mjs\n      - run: node all.mjs\n";
        let m = build(&sources(&[(".gitea/workflows/ci.yml", wf)]));
        let win = only(&m, "win.mjs");
        assert_eq!(win.os.as_deref(), Some("windows-latest"));
        assert_eq!(find(&m, "all.mjs").len(), 2);
    }

    #[test]
    fn the_plans_os_legs_of_ci_yml_run_on_every_os_and_a_rerun_gate_reads_as_a_normal_run() {
        let wf = "on:\n  pull_request:\njobs:\n  plan:\n    runs-on: ubuntu-latest\n    outputs:\n      crates: ${{ steps.plan.outputs.crates }}\n      rerun_jobs: ${{ steps.plan.outputs.rerun_jobs }}\n      crate_legs: ${{ steps.plan.outputs.crate_legs }}\n    steps:\n      - id: plan\n        run: node plan.mjs\n  c:\n    needs: [plan]\n    if: ${{ needs.plan.outputs.crates == 'true' && contains(fromJSON(needs.plan.outputs.rerun_jobs || '[\"c\"]'), 'c') }}\n    strategy:\n      matrix:\n        include: ${{ fromJSON(needs.plan.outputs.crate_legs) }}\n    runs-on: ${{ matrix.os }}\n    steps:\n      - if: runner.os == 'Windows'\n        run: node win.mjs\n      - run: node all.mjs\n";
        let m = build(&sources(&[(".gitea/workflows/ci.yml", wf)]));
        assert_eq!(find(&m, "all.mjs").len(), 3, "ubuntu, windows and macos");
        assert_eq!(only(&m, "win.mjs").os.as_deref(), Some("windows-latest"));
    }

    #[test]
    fn a_job_that_needs_a_skipped_job_is_skipped_unless_it_says_always() {
        let wf = "on:\n  pull_request:\njobs:\n  nightly:\n    if: ${{ github.event_name == 'schedule' }}\n    runs-on: ubuntu-latest\n    steps:\n      - run: node n.mjs\n  after:\n    needs: [nightly]\n    runs-on: ubuntu-latest\n    steps:\n      - run: node after.mjs\n  later:\n    needs: [after]\n    runs-on: ubuntu-latest\n    steps:\n      - run: node later.mjs\n  verdict:\n    needs: [nightly]\n    if: ${{ always() }}\n    runs-on: ubuntu-latest\n    steps:\n      - run: node verdict.mjs\n";
        let m = build(&sources(&[(".gitea/workflows/ci.yml", wf)]));
        assert!(find(&m, "n.mjs").is_empty());
        assert!(find(&m, "after.mjs").is_empty(), "needs a job that does not run at this level");
        assert!(find(&m, "later.mjs").is_empty(), "the skip follows the chain");
        assert_eq!(
            only(&m, "verdict.mjs").levels[&Level::Branch],
            Status::Gated,
            "always() runs after a skipped need"
        );
    }

    #[test]
    fn a_workflow_that_does_not_parse_is_a_problem_not_silence() {
        let m = build(&sources(&[(".gitea/workflows/bad.yml", "on: [push\njobs: {")]));
        assert_eq!(m.problems.len(), 1);
        assert!(m.problems[0].1.contains("YAML"), "{:?}", m.problems);
    }
}
