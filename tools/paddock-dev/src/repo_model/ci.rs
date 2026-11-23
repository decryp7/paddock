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
use paddock_repo_graph::model::Unresolved;
use paddock_repo_graph::model::extract::{BulkRead, ExtractError, Extracted, Extractor, texts_of};
use paddock_repo_graph::model::inputs::Paths;
use paddock_repo_graph::model::nodes::{Edge, EdgeKind, ModelNode, NodeKind};
use std::collections::{BTreeMap, BTreeSet};

/// The interpreters whose first non-option argument is the script they run.
const INTERPRETERS: [&str; 7] = ["bash", "sh", "node", "pwsh", "python3", "python", "zsh"];

/// The script extensions.
const SCRIPT_EXT: [&str; 7] = [".sh", ".mjs", ".cjs", ".js", ".ps1", ".py", ".ts"];

/// The declared CI facts the adapter needs: actions that are setup-only and actions that make a job a release job.
#[derive(Debug, Clone, Default)]
pub struct CiDecl {
    /// Setup-only actions (plumbing even in a release job).
    pub setup_only: BTreeSet<String>,
    /// Actions whose use makes a job a release job.
    pub release_actions: BTreeSet<String>,
    /// The declared folders and file names (`docs/repo-graph.toml` `[paths]`): the adapter holds none.
    pub paths: Paths,
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

fn is_workflow(p: &Paths, f: &str) -> bool {
    f.starts_with(&p.workflows)
        && (f.ends_with(".yml") || f.ends_with(".yaml"))
        && f.matches('/').count() == p.workflows.matches('/').count()
}

fn action_of<'a>(p: &Paths, f: &'a str) -> Option<&'a str> {
    let rest = f.strip_prefix(&p.actions)?;
    let (name, _) = rest.split_once('/')?;
    Some(name)
}

/// The folder of a local action (`<actions>/<name>`, no trailing slash).
fn action_dir(p: &Paths, name: &str) -> String {
    format!("{}{name}", p.actions)
}

fn in_script_folder(p: &Paths, f: &str) -> bool {
    p.script_folders.iter().any(|d| f.starts_with(d))
}

/// A text with every declared spelling of the repository root (`[paths]` `root_spellings`: the checkout, the stable host-runner
/// path, and the folder an app repository checks Paddock out to for the reusable workflows) replaced, so a path under it resolves.
fn rooted(p: &Paths, text: &str) -> String {
    p.root_spellings
        .iter()
        .fold(text.to_string(), |t, (from, to)| t.replace(from.as_str(), to))
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
    /// Tracked data files a command names as an argument or a redirect (`ports.txt`, `kept-folders.txt`): a change to one reaches
    /// what runs the block (review P1-4).
    pub reads: BTreeSet<String>,
    /// Paths it runs that resolve to no tracked file (a deleted script, a variable no `env:` or assignment names): its users run
    /// in full (ADR 0117 §4, review P1-4).
    pub unresolved: BTreeSet<String>,
}

/// Programs that are the runner's or the system's, never the repository's: an unresolved path among them is no finding.
fn system_path(w: &str) -> bool {
    w.contains(":-/")
        || w.starts_with('/')
        || w.starts_with('~')
        || [
            "$HOME",
            "${HOME}",
            "$RUNNER_",
            "${RUNNER_",
            "$TMPDIR",
            "${TMPDIR}",
            "${{ runner.",
            "$CARGO_HOME",
            "${CARGO_HOME}",
        ]
        .iter()
        .any(|p| w.starts_with(p))
}

/// A value that comes from the caller (`$1`, `${2:-x}`, `"$@"`) or defaults to an absolute path (`${X:-/tmp}`).
fn positional(v: &str) -> bool {
    let v = v.trim_matches('"');
    v.starts_with("$@")
        || v.contains(":-/")
        || v.strip_prefix('$')
            .map(|r| r.trim_start_matches('{'))
            .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
}

/// Whether a word could name a file a command runs: no assignment, redirect, option or quoting debris, and a folder separator or
/// a script extension.
fn looks_like_path(w: &str) -> bool {
    !w.is_empty()
        && !w.contains(['=', '<', '>', ' ', '(', ')', '|', '[', ']'])
        && !w.starts_with('-')
        && !w.starts_with("__subst__")
        && (w.contains('/') || is_script(w))
}

