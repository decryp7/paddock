//! `dev graph affected`: which projects a change reaches. The oracle-tested port of the core of `scripts/affected.mjs`
//! (`affected()`, `owners()`, `appOfWorkflow()` and the rule tables); the graph itself comes from `paddock-repo-graph`.
//! The CI matrix, the nextest narrowing and `--ci` are in `plan_ci` and `plan_cmd`.
//!
//! Paddock's own rules (which paths are global inputs, which markdown is read by code, who owns a folder) live here and not
//! in `paddock-repo-graph`: they are this repository's, the graph crate is generic (ADR 0022 §6, "CI-only").

use crate::plan_gitea::{GiteaSource, TRACED_SCRIPT, gitea_targets};
use paddock_cli::{CliError, Report};
use paddock_repo_graph::{CargoCli, FileSource, GitHistory, Graph, GraphError, History, cargo_graph, lock_diff, npm_graph};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(120);
const PROJECTS: [&str; 9] = [
    "crates",
    "ui",
    "eslint",
    "conformance",
    "tools",
    "icons",
    "ci-image",
    "paddock-dev",
    "c99",
];

fn re(pattern: &str) -> Regex {
    // The patterns are literals of this file, checked by the tests below.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("a valid pattern")
}

/// Inputs that every project's build or tests read: a change re-runs everything. Mirrors `GLOBAL` in `scripts/affected.mjs`
/// (its comment table says what each former global reaches). `.gitea/` and `conformance/(ci|release|src/util)/` are narrowed per
/// job from the file text at two revisions (`plan_gitea`); without a reader for those revisions they plan everything.
static GLOBAL: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"^rust-toolchain(\.toml)?$",
        r"^package\.json$",
        r"^\.npmrc$",
        r"^\.nvmrc$",
        r"^\.gitattributes$",
        r"^tsconfig/",
    ]
    .map(re)
    .into()
});

/// An app's `src-tauri/Cargo.toml` and `capabilities/`, which conformance C48 reads. Mirrors `APP_CAPABILITY_INPUT` in `scripts/affected.mjs`.
static APP_CAPABILITY_INPUT: LazyLock<Regex> = LazyLock::new(|| re(r"^apps/[^/]+/(app/)?src-tauri/(Cargo\.toml|capabilities/)"));

/// What conformance C99 (no macOS Keychain in a shipped build, ADR 0057 §3.4) reads; they run the dedicated `c99` job. Mirrors
/// `C99_INPUT` in `scripts/affected.mjs`: every Cargo.toml and Cargo.lock under apps/ and crates/, the sources the check scans,
/// the Tauri capability files, the paddock.ci.json files, the cargo registry configuration, the waivers and the check's code.
static C99_INPUT: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"^(apps|crates)/(.+/)?Cargo\.(toml|lock)$",
        r"^(apps|crates)/.+\.(rs|c|cc|cpp|m|mm|h|swift)$",
        r"^(.+/)?(app/src|ui/projects)/.+\.ts$",
        r"(^|/)src-tauri/capabilities/[^/]+\.json$",
        r"(^|/)paddock\.ci\.json$",
        r"^conformance/(src|bin)/",
        r"^(\.cargo/config\.toml|paddock\.waivers\.toml)$",
    ]
    .map(re)
    .into()
});

/// Files every app's playwright.config.ts imports by relative path (the e2e port and build guard of `@decryp7/paddock-e2e`): no
/// npm or tsconfig edge says so, so a change to one re-runs the UI half of every app.
static E2E_GUARD_INPUTS: LazyLock<Regex> = LazyLock::new(|| re(r"^ui/projects/paddock-e2e/src/(env|build-guard)\.ts$"));

/// The notices script and the shared cargo-about configuration (and the tool, conformance/src/notices/): inputs of every app with a
/// `notices` block.
static NOTICES_INPUTS: LazyLock<Regex> =
    LazyLock::new(|| re(r"^(scripts/notices\.sh|templates/repo/about\.toml|conformance/src/notices/.*)$"));

/// Generated TypeScript and JSON of the shared crates, written by their tests (a `generated` folder under ui).
static GENERATED: LazyLock<Regex> = LazyLock::new(|| re(r"^ui/.*/generated/[^/]+\.(ts|json)$"));
/// The hand-written files in a `generated` folder (they make `shell/generated` an entry); the regex crate has no look-ahead.
fn is_generated(f: &str) -> bool {
    GENERATED.is_match(f) && !f.ends_with("/public-api.ts") && !f.ends_with("/ng-package.json")
}

/// JavaScript truthiness of a `paddock.ci.json` field.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// Inputs of the Rust builds only (cargo registry and linker config, the shared clippy and workspace lint presets).
static RUST_INPUTS: LazyLock<Vec<Regex>> = LazyLock::new(|| [r"^\.cargo/", r"^lint/"].map(re).into());

/// The planner and its tests: besides the scripts' owners, this tool's golden test runs them as its oracle.
static PLANNER_FILES: LazyLock<Regex> = LazyLock::new(|| re(r"^scripts/affected(\.test)?\.mjs$"));

/// What conformance C91 and paddock-service's `per_machine` sync test read of a per-machine app (scripts/affected.mjs
/// `PER_MACHINE_INPUT`).
static PER_MACHINE_INPUT: LazyLock<Regex> = LazyLock::new(|| {
    re(r"^(apps/[^/]+/app/src-tauri/(tauri(\.windows)?\.conf\.json|windows/)|crates/paddock-setup/nsis/paddock-per-machine\.nsh)")
});

/// What C100 reads (ADR 0060 section 4): scripts/affected.mjs `C100_INPUTS` and `./dev gates inputs`. A change marks conformance.
/// The content rule (a test file that adds or removes an ignore attribute or an environment guard) is [`guards_changed`].
static C100_INPUTS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"^docs/gates\.toml$",
        r"^apps/[^/]+/paddock\.ci\.json$",
        r"(^|/)package\.json$",
        r"^lefthook\.yml$",
        r"^(apps/[^/]+/)?AGENTS\.md$",
        r"^apps/[^/]+/docs/performance\.md$",
        r"(^|/)README\.md$",
        r"^docs/wiki/.*\.md$",
        r"^conformance/src/(checks/|cli\.mjs$)",
        r"^tools/paddock-dev/src/gates/",
        r"^(scripts|conformance|tools|apps/[^/]+/scripts)/(.*/)?(check-[^/]*\.(mjs|sh|ps1)|[^/]*\.test\.mjs)$",
    ]
    .iter()
    .map(|p| re(p))
    .collect()
});

