//! `dev graph affected`: which projects a change reaches. The oracle-tested port of the core of `scripts/affected.mjs`
//! (`affected()`, `owners()`, `appOfWorkflow()` and the rule tables); the graph itself comes from `paddock-repo-graph`.
//! The CI matrix, the nextest narrowing and `--ci` are R4's; `affected.mjs` stays CI's planner until then.
//!
//! Paddock's own rules (which paths are global inputs, which markdown is read by code, who owns a folder) live here and not
//! in `paddock-repo-graph`: they are this repository's, the graph crate is generic (ADR 0022 §6, "CI-only").

use paddock_cli::{CliError, Report};
use paddock_repo_graph::{CargoCli, FileSource, GitHistory, Graph, GraphError, History, cargo_graph, lock_diff, npm_graph, real_path};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(120);
const PROJECTS: [&str; 8] = ["crates", "ui", "eslint", "conformance", "tools", "icons", "ci-image", "paddock-dev"];

fn re(pattern: &str) -> Regex {
    // The patterns are literals of this file, checked by the tests below.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("a valid pattern")
}

/// Inputs that every project's build or tests read: a change re-runs everything. Mirrors `GLOBAL` in `scripts/affected.mjs`
/// (its comment table says what each former global reaches). `.gitea/` and `conformance/(ci|release|src/util)/` stay global here: the
/// script narrows them per job from the file text at two revisions, which this port does not read, so it answers with a
/// superset (never less than the script; `--files` mode of the script answers everything for them too unless they are dirty).
static GLOBAL: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"^\.gitea/",
        r"^rust-toolchain(\.toml)?$",
        r"^package\.json$",
        r"^\.npmrc$",
        r"^\.nvmrc$",
        r"^\.gitattributes$",
        r"^tsconfig/",
        r"^conformance/(ci|release|src/util)/",
    ]
    .map(re)
    .into()
});

/// Inputs of the Rust builds only (cargo registry and linker config, the shared clippy and workspace lint presets).
static RUST_INPUTS: LazyLock<Vec<Regex>> = LazyLock::new(|| [r"^\.cargo/", r"^lint/"].map(re).into());

/// The planner and its tests: besides the scripts' owners, this tool's golden test runs them as its oracle.
static PLANNER_FILES: LazyLock<Regex> = LazyLock::new(|| re(r"^scripts/affected(\.test)?\.mjs$"));

/// What conformance C91 and paddock-service's `per_machine` sync test read of a per-machine app (scripts/affected.mjs
/// `PER_MACHINE_INPUT`).
static PER_MACHINE_INPUT: LazyLock<Regex> = LazyLock::new(|| {
    re(r"^(apps/[^/]+/app/src-tauri/(tauri(\.windows)?\.conf\.json|windows/)|crates/paddock-setup/nsis/paddock-per-machine\.nsh)")
});

/// Editor and VCS metadata: marks nothing.
static META: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [r"(^|/)\.gitignore$", r"^\.editorconfig$", r"(^|/)\.DS_Store$", r"^lefthook\.yml$"]
        .map(re)
        .into()
});

/// Documentation: marks only the link check, except declared inputs.
static DOCS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [r"\.md$", r"^docs/", r"^apps/[^/]+/docs/", r"^diagrams/", r"(^|/)LICENSE[^/]*$"]
        .map(re)
        .into()
});

/// What a declared doc input marks.
enum Marks {
    /// Its owner.
    Owner,
    /// These projects.
    Projects(&'static [&'static str]),
    /// An app's file marks its owner, any other the conformance project (the image reviews).
    Reviewed,
}

/// Markdown that code or tests read, a declared contract.
static DOC_INPUTS: LazyLock<Vec<(Regex, Marks)>> = LazyLock::new(|| {
    vec![
        (re(r"^(apps/[^/]+|crates/[^/]+|ui/projects/[^/]+)/CHANGELOG\.md$"), Marks::Owner),
        (
            re(r"(^|/)(resources|samples|src|tests|test|templates|e2e|fixtures)/.*\.md$"),
            Marks::Owner,
        ),
        (re(r"^docs/rules\.md$"), Marks::Projects(&["conformance", "tools"])),
        (re(r"^docs/blocks/"), Marks::Projects(&["conformance", "tools"])),
        (re(r"^apps/(shell-demo|workbench-demo)/README\.md$"), Marks::Projects(&["tools"])),
        (re(r"(^|/)REVIEWED\.md$"), Marks::Reviewed),
        (re(r"^ui/projects/paddock-ui/format/src/storage-cases\.json$"), Marks::Projects(&["ui", "app:dyno"])),
        (re(r"^apps/debrief/docs/ai/sql-views\.md$"), Marks::Owner),
        (re(r"^apps/debrief/docs/testing/quarantine\.md$"), Marks::Owner),
    ]
});