/// The bodies of the command substitutions `$( … )` of a line (balanced, not nested deeper than they open), and the line with
/// each replaced by a placeholder word.
fn substitutions(line: &str) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut inner = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && chars.get(i + 1) == Some(&'(') && chars.get(i + 2) != Some(&'(') {
            let mut depth = 0;
            let mut j = i + 1;
            while j < chars.len() {
                match chars[j] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            inner.push(chars[i + 2..j.min(chars.len())].iter().collect());
            out.push_str("__subst__");
            i = j + 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    (out, inner)
}

/// Reads a `run:` block or a script. `action_dir` is the action folder `github.action_path` names, `tracked` the tracked files,
/// `project_of` the project id a `./dev ci run` argument names.
///
/// A shell variable assigned a tracked file or folder (`here=$(cd "$(dirname "$0")" && pwd)`, `r="<action>/../x.sh"`) stands for
/// it in later words. An invoked path that still holds a variable after that (`"$here/host/$step.sh"`, dynamic dispatch) cannot be
/// told exactly, so it invokes every tracked file its literal parts match (a superset, never a guess that drops one).
pub fn invoked(script: &str, action_dir: Option<&str>, tracked: &BTreeSet<String>, project_of: &dyn Fn(&str) -> Option<String>) -> Invoked {
    invoked_env(script, action_dir, tracked, project_of, &BTreeMap::new())
}

/// [`invoked`] with the `env:` of the workflow, job and step (a variable that names a tracked file or folder stands for it), and
/// with `source`/`.` scripts and the commands inside `$( … )` read too (review P1-4).
pub fn invoked_env(
    script: &str,
    action_dir: Option<&str>,
    tracked: &BTreeSet<String>,
    project_of: &dyn Fn(&str) -> Option<String>,
    env: &BTreeMap<String, String>,
) -> Invoked {
    let mut out = Invoked::default();
    let mut text = script.to_string();
    if let Some(dir) = action_dir {
        for v in ["${{ github.action_path }}", "${GITHUB_ACTION_PATH}", "$GITHUB_ACTION_PATH"] {
            text = text.replace(v, dir);
        }
    }
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
    // Any other `$( … )` runs its commands: they become lines of their own, and the outer word a placeholder. Comment lines and
    // here-document bodies are text, not commands.
    let mut lines: Vec<String> = Vec::new();
    let mut heredoc: Option<String> = None;
    for l in text.lines() {
        if let Some(end) = &heredoc {
            if l.trim() == end {
                heredoc = None;
            }
            continue;
        }
        if l.trim_start().starts_with('#') {
            continue;
        }
        if let Some(i) = l.find("<<") {
            let tag: String = l[i + 2..]
                .trim_start_matches('-')
                .trim_start()
                .trim_matches(|c| c == '\'' || c == '"')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !tag.is_empty() {
                heredoc = Some(tag);
            }
        }
        let (outer, inner) = substitutions(l);
        lines.push(outer);
        lines.extend(inner);
    }
    let text = lines.join("\n");
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
        if w.contains('$') || w.contains('*') {
            // Dynamic or a glob: every tracked file the literal parts match, in the folders the command could mean.
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
    // Variables that hold the runner's or a download's folder (`dl=$(mktemp -d)`, `bin=$HOME/...`): a path under them is no file
    // of the repository.
    let mut system: BTreeSet<String> = BTreeSet::new();
    for (k, v) in env {
        let mut v = v.trim_matches('"').to_string();
        if let Some(dir) = action_dir {
            v = v.replace("${{ github.action_path }}", dir);
        }
        let v = v.replace("${{ github.workspace }}/", "");
        if system_path(&v) {
            system.insert(k.clone());
        } else if let Some(p) = as_path(&v, "").filter(|_| v == "." || v.is_empty()) {
            vars.insert(k.clone(), p);
        } else if !v.contains('$') && (v.contains('/') || is_script(&v)) {
            // Kept as written: a relative value is relative to the folder the command runs in.
            vars.insert(k.clone(), v.trim_start_matches("./").to_string());
        }
    }
    // A path a command runs: tracked (resolved), dynamic or missing (unresolved, unless it is the system's).
    // `script_arg`: the word is an interpreter's script argument, so even a bare `$X` names a file.
    let run_path = |w: &str, cwd: &str, out: &mut Invoked, system: &BTreeSet<String>, script_arg: bool| {
        let found = resolve(w, cwd);
        let under_system = system.iter().any(|v| {
            [format!("${v}"), format!("${{{v}}}")]
                .iter()
                .any(|f| w == f || w.starts_with(&format!("{f}/")))
        });
        let pathlike = looks_like_path(w) || (script_arg && w.starts_with('$') && !w.contains(['@', '*', '#']));
        if found.is_empty() && pathlike && !system_path(w) && !under_system {
            out.unresolved.insert(w.to_string());
        }
        out.scripts.extend(found);
    };
    for cmd in shell::commands(&text, Some(String::new())) {
        let cwd = cmd.cwd.clone().unwrap_or_default();
        let ws = words(&cmd.text);
        // Assignments in front of a command, or alone, may hold a path the command uses as `$name`.
        let mut i = 0;
        while ws
            .get(i)
            .is_some_and(|w| ["local", "export", "readonly", "declare", "typeset"].contains(&w.as_str()))
        {
            i += 1;
        }
        while let Some((k, v)) = ws
            .get(i)
            .and_then(|w| w.split_once('='))
            .filter(|(k, _)| is_name(k.trim_end_matches('+')))
        {
            let k = k.trim_end_matches('+');
            i += 1;
            // An array (`extra=(--config x.json)`): its words are values, not a command.
            if v.starts_with('(') {
                while !ws.get(i - 1).is_some_and(|w| w.ends_with(')')) && i < ws.len() {
                    i += 1;
                }
                continue;
            }
            let v = expand(v.trim_matches('"'), &vars);
            let from_system = system.iter().any(|s| {
                [format!("${s}"), format!("${{{s}}}")]
                    .iter()
                    .any(|f| v == *f || v.starts_with(&format!("{f}/")))
            });
            if let Some(p) = as_path(&v, &cwd) {
                vars.insert(k.to_string(), p);
            } else if system_path(&v) || v.starts_with("__subst__") || from_system || positional(&v) {
                // A runner folder, the output of a command (`$(mktemp -d)`), an argument (`$1`) or a default to an absolute path:
                // not a file of the repository.
                system.insert(k.to_string());
            }
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
        // Data a command names as an argument or a redirect target (every word after the program that names a tracked file
        // that is not a script).
        for w in rest.iter().skip(1) {
            let w = w.trim_start_matches(['<', '>']);
            if w.contains('/') || w.contains('.') {
                for p in [cwd.as_str(), ""].iter().filter_map(|d| shell::join(d, w.trim_start_matches("./"))) {
                    if tracked.contains(&p) && !is_script(&p) {
                        out.reads.insert(p);
                    }
                }
            }
        }
        if base == "source" || base == "." {
            if let Some(arg) = rest.get(1) {
                run_path(arg, &cwd, &mut out, &system, true);
            }
            continue;
        }
        if INTERPRETERS.contains(&base.as_str()) {
            let mut k = 1;
            while let Some(a) = rest.get(k) {
                if a == "-o" || a == "-O" {
                    k += 2;
                } else if a == "-c"
                    || a == "-"
                    || (base == "node" && (["-e", "--eval", "-p", "--print"].contains(&a.as_str()) || a.starts_with("--eval=")))
                    || (base.starts_with("python") && a == "-m")
                {
                    // Code on the command line or a module by name: no script file.
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
                run_path(arg, &cwd, &mut out, &system, true);
            }
            continue;
        }
        // A program named by a path (`./x.sh`, `conformance/ci/y.mjs`, `"$here/z.sh"`); a bare name is a program on PATH.
        if program.contains('/') {
            run_path(&program, &cwd, &mut out, &system, false);
        } else {
            out.scripts.extend(resolve(&program, &cwd));
        }
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

/// The local action a `uses:` names (`./<actions>/<a>`).
fn local_action(p: &Paths, uses: &str) -> Option<String> {
    // `./<checkout>/<actions>/<a>`: a reusable workflow names Paddock's own actions through the checkout an app repository keeps
    // Paddock in (`[paths]` `checkout`).
    let rest = uses.strip_prefix("./").unwrap_or(uses);
    let rest = rest.strip_prefix(p.checkout.as_str()).unwrap_or(rest);
    rest.strip_prefix(p.actions.as_str()).map(|a| a.trim_end_matches('/').to_string())
}

/// The values every caller passes to each input (`with:`) of the local actions (`action:<a>`) and reusable workflows
/// (`wf:<file>`): `${{ inputs.<name> }}` inside them stands for each of these values in turn.
fn caller_inputs(p: &Paths, docs: &[&yaml::Node]) -> BTreeMap<String, BTreeMap<String, BTreeSet<String>>> {
    let mut out: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    let mut add = |key: String, with: Option<&yaml::Node>| {
        for (k, v) in with.map(yaml::Node::entries).unwrap_or_default() {
            if let Some(t) = v.text() {
                out.entry(key.clone())
                    .or_default()
                    .entry(k.clone())
                    .or_default()
                    .insert(t.to_string());
            }
        }
    };
    for root in docs {
        let mut step_lists: Vec<&[yaml::Node]> = Vec::new();
        if let Some(steps) = root.get("runs").and_then(|r| r.get("steps")) {
            step_lists.push(steps.items());
        }
        for (_, job) in root.get("jobs").map(yaml::Node::entries).unwrap_or_default() {
            if let Some(w) = job.get_text("uses").and_then(|u| u.strip_prefix("./")) {
                add(workflow_id(w), job.get("with"));
            }
            if let Some(steps) = job.get("steps") {
                step_lists.push(steps.items());
            }
        }
        for steps in step_lists {
            for st in steps {
                if let Some(a) = st.get_text("uses").and_then(|u| local_action(p, u)) {
                    add(action_id(&a), st.get("with"));
                }
            }
        }
    }
    out
}

/// The variants of a step's script and env with each `${{ inputs.<name> }}` replaced by each value a caller passes (at most 16;
/// a name no caller passes stays as written, so a path through it is unresolved).
fn input_variants(
    script: &str,
    env: &BTreeMap<String, String>,
    inputs: Option<&BTreeMap<String, BTreeSet<String>>>,
) -> Vec<(String, BTreeMap<String, String>)> {
    let mut variants = vec![(script.to_string(), env.clone())];
    for (name, values) in inputs.into_iter().flatten() {
        let forms = [format!("${{{{ inputs.{name} }}}}"), format!("${{{{inputs.{name}}}}}")];
        let mentioned = |t: &str| forms.iter().any(|f| t.contains(f.as_str()));
        if !mentioned(script) && !env.values().any(|v| mentioned(v)) {
            continue;
        }
        let mut next = Vec::new();
        for (sc, en) in &variants {
            for v in values {
                let sub = |t: &str| forms.iter().fold(t.to_string(), |acc, f| acc.replace(f.as_str(), v));
                next.push((sub(sc), en.iter().map(|(k, x)| (k.clone(), sub(x))).collect()));
            }
        }
        next.truncate(16);
        variants = next;
    }
    variants
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

/// The `env:` map of a workflow, job or step: names to their one-line values.
fn env_of(n: &yaml::Node) -> BTreeMap<String, String> {
    n.get("env")
        .map(yaml::Node::entries)
        .unwrap_or_default()
        .iter()
        .filter_map(|(k, v)| v.text().map(|t| (k.clone(), t.to_string())))
        .collect()
}

/// A workflow's or job's `defaults.run.working-directory` (`.` is the root).
fn default_dir(n: &yaml::Node) -> Option<&str> {
    n.get("defaults")?
        .get("run")?
        .get_text("working-directory")
        .map(|d| if d == "." { "" } else { d })
}

/// The data reads and unresolved paths of what a node runs: a data node per read file with a `reads` edge from `node`, and an
/// unresolved reference held by `node` (its users run in full, ADR 0117 §4) for each path that resolves to nothing tracked.
fn record(x: &mut Extracted, node: &str, file: &str, inv: &Invoked) {
    for p in &inv.reads {
        let id = format!("data:{p}");
        if !x.nodes.iter().any(|n| n.id == id) {
            x.nodes.push(ModelNode::new(&id, NodeKind::Project, &[p.as_str()]));
        }
        x.edges.push(Edge::new(node, id, EdgeKind::Reads));
    }
    for u in &inv.unresolved {
        x.unresolved.push(Unresolved {
            referrer: file.to_string(),
            node: Some(node.to_string()),
            reference: u.clone(),
            why: "a path it runs resolves to no tracked file".into(),
        });
    }
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
        file: &str,
        env: &BTreeMap<String, String>,
        default_dir: &str,
        inputs: Option<&BTreeMap<String, BTreeSet<String>>>,
    ) {
        for (i, s) in steps.iter().enumerate() {
            let sid = format!("step:{prefix}/{i}");
            x.nodes.push(ModelNode::new(&sid, NodeKind::Step, &[]));
            x.edges.push(Edge::new(owner, &sid, EdgeKind::Uses));
            if let Some(a) = s.get_text("uses").and_then(|u| local_action(&self.decl.paths, u)) {
                x.edges.push(Edge::new(&sid, action_id(&a), EdgeKind::Uses));
                used.insert(a);
            }
            if let Some(run) = s.get_text("run") {
                let start = rooted(&self.decl.paths, s.get_text("working-directory").unwrap_or(default_dir));
                let script = if start.is_empty() {
                    run.to_string()
                } else {
                    format!("cd {start}\n{run}")
                };
                let mut env = env.clone();
                env.extend(env_of(s));
                // A variable that names the Paddock checkout itself (`[paths]` `checkout`) is the repository root, also when a
                // caller's input value (`paddock: .paddock`) is what names it: checked after the inputs are put in.
                let checkout = self.decl.paths.checkout.trim_end_matches('/');
                let mut inv = Invoked::default();
                for (script, mut env) in input_variants(&script, &env, inputs) {
                    for v in env.values_mut() {
                        if !checkout.is_empty() && v.trim_matches('"').replace("${{ github.workspace }}/", "") == checkout {
                            *v = ".".to_string();
                        }
                    }
                    // After the inputs are put in: a value a caller passes may spell the root too.
                    let one = invoked_env(
                        &rooted(&self.decl.paths, &script),
                        action_dir,
                        tracked,
                        &|w| self.project_of(w),
                        &env,
                    );
                    inv.scripts.extend(one.scripts);
                    inv.projects.extend(one.projects);
                    inv.reads.extend(one.reads);
                    inv.unresolved.extend(one.unresolved);
                }
                for p in &inv.scripts {
                    x.edges.push(Edge::new(&sid, script_id(p), EdgeKind::Runs));
                    x.edges.push(Edge::new(owner, script_id(p), EdgeKind::Runs));
                }
                record(x, &sid, file, &inv);
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
        let paths = &self.decl.paths;
        let workflows: Vec<String> = files.iter().filter(|f| is_workflow(paths, f)).cloned().collect();
        let actions: Vec<String> = files
            .iter()
            .filter(|f| action_of(paths, f).is_some() && (f.ends_with("/action.yml") || f.ends_with("/action.yaml")))
            .cloned()
            .collect();
        let shell_scripts: Vec<String> = files
            .iter()
            .filter(|f| f.ends_with(".sh") && (in_script_folder(paths, f) || action_of(paths, f).is_some()))
            .cloned()
            .collect();
        let contracts: Vec<String> = self
            .apps
            .iter()
            .map(|a| format!("{}{a}/{}", paths.apps, paths.contract))
            .chain(
                crate::ci_run::contract::ROOT_PROJECTS
                    .iter()
                    .map(|p| format!("{p}/{}", paths.contract)),
            )
            .filter(|c| tracked.contains(c))
            .collect();
        let wanted: Vec<String> = workflows
            .iter()
            .chain(&actions)
            .chain(&shell_scripts)
            .chain(&contracts)
            .cloned()
            .collect();
        // A text that cannot be read is an error (fail closed, review P1-5); a file that is not there is skipped below.
        let texts: BTreeMap<String, String> = texts_of("ci", read, &wanted)?;
        let bad = |file: &str, reason: String| ExtractError {
            extractor: "ci",
            file: file.to_string(),
            reason,
        };
        let parsed: Vec<yaml::Node> = workflows
            .iter()
            .chain(&actions)
            .filter_map(|f| texts.get(f))
            .filter_map(|t| yaml::parse(t).ok())
            .collect();
        let callers = caller_inputs(paths, &parsed.iter().collect::<Vec<_>>());
        let mut x = Extracted::default();
        let mut scripts: BTreeSet<String> = files
            .iter()
            .filter(|f| in_script_folder(paths, f) && is_script(f))
            .cloned()
            .collect();
        // Actions first: their own uses and runs, and their release and setup flags.
        let mut action_names: BTreeSet<String> = files.iter().filter_map(|f| action_of(paths, f)).map(str::to_string).collect();
        action_names.retain(|a| {
            tracked
                .iter()
                .any(|f| f.starts_with(&format!("{}/", action_dir(paths, a))) && !f.ends_with(".md"))
        });
        for a in &action_names {
            let mut n = ModelNode::new(action_id(a), NodeKind::Action, &[&format!("{}/", action_dir(paths, a))]);
            n.setup_only = self.decl.setup_only.contains(a);
            x.nodes.push(n);
        }
        for f in &actions {
            let Some(text) = texts.get(f) else { continue };
            let root = yaml::parse(text).map_err(|e| bad(f, e))?;
            let a = action_of(paths, f).unwrap_or_default().to_string();
            let dir = action_dir(paths, &a);
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
                f,
                &BTreeMap::new(),
                "",
                callers.get(&action_id(&a)),
            );
            for u in used {
                x.edges.push(Edge::new(action_id(&a), action_id(&u), EdgeKind::Uses));
            }
            for p in runs {
                x.edges.push(Edge::new(action_id(&a), p, EdgeKind::Runs));
            }
        }
        let mut reusers: Vec<(String, String, String)> = Vec::new();
        for f in &workflows {
            let Some(text) = texts.get(f) else { continue };
            let root = yaml::parse(text).map_err(|e| bad(f, e))?;
            let wf = workflow_id(f);
            x.nodes.push(ModelNode::new(&wf, NodeKind::Workflow, &[f.as_str()]));
            let app = self.app_of_workflow(f);
            let wf_env = env_of(&root);
            for (job, body) in root.get("jobs").map(yaml::Node::entries).unwrap_or_default() {
                let jid = job_id(f, job);
                let (mut used, mut runs) = (BTreeSet::new(), BTreeSet::new());
                let steps = body.get("steps").map(yaml::Node::items).unwrap_or_default();
                let mut env = wf_env.clone();
                env.extend(env_of(body));
                let dir = default_dir(body).or_else(|| default_dir(&root)).unwrap_or_default().to_string();
                self.steps(
                    &mut x,
                    &jid,
                    &format!("{}#{job}", f.rsplit('/').next().unwrap_or(f)),
                    steps,
                    None,
                    &tracked,
                    &mut used,
                    &mut runs,
                    f,
                    &env,
                    &dir,
                    callers.get(&workflow_id(f)),
                );
                let callee = body.get_text("uses").and_then(|u| u.strip_prefix("./")).map(workflow_id);
                if let Some(w) = &callee {
                    x.edges.push(Edge::new(&jid, w, EdgeKind::Uses));
                }
                let mut node = ModelNode::new(&jid, NodeKind::Job, &[]);
                node.release = used.iter().any(|a| self.decl.release_actions.contains(a));
                // The steps of a release job that use an action (bundle, sign, notarise, publish, and the actions beside them) are
                // release steps: an action one of them uses reaches the job as release (its installers). A `run:` step is a check
                // of the release (a gate: the app's smoke set and checks phase, rule 54 narrowed, user decision 2026-11-23).
                if node.release {
                    let prefix = format!("step:{}#{job}/", f.rsplit('/').next().unwrap_or(f));
                    let action_steps: BTreeSet<String> = x
                        .edges
                        .iter()
                        .filter(|e| e.kind == EdgeKind::Uses && e.from.starts_with(&prefix) && e.to.starts_with("action:"))
                        .map(|e| e.from.clone())
                        .collect();
                    for n in x.nodes.iter_mut().filter(|n| action_steps.contains(&n.id)) {
                        n.release = true;
                    }
                }
                x.nodes.push(node);
                x.edges.push(Edge::new(&wf, &jid, EdgeKind::Uses));
                for out in plan_outputs(body.get_text("if").unwrap_or("")) {
                    runs.extend(self.output_projects(&out));
                }
                // A job that calls a reusable workflow runs for its app through that workflow's own jobs (ci.yml's app job
                // runs every app): it adds its app only when the callee releases (bundles, signs, publishes), and is then a
                // release job itself (fixed up below, once every workflow's release jobs are known).
                match (&app, &callee) {
                    (Some(a), Some(w)) => reusers.push((jid.clone(), w.clone(), a.clone())),
                    (Some(a), None) => {
                        runs.insert(a.clone());
                    }
                    _ => {}
                }
                for p in runs {
                    x.edges.push(Edge::new(&jid, p, EdgeKind::Runs));
                }
            }
        }
        // The callers of a reusable workflow that has a release job: release jobs that run their app (its installers); the
        // other callers add nothing of their own (a conformance change reaches ci.yml's conformance job, not every app's suite
        // through dyno-release.yml's gates job, difference 4 of §8).
        let releasing: BTreeSet<String> = x
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Job && n.release)
            .filter_map(|n| {
                n.id.strip_prefix("job:")
                    .and_then(|r| r.split_once('#'))
                    .map(|(w, _)| workflow_id(w))
            })
            .collect();
        for (jid, callee, app) in reusers {
            if releasing.contains(&callee) {
                if let Some(n) = x.nodes.iter_mut().find(|n| n.id == jid) {
                    n.release = true;
                }
                x.edges.push(Edge::new(&jid, app, EdgeKind::Runs));
            }
        }
        // Every command list of every contract (its phases, run by app-gates or `./dev ci run`): a step node per phase that runs its
        // project and the scripts its lines invoke, with its data reads and unresolved paths (review P1-4).
        for c in &contracts {
            let Some(text) = texts.get(c) else { continue };
            let v: serde_json::Value = serde_json::from_str(text).map_err(|e| bad(c, e.to_string()))?;
            for phase in crate::ci_run::contract::phases(&v) {
                let lines: Vec<&str> = v[phase]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_str)
                    .collect();
                let inv = invoked(&rooted(paths, &lines.join("\n")), None, &tracked, &|w| self.project_of(w));
                if inv.scripts.is_empty() && inv.reads.is_empty() && inv.unresolved.is_empty() {
                    continue;
                }
                // The step's home is its project's folder: a change it reaches selects this phase of the project, not the
                // project's suite (ADR 0117 §6, difference 4 of §8: a conformance change runs each app's conformance step).
                let sid = format!("step:{c}#{phase}");
                let home = c.strip_suffix(&format!("/{}", paths.contract)).unwrap_or(c);
                x.nodes.push(ModelNode::new(&sid, NodeKind::Step, &[]).with_home(home));
                for p in &inv.scripts {
                    x.edges.push(Edge::new(&sid, script_id(p), EdgeKind::Runs));
                    scripts.insert(p.clone());
                }
                record(&mut x, &sid, c, &inv);
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
            if action_of(paths, s).is_none() {
                x.nodes.push(ModelNode::new(script_id(s), NodeKind::Script, &[s.as_str()]));
            }
            if let Some(text) = texts.get(s) {
                let dir = s.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                let inv = invoked(&rooted(paths, &script_dir_named(text, dir)), None, &tracked, &|w| {
                    self.project_of(w)
                });
                let from = if action_of(paths, s).is_some() {
                    action_id(action_of(paths, s).unwrap_or_default())
                } else {
                    script_id(s)
                };
                for p in inv.scripts.iter().filter(|p| *p != s) {
                    x.edges.push(Edge::new(&from, script_id(p), EdgeKind::ReadsScript));
                }
                record(&mut x, &from, s, &inv);
            }
        }
        // An edge to a script inside an action folder goes to the action.
        for e in &mut x.edges {
            if let Some(a) = e.to.strip_prefix("script:").and_then(|s| action_of(paths, s)) {
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