static GUARD: LazyLock<Regex> = LazyLock::new(|| re(r"#\[ignore|cfg_attr\([^)\n]*ignore|env::var|process\.env"));
static RUST_FN: LazyLock<Regex> = LazyLock::new(|| re(r"(?-u:\b)fn\s+((?-u:\w)+)"));
static TS_TEST: LazyLock<Regex> = LazyLock::new(|| re(r#"(?-u:\b)(?:test|it)(?:\.(?-u:\w)+)?\(\s*['"`]([^'"`]+)"#));

/// The ignore attributes and environment guards of a test file, each paired with the Rust function or TypeScript test title it
/// belongs to (`guardSet` in scripts/affected.mjs): moving an ignore from one test to another changes the set though the count
/// stays.
pub fn guard_set(text: &str) -> BTreeSet<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut set = BTreeSet::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(m) = GUARD.find(line) else {
            continue;
        };
        let mut name = String::new();
        for l in lines.iter().take((i + 8).min(lines.len())).skip(i) {
            if let Some(c) = RUST_FN.captures(l) {
                name = c[1].to_string();
                break;
            }
        }
        if name.is_empty() {
            for k in (0..=i).rev().take_while(|k| *k + 40 > i) {
                if let Some(c) = TS_TEST.captures(lines[k]) {
                    name = c[1].to_string();
                    break;
                }
            }
        }
        set.insert(format!("{}|{name}", m.as_str()));
    }
    set
}

/// Whether a file's guards differ between two texts (a missing file has none).
pub fn guards_changed(before: Option<&str>, after: Option<&str>) -> bool {
    guard_set(before.unwrap_or_default()) != guard_set(after.unwrap_or_default())
}

/// The text of a changed file before and after the change (`None`: no such file at that end).
pub type ContentOf<'a> = &'a dyn Fn(&str) -> (Option<String>, Option<String>);

/// The conformance tool's own code.
static CONFORMANCE_CODE: LazyLock<Regex> = LazyLock::new(|| re(r"^conformance/(src|bin)/"));

/// Editor and VCS metadata: marks nothing.
static META: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(^|/)\.gitignore$",
        r"^\.editorconfig$",
        r"(^|/)\.DS_Store$",
        r"^lefthook\.yml$",
        r"^\.gitea/ISSUE_TEMPLATE/",
    ]
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

/// Whether `path` is documentation (it marks only the link check unless it is a declared input).
pub fn is_docs(path: &str) -> bool {
    DOCS.iter().any(|r| r.is_match(path))
}

/// Whether `path` is markdown that code or tests read, a declared contract (it marks its project, not the link check).
pub fn is_declared_doc_input(path: &str) -> bool {
    DOC_INPUTS.iter().any(|(r, _)| r.is_match(path))
}

/// The Rust sources that can emit a health cause code: ui's cause-codes.spec.ts reads every `.rs` file under `crates/` and `apps/`
/// against the chip's table, skipping folders named target, node_modules, tests and benches and files named tests.rs. A change to one
/// marks ui too (the owner keeps its own halves). Same as `isCauseCodeSource` in scripts/affected.mjs.
fn is_cause_code_source(f: &str) -> bool {
    let parts: Vec<&str> = f.split('/').collect();
    let (Some(last), Some(first)) = (parts.last(), parts.first()) else {
        return false;
    };
    (*first == "crates" || *first == "apps")
        && parts.len() > 1
        && last.ends_with(".rs")
        && *last != "tests.rs"
        && !parts[1..parts.len() - 1]
            .iter()
            .any(|d| matches!(*d, "target" | "node_modules" | "tests" | "benches"))
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
        (re(r"^docs/(design-guide|benchmarks)\.md$"), Marks::Projects(&["conformance"])),
        (re(r"^docs/wiki/"), Marks::Projects(&["tools"])),
        (re(r"^apps/(shell-demo|workbench-demo)/README\.md$"), Marks::Projects(&["tools"])),
        (re(r"(^|/)REVIEWED\.md$"), Marks::Reviewed),
        (
            re(r"^ui/projects/paddock-ui/format/src/storage-cases\.json$"),
            Marks::Projects(&["ui", "app:dyno"]),
        ),
        (
            re(r"^crates/paddock-service/tests/fixtures/launchctl-print-running\.txt$"),
            Marks::Projects(&["crates", "app:pitcrew"]),
        ),
        (re(r"^apps/debrief/docs/ai/sql-views\.md$"), Marks::Owner),
        (re(r"^apps/debrief/docs/testing/quarantine\.md$"), Marks::Owner),
        (re(r"^apps/[^/]+/vendor/.*\.md$"), Marks::Owner),
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
        (re(r"^\.gitleaks\.toml$"), &["conformance"]),
        (re(r"^wiki-lint\.json$"), &["tools"]),
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
    /// Each app's `paddock.ci.json` with its `dir` (`apps/<name>`): the CI contract of a matrix leg.
    pub configs: BTreeMap<String, Value>,
}

impl RepoGraph {
    /// An app's `paddock.ci.json` (JSON null when it has none).
    pub fn config(&self, app: &str) -> &Value {
        static NONE: Value = Value::Null;
        self.configs.get(app).unwrap_or(&NONE)
    }
}

/// Finds the Rust files that name a generated file (given its path).
pub type CratesNaming<'a> = &'a dyn Fn(&str) -> Vec<String>;

/// What changed besides the file list.
#[derive(Default)]
pub struct Options<'a> {
    /// Plan everything.
    pub all: bool,
    /// Give the reason for every project.
    pub explain: bool,
    /// The root `package-lock.json` before and after.
    pub lock: (Option<String>, Option<String>),
    /// The `apps/<app>/app/src-tauri` folders that adopt the shared per-machine installer include (see
    /// [`per_machine_apps`]); read from the tree by [`plan_for`].
    pub per_machine_apps: Vec<String>,
    /// The apps whose `paddock.ci.json` `checks` run `paddock-conformance` ([`conformance_apps`]).
    pub conformance_apps: Vec<String>,
    /// The `.gitea` and `conformance` files at both revisions, to narrow a workflow, action or script change; without it any
    /// such change plans everything.
    pub gitea: Option<&'a dyn GiteaSource>,
    /// The crate folders whose Rust sources name a generated file (by its file name); without it a generated file marks every crate.
    pub crates_naming: Option<CratesNaming<'a>>,
    /// The text of a changed file before and after, for the C100 content rule; without it that rule is not applied.
    pub content: Option<ContentOf<'a>>,
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