/// Top-level folders and the projects that own them.
static OWNERS: LazyLock<Vec<(Regex, &'static [&'static str])>> = LazyLock::new(|| {
    vec![
        (re(r"^crates/"), &["crates"][..]),
        (re(r"^ui/"), &["ui"]),
        (re(r"^eslint/"), &["eslint"]),
        (re(r"^conformance/"), &["conformance"]),
        (re(r"^paddock\.(conformance\.json|waivers\.toml)$"), &["conformance"]),
        (re(r"^(tools/paddock-dev/|dev$|dev\.cmd$)"), &["paddock-dev"]),
        (re(r"^(tools|templates)/"), &["tools"]),
        (re(r"^scripts/"), &["tools", "ui"]),
        (re(r"^icons/"), &["icons"]),
        (re(r"^ci-image/"), &["ci-image", "ui"]),
        (re(r"^(renovate/|renovate\.json$|CODEOWNERS$|AGENTS\.md$)"), &["tools"]),
    ]
});

/// The projects that own a path before dependencies, or `None` for an unknown root file.
pub fn owners(path: &str) -> Option<Vec<String>> {
    if let Some(rest) = path.strip_prefix("apps/")
        && let Some((app, _)) = rest.split_once('/')
    {
        return Some(vec![format!("app:{app}")]);
    }
    OWNERS
        .iter()
        .find(|(r, _)| r.is_match(path))
        .map(|(_, p)| p.iter().map(|s| s.to_string()).collect())
}

/// The app a workflow belongs to: `release-<app>.yml`, `<app>.yml` or `<app>-<anything>.yml` for a known app.
pub fn app_of_workflow<'a>(path: &str, apps: &'a BTreeSet<String>) -> Option<&'a str> {
    static WORKFLOW: LazyLock<Regex> = LazyLock::new(|| re(r"^\.gitea/workflows/([a-z0-9-]+)\.ya?ml$"));
    let name = WORKFLOW.captures(path)?.get(1)?.as_str();
    let name = name.strip_prefix("release-").unwrap_or(name);
    apps.iter()
        .filter(|a| name == a.as_str() || name.starts_with(&format!("{a}-")))
        .max_by_key(|a| a.len())
        .map(String::as_str)
}

fn project_of_dir(dir: &str) -> Option<String> {
    owners(&format!("{dir}/x")).and_then(|o| o.into_iter().next())
}

/// The graphs a plan is computed from.
pub struct RepoGraph {
    /// Rust crates.
    pub cargo: Graph,
    /// npm packages.
    pub npm: Graph,
    /// The apps (folders with a `paddock.ci.json`).
    pub apps: BTreeSet<String>,
}

/// What changed besides the file list.
#[derive(Default)]
pub struct Options {
    /// Plan everything.
    pub all: bool,
    /// Give the reason for every project.
    pub explain: bool,
    /// The root `package-lock.json` before and after.
    pub lock: (Option<String>, Option<String>),
    /// The `apps/<app>/app/src-tauri` folders that adopt the shared per-machine installer include (see
    /// [`per_machine_apps`]); read from the tree by [`plan_for`].
    pub per_machine_apps: Vec<String>,
}

