//! The CI adapter of the repository model (ADR 0117 §4, §15, WP-2): workflows, jobs, steps, local actions and the scripts they run
//! become nodes, and `uses:` and the programs a step invokes become edges. It reads YAML with the CI model's one reader
//! ([`crate::gates::yaml`], ADR 0060) and shell with its command splitter ([`crate::gates::shell`]), so there is no second CI
//! parser.
//!
//! - **Edges come only from uses and invoked programs** (G4): `uses: ./.gitea/actions/<a>` (a step or a composite action), a job's
//!   `uses: ./.gitea/workflows/<w>.yml`, the program of a command or the script an interpreter (`bash`, `sh`, `node`, `pwsh`,
//!   `python3`) is given, including a path held in a shell variable assigned in the same block. A path inside other text (a grep
//!   pattern, an echo, a comment) is no edge.
//! - **What a job runs for**: a ci.yml job's `if:` reads the plan's outputs (`needs.plan.outputs.<name>`); the output names the
//!   project ([`crate::plan_ci::PROJECT_OUTPUTS`], the planner's own table). `./dev ci run <project> <phase>` runs that project; `dev
//!   plan` runs the planner (paddock-dev). A job of an app's own workflow (`<app>.yml`, `<app>-*.yml`) runs for that app, the
//!   repository's naming rule (AGENTS.md "Workflows are named after their app").
//! - **Release**: a job that uses a declared release action (bundling, signing, publishing) is a release node, and an action
//!   declared setup-only stays plumbing even there (`docs/repo-graph.toml` `[ci]`, rule 54).
//! - A contract's check line that runs a script (`node conformance/bin/paddock-conformance.mjs --only C13` in an app's
//!   `paddock.ci.json`) is a step node of that app: a change to the script reaches the app's conformance step, not its tests (§6).

use crate::gates::{shell, yaml};
use paddock_repo_graph::model::extract::{BulkRead, ExtractError, Extracted, Extractor};
use paddock_repo_graph::model::nodes::{Edge, EdgeKind, ModelNode, NodeKind};
use std::collections::{BTreeMap, BTreeSet};

/// The interpreters whose first non-option argument is the script they run.
const INTERPRETERS: [&str; 7] = ["bash", "sh", "node", "pwsh", "python3", "python", "zsh"];

/// The folders whose files are script nodes even before a step runs them (ADR 0117 §2).
const SCRIPT_FOLDERS: [&str; 4] = ["conformance/ci/", "conformance/release/", "conformance/src/util/", "scripts/"];

/// The script extensions.
const SCRIPT_EXT: [&str; 7] = [".sh", ".mjs", ".cjs", ".js", ".ps1", ".py", ".ts"];

/// The declared CI facts the adapter needs: actions that are setup-only and actions that make a job a release job.
#[derive(Debug, Clone, Default)]
pub struct CiDecl {
    /// Setup-only actions (plumbing even in a release job).
    pub setup_only: BTreeSet<String>,
    /// Actions whose use makes a job a release job.
    pub release_actions: BTreeSet<String>,
}

/// The CI adapter.
pub struct CiExtractor {
    /// The apps (`apps/<name>/paddock.ci.json`).
    pub apps: BTreeSet<String>,
    /// The root project ids.
    pub projects: BTreeSet<String>,
    /// The declarations.
    pub decl: CiDecl,
}

/// The id of a workflow node.
pub fn workflow_id(file: &str) -> String {
    format!("wf:{}", file.rsplit('/').next().unwrap_or(file))
}

/// The id of a job node.
pub fn job_id(file: &str, job: &str) -> String {
    format!("job:{}#{job}", file.rsplit('/').next().unwrap_or(file))
}

/// The id of an action node.
pub fn action_id(name: &str) -> String {
    format!("action:{name}")
}

/// The id of a script node.
pub fn script_id(path: &str) -> String {
    format!("script:{path}")
}

fn is_workflow(f: &str) -> bool {
    f.starts_with(".gitea/workflows/") && (f.ends_with(".yml") || f.ends_with(".yaml")) && f.matches('/').count() == 2
}

fn action_of(f: &str) -> Option<&str> {
    let rest = f.strip_prefix(".gitea/actions/")?;
    let (name, _) = rest.split_once('/')?;
    Some(name)
}