/// The apps whose contract `checks` run `paddock-conformance` (scripts/affected.mjs `app.checks`).
pub fn conformance_apps(root: &Path) -> Vec<String> {
    let Ok(apps) = std::fs::read_dir(root.join("apps")) else {
        return Vec::new();
    };
    let mut out: Vec<String> = apps
        .flatten()
        .filter_map(|a| {
            let name = a.file_name().to_string_lossy().to_string();
            let text = std::fs::read_to_string(root.join("apps").join(&name).join("paddock.ci.json")).ok()?;
            let json: Value = serde_json::from_str(&text).ok()?;
            let checks = json.get("checks")?.to_string();
            checks.contains("paddock-conformance").then_some(name)
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
        if C100_INPUTS.iter().any(|r| r.is_match(f)) {
            st.mark(Some("conformance".to_string()), format!("C100 reads {f} (./dev gates inputs)"));
        }
        if let Some(content) = opts
            .content
            .filter(|_| (f.ends_with(".rs") || f.ends_with(".ts")) && !f.contains("node_modules/"))
        {
            let (before, after) = content(f);
            if guards_changed(before.as_deref(), after.as_deref()) {
                st.mark(
                    Some("conformance".to_string()),
                    format!("C100: an ignore attribute or an environment guard changed in {f}"),
                );
            }
        }
        // The apps whose own checks run paddock-conformance rerun those checks when conformance changes (scripts/affected.mjs).
        if CONFORMANCE_CODE.is_match(f) {
            for a in &opts.conformance_apps {
                st.mark(Some(format!("app:{a}")), format!("its checks run paddock-conformance ({f})"));
            }
        }
        if META.iter().any(|r| r.is_match(f)) {
            continue;
        }
        if C99_INPUT.iter().any(|r| r.is_match(f)) {
            st.mark(Some("c99".to_string()), format!("C99 reads {f}"));
        }
        let ci_file = f.starts_with(".gitea/");
        if ci_file || TRACED_SCRIPT.is_match(f) {
            // `.gitea` runs the conformance checks over workflows and actions; the rest is narrowed per job or user. The shared
            // actions' scripts: conformance, every app's gates, and the jobs of the actions that run the script.
            let hit = opts.gitea.and_then(|g| gitea_targets(f, g));
            let Some(hit) = hit else {
                mark_all(&mut st, &format!("global {f}"), &mut everything);
                continue;
            };
            if ci_file {
                st.mark(Some("conformance".to_string()), format!("ci files {f}"));
            }
            for (p, reason) in hit {
                if p == "everything" {
                    mark_all(&mut st, &reason, &mut everything);
                } else if p == "apps" {
                    for a in &graph.apps {
                        st.mark(Some(format!("app:{a}")), reason.clone());
                        st.kind(Some(&format!("app:{a}")), "all");
                    }
                } else if p.starts_with("app:") {
                    st.mark(Some(p.clone()), reason);
                    st.kind(Some(&p), "all");
                } else {
                    st.mark(Some(p), reason);
                }
            }
            if !ci_file {
                st.mark(Some("conformance".to_string()), f.clone());
                for a in &graph.apps {
                    st.mark(Some(format!("app:{a}")), format!("conformance {f}"));
                    st.kind(Some(&format!("app:{a}")), "all");
                }
            }
            continue;
        }
        if PLANNER_FILES.is_match(f) {
            st.mark(
                Some("paddock-dev".to_string()),
                format!("its golden test compares {f} (tools/paddock-dev/tests/it/golden.rs)"),
            );
        }
        if RUST_INPUTS.iter().any(|r| r.is_match(f)) {
            for p in ["crates", "paddock-dev"] {
                st.mark(Some(p.to_string()), format!("rust input {f}"));
            }
            if f.starts_with("lint/") {
                st.mark(Some("conformance".to_string()), format!("C13 reads {f}"));
            }
            for a in &graph.apps {
                if graph.config(a).get("cargo").is_some_and(truthy) {
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
        if E2E_GUARD_INPUTS.is_match(f) {
            for a in &graph.apps {
                st.mark(Some(format!("app:{a}")), format!("e2e guard {f}"));
                st.kind(Some(&format!("app:{a}")), "ui");
            }
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
        // A change to a shared workspace's own config (crates/Cargo.toml, crates/.config/...) reaches all its members.
        if c.is_none() && own.iter().any(|p| p == "crates") {
            for (d, x) in graph.cargo.iter() {
                if x.workspace == "crates" {
                    push_unique(&mut changed_crates, d);
                }
            }
        }
        // A generated TypeScript or JSON file under ui/ is compared with its Rust source by a crate's test: a stale one must
        // fail on the branch, so the crates that name the file are tested (all crates when none does).
        if is_generated(f) {
            st.mark(Some("crates".to_string()), format!("generated file {f}"));
            let named = opts.crates_naming.map(|g| g(f)).unwrap_or_default();
            let named: Vec<String> = named
                .iter()
                .filter_map(|h| graph.cargo.node_of(h))
                .filter(|d| graph.cargo.get(d).is_some_and(|x| x.workspace == "crates"))
                .map(str::to_string)
                .collect();
            if named.is_empty() {
                for (d, x) in graph.cargo.iter() {
                    if x.workspace == "crates" {
                        push_unique(&mut changed_crates, d);
                    }
                }
            } else {
                for d in named {
                    push_unique(&mut changed_crates, &d);
                }
            }
        }
        if is_cause_code_source(f) {
            st.mark(Some("ui".to_string()), format!("ui cause-codes.spec.ts scans {f}")); // ui's own tests, not its dependents
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
        // An app's Cargo dependencies and capabilities are read by conformance C48.
        if APP_CAPABILITY_INPUT.is_match(f) {
            st.mark(Some("conformance".to_string()), format!("C48 reads {f}"));
        }
        // A per-machine app's installer files and configuration are read by conformance C91 and by paddock-service's per_machine
        // sync test, which runs with the shared crates.
        if PER_MACHINE_INPUT.is_match(f) {
            st.mark(Some("conformance".to_string()), format!("C91 reads {f}"));
            if f.starts_with("crates/") || opts.per_machine_apps.iter().any(|d| f.starts_with(&format!("{d}/"))) {
                st.mark(Some("crates".to_string()), format!("the per_machine sync test reads {f}"));
            }
        }
        // The notices tool and its cargo-about configuration decide the committed THIRD_PARTY_NOTICES.md of every app with a
        // `notices` block.
        if NOTICES_INPUTS.is_match(f) {
            for a in &graph.apps {
                if graph.config(a).get("notices").is_some_and(truthy) {
                    st.mark(Some(format!("app:{a}")), format!("notices tool {f}"));
                    st.kind(Some(&format!("app:{a}")), "all");
                }
            }
        }
        // The rest of conformance/ (bin, src) runs in every app's gates through the shared actions.
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

    // The conformance job is in every plan, whatever changed (user decision 2026-11-12); mirrors `affected` in scripts/affected.mjs.
    st.mark(Some("conformance".to_string()), "conformance always runs".to_string());
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
        ("c99", "c99"),
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

/// The resolved root as cargo is given it and as its output is read: plain, `/` separators; a root that has no such spelling
/// (a volume without a drive letter, a path over 259 units) is refused with the next step, never passed on with its prefix.
fn graph_root(resolved: &Path, os: paddock_env::Os) -> Result<(PathBuf, String), GraphError> {
    let text = paddock_env::winpath::for_tools(resolved, os).map_err(|e| GraphError::Read {
        path: resolved.display().to_string(),
        reason: e.to_string(),
    })?;
    Ok((PathBuf::from(&text), text))
}

/// Reads the graphs of the repository at `root` (tracked files only, as `git ls-files` lists them).
pub fn read_graph(root: &Path, history: &dyn History) -> Result<RepoGraph, GraphError> {
    let files = history.tracked_files()?;
    // The root in the spelling cargo prints its paths in (a temp folder's short name or a symlinked workspace is not).
    let resolved = paddock_env::winpath::real_verbatim(root).unwrap_or_else(|_| root.to_path_buf());
    let (root, root_text) = graph_root(&resolved, paddock_env::Os::current())?;
    let root = root.as_path();
    let disk = DiskFiles(root.to_path_buf());
    let cargo = cargo_graph(&files, &disk, &CargoCli::new(root, TIMEOUT), &root_text)?;
    let apps = apps_of(&files);
    let mut configs = BTreeMap::new();
    for app in &apps {
        let path = format!("apps/{app}/paddock.ci.json");
        let text = disk.read_to_string(&path)?;
        let mut config: Value = serde_json::from_str(&text).map_err(|e| GraphError::Read {
            path: path.clone(),
            reason: format!("not valid JSON: {e}"),
        })?;
        if let Some(map) = config.as_object_mut() {
            map.insert("dir".into(), json!(format!("apps/{app}")));
        }
        configs.insert(app.clone(), config);
    }
    Ok(RepoGraph {
        cargo,
        npm: npm_graph(&files, &disk)?,
        apps,
        configs,
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
    plan_with_graph(root, history, req).map(|(_, plan)| plan)
}

/// The graph the plan was computed from and the plan (the CI matrix reads the apps' contracts from the graph).
pub fn plan_with_graph(root: &Path, history: &dyn History, req: &Request) -> Result<(RepoGraph, Value), GraphError> {
    let graph = read_graph(root, history)?;
    let plan = plan_on_graph(root, history, req, &graph)?;
    Ok((graph, plan))
}

/// The plan for a request on a graph already read from `root` (reading it runs `cargo metadata` over every workspace). The graph
/// depends on the tracked files and manifests, not on the text of the changed files, so one reading serves many requests.
pub fn plan_on_graph(root: &Path, history: &dyn History, req: &Request, graph: &RepoGraph) -> Result<Value, GraphError> {
    let files_mode = !req.files.is_empty();
    let gitea = if req.all {
        None
    } else {
        Some(crate::plan_repo::GitGitea::new(
            root,
            graph.apps.iter().cloned().collect(),
            files_mode,
            req.base.as_deref().unwrap_or("HEAD"),
            &req.head,
        )?)
    };
    let naming = |f: &str| crate::plan_repo::files_naming_generated(root, f);
    // Files mode: HEAD before, the working tree after (as the script does); base mode: the two revisions.
    let content = |f: &str| {
        if files_mode {
            (
                history.file_at("HEAD", f).ok().flatten(),
                std::fs::read_to_string(root.join(f)).ok(),
            )
        } else {
            let base = req.base.as_deref().unwrap_or("HEAD");
            (
                history.file_at(base, f).ok().flatten(),
                history.file_at(&req.head, f).ok().flatten(),
            )
        }
    };
    let mut opts = Options {
        all: req.all,
        explain: req.explain,
        lock: (None, None),
        per_machine_apps: per_machine_apps(root),
        conformance_apps: conformance_apps(root),
        gitea: gitea.as_ref().map(|g| g as &dyn GiteaSource),
        crates_naming: Some(&naming),
        content: Some(&content),
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
    affected(&changed, graph, &opts)
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

    #[test]
    fn a_graph_root_over_the_limit_is_refused_and_a_short_verbatim_one_is_plain() {
        let long = format!(r"\\?\C:\{}", vec!["a".repeat(50); 6].join(r"\"));
        let e = graph_root(Path::new(&long), paddock_env::Os::Windows).unwrap_err().to_string();
        assert!(e.contains("longer than Windows") && e.contains("shorter path"), "{e}");
        let (path, text) = graph_root(Path::new(r"\\?\C:\w\repo"), paddock_env::Os::Windows).unwrap();
        assert_eq!((path, text.as_str()), (PathBuf::from("C:/w/repo"), "C:/w/repo"));
        let (_, unix) = graph_root(Path::new("/w/repo"), paddock_env::Os::Linux).unwrap();
        assert_eq!(unix, "/w/repo");
    }
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
            configs: BTreeMap::from([
                ("dyno".to_string(), json!({"cargo": "app/src-tauri", "dir": "apps/dyno"})),
                ("debrief".to_string(), json!({"dir": "apps/debrief"})),
            ]),
        }
    }

    /// Whether conformance is marked for a reason of the change: it is in every plan (user decision 2026-11-12), so the plain
    /// `conformance` flag no longer tells.
    fn because(p: &Value) -> bool {
        p["why"]["conformance"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x != "conformance always runs"))
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
    fn conformance_is_in_every_plan() {
        for files in [
            vec!["apps/dyno/app/src-tauri/src/lib.rs"],
            vec![".gitea/workflows/dyno.yml"],
            vec!["apps/dyno/app/src/app/x.spec.ts"],
            vec!["docs/plans/x.md"],
            vec![],
        ] {
            let p = plan(&files);
            assert_eq!(p["conformance"], true, "{files:?}");
            assert!(
                p["why"]["conformance"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("conformance always runs")),
                "{files:?}"
            );
        }
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
    fn c99_inputs_plan_the_c99_job_and_a_docs_change_does_not() {
        for f in [
            "apps/dyno/Cargo.lock",
            "apps/dyno/crates/dyno-core/Cargo.toml",
            "apps/dyno/crates/dyno-core/src/lib.rs",
            "crates/paddock-secrets/Cargo.toml",
            "apps/dyno/app/src-tauri/capabilities/default.json",
            "apps/dyno/paddock.ci.json",
            "apps/dyno/app/src/app/a.ts",
            "conformance/src/checks/C99-no-keychain.mjs",
            "paddock.waivers.toml",
        ] {
            assert_eq!(plan(&[f])["c99"], json!(true), "{f}");
        }
        // Markdown paths are built, not quoted: affected.test.mjs fails on a quoted repo markdown path that no DOC_INPUTS entry declares.
        let docs = [
            format!("docs/{}.md", "benchmarks"),
            format!("apps/dyno/{}", "README.md"),
            "README.md".to_string(),
            "conformance/test/c99-graph.test.mjs".to_string(),
        ];
        for f in &docs {
            assert_eq!(plan(&[f.as_str()])["c99"], json!(false), "{f}");
        }
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
            assert_eq!(because(&p), f.starts_with("lint/"), "{f}");
            assert_eq!(p["kinds"], json!({"dyno": ["rust"]}), "{f}");
        }
    }

    #[test]
    fn docs_meta_workflows_and_declared_inputs() {
        assert_eq!(plan(&["docs/x.md", ".gitignore"])["docs"], true);
        assert_eq!(plan(&[".gitignore"])["crates"], false);
        let t = plan(&[".gitea/ISSUE_TEMPLATE/bug.yaml", ".gitea/ISSUE_TEMPLATE/config.yaml"]);
        assert_eq!(t["all"], false);
        assert_eq!(t["apps"], json!([]));
        let w = plan(&[".gitea/workflows/dyno.yml"]);
        assert_eq!(w["apps"], json!(["dyno"]));
        assert_eq!(plan(&["docs/rules.md"])["conformance"], true);
        assert_eq!(plan(&["docs/rules.md"])["docs"], false);
        // the project wiki is linted by tools/wiki on every wiki change (ER-30), as is its settings file
        for f in ["docs/wiki/index.md", "wiki-lint.json"] {
            let p = plan(&[f]);
            assert_eq!(
                (&p["tools"], &p["docs"], &p["crates"], &p["all"]),
                (&json!(true), &json!(false), &json!(false), &json!(false)),
                "{f}"
            );
        }
        assert_eq!(plan(&["apps/dyno/CHANGELOG.md"])["apps"], json!(["dyno"]));
        assert_eq!(plan(&["apps/dyno/REVIEWED.md"])["apps"], json!(["dyno"]));
        assert_eq!(plan(&["icons/REVIEWED.md"])["conformance"], true);
        let src = plan(&["apps/dyno/app/src-tauri/src/health.rs"]);
        assert_eq!(src["ui"], true, "ui's cause-codes spec scans the apps' Rust sources");
        assert!(src["kinds"]["dyno"].as_array().unwrap().contains(&json!("rust")));
        assert_ne!(plan(&["apps/dyno/app/src-tauri/Cargo.toml"])["ui"], true);
        // Exactly what the spec scans: every .rs under crates/ and apps/, but not tests.rs or tests/, benches/, target/ folders.
        for f in [
            "crates/paddock-health/src/lib.rs",
            "apps/shell-demo/src-tauri/src/main.rs",
            "apps/dyno/crates/dyno-agent/src/server.rs",
        ] {
            assert_eq!(plan(&[f])["ui"], true, "{f}");
        }
        for f in [
            "crates/paddock-health/src/tests.rs",
            "crates/paddock-health/tests/it.rs",
            "apps/dyno/crates/dyno-agent/benches/b.rs",
            "tools/paddock-dev/src/plan.rs",
        ] {
            assert_ne!(plan(&[f])["ui"], true, "{f}");
        }
        let cases = plan(&["ui/projects/paddock-ui/format/src/storage-cases.json"]);
        assert!(cases["apps"].as_array().unwrap().contains(&json!("dyno")));
        assert!(cases["kinds"]["dyno"].as_array().unwrap().contains(&json!("rust")));
        let capture = plan(&["crates/paddock-service/tests/fixtures/launchctl-print-running.txt"]);
        // The sample repo has no Pitcrew app: the declared input names it, so its explanation does too.
        assert!(capture.to_string().contains("app:pitcrew"), "{capture}");
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

    fn guard_plan(file: &str, before: Option<&str>, after: Option<&str>) -> Value {
        let (b, a) = (before.map(String::from), after.map(String::from));
        let read = move |_: &str| (b.clone(), a.clone());
        let opts = Options {
            explain: true,
            content: Some(&read),
            ..Options::default()
        };
        affected(&[file.to_string()], &sample(), &opts).unwrap()
    }

    #[test]
    fn guard_set_pairs_each_guard_with_its_rust_function_or_ts_test_title() {
        let rust = "#[test]\n#[ignore]\nfn slow() {}\nfn other() { let _ = std::env::var(\"X\"); }\n";
        assert_eq!(
            guard_set(rust),
            BTreeSet::from(["#[ignore|slow".to_string(), "env::var|other".to_string()])
        );
        let ts = "it('reads it', () => {\n  const a = process.env.X;\n});\n";
        assert_eq!(guard_set(ts), BTreeSet::from(["process.env|reads it".to_string()]));
        assert!(guard_set("").is_empty());
    }

    #[test]
    fn an_added_moved_or_removed_guard_in_a_test_file_marks_conformance() {
        let before = "#[test]\n#[ignore]\nfn a() {}\n#[test]\nfn b() {}\n";
        let moved = "#[test]\nfn a() {}\n#[test]\n#[ignore]\nfn b() {}\n";
        for (b, a) in [(Some(before), Some(moved)), (None, Some(before)), (Some(before), None)] {
            let p = guard_plan("crates/paddock-cli/src/lib.rs", b, a);
            assert_eq!(
                p["why"]["conformance"],
                json!([
                    "C100: an ignore attribute or an environment guard changed in crates/paddock-cli/src/lib.rs",
                    "conformance always runs"
                ])
            );
        }
        let same = guard_plan("crates/paddock-cli/src/lib.rs", Some(before), Some(before));
        assert!(!because(&same));
        let other = guard_plan("crates/paddock-cli/README.txt", Some(before), Some(moved));
        assert!(!because(&other));
        let nm = guard_plan("ui/node_modules/x/a.ts", Some(before), Some(moved));
        assert!(!because(&nm));
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

    /// A repository with one empty commit: the `.gitea` readers need a real one.
    fn repo(d: &Path) {
        for args in [
            ["init", "-q"].as_slice(),
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "x",
            ],
        ] {
            assert!(crate::plan_repo::git_text(d, args).is_some());
        }
    }

    /// A history of one commit range that changed the lockfile, with files that are not on disk.
    struct Tracked(Vec<String>);

    impl History for Tracked {
        fn tracked_files(&self) -> Result<Vec<String>, GraphError> {
            Ok(self.0.clone())
        }
        fn changed_files(&self, base: &str, head: &str) -> Result<Vec<String>, GraphError> {
            assert_eq!((base, head), ("old", "HEAD"));
            Ok(vec!["package-lock.json".into()])
        }
        fn file_at(&self, rev: &str, path: &str) -> Result<Option<String>, GraphError> {
            if path != "package-lock.json" {
                return Ok(None);
            }
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
            head: "HEAD".into(),
        }
    }

    #[test]
    fn a_tracked_file_missing_on_disk_is_a_typed_read_error_naming_the_file() {
        let d = tempfile::tempdir().unwrap();
        repo(d.path());
        let err = plan_for(d.path(), &Tracked(vec!["ui/package.json".into()]), &request(None))
            .err()
            .unwrap();
        assert!(matches!(err, GraphError::Read { .. }) && err.to_string().contains("ui/package.json"));
        std::fs::create_dir_all(d.path().join("apps/a")).unwrap();
        std::fs::write(d.path().join("apps/a/paddock.ci.json"), "{}").unwrap();
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
        repo(d.path());
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

    // ---- .gitea changes are narrowed to the jobs they reach (the cases of scripts/affected.test.mjs, on synthetic trees) ----

    use crate::plan_gitea::{Fixed, changed_jobs, script_importers};

    fn ci_text(conf: &str, app: &str) -> String {
        format!(
            "name: ci\non:\n  push:\njobs:\n  plan:\n    steps:\n      - uses: ./.gitea/actions/plan-only\n  ui:\n    steps:\n      - uses: ./.gitea/actions/node-caches\n  conformance: # the checks\n    steps:\n      - run: {conf}\n  app:\n    steps:\n      - run: {app}\n      - uses: ./.gitea/actions/app-gates\n  crates:\n    steps:\n      - uses: ./.gitea/actions/build-folder\n      - uses: ./.gitea/actions/install-test-browser\n"
        )
    }

    const CI_FILE: &str = ".gitea/workflows/ci.yml";

    fn base_tree() -> BTreeMap<String, String> {
        [
            (CI_FILE, ci_text("node a", "node b").as_str()),
            (
                ".gitea/workflows/dyno.yml",
                "jobs:\n  gates:\n    steps:\n      - uses: ./.gitea/actions/node-caches\n",
            ),
            (
                ".gitea/workflows/nightly.yml",
                "jobs:\n  night:\n    steps:\n      - uses: ./.gitea/actions/nightly-only\n",
            ),
            (".gitea/actions/nightly-only/action.yml", "runs: n"),
            (".gitea/actions/plan-only/action.yml", "runs: x"),
            (".gitea/actions/node-caches/action.yml", "runs: x"),
            (".gitea/actions/app-gates/action.yml", "runs: y"),
            (
                ".gitea/actions/build-folder/action.yml",
                "run: bash \"${{ github.action_path }}/../../../conformance/ci/build-folder.sh\"",
            ),
            (
                ".gitea/actions/install-test-browser/action.yml",
                "run: node \"${{ github.action_path }}/../../../conformance/ci/install-test-browser.mjs\"",
            ),
            (
                ".gitea/actions/README.md",
                "- uses: decryp7/paddock/.gitea/actions/node-caches@v0.3.0 and .gitea/actions/unused",
            ),
            (".gitea/actions/unused/action.yml", "runs: u"),
            (".gitea/actions/only-commented/action.yml", "runs: c"),
            (
                "conformance/ci/install-test-browser.mjs",
                "import { entryFor } from './runtime-catalogue.mjs';\nimport { isMain } from '../src/util/is-main.mjs';",
            ),
            ("conformance/ci/runtime-catalogue.mjs", "export const entryFor = 1;"),
            ("conformance/src/util/is-main.mjs", "export const isMain = 1;"),
            ("conformance/ci/unrelated.mjs", "export const x = 1;"),
            ("conformance/ci/build-folder.sh", "echo"),
            ("conformance/release/latest-json.mjs", "export const y = 1;"),
            (
                ".gitea/actions/nested/action.yml",
                "steps:\n  - uses: ./.paddock/.gitea/actions/build-folder",
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    fn fixed(before: &BTreeMap<String, String>, changes: &[(&str, String)], dirty: Option<&[&str]>) -> Fixed {
        let mut after = before.clone();
        for (k, v) in changes {
            after.insert(k.to_string(), v.clone());
        }
        Fixed {
            apps: vec!["debrief".into(), "dyno".into()],
            before: before.clone(),
            after,
            dirty: dirty.map(|d| d.iter().map(|s| s.to_string()).collect()),
        }
    }

    fn gitea_plan(files: &[&str], g: &Fixed) -> Value {
        let files: Vec<String> = files.iter().map(|s| s.to_string()).collect();
        affected(
            &files,
            &sample(),
            &Options {
                explain: true,
                gitea: Some(g),
                ..Options::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn a_ci_yml_change_to_one_job_marks_that_job_only_and_the_app_job_marks_the_app_legs() {
        let t = base_tree();
        let r = gitea_plan(&[CI_FILE], &fixed(&t, &[(CI_FILE, ci_text("node a --only C06", "node b"))], None));
        assert_eq!(
            (
                r["all"].clone(),
                r["conformance"].clone(),
                r["crates"].clone(),
                r["ui"].clone(),
                r["apps"].clone()
            ),
            (json!(false), json!(true), json!(false), json!(false), json!([]))
        );
        let a = gitea_plan(&[CI_FILE], &fixed(&t, &[(CI_FILE, ci_text("node a", "node c"))], None));
        assert_eq!(
            (a["all"].clone(), a["crates"].clone(), a["apps"].clone()),
            (json!(false), json!(false), json!(["debrief", "dyno"]))
        );
    }

    #[test]
    fn a_change_to_the_plan_job_a_header_an_unknown_job_or_no_reader_marks_everything() {
        let t = base_tree();
        let plan_job = ci_text("node a", "node b").replace("plan-only", "plan-only2");
        let r = gitea_plan(&[CI_FILE], &fixed(&t, &[(CI_FILE, plan_job)], None));
        assert_eq!(
            (r["all"].clone(), r["ui"].clone(), r["crates"].clone()),
            (json!(true), json!(true), json!(true))
        );
        let header = ci_text("node a", "node b").replace("  push:", "  pull_request:");
        assert_eq!(gitea_plan(&[CI_FILE], &fixed(&t, &[(CI_FILE, header)], None))["all"], true);
        let brand_new = format!("{}  brand-new:\n    steps: []\n", ci_text("node a", "node b"));
        assert_eq!(gitea_plan(&[CI_FILE], &fixed(&t, &[(CI_FILE, brand_new)], None))["all"], true);
        let not_a_workflow = "this: [is, not\n  a workflow".to_string();
        assert_eq!(gitea_plan(&[CI_FILE], &fixed(&t, &[(CI_FILE, not_a_workflow)], None))["all"], true);
        assert_eq!(plan(&[CI_FILE])["all"], true, "no reader for the revisions: everything");
        assert_eq!(
            gitea_plan(&[CI_FILE], &fixed(&t, &[], None))["all"],
            true,
            "identical revisions: everything"
        );
        assert_eq!(changed_jobs(Some(&ci_text("a", "b")), Some("jobs:\nx: 1")), None);
    }

    #[test]
    fn a_job_split_that_could_hide_a_job_is_refused() {
        let base = ci_text("node a", "node b");
        let r = gitea_plan(
            &[CI_FILE],
            &fixed(&base_tree(), &[(CI_FILE, base.replace("node a", "node a2"))], None),
        );
        assert_eq!(
            (r["all"].clone(), r["conformance"].clone(), r["ui"].clone(), r["crates"].clone()),
            (json!(false), json!(true), json!(false), json!(false))
        );
        let bad = [
            format!("{base}  \"quoted\": {{}}\n"),
            format!("{base}  crates2: {{ runs-on: x }}\n"),
            format!("{base}anchors:\n  x: &rs 1\n"),
            base.replace("node b", "node *rs"),
            base.replace(
                "steps:\n      - uses: ./.gitea/actions/build-folder",
                "steps: &rs\n      - uses: ./.gitea/actions/build-folder",
            ),
        ];
        for b in bad {
            assert_eq!(changed_jobs(Some(&base), Some(&b)), None, "{b}");
            assert_eq!(
                gitea_plan(&[CI_FILE], &fixed(&base_tree(), &[(CI_FILE, b.clone())], None))["all"],
                true,
                "{b}"
            );
        }
        assert_eq!(
            changed_jobs(
                Some(&base),
                Some(&base.replace("  conformance: # the checks", "  conformance: # checks"))
            ),
            Some(vec![])
        );
    }

    #[test]
    fn an_action_marks_only_the_jobs_and_app_workflows_that_use_it() {
        let g = fixed(&base_tree(), &[], None);
        let ui = gitea_plan(&[".gitea/actions/node-caches/action.yml"], &g);
        assert_eq!(
            (ui["all"].clone(), ui["ui"].clone(), ui["crates"].clone(), ui["apps"].clone()),
            (json!(false), json!(true), json!(false), json!(["dyno"]))
        );
        let gates = gitea_plan(&[".gitea/actions/app-gates/action.yml"], &g);
        assert_eq!(
            (gates["all"].clone(), gates["crates"].clone(), gates["apps"].clone()),
            (json!(false), json!(false), json!(["debrief", "dyno"]))
        );
        let bf = gitea_plan(&[".gitea/actions/build-folder/action.yml"], &g);
        assert_eq!(
            (bf["all"].clone(), bf["crates"].clone(), bf["ui"].clone(), bf["apps"].clone()),
            (json!(false), json!(true), json!(false), json!([]))
        );
        assert_eq!(
            gitea_plan(&[".gitea/actions/nested/action.yml"], &g)["all"],
            true,
            "an action nobody uses"
        );
        assert_eq!(
            gitea_plan(&[".gitea/actions/unused/action.yml"], &g)["all"],
            true,
            "a README mention is no use"
        );
        let night = gitea_plan(&[".gitea/actions/nightly-only/action.yml"], &g);
        assert_eq!(
            (night["all"].clone(), night["tools"].clone(), night["apps"].clone()),
            (json!(false), json!(true), json!([]))
        );
        assert!(
            night["why"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w.as_str().unwrap().contains("not a pull request"))
        );
    }

    #[test]
    fn a_script_is_traced_through_its_importers_to_the_actions_that_run_it() {
        let g = fixed(&base_tree(), &[], None);
        let r = gitea_plan(&["conformance/ci/build-folder.sh"], &g);
        assert_eq!(
            (
                r["all"].clone(),
                r["crates"].clone(),
                r["conformance"].clone(),
                r["ui"].clone(),
                r["eslint"].clone(),
                r["apps"].clone()
            ),
            (
                json!(false),
                json!(true),
                json!(true),
                json!(false),
                json!(false),
                json!(["debrief", "dyno"])
            )
        );
        let other = gitea_plan(&["conformance/release/latest-json.mjs"], &g);
        assert_eq!(
            (other["all"].clone(), other["crates"].clone(), other["ui"].clone()),
            (json!(false), json!(false), json!(false))
        );
        assert_eq!(plan(&["conformance/ci/build-folder.sh"])["all"], true, "no reader: everything");
        for f in [
            "conformance/ci/runtime-catalogue.mjs",
            "conformance/src/util/is-main.mjs",
            "conformance/ci/install-test-browser.mjs",
        ] {
            let r = gitea_plan(&[f], &g);
            assert_eq!(
                (r["all"].clone(), r["crates"].clone(), r["ui"].clone()),
                (json!(false), json!(true), json!(false)),
                "{f}"
            );
        }
        assert_eq!(gitea_plan(&["conformance/ci/unrelated.mjs"], &g)["crates"], false);
    }

    #[test]
    fn importer_tracing_is_transitive() {
        let mut tree = base_tree();
        tree.insert(
            ".gitea/actions/chain/action.yml".into(),
            "run: node conformance/ci/chain-a.mjs".into(),
        );
        tree.insert(CI_FILE.into(), ci_text("node a", "node b").replace("install-test-browser", "chain"));
        tree.insert("conformance/ci/chain-a.mjs".into(), "import './chain-b.mjs';".into());
        tree.insert("conformance/ci/chain-b.mjs".into(), "import { c } from './chain-c.mjs';".into());
        tree.insert("conformance/ci/chain-c.mjs".into(), "export const c = 1;".into());
        let g = fixed(&tree, &[], None);
        let mut got = script_importers("conformance/ci/chain-c.mjs", &g);
        got.sort();
        assert_eq!(
            got,
            [
                "conformance/ci/chain-a.mjs",
                "conformance/ci/chain-b.mjs",
                "conformance/ci/chain-c.mjs"
            ]
        );
        let r = gitea_plan(&["conformance/ci/chain-c.mjs"], &g);
        assert_eq!((r["all"].clone(), r["crates"].clone()), (json!(false), json!(true)));
    }

    #[test]
    fn a_mention_only_in_a_comment_is_no_use_and_a_reusable_workflow_is_followed_to_its_callers() {
        let mut tree = base_tree();
        tree.insert(
            CI_FILE.into(),
            format!(
                "# see ./.gitea/actions/only-commented\n{}  # ./.gitea/actions/only-commented\n",
                ci_text("node a", "node b")
            ),
        );
        let g = fixed(&tree, &[], None);
        assert_eq!(gitea_plan(&[".gitea/actions/only-commented/action.yml"], &g)["all"], true);
        let mut tree = base_tree();
        tree.insert(
            ".gitea/workflows/app-e2e.yml".into(),
            "on:\n  workflow_call:\njobs:\n  e2e:\n    steps:\n      - uses: ./.gitea/actions/unused\n".into(),
        );
        tree.insert(
            ".gitea/workflows/dyno-release.yml".into(),
            "jobs:\n  e2e:\n    uses: ./.gitea/workflows/app-e2e.yml\n".into(),
        );
        let r = gitea_plan(&[".gitea/actions/unused/action.yml"], &fixed(&tree, &[], None));
        assert_eq!(
            (r["all"].clone(), r["apps"].clone(), r["crates"].clone()),
            (json!(false), json!(["dyno"]), json!(false))
        );
        tree.insert(".gitea/workflows/dyno-release.yml".into(), "jobs: {}\n".into());
        assert_eq!(
            gitea_plan(&[".gitea/actions/unused/action.yml"], &fixed(&tree, &[], None))["all"],
            true,
            "no caller"
        );
    }

    #[test]
    fn in_files_mode_a_file_that_is_not_dirty_marks_everything() {
        let t = base_tree();
        let changed = [(CI_FILE, ci_text("node a --only C06", "node b"))];
        assert_eq!(gitea_plan(&[CI_FILE], &fixed(&t, &changed, Some(&[CI_FILE])))["all"], false);
        assert_eq!(gitea_plan(&[CI_FILE], &fixed(&t, &changed, Some(&[])))["all"], true);
        assert_eq!(
            gitea_plan(&[".gitea/actions/node-caches/action.yml"], &fixed(&t, &[], Some(&[])))["all"],
            true
        );
        assert_eq!(
            gitea_plan(&["conformance/ci/build-folder.sh"], &fixed(&t, &[], Some(&[])))["all"],
            true
        );
    }

    #[test]
    fn app_capability_notices_e2e_guard_and_generated_inputs() {
        for f in [
            "apps/dyno/app/src-tauri/Cargo.toml",
            "apps/debrief/app/src-tauri/capabilities/dialog.json",
        ] {
            assert_eq!(plan(&[f])["conformance"], true, "{f}");
        }
        assert!(!because(&plan(&["apps/dyno/app/src-tauri/src/lib.rs"])));
        assert_eq!(
            plan(&["apps/dyno/app/src-tauri/src/lib.rs"])["conformance"],
            true,
            "but it is planned: conformance always runs"
        );
        for f in ["ui/projects/paddock-e2e/src/env.ts", "ui/projects/paddock-e2e/src/build-guard.ts"] {
            assert_eq!(plan(&[f])["apps"], json!(["debrief", "dyno"]), "{f}");
        }
        // Only apps with a `notices` block are inputs of the notices tool.
        let mut graph = sample();
        graph.configs.insert(
            "dyno".into(),
            json!({"cargo": "app/src-tauri", "notices": {"x": 1}, "dir": "apps/dyno"}),
        );
        let n = affected(&["scripts/notices.sh".to_string()], &graph, &Options::default()).unwrap();
        assert_eq!(n["apps"], json!(["dyno"]));
        // A generated file marks the crates that name it, or every crate of the workspace when none does.
        let naming = |f: &str| {
            if f.ends_with("dialogs.ts") {
                vec!["crates/core/src/lib.rs".to_string()]
            } else {
                Vec::new()
            }
        };
        let opts = Options {
            crates_naming: Some(&naming),
            ..Options::default()
        };
        let named = affected(&["ui/projects/x/generated/dialogs.ts".to_string()], &sample(), &opts).unwrap();
        assert_eq!(named["packages"]["crates"], json!(["paddock-core", "paddock-k", "paddock-kit"]));
        let unknown = affected(&["ui/projects/x/generated/other.json".to_string()], &sample(), &opts).unwrap();
        assert_eq!(unknown["crates"], true);
        assert_eq!(unknown["packages"]["crates"], json!(["paddock-core", "paddock-k", "paddock-kit"]));
    }
}