/// The apps whose Windows installer adopts the shared include: hooks that include `paddock-per-machine.nsh`, or a
/// `perMachine` install mode in their Tauri configuration (scripts/affected.mjs `adoptsPerMachine`).
pub fn per_machine_apps(root: &Path) -> Vec<String> {
    let Ok(apps) = std::fs::read_dir(root.join("apps")) else {
        return Vec::new();
    };
    let mut out: Vec<String> = apps
        .flatten()
        .filter_map(|a| {
            let dir = format!("apps/{}/app/src-tauri", a.file_name().to_string_lossy());
            let read = |p: &str| std::fs::read_to_string(root.join(&dir).join(p)).unwrap_or_default();
            let per_machine = read("tauri.windows.conf.json") + &read("tauri.conf.json");
            let adopts = read("windows/installer-hooks.nsh").contains("paddock-per-machine.nsh")
                || Regex::new(r#""installMode"\s*:\s*"perMachine""#).is_ok_and(|r| r.is_match(&per_machine));
            adopts.then_some(dir)
        })
        .collect();
    out.sort();
    out
}

#[derive(Default)]
struct State {
    why: BTreeMap<String, Vec<String>>,
    kinds: BTreeMap<String, BTreeSet<&'static str>>,
}

impl State {
    fn mark(&mut self, project: Option<String>, reason: String) {
        if let Some(p) = project {
            self.why.entry(p).or_default().push(reason);
        }
    }

    fn kind(&mut self, project: Option<&str>, half: &str) {
        let Some(p) = project.filter(|p| p.starts_with("app:")) else {
            return;
        };
        let set = self.kinds.entry(p.to_string()).or_default();
        match half {
            "all" => set.extend(["rust", "ui"]),
            "rust" => {
                set.insert("rust");
            }
            _ => {
                set.insert("ui");
            }
        }
    }
}

/// The plan for a list of changed files, in the JSON shape of `scripts/affected.mjs`.
pub fn affected(files: &[String], graph: &RepoGraph, opts: &Options) -> Result<Value, GraphError> {
    let mut st = State::default();
    let every: Vec<String> = PROJECTS
        .iter()
        .map(|s| s.to_string())
        .chain(graph.apps.iter().map(|a| format!("app:{a}")))
        .collect();
    let mut everything = opts.all;
    let mut docs = false;
    let mut changed_crates: Vec<String> = Vec::new();
    let mut changed_pkgs: Vec<String> = Vec::new();
    let push_unique = |v: &mut Vec<String>, s: &str| {
        if !v.iter().any(|x| x == s) {
            v.push(s.to_string());
        }
    };
    let mark_all = |st: &mut State, reason: &str, everything: &mut bool| {
        *everything = true;
        for p in &every {
            st.mark(Some(p.clone()), reason.to_string());
            st.kind(Some(p), "all");
        }
    };
    if opts.all {
        mark_all(&mut st, "everything (no base)", &mut everything);
    }
    for f in if opts.all { &[][..] } else { files } {
        if let Some(app) = app_of_workflow(f, &graph.apps) {
            st.mark(Some(format!("app:{app}")), format!("its workflow {f}"));
            st.kind(Some(&format!("app:{app}")), "all");
            continue;
        }
        if META.iter().any(|r| r.is_match(f)) {
            continue;
        }
        if PLANNER_FILES.is_match(f) {
            st.mark(
                Some("paddock-dev".to_string()),
                format!("its golden test compares {f} (tools/paddock-dev/tests/it/golden.rs)"),
            );
        }
        if PER_MACHINE_INPUT.is_match(f) {
            st.mark(Some("conformance".to_string()), format!("C91 reads {f}"));
            if f.starts_with("crates/") || opts.per_machine_apps.iter().any(|d| f.starts_with(&format!("{d}/"))) {
                st.mark(Some("crates".to_string()), format!("the per_machine sync test reads {f}"));
            }
        }
        if RUST_INPUTS.iter().any(|r| r.is_match(f)) {
            for p in ["crates", "paddock-dev"] {
                st.mark(Some(p.to_string()), format!("rust input {f}"));
            }
            if f.starts_with("lint/") {
                st.mark(Some("conformance".to_string()), format!("C13 reads {f}"));
            }
            for a in &graph.apps {
                if graph.cargo.iter().any(|(d, _)| d.starts_with(&format!("apps/{a}/"))) {
                    st.mark(Some(format!("app:{a}")), format!("rust input {f}"));
                    st.kind(Some(&format!("app:{a}")), "rust");
                }
            }
            continue;
        }
        if GLOBAL.iter().any(|r| r.is_match(f)) {
            mark_all(&mut st, &format!("global {f}"), &mut everything);
            continue;
        }
        if f == "package-lock.json" {
            for ws in lock_diff(opts.lock.0.as_deref(), opts.lock.1.as_deref())? {
                for p in owners(&format!("{ws}/x")).unwrap_or_default() {
                    st.mark(Some(p.clone()), format!("lock {ws}"));
                    st.kind(Some(&p), "ui");
                }
                if graph.npm.contains(&ws) {
                    push_unique(&mut changed_pkgs, &ws);
                }
            }
            continue;
        }
        let input = DOC_INPUTS.iter().find(|(r, _)| r.is_match(f)).map(|(_, m)| m);
        if DOCS.iter().any(|r| r.is_match(f)) && input.is_none() {
            docs = true;
            continue;
        }
        let own = match input {
            Some(Marks::Projects(p)) => Some(p.iter().map(|s| s.to_string()).collect()),
            Some(Marks::Reviewed) if f.starts_with("apps/") => owners(f),
            Some(Marks::Reviewed) => Some(vec!["conformance".to_string()]),
            Some(Marks::Owner) | None => owners(f),
        };
        let Some(own) = own else {
            mark_all(&mut st, &format!("unknown root file {f}"), &mut everything);
            continue;
        };
        for p in &own {
            st.mark(Some(p.clone()), f.clone());
        }
        let c = graph.cargo.node_of(f).map(str::to_string);
        let n = graph.npm.node_of(f).map(str::to_string);
        if let Some(c) = &c
            && project_of_dir(c).is_some_and(|p| own.contains(&p))
        {
            push_unique(&mut changed_crates, c);
        }
        if let Some(n) = &n
            && project_of_dir(n).is_some_and(|p| own.contains(&p))
        {
            push_unique(&mut changed_pkgs, n);
        }
        let longer = |a: &Option<String>, b: &Option<String>| a.as_ref().is_some_and(|a| b.as_ref().is_none_or(|b| a.len() > b.len()));
        let half = if input.is_some() {
            "all"
        } else if c.is_some() && !longer(&n, &c) {
            "rust"
        } else if n.is_some() && !longer(&c, &n) && !f.contains("/src-tauri/") {
            "ui"
        } else {
            "all"
        };
        for p in &own {
            st.kind(Some(p), half);
        }
        if c.is_none() && own.iter().any(|p| p == "crates") {
            for (d, x) in graph.cargo.iter() {
                if x.workspace == "crates" {
                    push_unique(&mut changed_crates, d);
                }
            }
        }
        if own.iter().any(|p| p == "ui") && f.starts_with("ui/") {
            push_unique(&mut changed_pkgs, "ui");
        }
        if let Some(rest) = f.strip_prefix("icons/") {
            let sub = rest.split('/').next().unwrap_or("");
            let targets: Vec<&String> = if graph.apps.contains(sub) {
                graph.apps.iter().filter(|a| a.as_str() == sub).collect()
            } else if sub == "tools" || sub == "package.json" {
                graph.apps.iter().collect()
            } else {
                Vec::new()
            };
            for a in targets {
                st.mark(Some(format!("app:{a}")), format!("icons {f}"));
                st.kind(Some(&format!("app:{a}")), "rust");
            }
        }
        if f.starts_with("conformance/") && !f.starts_with("conformance/test/") && !f.starts_with("conformance/fixtures/") {
            for a in &graph.apps {
                st.mark(Some(format!("app:{a}")), format!("conformance {f}"));
                st.kind(Some(&format!("app:{a}")), "all");
            }
        }
    }

    let crates = graph.cargo.dependents(&changed_crates.iter().cloned().collect());
    let mut sorted_changed = changed_crates.clone();
    sorted_changed.sort();
    for d in crates.iter().filter(|d| !changed_crates.contains(d)) {
        let p = project_of_dir(d);
        st.mark(p.clone(), format!("cargo {d} reaches {}", sorted_changed.join(", ")));
        st.kind(p.as_deref(), "rust");
    }
    for d in graph
        .npm
        .dependents(&changed_pkgs.iter().cloned().collect())
        .iter()
        .filter(|d| !changed_pkgs.contains(d))
    {
        let p = project_of_dir(d);
        st.mark(p.clone(), format!("npm {d}"));
        st.kind(p.as_deref(), if d.ends_with("src-tauri") { "all" } else { "ui" });
    }

    let has = |p: &str| st.why.contains_key(p);
    let mut packages: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for d in &crates {
        if let Some(n) = graph.cargo.get(d) {
            packages.entry(n.workspace.clone()).or_default().push(n.name.clone());
        }
    }
    packages.values_mut().for_each(|v| v.sort());
    let app_names: Vec<String> = st
        .why
        .keys()
        .filter_map(|p| p.strip_prefix("app:"))
        .filter(|a| graph.apps.contains(*a))
        .map(str::to_string)
        .collect();
    let mut out = Map::new();
    out.insert("all".into(), json!(everything));
    for (key, project) in [
        ("crates", "crates"),
        ("ui", "ui"),
        ("eslint", "eslint"),
        ("conformance", "conformance"),
        ("tools", "tools"),
        ("icons", "icons"),
        ("ciImage", "ci-image"),
        ("paddockDev", "paddock-dev"),
    ] {
        out.insert(key.into(), json!(has(project)));
    }
    out.insert("docs".into(), json!(docs));
    out.insert("apps".into(), json!(app_names));
    let kinds: Map<String, Value> = st
        .kinds
        .iter()
        .filter(|(p, _)| p.strip_prefix("app:").is_some_and(|a| graph.apps.contains(a)))
        .map(|(p, k)| (p["app:".len()..].to_string(), json!(k.iter().collect::<Vec<_>>())))
        .collect();
    out.insert("kinds".into(), Value::Object(kinds));
    out.insert("packages".into(), json!(packages));
    if opts.explain {
        out.insert("why".into(), json!(st.why));
    }
    Ok(Value::Object(out))
}

// ---------------------------------------------------------------------------------------------------------------------
// The repository reader (adapters over the working tree and git).

/// Files of the working tree under `root`.
pub struct DiskFiles(pub PathBuf);

impl FileSource for DiskFiles {
    fn read_to_string(&self, path: &str) -> Result<String, GraphError> {
        std::fs::read_to_string(self.0.join(path)).map_err(|e| GraphError::Read {
            path: path.to_string(),
            reason: e.to_string(),
        })
    }
}

/// The apps among the tracked files: `apps/<name>/paddock.ci.json`.
pub fn apps_of(files: &[String]) -> BTreeSet<String> {
    files
        .iter()
        .filter_map(|f| f.strip_prefix("apps/")?.strip_suffix("/paddock.ci.json"))
        .filter(|name| !name.contains('/'))
        .map(str::to_string)
        .collect()
}

/// Reads the graphs of the repository at `root` (tracked files only, as `git ls-files` lists them).
pub fn read_graph(root: &Path, history: &dyn History) -> Result<RepoGraph, GraphError> {
    let files = history.tracked_files()?;
    // The root in the spelling cargo prints its paths in (a temp folder's short name or a symlinked workspace is not).
    let root = real_path(root);
    let root = root.as_path();
    let disk = DiskFiles(root.to_path_buf());
    let root_text = root.to_string_lossy().replace('\\', "/");
    let cargo = cargo_graph(&files, &disk, &CargoCli::new(root, TIMEOUT), &root_text)?;
    Ok(RepoGraph {
        cargo,
        npm: npm_graph(&files, &disk)?,
        apps: apps_of(&files),
    })
}

/// What `dev graph affected` was asked.
pub struct Request {
    /// Explicit changed files.
    pub files: Vec<String>,
    /// Everything.
    pub all: bool,
    /// Reasons.
    pub explain: bool,
    /// The repository root, or found from the current folder.
    pub root: Option<PathBuf>,
    /// The base revision.
    pub base: Option<String>,
    /// The head revision.
    pub head: String,
}

/// The plan for a request against the repository at `root`.
pub fn plan_for(root: &Path, history: &dyn History, req: &Request) -> Result<Value, GraphError> {
    let graph = read_graph(root, history)?;
    let mut opts = Options {
        all: req.all,
        explain: req.explain,
        lock: (None, None),
        per_machine_apps: per_machine_apps(root),
    };
    let mut changed = req.files.clone();
    if let (Some(base), false) = (&req.base, req.all) {
        changed = history.changed_files(base, &req.head)?;
        if changed.iter().any(|f| f == "package-lock.json") {
            opts.lock = (
                history.file_at(base, "package-lock.json")?,
                history.file_at(&req.head, "package-lock.json")?,
            );
        }
    }
    affected(&changed, &graph, &opts)
}

fn cli_error(e: GraphError) -> CliError {
    CliError::failure(e.to_string())
}

/// `dev graph affected`.
pub fn command(req: &Request) -> Result<Report, CliError> {
    if req.files.is_empty() && req.base.is_none() && !req.all {
        return Err(CliError::usage("No base, --files or --all given.").with_hint("Pass a base revision, --files a,b or --all."));
    }
    let root = match &req.root {
        Some(r) => r.clone(),
        None => {
            let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
            let out = crate::input_hash::Exec::run(&crate::input_hash::Real, "git", &["rev-parse", "--show-toplevel"], &cwd, None)?;
            PathBuf::from(out.trim())
        }
    };
    let history = GitHistory::new(&root, TIMEOUT);
    let plan = plan_for(&root, &history, req).map_err(cli_error)?;
    Ok(Report::new(plan.clone(), plan.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_repo_graph::Node;

    fn node(name: &str, workspace: &str, deps: &[&str]) -> Node {
        Node {
            name: name.into(),
            workspace: workspace.into(),
            deps: deps.iter().map(|d| d.to_string()).collect(),
        }
    }

    fn sample() -> RepoGraph {
        let mut cargo = Graph::new();
        cargo.insert("crates/core", node("paddock-core", "crates", &[]));
        cargo.insert("crates/ui-kit", node("paddock-kit", "crates", &["crates/core"]));
        cargo.insert("apps/dyno/app/src-tauri", node("dyno", "apps/dyno/app/src-tauri", &["crates/kit"]));
        cargo.insert("crates/kit", node("paddock-k", "crates", &["crates/core"]));
        let mut npm = Graph::new();
        npm.insert("ui", node("@d/ui", "", &[]));
        npm.insert("apps/dyno/app", node("dyno-ui", "", &["ui"]));
        RepoGraph {
            cargo,
            npm,
            apps: ["dyno", "debrief"].map(String::from).into(),
        }
    }

    fn plan(files: &[&str]) -> Value {
        let files: Vec<String> = files.iter().map(|s| s.to_string()).collect();
        affected(
            &files,
            &sample(),
            &Options {
                explain: true,
                ..Options::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn rule_tables_compile_and_own_paths() {
        assert_eq!(owners("apps/dyno/x"), Some(vec!["app:dyno".to_string()]));
        assert_eq!(owners("scripts/a.mjs"), Some(vec!["tools".to_string(), "ui".to_string()]));
        assert_eq!(owners("tools/paddock-dev/src/lib.rs"), Some(vec!["paddock-dev".to_string()]));
        assert_eq!(owners("dev"), Some(vec!["paddock-dev".to_string()]));
        assert_eq!(owners("tools/bootstrap.sh"), Some(vec!["tools".to_string()]));
        assert_eq!(owners("Cargo.lock.unknown"), None);
        assert_eq!(owners("apps/loose-file"), None);
        let apps: BTreeSet<String> = ["debrief", "debrief-ai"].map(String::from).into();
        assert_eq!(app_of_workflow(".gitea/workflows/debrief-ai-fuzz.yml", &apps), Some("debrief-ai"));
        assert_eq!(app_of_workflow(".gitea/workflows/release-debrief.yaml", &apps), Some("debrief"));
        assert_eq!(app_of_workflow(".gitea/workflows/ci.yml", &apps), None);
        assert_eq!(app_of_workflow("README.md", &apps), None);
    }

    #[test]
    fn a_shared_crate_change_reaches_its_dependents_and_apps() {
        let p = plan(&["crates/core/src/lib.rs"]);
        assert_eq!(p["crates"], true);
        assert_eq!(p["apps"], json!(["dyno"]));
        assert_eq!(p["kinds"], json!({"dyno": ["rust"]}));
        assert_eq!(p["packages"]["crates"], json!(["paddock-core", "paddock-k", "paddock-kit"]));
        assert!(
            p["why"]["app:dyno"][0]
                .as_str()
                .unwrap()
                .starts_with("cargo apps/dyno/app/src-tauri reaches crates/core")
        );
    }

    #[test]
    fn ui_changes_reach_apps_through_npm_and_global_files_reach_everything() {
        let p = plan(&["ui/src/a.ts"]);
        assert_eq!(p["ui"], true);
        assert_eq!(p["kinds"], json!({"dyno": ["ui"]}));
        let g = plan(&[".gitea/actions/x/action.yml"]);
        assert_eq!(g["all"], true);
        assert_eq!(g["apps"], json!(["debrief", "dyno"]));
        assert_eq!(g["kinds"]["debrief"], json!(["rust", "ui"]));
        assert_eq!(plan(&["rust-toolchain.toml"])["all"], true);
    }

    #[test]
    fn the_planner_marks_scripts_tools_and_paddock_dev_but_no_app_and_rust_inputs_only_the_rust_builds() {
        for f in ["scripts/affected.mjs", "scripts/affected.test.mjs"] {
            let p = plan(&[f]);
            assert_eq!((p["all"].clone(), p["apps"].clone()), (json!(false), json!([])), "{f}");
            assert_eq!(
                (p["tools"].clone(), p["ui"].clone(), p["paddockDev"].clone()),
                (json!(true), json!(true), json!(true)),
                "{f}"
            );
        }
        for f in [".cargo/config.toml", "lint/clippy.toml"] {
            let p = plan(&[f]);
            assert_eq!(
                (p["all"].clone(), p["ui"].clone(), p["crates"].clone(), p["paddockDev"].clone()),
                (json!(false), json!(false), json!(true), json!(true)),
                "{f}"
            );
            assert_eq!(p["conformance"], json!(f.starts_with("lint/")), "{f}");
            assert_eq!(p["kinds"], json!({"dyno": ["rust"]}), "{f}");
        }
    }

    #[test]
    fn docs_meta_workflows_and_declared_inputs() {
        assert_eq!(plan(&["docs/x.md", ".gitignore"])["docs"], true);
        assert_eq!(plan(&[".gitignore"])["crates"], false);
        let w = plan(&[".gitea/workflows/dyno.yml"]);
        assert_eq!(w["apps"], json!(["dyno"]));
        assert_eq!(plan(&["docs/rules.md"])["conformance"], true);
        assert_eq!(plan(&["docs/rules.md"])["docs"], false);
        assert_eq!(plan(&["apps/dyno/CHANGELOG.md"])["apps"], json!(["dyno"]));
        assert_eq!(plan(&["apps/dyno/REVIEWED.md"])["apps"], json!(["dyno"]));
        assert_eq!(plan(&["icons/REVIEWED.md"])["conformance"], true);
        let cases = plan(&["ui/projects/paddock-ui/format/src/storage-cases.json"]);
        assert!(cases["apps"].as_array().unwrap().contains(&json!("dyno")));
        assert!(cases["kinds"]["dyno"].as_array().unwrap().contains(&json!("rust")));
        assert_eq!(plan(&["Mystery"])["all"], true);
    }

    #[test]
    fn icons_and_conformance_scripts_reach_apps() {
        assert_eq!(plan(&["icons/dyno/a.svg"])["apps"], json!(["dyno"]));
        assert_eq!(plan(&["icons/tools/x.mjs"])["apps"], json!(["debrief", "dyno"]));
        assert_eq!(plan(&["icons/other/x"])["apps"], json!([]));
        assert_eq!(plan(&["conformance/src/a.mjs"])["apps"], json!(["debrief", "dyno"]));
        assert_eq!(plan(&["conformance/test/a.mjs"])["apps"], json!([]));
    }

    #[test]
    fn crates_workspace_config_marks_every_member() {
        let p = plan(&["crates/Cargo.toml"]);
        assert_eq!(p["packages"]["crates"], json!(["paddock-core", "paddock-k", "paddock-kit"]));
        assert_eq!(p["apps"], json!(["dyno"]));
    }

    #[test]
    fn a_per_machine_apps_files_mark_the_sync_test_a_per_user_apps_only_conformance() {
        let opts = Options {
            per_machine_apps: vec!["apps/dyno/app/src-tauri".to_string()],
            ..Options::default()
        };
        let plan = |f: &str| affected(&[f.to_string()], &sample(), &opts).unwrap();
        let adopter = plan("apps/dyno/app/src-tauri/windows/per-machine.json");
        assert_eq!(
            (adopter["conformance"].clone(), adopter["crates"].clone()),
            (json!(true), json!(true))
        );
        let other = plan("apps/debrief/app/src-tauri/tauri.conf.json");
        assert_eq!((other["conformance"].clone(), other["crates"].clone()), (json!(true), json!(false)));
        let include = plan("crates/paddock-setup/nsis/paddock-per-machine.nsh");
        assert_eq!(
            (include["conformance"].clone(), include["crates"].clone()),
            (json!(true), json!(true))
        );
    }

    #[test]
    fn the_lock_marks_workspaces_whose_resolution_changed_and_all_means_all() {
        let lock = |v: &str| format!(r#"{{"packages":{{"ui":{{"dependencies":{{"x":"1"}}}},"node_modules/x":{{"version":"{v}"}}}}}}"#);
        let opts = Options {
            lock: (Some(lock("1")), Some(lock("2"))),
            ..Options::default()
        };
        let p = affected(&["package-lock.json".to_string()], &sample(), &opts).unwrap();
        assert_eq!(p["ui"], true);
        assert_eq!(p["kinds"], json!({"dyno": ["ui"]}));
        let all = affected(
            &[],
            &sample(),
            &Options {
                all: true,
                explain: true,
                ..Options::default()
            },
        )
        .unwrap();
        assert_eq!(all["why"]["eslint"], json!(["everything (no base)"]));
        assert_eq!(all["all"], true);
        assert!(
            affected(
                &["package-lock.json".to_string()],
                &sample(),
                &Options {
                    lock: (Some("{".into()), None),
                    ..Options::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn apps_are_the_folders_with_a_ci_contract() {
        let files: Vec<String> = [
            "apps/a/paddock.ci.json",
            "apps/a/b/paddock.ci.json",
            "apps/c/README.md",
            "apps/d/paddock.ci.json",
        ]
        .map(String::from)
        .into();
        assert_eq!(apps_of(&files), BTreeSet::from(["a".to_string(), "d".to_string()]));
    }

    /// A history of one commit range that changed the lockfile, with files that are not on disk.
    struct Tracked(Vec<String>);

    impl History for Tracked {
        fn tracked_files(&self) -> Result<Vec<String>, GraphError> {
            Ok(self.0.clone())
        }
        fn changed_files(&self, base: &str, head: &str) -> Result<Vec<String>, GraphError> {
            assert_eq!((base, head), ("old", "new"));
            Ok(vec!["package-lock.json".into()])
        }
        fn file_at(&self, rev: &str, path: &str) -> Result<Option<String>, GraphError> {
            assert_eq!(path, "package-lock.json");
            let version = if rev == "old" { "1" } else { "2" };
            Ok(Some(format!(
                r#"{{"packages":{{"ui":{{"dependencies":{{"x":"1"}}}},"node_modules/x":{{"version":"{version}"}}}}}}"#
            )))
        }
        fn log(&self, _: &str, _: usize) -> Result<Vec<paddock_repo_graph::Commit>, GraphError> {
            Ok(vec![])
        }
    }

    fn request(base: Option<&str>) -> Request {
        Request {
            files: vec!["x".into()],
            all: false,
            explain: false,
            root: None,
            base: base.map(String::from),
            head: "new".into(),
        }
    }

    #[test]
    fn a_tracked_file_missing_on_disk_is_a_typed_read_error_naming_the_file() {
        let d = tempfile::tempdir().unwrap();
        let err = plan_for(d.path(), &Tracked(vec!["ui/package.json".into()]), &request(None))
            .err()
            .unwrap();
        assert!(matches!(err, GraphError::Read { .. }) && err.to_string().contains("ui/package.json"));
        let ok = plan_for(d.path(), &Tracked(vec!["apps/a/paddock.ci.json".into()]), &request(None)).unwrap();
        assert_eq!(
            ok["all"], true,
            "an unknown root file plans everything, with the app list read from the tracked files"
        );
        assert_eq!(ok["apps"], json!(["a"]));
    }

    #[test]
    fn a_base_revision_plans_the_changed_files_and_reads_the_lock_at_both_ends() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("ui")).unwrap();
        std::fs::write(d.path().join("ui/package.json"), r#"{"name":"ui"}"#).unwrap();
        let history = Tracked(vec!["ui/package.json".into()]);
        let plan = plan_for(d.path(), &history, &request(Some("old"))).unwrap();
        assert_eq!(
            (plan["ui"].clone(), plan["all"].clone()),
            (true.into(), false.into()),
            "the lock change reaches the ui workspace only"
        );
        assert!(history.log("new", 1).unwrap().is_empty());
        let none = plan_for(
            d.path(),
            &history,
            &Request {
                all: true,
                ..request(Some("old"))
            },
        )
        .unwrap();
        assert_eq!(none["all"], true, "--all ignores the base");
    }

    #[test]
    fn a_request_needs_something_to_plan() {
        let req = Request {
            files: vec![],
            all: false,
            explain: false,
            root: None,
            base: None,
            head: "HEAD".into(),
        };
        assert_eq!(command(&req).err().map(|e| e.exit), Some(paddock_cli::Exit::Usage));
    }
}