fn is_script(f: &str) -> bool {
    SCRIPT_EXT.iter().any(|e| f.ends_with(e))
}

/// Splits a command line into words, honouring single and double quotes (no expansion).
pub fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for c in text.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            (None, ';' | '|' | '&') => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
                break;
            }
            (None, c) => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// What one shell block runs: the tracked scripts it invokes and the projects it runs (`./dev ci run <p>`, `dev plan`).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Invoked {
    /// Tracked script paths.
    pub scripts: BTreeSet<String>,
    /// Project ids.
    pub projects: BTreeSet<String>,
}

/// Reads a `run:` block or a script. `action_dir` is the action folder `github.action_path` names, `tracked` the tracked files,
/// `project_of` the project id a `./dev ci run` argument names.
///
/// A shell variable assigned a tracked file or folder (`here=$(cd "$(dirname "$0")" && pwd)`, `r="<action>/../x.sh"`) stands for
/// it in later words. An invoked path that still holds a variable after that (`"$here/host/$step.sh"`, dynamic dispatch) cannot be
/// told exactly, so it invokes every tracked file its literal parts match (a superset, never a guess that drops one).
pub fn invoked(script: &str, action_dir: Option<&str>, tracked: &BTreeSet<String>, project_of: &dyn Fn(&str) -> Option<String>) -> Invoked {
    let mut out = Invoked::default();
    let mut text = script.to_string();
    if let Some(dir) = action_dir {
        for v in ["${{ github.action_path }}", "${GITHUB_ACTION_PATH}", "$GITHUB_ACTION_PATH"] {
            text = text.replace(v, dir);
        }
    }
    let mut text = text.replace("${{ github.workspace }}/", "").replace("$GITHUB_WORKSPACE/", "");
    // A function body may run: its header line is dropped so the splitter reads the body as commands (a superset).
    text = text
        .lines()
        .map(|l| {
            let tr = l.trim_end();
            if tr.ends_with("() {") || tr.ends_with("(){") { "" } else { l }
        })
        .collect::<Vec<_>>()
        .join("\n");
    // `$(cd "X" && pwd)` is the folder X.
    while let Some(i) = text.find("$(cd \"") {
        let Some(j) = text[i..].find("\" && pwd)") else { break };
        let inner = text[i + 6..i + j].to_string();
        text.replace_range(i..i + j + "\" && pwd)".len(), &inner);
    }
    let is_dir = |p: &str| {
        p.is_empty()
            || tracked
                .range(format!("{p}/")..)
                .next()
                .is_some_and(|f| f.starts_with(&format!("{p}/")))
    };
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    // `$(cd "X" && pwd)` is X; `$name` and `${name}` of a known variable are its folder or file.
    let expand = |w: &str, vars: &BTreeMap<String, String>| -> String {
        let mut s = w.to_string();
        for (k, v) in vars {
            for form in [format!("${{{k}}}"), format!("${k}")] {
                if let Some(rest) = s.strip_prefix(&form)
                    && (rest.is_empty() || rest.starts_with('/'))
                {
                    s = if v.is_empty() {
                        rest.trim_start_matches('/').to_string()
                    } else {
                        format!("{v}{rest}")
                    };
                }
            }
        }
        s
    };
    let resolve = |word: &str, cwd: &str| -> Vec<String> {
        if word.starts_with('/') || word.is_empty() {
            return Vec::new();
        }
        let w = word.trim_start_matches("./");
        if w.contains('$') {
            // Dynamic: every tracked file the literal parts match, in the folders the command could mean.
            let pattern: String = w
                .split('/')
                .map(|seg| if seg.contains('$') { "*".to_string() } else { seg.to_string() })
                .collect::<Vec<_>>()
                .join("/");
            if pattern.split('/').all(|seg| seg == "*") {
                return Vec::new();
            }
            return [cwd, ""]
                .iter()
                .filter_map(|d| shell::join(d, &pattern))
                .flat_map(|p| {
                    tracked
                        .iter()
                        .filter(move |f| paddock_repo_graph::model::glob::matches(&p, f))
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .collect();
        }
        [cwd, ""]
            .iter()
            .filter_map(|d| shell::join(d, w))
            .find(|p| tracked.contains(p))
            .into_iter()
            .collect()
    };
    let as_path = |word: &str, cwd: &str| -> Option<String> {
        let w = word.trim_start_matches("./");
        if w.contains('$') || word.starts_with('/') {
            return None;
        }
        [cwd, ""]
            .iter()
            .filter_map(|d| shell::join(d, w))
            .find(|p| tracked.contains(p) || is_dir(p))
    };
    for cmd in shell::commands(&text, Some(String::new())) {
        let cwd = cmd.cwd.clone().unwrap_or_default();
        let ws = words(&cmd.text);
        // Assignments in front of a command, or alone, may hold a path the command uses as `$name`.
        let mut i = 0;
        while let Some((k, v)) = ws.get(i).and_then(|w| w.split_once('=')).filter(|(k, _)| is_name(k)) {
            let v = expand(v.trim_matches('"'), &vars);
            if let Some(p) = as_path(&v, &cwd) {
                vars.insert(k.to_string(), p);
            }
            i += 1;
        }
        let mut rest: Vec<String> = ws[i..].iter().map(|w| expand(w, &vars)).collect();
        while rest
            .first()
            // Prefix words (`exec`, `env` and the like) and a `case` pattern (`build-folder)`).
            .is_some_and(|w| {
                ["exec", "command", "env", "time", "nohup", "job_setup"].contains(&w.as_str()) || (w.ends_with(')') && !w.contains('('))
            })
        {
            rest.remove(0);
        }
        let Some(program) = rest.first().cloned() else {
            continue;
        };
        let base = program.rsplit('/').next().unwrap_or(&program).to_string();
        if base == "dev" || program.ends_with("paddock-dev") || program == "$PADDOCK_DEV" || program == "${PADDOCK_DEV}" {
            // `bash "$root/dev" ci ...` arrives here through the interpreter branch below.
            dev_call(&rest[1..], &mut out, project_of);
            continue;
        }
        if INTERPRETERS.contains(&base.as_str()) {
            let mut k = 1;
            while let Some(a) = rest.get(k) {
                if a == "-o" || a == "-O" {
                    k += 2;
                } else if a == "-c" {
                    k = usize::MAX;
                    break;
                } else if a.starts_with('-') {
                    k += 1;
                } else {
                    break;
                }
            }
            if let Some(arg) = rest.get(k) {
                let target = arg.rsplit('/').next().unwrap_or(arg);
                if target == "dev" {
                    dev_call(&rest[k + 1..], &mut out, project_of);
                    if let Some(p) = resolve(arg, &cwd).into_iter().next() {
                        out.scripts.insert(p);
                    }
                    continue;
                }
                out.scripts.extend(resolve(arg, &cwd));
            }
            continue;
        }
        out.scripts.extend(resolve(&program, &cwd));
    }
    out
}

/// A `dev` call's arguments: `ci run <project> <phase>` runs that project, `plan` runs the planner.
fn dev_call(args: &[String], out: &mut Invoked, project_of: &dyn Fn(&str) -> Option<String>) {
    match (args.first().map(String::as_str), args.get(1).map(String::as_str), args.get(2)) {
        (Some("ci"), Some("run"), Some(p)) => {
            if let Some(id) = project_of(p) {
                out.projects.insert(id);
            }
        }
        (Some("plan"), _, _) => {
            out.projects.insert("paddock-dev".to_string());
        }
        _ => {}
    }
}

/// A script's text with the usual spellings of its own folder (`$(dirname "$0")`, `${BASH_SOURCE[0]%/*}`) replaced by `dir`.
fn script_dir_named(text: &str, dir: &str) -> String {
    let mut out = text.to_string();
    for v in [
        "$(cd \"$(dirname \"$0\")\" && pwd)",
        "$(dirname \"$0\")",
        "$(dirname \"${BASH_SOURCE[0]}\")",
        "$(dirname \"$BASH_SOURCE\")",
        "${0%/*}",
        "${BASH_SOURCE[0]%/*}",
    ] {
        out = out.replace(v, dir);
    }
    out
}

fn is_name(k: &str) -> bool {
    !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !k.starts_with(|c: char| c.is_ascii_digit())
}

/// The local action a `uses:` names (`./.gitea/actions/<a>`).
fn local_action(uses: &str) -> Option<String> {
    uses.strip_prefix("./.gitea/actions/").map(|a| a.trim_end_matches('/').to_string())
}

/// The plan outputs a job's `if:` reads.
fn plan_outputs(cond: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = cond;
    while let Some(i) = rest.find("needs.plan.outputs.") {
        rest = &rest[i + "needs.plan.outputs.".len()..];
        let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

impl CiExtractor {
    fn project_of(&self, word: &str) -> Option<String> {
        if self.apps.contains(word) {
            Some(format!("app:{word}"))
        } else if self.projects.contains(word) {
            Some(word.to_string())
        } else {
            None
        }
    }

    /// The projects a plan output names: a project key of the plan, or every app for `any_app`.
    fn output_projects(&self, output: &str) -> Vec<String> {
        if output == "any_app" {
            return self.apps.iter().map(|a| format!("app:{a}")).collect();
        }
        crate::plan_ci::PROJECT_OUTPUTS
            .iter()
            .filter(|(o, _, _)| *o == output)
            .map(|(_, p, _)| p.to_string())
            .collect()
    }

    fn app_of_workflow(&self, file: &str) -> Option<String> {
        let name = file.rsplit('/').next()?.trim_end_matches(".yml").trim_end_matches(".yaml");
        let name = name.strip_prefix("release-").unwrap_or(name);
        self.apps
            .iter()
            .filter(|a| name == a.as_str() || name.starts_with(&format!("{a}-")))
            .max_by_key(|a| a.len())
            .map(|a| format!("app:{a}"))
    }

    /// Steps of a job or a composite action: their nodes and edges, the actions they use, and what they run.
    #[allow(clippy::too_many_arguments)]
    fn steps(
        &self,
        x: &mut Extracted,
        owner: &str,
        prefix: &str,
        steps: &[yaml::Node],
        action_dir: Option<&str>,
        tracked: &BTreeSet<String>,
        used: &mut BTreeSet<String>,
        runs: &mut BTreeSet<String>,
    ) {
        for (i, s) in steps.iter().enumerate() {
            let sid = format!("step:{prefix}/{i}");
            x.nodes.push(ModelNode::new(&sid, NodeKind::Step, &[]));
            x.edges.push(Edge::new(owner, &sid, EdgeKind::Uses));
            if let Some(a) = s.get_text("uses").and_then(local_action) {
                x.edges.push(Edge::new(&sid, action_id(&a), EdgeKind::Uses));
                used.insert(a);
            }
            if let Some(run) = s.get_text("run") {
                let start = s.get_text("working-directory").unwrap_or("");
                let script = if start.is_empty() {
                    run.to_string()
                } else {
                    format!("cd {start}\n{run}")
                };
                let inv = invoked(&script, action_dir, tracked, &|w| self.project_of(w));
                for p in inv.scripts {
                    x.edges.push(Edge::new(&sid, script_id(&p), EdgeKind::Runs));
                    x.edges.push(Edge::new(owner, script_id(&p), EdgeKind::Runs));
                }
                for p in inv.projects {
                    runs.insert(p);
                }
            }
        }
    }
}

impl Extractor for CiExtractor {
    fn name(&self) -> &'static str {
        "ci"
    }

    fn claims(&self) -> &[&'static str] {
        // Every tracked file: workflows, actions, scripts and the contracts are told apart in `extract`.
        &[""]
    }

    fn extract(&self, files: &[String], read: &dyn BulkRead) -> Result<Extracted, ExtractError> {
        let tracked: BTreeSet<String> = files.iter().cloned().collect();
        let workflows: Vec<String> = files.iter().filter(|f| is_workflow(f)).cloned().collect();
        let actions: Vec<String> = files
            .iter()
            .filter(|f| action_of(f).is_some() && (f.ends_with("/action.yml") || f.ends_with("/action.yaml")))
            .cloned()
            .collect();
        let shell_scripts: Vec<String> = files
            .iter()
            .filter(|f| f.ends_with(".sh") && (SCRIPT_FOLDERS.iter().any(|d| f.starts_with(d)) || action_of(f).is_some()))
            .cloned()
            .collect();
        let contracts: Vec<String> = self.apps.iter().map(|a| format!("apps/{a}/paddock.ci.json")).collect();
        let wanted: Vec<String> = workflows
            .iter()
            .chain(&actions)
            .chain(&shell_scripts)
            .chain(&contracts)
            .cloned()
            .collect();
        let texts: BTreeMap<String, String> = wanted
            .iter()
            .cloned()
            .zip(read.read_all(&wanted))
            .filter_map(|(p, t)| t.map(|t| (p, t)))
            .collect();
        let bad = |file: &str, reason: String| ExtractError {
            extractor: "ci",
            file: file.to_string(),
            reason,
        };
        let mut x = Extracted::default();
        let mut scripts: BTreeSet<String> = files
            .iter()
            .filter(|f| SCRIPT_FOLDERS.iter().any(|d| f.starts_with(d)) && is_script(f))
            .cloned()
            .collect();
        // Actions first: their own uses and runs, and their release and setup flags.
        let mut action_names: BTreeSet<String> = files.iter().filter_map(|f| action_of(f)).map(str::to_string).collect();
        action_names.retain(|a| {
            tracked
                .iter()
                .any(|f| f.starts_with(&format!(".gitea/actions/{a}/")) && !f.ends_with(".md"))
        });
        for a in &action_names {
            let mut n = ModelNode::new(action_id(a), NodeKind::Action, &[&format!(".gitea/actions/{a}/")]);
            n.setup_only = self.decl.setup_only.contains(a);
            x.nodes.push(n);
        }
        for f in &actions {
            let Some(text) = texts.get(f) else { continue };
            let root = yaml::parse(text).map_err(|e| bad(f, e))?;
            let a = action_of(f).unwrap_or_default().to_string();
            let dir = format!(".gitea/actions/{a}");
            let steps = root
                .get("runs")
                .and_then(|r| r.get("steps"))
                .map(yaml::Node::items)
                .unwrap_or_default();
            let (mut used, mut runs) = (BTreeSet::new(), BTreeSet::new());
            self.steps(
                &mut x,
                &action_id(&a),
                &format!("action:{a}"),
                steps,
                Some(&dir),
                &tracked,
                &mut used,
                &mut runs,
            );
            for u in used {
                x.edges.push(Edge::new(action_id(&a), action_id(&u), EdgeKind::Uses));
            }
            for p in runs {
                x.edges.push(Edge::new(action_id(&a), p, EdgeKind::Runs));
            }
        }
        for f in &workflows {
            let Some(text) = texts.get(f) else { continue };
            let root = yaml::parse(text).map_err(|e| bad(f, e))?;
            let wf = workflow_id(f);
            x.nodes.push(ModelNode::new(&wf, NodeKind::Workflow, &[f.as_str()]));
            let app = self.app_of_workflow(f);
            for (job, body) in root.get("jobs").map(yaml::Node::entries).unwrap_or_default() {
                let jid = job_id(f, job);
                let (mut used, mut runs) = (BTreeSet::new(), BTreeSet::new());
                let steps = body.get("steps").map(yaml::Node::items).unwrap_or_default();
                self.steps(
                    &mut x,
                    &jid,
                    &format!("{}#{job}", f.rsplit('/').next().unwrap_or(f)),
                    steps,
                    None,
                    &tracked,
                    &mut used,
                    &mut runs,
                );
                if let Some(w) = body.get_text("uses").and_then(|u| u.strip_prefix("./")) {
                    x.edges.push(Edge::new(&jid, workflow_id(w), EdgeKind::Uses));
                }
                let mut node = ModelNode::new(&jid, NodeKind::Job, &[]);
                node.release = used.iter().any(|a| self.decl.release_actions.contains(a));
                x.nodes.push(node);
                x.edges.push(Edge::new(&wf, &jid, EdgeKind::Uses));
                for out in plan_outputs(body.get_text("if").unwrap_or("")) {
                    runs.extend(self.output_projects(&out));
                }
                if let Some(a) = &app {
                    runs.insert(a.clone());
                }
                for p in runs {
                    x.edges.push(Edge::new(&jid, p, EdgeKind::Runs));
                }
            }
        }
        // A contract's check lines (its own steps, run by app-gates): the scripts they run.
        for c in &contracts {
            let Some(text) = texts.get(c) else { continue };
            let v: serde_json::Value = serde_json::from_str(text).map_err(|e| bad(c, e.to_string()))?;
            let app = c.trim_start_matches("apps/").trim_end_matches("/paddock.ci.json");
            let sid = format!("step:{c}#checks");
            let lines: Vec<&str> = v["checks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .collect();
            let inv = invoked(&lines.join("\n"), None, &tracked, &|w| self.project_of(w));
            if inv.scripts.is_empty() {
                continue;
            }
            x.nodes.push(ModelNode::new(&sid, NodeKind::Step, &[]));
            x.edges.push(Edge::new(&sid, format!("app:{app}"), EdgeKind::Runs));
            for p in inv.scripts {
                x.edges.push(Edge::new(&sid, script_id(&p), EdgeKind::Runs));
                scripts.insert(p);
            }
        }
        // Scripts: every one a step or action runs, and every script in the script folders; a shell script's own invocations.
        for e in &x.edges {
            if let Some(p) = e.to.strip_prefix("script:") {
                scripts.insert(p.to_string());
            }
        }
        for s in &scripts {
            // A script inside an action folder is the action's own file (the action node owns it).
            if action_of(s).is_none() {
                x.nodes.push(ModelNode::new(script_id(s), NodeKind::Script, &[s.as_str()]));
            }
            if let Some(text) = texts.get(s) {
                let dir = s.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                let inv = invoked(&script_dir_named(text, dir), None, &tracked, &|w| self.project_of(w));
                for p in inv.scripts.iter().filter(|p| *p != s) {
                    let from = if action_of(s).is_some() {
                        action_id(action_of(s).unwrap_or_default())
                    } else {
                        script_id(s)
                    };
                    x.edges.push(Edge::new(from, script_id(p), EdgeKind::ReadsScript));
                }
            }
        }
        // An edge to a script inside an action folder goes to the action.
        for e in &mut x.edges {
            if let Some(a) = e.to.strip_prefix("script:").and_then(action_of) {
                e.to = action_id(a);
            }
        }
        Ok(x)
    }

    fn spawn_budget(&self, _: &[String]) -> usize {
        0
    }
}

/// The jobs of a workflow whose text changed between two revisions, or every job when the header (everything outside `jobs`) changed
/// or either side cannot be read (fail closed). The model seeds these instead of the whole workflow (G5).
pub fn changed_jobs(before: Option<&str>, after: Option<&str>) -> Option<Vec<String>> {
    let parse = |t: Option<&str>| t.map(yaml::parse).transpose().ok();
    let (b, a) = (parse(before)?, parse(after)?);
    let jobs = |n: &Option<yaml::Node>| -> BTreeMap<String, String> {
        n.as_ref()
            .and_then(|n| n.get("jobs"))
            .map(|j| {
                j.entries()
                    .iter()
                    .map(|(k, v)| (k.clone(), format!("{:?}", strip_lines(v))))
                    .collect()
            })
            .unwrap_or_default()
    };
    let header = |n: &Option<yaml::Node>| -> String {
        n.as_ref()
            .map(|n| {
                n.entries()
                    .iter()
                    .filter(|(k, _)| k != "jobs")
                    .map(|(k, v)| format!("{k}={:?}", strip_lines(v)))
                    .collect()
            })
            .unwrap_or_default()
    };
    let (jb, ja) = (jobs(&b), jobs(&a));
    if header(&b) != header(&a) {
        return None;
    }
    let names: BTreeSet<&String> = jb.keys().chain(ja.keys()).collect();
    Some(names.into_iter().filter(|n| jb.get(*n) != ja.get(*n)).cloned().collect())
}

/// A node with its line numbers zeroed (moving a job down a line is no change to it).
fn strip_lines(n: &yaml::Node) -> yaml::Node {
    let value = match &n.value {
        yaml::Value::Scalar { text, block } => yaml::Value::Scalar {
            text: text.clone(),
            block: *block,
        },
        yaml::Value::Seq(s) => yaml::Value::Seq(s.iter().map(strip_lines).collect()),
        yaml::Value::Map(m) => yaml::Value::Map(m.iter().map(|(k, v)| (k.clone(), strip_lines(v))).collect()),
    };
    yaml::Node { line: 0, value }
}

#[cfg(test)]
mod tests;
