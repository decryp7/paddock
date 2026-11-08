//! Is a check gated? (ADR 0060 §2.6.) Answers come from the CI model only: a check counts as gated at a level when a
//! reachable, resolved, non-report-only line runs it there. A line the model could not resolve that mentions the check makes
//! the answer "check broken" (ER-33), never a pass.

use super::checks::{Check, Kind, Needs, Scope};
use super::ci::{CiModel, Level, Reach, Status};
use super::shell;
use super::tree::{Tree, glob_regex};
use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};

/// A command CI can run, with the levels it runs at (npm scripts expanded).
#[derive(Debug, Clone)]
pub struct Invocation {
    /// Status per level.
    pub levels: BTreeMap<Level, Status>,
    /// The command.
    pub text: String,
    /// Its folder.
    pub cwd: String,
    /// Environment of the step, plus assignments written before the command.
    pub env: BTreeMap<String, String>,
    /// Where it is written.
    pub file: String,
    /// The line there.
    pub line: usize,
    /// The command line could not be resolved (its folder or a value is unknown); only mentions matter.
    pub unresolved: bool,
    /// Path-like words of the command, resolved against its folder.
    pub paths: Vec<String>,
}

/// One `npm run` of a script, by workspace folder (`root` for the root package).
#[derive(Debug, Clone)]
pub struct NpmRun {
    /// Status per level.
    pub levels: BTreeMap<Level, Status>,
    /// The package folder (`ui`, `root`).
    pub dir: String,
    /// The script name.
    pub script: String,
}

/// The invocations of the model and what they expand to.
#[derive(Debug, Default)]
pub struct Index {
    /// Every command, npm script bodies included.
    pub invocations: Vec<Invocation>,
    /// Every npm script run.
    pub npm_runs: Vec<NpmRun>,
}

struct Packages {
    scripts: BTreeMap<String, BTreeMap<String, String>>,
    by_name: BTreeMap<String, String>,
}

fn packages(tree: &dyn Tree) -> Packages {
    let mut p = Packages {
        scripts: BTreeMap::new(),
        by_name: BTreeMap::new(),
    };
    for f in tree
        .files()
        .iter()
        .filter(|f| f.as_str() == "package.json" || f.ends_with("/package.json"))
    {
        if f.contains("node_modules/") {
            continue;
        }
        let Some(j) = tree.read(f).and_then(|t| serde_json::from_str::<Json>(&t).ok()) else {
            continue;
        };
        let dir = f.strip_suffix("/package.json").unwrap_or("root").to_string();
        if let Some(n) = j.get("name").and_then(Json::as_str) {
            p.by_name.insert(n.to_string(), dir.clone());
        }
        let scripts = j
            .get("scripts")
            .and_then(Json::as_object)
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        p.scripts.insert(dir, scripts);
    }
    p
}

fn split_env(text: &str) -> (BTreeMap<String, String>, String) {
    let mut env = BTreeMap::new();
    let mut rest = text.trim_start();
    while let Some((word, after)) = rest.split_once(char::is_whitespace) {
        match word.split_once('=') {
            Some((k, v))
                if !k.is_empty()
                    && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !k.starts_with(|c: char| c.is_ascii_digit()) =>
            {
                env.insert(k.to_string(), v.trim_matches(|c| c == '"' || c == '\'').to_string());
                rest = after.trim_start();
            }
            _ => break,
        }
    }
    (env, rest.to_string())
}

/// Parses `npm test`, `npm run s`, `npm run s -w ws`, `npm run -w ws s`, `npm t`; returns (script, workspace).
fn npm_command(text: &str) -> Option<(String, Option<String>)> {
    let t: Vec<&str> = text.split_whitespace().collect();
    if t.first() != Some(&"npm") {
        return None;
    }
    let mut ws = None;
    let mut script = None;
    let mut i = 1;
    let mut run = false;
    while i < t.len() {
        match t[i] {
            "test" | "t" | "tst" if script.is_none() && !run => script = Some("test".to_string()),
            "run" | "run-script" => run = true,
            "-w" | "--workspace" => {
                ws = t.get(i + 1).map(|s| s.to_string());
                i += 1;
            }
            x if x.starts_with("--workspace=") => ws = Some(x["--workspace=".len()..].to_string()),
            x if x.starts_with('-') => {}
            x if run && script.is_none() => script = Some(x.to_string()),
            _ => {}
        }
        i += 1;
    }
    script.map(|s| (s, ws))
}

#[allow(clippy::too_many_arguments)]
fn expand(
    idx: &mut Index,
    pk: &Packages,
    levels: &BTreeMap<Level, Status>,
    text: &str,
    cwd: &str,
    env: &BTreeMap<String, String>,
    file: &str,
    line: usize,
    unresolved: bool,
    depth: u8,
) {
    let (assign, cmd) = split_env(text);
    let mut e = env.clone();
    e.extend(assign);
    idx.invocations.push(Invocation {
        levels: levels.clone(),
        text: cmd.clone(),
        cwd: cwd.to_string(),
        env: e.clone(),
        file: file.to_string(),
        line,
        unresolved,
        paths: Vec::new(),
    });
    if let Some(last) = idx.invocations.last_mut() {
        last.paths = token_paths_of(&last.text, &last.cwd);
    }
    let Some((script, ws)) = npm_command(&cmd) else { return };
    let dir = match &ws {
        Some(w) => pk
            .by_name
            .get(w)
            .cloned()
            .or_else(|| pk.scripts.contains_key(w.as_str()).then(|| w.clone())),
        None => Some(if cwd.is_empty() { "root".to_string() } else { cwd.to_string() }),
    };
    let Some(dir) = dir else { return };
    idx.npm_runs.push(NpmRun {
        levels: levels.clone(),
        dir: dir.clone(),
        script: script.clone(),
    });
    if depth >= 3 {
        return;
    }
    if let Some(body) = pk.scripts.get(&dir).and_then(|s| s.get(&script)) {
        let pkg_dir = if dir == "root" { String::new() } else { dir.clone() };
        for c in shell::commands(body, Some(pkg_dir.clone())) {
            expand(
                idx,
                pk,
                levels,
                &c.text,
                c.cwd.as_deref().unwrap_or(&pkg_dir),
                &e,
                file,
                line,
                unresolved,
                depth + 1,
            );
        }
    }
}

/// Builds the index of invocations from the model.
pub fn index(model: &CiModel, tree: &dyn Tree) -> Index {
    let pk = packages(tree);
    let mut idx = Index::default();
    for l in &model.lines {
        let unresolved = l.cwd.is_none() || l.levels.values().any(|s| *s == Status::Unresolved);
        expand(
            &mut idx,
            &pk,
            &l.levels,
            &l.text,
            l.cwd.as_deref().unwrap_or(""),
            &l.env,
            &l.file,
            l.line,
            unresolved,
            0,
        );
    }
    idx
}

/// The answer for one check.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Gating {
    /// Levels with a resolved, gating line.
    pub gated: BTreeSet<Level>,
    /// Levels where the check runs only under a condition the model cannot decide.
    pub conditional: BTreeSet<Level>,
    /// Levels where it runs but cannot fail the job.
    pub report_only: BTreeSet<Level>,
    /// `file:line` of unresolved lines that mention the check: the model is blind there.
    pub unresolved: Vec<String>,
    /// For per-app checks: the apps no gating line covers.
    pub missing_apps: Vec<String>,
    /// Registered harnesses whose files run the test.
    pub harness: BTreeSet<String>,
    /// A per-app check that no app needs (no app has what it applies to): nothing to gate.
    pub not_applicable: bool,
}

impl Gating {
    fn add(&mut self, levels: &BTreeMap<Level, Status>) {
        for (l, s) in levels {
            match s {
                Status::Gated => {
                    self.gated.insert(*l);
                }
                Status::ReportOnly => {
                    self.report_only.insert(*l);
                }
                Status::Conditional => {
                    self.conditional.insert(*l);
                }
                Status::Unresolved => {}
            }
        }
    }

    /// Folds `other` in (the levels a union of ways to run it gives).
    fn merge(&mut self, o: Gating) {
        self.gated.extend(o.gated);
        self.conditional.extend(o.conditional);
        self.report_only.extend(o.report_only);
        self.unresolved.extend(o.unresolved);
        self.harness.extend(o.harness);
    }

    fn clean(mut self) -> Gating {
        self.conditional = self.conditional.difference(&self.gated.clone()).copied().collect();
        self.report_only = self.report_only.difference(&self.gated.clone()).copied().collect();
        self.unresolved.sort();
        self.unresolved.dedup();
        self
    }
}

/// The facts about the apps that per-app checks need.
#[derive(Debug, Default, Clone)]
pub struct AppFacts {
    /// Apps and whether each has Rust and a UI, from `paddock.ci.json`.
    pub apps: BTreeMap<String, (bool, bool)>,
}

impl AppFacts {
    /// Reads the contracts.
    pub fn from_contracts(contracts: &BTreeMap<String, String>) -> AppFacts {
        let mut apps = BTreeMap::new();
        for (name, text) in contracts {
            let Ok(j) = serde_json::from_str::<Json>(text) else { continue };
            let rust = j.get("cargo").and_then(Json::as_str).is_some_and(|c| !c.is_empty());
            let ui = j.get("ui").and_then(Json::as_array).is_some_and(|a| !a.is_empty()) || j.get("angularDir").is_some();
            apps.insert(name.clone(), (rust, ui));
        }
        AppFacts { apps }
    }

    /// The apps a check with these needs applies to.
    pub fn applicable(&self, needs: Needs) -> Vec<String> {
        self.apps
            .iter()
            .filter(|(_, (rust, ui))| match needs {
                Needs::All => true,
                Needs::Rust => *rust,
                Needs::Ui => *ui,
            })
            .map(|(n, _)| n.clone())
            .collect()
    }
}

/// A conformance run found in a command.
struct ConfRun {
    only: Option<Vec<String>>,
    strict: Vec<String>,
    /// The tree it checks: `""` is the repository, `apps/x` an app.
    root: String,
    app: Option<String>,
}

fn conformance_run(inv: &Invocation) -> Option<ConfRun> {
    let tokens: Vec<&str> = inv
        .text
        .split_whitespace()
        .map(|t| t.trim_matches(|c| c == '"' || c == '\''))
        .collect();
    let at = tokens.iter().position(|t| {
        t.rsplit('/')
            .next()
            .is_some_and(|b| b == "paddock-conformance.mjs" || b == "paddock-conformance")
    })?;
    let rest = &tokens[at + 1..];
    if rest.first().is_some_and(|t| !t.starts_with("--")) {
        return None; // a subcommand: commit-lint, sync, run, list, window-weight
    }
    let flag = |name: &str| {
        rest.iter()
            .position(|t| *t == name)
            .and_then(|i| rest.get(i + 1))
            .map(|s| s.to_string())
    };
    let list = |name: &str| {
        flag(name).map(|v| {
            v.split(',')
                .map(|s| s.trim().to_uppercase())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
    };
    let root = match flag("--root") {
        Some(r) => shell::join(&inv.cwd, &r)?,
        None => inv.cwd.clone(),
    };
    let app = flag("--app").or_else(|| root.strip_prefix("apps/").and_then(|r| r.split('/').next()).map(str::to_string));
    Some(ConfRun {
        only: list("--only"),
        strict: list("--strict").unwrap_or_default(),
        root,
        app,
    })
}

fn mentions(inv: &Invocation, needle: &str) -> bool {
    inv.text.contains(needle)
}

fn gate_conformance(check: &Check, idx: &Index, apps: &AppFacts) -> Gating {
    let mut root = Gating::default();
    let mut per_app: BTreeMap<String, Gating> = BTreeMap::new();
    for inv in &idx.invocations {
        let Some(run) = conformance_run(inv) else {
            if inv.unresolved && mentions(inv, "paddock-conformance") && mentions(inv, &check.id) {
                root.unresolved.push(format!("{}:{}", inv.file, inv.line));
            }
            continue;
        };
        if run.only.as_ref().is_some_and(|o| !o.contains(&check.id)) {
            continue;
        }
        let mut levels = inv.levels.clone();
        if check.report_only && !run.strict.contains(&check.id) {
            for s in levels.values_mut() {
                if *s == Status::Gated {
                    *s = Status::ReportOnly;
                }
            }
        }
        if inv.unresolved {
            root.unresolved.push(format!("{}:{}", inv.file, inv.line));
            continue;
        }
        if run.root.is_empty() {
            root.add(&levels);
        }
        if let Some(a) = run.app.filter(|a| run.root.starts_with("apps/") || a != "paddock") {
            per_app.entry(a).or_default().add(&levels);
        }
    }
    let applicable = apps.applicable(check.needs);
    let all_apps = |per_app: &BTreeMap<String, Gating>| -> (Gating, Vec<String>) {
        let mut missing = Vec::new();
        let mut common: Option<Gating> = None;
        for a in &applicable {
            match per_app.get(a) {
                Some(g) if !g.gated.is_empty() => {
                    common = Some(match common {
                        None => g.clone(),
                        Some(c) => Gating {
                            gated: c.gated.intersection(&g.gated).copied().collect(),
                            ..c
                        },
                    });
                }
                _ => missing.push(a.clone()),
            }
        }
        (common.unwrap_or_default(), missing)
    };
    let mut out = Gating::default();
    match check.scope {
        Scope::Root => out.merge(root),
        Scope::Apps if applicable.is_empty() => out.not_applicable = true,
        Scope::Apps => {
            let (g, missing) = all_apps(&per_app);
            if missing.is_empty() {
                out.merge(g);
            } else {
                out.missing_apps = missing;
                // levels some apps have, for the report
                for g in per_app.values() {
                    out.conditional.extend(g.conditional.iter().copied());
                }
            }
            out.unresolved = root.unresolved;
        }
        Scope::Both => {
            let (g, missing) = all_apps(&per_app);
            if missing.is_empty() && !applicable.is_empty() {
                out.merge(g);
            }
            out.merge(root);
        }
    }
    out.clean()
}

fn gate_dev(check: &Check, idx: &Index) -> Gating {
    let mut out = Gating::default();
    let sub = super::checks::CHECKS.iter().find(|i| i.id == check.id).map_or("", |i| i.subcommand);
    for inv in &idx.invocations {
        let tokens: Vec<&str> = inv.text.split_whitespace().collect();
        let has_dev = tokens
            .iter()
            .any(|t| t.trim_matches('"').ends_with("./dev") || *t == "dev" || t.ends_with("/dev"));
        let sub_tokens: Vec<&str> = sub.split_whitespace().collect();
        let present = has_dev && tokens.windows(sub_tokens.len()).any(|w| w == sub_tokens.as_slice());
        if !present {
            if inv.unresolved && mentions(inv, "gates check") {
                out.unresolved.push(format!("{}:{}", inv.file, inv.line));
            }
            continue;
        }
        let mut levels = inv.levels.clone();
        if tokens.contains(&"--warn") {
            for s in levels.values_mut() {
                if *s == Status::Gated {
                    *s = Status::ReportOnly;
                }
            }
        }
        out.add(&levels);
    }
    out.clean()
}

fn token_paths_of(text: &str, cwd: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|t| t.trim_matches(|c| c == '"' || c == '\'' || c == ';'))
        .filter(|t| !t.starts_with('-') && (t.contains('/') || [".mjs", ".sh", ".ps1", ".js", ".cjs"].iter().any(|e| t.ends_with(e))))
        .filter_map(|t| shell::join(cwd, t))
        // `.paddock/` is the same repository checked out a second time (app-release.yml).
        .map(|p| p.strip_prefix(".paddock/").map_or(p.clone(), str::to_string))
        .collect()
}

fn gate_script(check: &Check, idx: &Index) -> Gating {
    let path = &check.id;
    let mut out = Gating::default();
    let name = path.rsplit('/').next().unwrap_or(path);
    for inv in &idx.invocations {
        let runs_tests = {
            let t: Vec<&str> = inv.text.split_whitespace().collect();
            t.first() == Some(&"node") && t.contains(&"--test")
        };
        let mut hit = inv
            .paths
            .iter()
            .any(|p| p == path || (p.contains('*') && glob_regex(p).is_match(path)));
        if !hit && runs_tests && path.ends_with(".test.mjs") {
            let dirs: Vec<String> = inv
                .text
                .split_whitespace()
                .skip(1)
                .filter(|t| !t.starts_with('-') && *t != "node")
                .filter_map(|t| shell::join(&inv.cwd, t.trim_matches(|c| c == '"' || c == '\'')))
                .collect();
            hit = if dirs.is_empty() {
                inv.cwd.is_empty() || path.starts_with(&format!("{}/", inv.cwd))
            } else {
                dirs.iter()
                    .any(|d| d.is_empty() || path.starts_with(&format!("{d}/")) || (d.contains('*') && glob_regex(d).is_match(path)))
            };
        }
        if hit && !inv.unresolved {
            out.add(&inv.levels);
        } else if inv.unresolved && mentions(inv, name) {
            out.unresolved.push(format!("{}:{}", inv.file, inv.line));
        }
    }
    out.clean()
}

/// `npm run s` is gated, and so is the command written out: CI that runs the script's own command in the package folder
/// (`npx ng test` for `"test": "ng test"`) gates the script just the same.
fn gate_npm(check: &Check, idx: &Index, tree: &dyn Tree) -> Gating {
    let mut out = Gating::default();
    for r in &idx.npm_runs {
        if format!("{}:{}", r.dir, r.script) == check.id {
            out.add(&r.levels);
        }
    }
    let (dir, script) = check.id.split_once(':').unwrap_or(("root", &check.id));
    let pkg = if dir == "root" {
        "package.json".to_string()
    } else {
        format!("{dir}/package.json")
    };
    let body = tree.read(&pkg).and_then(|t| serde_json::from_str::<Json>(&t).ok()).and_then(|j| {
        j.get("scripts")
            .and_then(|s| s.get(script))
            .and_then(Json::as_str)
            .map(str::to_string)
    });
    if let Some(body) = body {
        let want = body.trim().trim_start_matches("npx ").trim().to_string();
        let folder = if dir == "root" { "" } else { dir };
        for inv in idx.invocations.iter().filter(|i| i.cwd == folder && !i.unresolved) {
            if inv.text.trim().trim_start_matches("npx ").trim().starts_with(&want) {
                out.add(&inv.levels);
            }
        }
    }
    out.clean()
}

/// The Cargo workspace folder that holds `file`: the nearest ancestor `Cargo.toml` with `[workspace]`.
pub fn workspace_of(tree: &dyn Tree, file: &str) -> String {
    let mut dir = file.rsplit_once('/').map_or("", |(d, _)| d).to_string();
    loop {
        let manifest = if dir.is_empty() {
            "Cargo.toml".to_string()
        } else {
            format!("{dir}/Cargo.toml")
        };
        if tree.read(&manifest).is_some_and(|t| t.contains("[workspace]")) {
            return dir;
        }
        match dir.rsplit_once('/') {
            Some((d, _)) => dir = d.to_string(),
            None if dir.is_empty() => return String::new(),
            None => dir = String::new(),
        }
    }
}

fn runs_cargo_tests(inv: &Invocation, ws: &str) -> bool {
    let t = &inv.text;
    let is_cargo = t.starts_with("cargo ") || t.contains(" cargo ") || t.starts_with("step cargo");
    (is_cargo && (t.contains("nextest run") || t.contains(" test") || t.contains("llvm-cov")))
        && (inv.cwd == ws || inv.cwd.starts_with(&format!("{ws}/")) || (ws.is_empty()) || t.contains(&format!("{ws}/Cargo.toml")))
}

const IGNORE_FLAGS: &[&str] = &["--run-ignored", "--ignored", "--include-ignored"];

/// Whether a cargo/nextest command line selects this test (target by `--test`, or the function name).
fn selects(text: &str, file: &str, name: &str) -> bool {
    let stem = file.rsplit('/').next().unwrap_or(file).trim_end_matches(".rs");
    let target = if stem == "main" {
        file.rsplit('/').nth(1).unwrap_or(stem)
    } else {
        stem
    };
    let toks: Vec<&str> = text.split_whitespace().collect();
    let by_target = toks
        .windows(2)
        .any(|w| w[0] == "--test" && w[1].trim_matches(|c| c == '"' || c == '\'') == target);
    by_target || text.contains(name)
}

/// Whether a tool or harness file has a cargo/nextest/playwright line that selects the test (`--test <target>` or its name),
/// with an ignore flag for an ignored test.
pub fn tool_runs(text: &str, file: &str, name: &str, ignored: bool) -> bool {
    shell::commands(text, Some(String::new())).iter().any(|c| {
        let t = &c.text;
        let runner = t.contains("cargo") || t.contains("nextest") || t.contains("playwright");
        runner && selects(t, file, name) && (!ignored || IGNORE_FLAGS.iter().any(|f| t.contains(f)))
    })
}

fn gate_test(check: &Check, idx: &Index, tree: &dyn Tree, harness_files: &[(String, String, String)]) -> Gating {
    let (file, name) = check.id.split_once("::").unwrap_or((&check.id, ""));
    let mut out = Gating::default();
    if file.ends_with(".rs") {
        let ws = workspace_of(tree, file);
        for inv in &idx.invocations {
            if !runs_cargo_tests(inv, &ws) {
                continue;
            }
            let ok = match check.kind {
                Kind::IgnoredTest => {
                    if check.detail.as_deref() == Some("debug_assertions") {
                        inv.text.contains("--release")
                            || inv.text.contains("--cargo-profile release")
                            || inv.text.contains("--profile release")
                    } else {
                        IGNORE_FLAGS.iter().any(|f| inv.text.contains(f)) && selects(&inv.text, file, name)
                    }
                }
                _ => check.detail.as_ref().is_some_and(|v| inv.env.get(v).is_some_and(|x| !x.is_empty())),
            };
            if ok && !inv.unresolved {
                out.add(&inv.levels);
            }
        }
    } else {
        // TypeScript: a Playwright or Angular test run in the package of the spec with the variable set.
        let pkg = {
            let mut d = file.rsplit_once('/').map_or("", |(d, _)| d).to_string();
            loop {
                if tree.read(&format!("{d}/package.json")).is_some() || d.is_empty() {
                    break d;
                }
                d = d.rsplit_once('/').map_or(String::new(), |(p, _)| p.to_string());
            }
        };
        for inv in &idx.invocations {
            let runs = inv.text.contains("playwright test") || inv.text.contains("ng test") || inv.text.contains("vitest");
            let var_set = check
                .detail
                .as_ref()
                .is_some_and(|v| v == "process.env" || inv.env.get(v).is_some_and(|x| !x.is_empty()));
            if runs
                && var_set
                && (inv.cwd == pkg || pkg.starts_with(&format!("{}/", inv.cwd)) || inv.cwd.starts_with(&format!("{pkg}/")))
                && !inv.unresolved
            {
                out.add(&inv.levels);
            }
        }
    }
    // Harness gates: a file of a registered harness whose cargo line selects the test (and sets the variable).
    for (hname, _path, text) in harness_files {
        let hit = tool_runs(text, file, name, check.kind == Kind::IgnoredTest);
        if hit {
            out.harness.insert(hname.clone());
        }
    }
    out.clean()
}

fn gate_reach(reach: &Reach, key: &str) -> Gating {
    let mut out = Gating::default();
    if let Some(levels) = reach.get(key) {
        out.add(levels);
    }
    out.clean()
}

/// Everything the gating questions need besides the model.
pub struct Env<'a> {
    /// The model.
    pub model: &'a CiModel,
    /// The invocation index.
    pub index: &'a Index,
    /// The files.
    pub tree: &'a dyn Tree,
    /// Per-app facts.
    pub apps: &'a AppFacts,
    /// `(harness name, path, text)` of every file of every registered harness.
    pub harness_files: &'a [(String, String, String)],
}

/// Whether, where and how a check is gated.
pub fn gating(check: &Check, env: &Env) -> Gating {
    match check.kind {
        Kind::Conformance => gate_conformance(check, env.index, env.apps),
        Kind::DevCheck => gate_dev(check, env.index),
        Kind::Script => gate_script(check, env.index),
        Kind::NpmCheck => gate_npm(check, env.index, env.tree),
        Kind::IgnoredTest | Kind::EnvGatedTest => gate_test(check, env.index, env.tree, env.harness_files),
        Kind::Action => {
            let dir = format!(".gitea/actions/{}", check.id.trim_start_matches("action:"));
            gate_reach(&env.model.actions, &dir)
        }
        Kind::Workflow => {
            let path = check.file.clone().unwrap_or_default();
            gate_reach(&env.model.called, &path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gates::checks::{Check, Needs, Scope};
    use crate::gates::ci::{self, Sources};
    use crate::gates::tree::MemTree;

    const ROOT_CI: &str = "on:\n  pull_request:\njobs:\n  conformance:\n    runs-on: ubuntu-latest\n    defaults:\n      run:\n        working-directory: conformance\n    steps:\n      - run: npm test\n      - working-directory: .\n        run: node conformance/bin/paddock-conformance.mjs --app paddock --root . --only C06,C14 --strict C14\n";

    fn check(kind: Kind, id: &str) -> Check {
        Check {
            kind,
            id: id.to_string(),
            file: None,
            line: 1,
            scope: Scope::Both,
            report_only: false,
            needs: Needs::All,
            detail: None,
        }
    }

    struct World {
        model: CiModel,
        tree: MemTree,
        apps: AppFacts,
    }

    fn world(workflows: &[(&str, &str)], contracts: &[(&str, &str)], files: &[(&str, &str)]) -> World {
        let mut s = Sources::default();
        for (p, t) in workflows {
            if let Some(d) = p.strip_suffix("/action.yml") {
                s.actions.insert(d.to_string(), t.to_string());
            } else {
                s.workflows.insert(p.to_string(), t.to_string());
            }
        }
        for (a, t) in contracts {
            s.contracts.insert(a.to_string(), t.to_string());
        }
        World {
            apps: AppFacts::from_contracts(&s.contracts),
            model: ci::build(&s),
            tree: MemTree::new(files),
        }
    }

    fn ask(w: &World, c: &Check) -> Gating {
        let index = index(&w.model, &w.tree);
        gating(
            c,
            &Env {
                model: &w.model,
                index: &index,
                tree: &w.tree,
                apps: &w.apps,
                harness_files: &[],
            },
        )
    }

    #[test]
    fn a_root_conformance_line_gates_only_the_ids_it_names() {
        let w = world(&[(".gitea/workflows/ci.yml", ROOT_CI)], &[], &[]);
        let mut c06 = check(Kind::Conformance, "C06");
        c06.scope = Scope::Root;
        assert_eq!(ask(&w, &c06).gated, BTreeSet::from([Level::Branch]));
        let mut c13 = check(Kind::Conformance, "C13");
        c13.scope = Scope::Root;
        assert!(ask(&w, &c13).gated.is_empty());
        // report-only unless --strict names it: C14 is strict here, C06 is not
        let mut c06r = c06.clone();
        c06r.report_only = true;
        assert_eq!(ask(&w, &c06r).report_only, BTreeSet::from([Level::Branch]));
        let mut c14 = check(Kind::Conformance, "C14");
        c14.scope = Scope::Root;
        c14.report_only = true;
        assert_eq!(ask(&w, &c14).gated, BTreeSet::from([Level::Branch]));
    }

    #[test]
    fn an_apps_scope_check_needs_every_applicable_app() {
        let ci = "on:\n  pull_request:\njobs:\n  plan:\n    runs-on: ubuntu-latest\n    outputs:\n      any_app: ${{ steps.plan.outputs.any_app }}\n      apps: ${{ steps.plan.outputs.apps }}\n    steps:\n      - id: plan\n        run: node scripts/affected.mjs --ci\n  app:\n    needs: [plan]\n    if: ${{ needs.plan.outputs.any_app == 'true' }}\n    runs-on: ${{ matrix.os }}\n    strategy:\n      matrix:\n        include: ${{ fromJSON(needs.plan.outputs.apps) }}\n    steps:\n      - uses: ./.gitea/actions/app-gates\n        with:\n          checks: ${{ matrix.checks }}\n";
        let gates = "inputs:\n  checks:\n    default: ''\nruns:\n  using: composite\n  steps:\n    - if: inputs.checks != ''\n      shell: bash\n      env:\n        COMMANDS: ${{ inputs.checks }}\n      run: bash \"${{ github.action_path }}/../../../conformance/ci/run-commands.sh\"\n";
        let dyno = r#"{"cargo": ".", "checks": ["node conformance/bin/paddock-conformance.mjs --app dyno --root apps/dyno --only C13 --paddock ."]}"#;
        let spotter = r#"{"cargo": "."}"#;
        let pacenotes = r#"{"checks": ["echo docs"]}"#;
        let w = world(
            &[(".gitea/workflows/ci.yml", ci), (".gitea/actions/app-gates/action.yml", gates)],
            &[("dyno", dyno), ("spotter", spotter), ("pacenotes", pacenotes)],
            &[],
        );
        let mut c13 = check(Kind::Conformance, "C13");
        c13.scope = Scope::Apps;
        c13.needs = Needs::Rust;
        let g = ask(&w, &c13);
        // Spotter has Rust and no line; Pacenotes has no Rust, so it is not applicable
        assert_eq!(g.missing_apps, vec!["spotter".to_string()]);
        assert!(g.gated.is_empty());
        // good: with a line in Spotter's contract too, every applicable app is covered
        let spotter_ok = r#"{"cargo": ".", "checks": ["node conformance/bin/paddock-conformance.mjs --app spotter --root apps/spotter --only C13 --paddock ."]}"#;
        let w = world(
            &[(".gitea/workflows/ci.yml", ci), (".gitea/actions/app-gates/action.yml", gates)],
            &[("dyno", dyno), ("spotter", spotter_ok), ("pacenotes", pacenotes)],
            &[],
        );
        let g = ask(&w, &c13);
        assert!(g.missing_apps.is_empty());
        assert_eq!(g.gated, BTreeSet::from([Level::Branch]));
    }

    #[test]
    fn scripts_are_gated_by_path_glob_or_node_test_dir() {
        let wf = "on:\n  pull_request:\njobs:\n  j:\n    runs-on: ubuntu-latest\n    steps:\n      - run: |\n          cd apps/dyno\n          node scripts/check-capabilities.mjs\n          node --test scripts/check-wording.test.mjs\n      - run: node --test scripts/\n      - run: bash apps/dyno/scripts/*.sh\n";
        let w = world(&[(".gitea/workflows/ci.yml", wf)], &[], &[]);
        for (path, gated) in [
            ("apps/dyno/scripts/check-capabilities.mjs", true),
            ("apps/dyno/scripts/check-wording.test.mjs", true),
            ("scripts/affected.test.mjs", true),
            ("apps/dyno/scripts/check-bundle.sh", true),
            ("apps/dyno/scripts/check-other.mjs", false),
            ("apps/pitcrew/scripts/check-capabilities.mjs", false),
        ] {
            let got = !ask(&w, &check(Kind::Script, path)).gated.is_empty();
            assert_eq!(got, gated, "{path}");
        }
    }

    #[test]
    fn npm_checks_follow_npm_run_and_nested_scripts() {
        let wf = "on:\n  pull_request:\njobs:\n  j:\n    runs-on: ubuntu-latest\n    steps:\n      - run: npm run test:all -w ui\n      - run: npm test\n        working-directory: conformance\n";
        let files = [
            (
                "ui/package.json",
                r#"{"name":"@decryp7/paddock-ui","scripts":{"test:all":"npm run test:tokens && node --test t/x.test.mjs","test:tokens":"node a.mjs","test:visual":"x"}}"#,
            ),
            (
                "conformance/package.json",
                r#"{"name":"c","scripts":{"test":"node --test test/*.test.mjs"}}"#,
            ),
            ("conformance/test/a.test.mjs", ""),
        ];
        let w = world(&[(".gitea/workflows/ci.yml", wf)], &[], &files);
        assert!(!ask(&w, &check(Kind::NpmCheck, "ui:test:tokens")).gated.is_empty());
        assert!(!ask(&w, &check(Kind::NpmCheck, "conformance:test")).gated.is_empty());
        assert!(ask(&w, &check(Kind::NpmCheck, "ui:test:visual")).gated.is_empty());
        assert!(!ask(&w, &check(Kind::Script, "conformance/test/a.test.mjs")).gated.is_empty());
    }

    #[test]
    fn actions_and_workflows_need_a_live_caller() {
        let dead = "on:\n  workflow_call:\njobs:\n  l:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: ./.gitea/actions/licences\n";
        let action = "runs:\n  using: composite\n  steps:\n    - run: echo\n      shell: bash\n";
        let w = world(
            &[
                (".gitea/workflows/app-licences.yml", dead),
                (".gitea/actions/licences/action.yml", action),
            ],
            &[],
            &[],
        );
        let mut wf = check(Kind::Workflow, "workflow:app-licences.yml");
        wf.file = Some(".gitea/workflows/app-licences.yml".into());
        assert!(ask(&w, &wf).gated.is_empty());
        assert!(ask(&w, &check(Kind::Action, "action:licences")).gated.is_empty());
        let caller = "on:\n  pull_request:\njobs:\n  c:\n    uses: ./.gitea/workflows/app-licences.yml\n";
        let w = world(
            &[
                (".gitea/workflows/app-licences.yml", dead),
                (".gitea/actions/licences/action.yml", action),
                (".gitea/workflows/ci.yml", caller),
            ],
            &[],
            &[],
        );
        assert!(!ask(&w, &wf).gated.is_empty());
        assert!(!ask(&w, &check(Kind::Action, "action:licences")).gated.is_empty());
    }

    #[test]
    fn ignored_tests_need_the_flag_the_workspace_and_a_filter() {
        let files = [
            ("apps/dyno/Cargo.toml", "[workspace]\n"),
            ("apps/dyno/crates/dyno-agent/tests/helper_wiring.rs", ""),
        ];
        let test = "apps/dyno/crates/dyno-agent/tests/helper_wiring.rs::every_advertised_reading_has_a_production_sender";
        // bad: runs the workspace, never the ignored tests; names the test without the flag
        let names_only = "on:\n  pull_request:\njobs:\n  j:\n    runs-on: ubuntu-latest\n    steps:\n      - run: |\n          cd apps/dyno\n          cargo nextest run every_advertised_reading_has_a_production_sender\n";
        let w = world(&[(".gitea/workflows/ci.yml", names_only)], &[], &files);
        assert!(ask(&w, &check(Kind::IgnoredTest, test)).gated.is_empty());
        // good: the flag, the workspace and the target
        let good = names_only.replace(
            "cargo nextest run every_advertised_reading_has_a_production_sender",
            "cargo nextest run --run-ignored all --test helper_wiring",
        );
        let w = world(&[(".gitea/workflows/ci.yml", &good)], &[], &files);
        assert_eq!(ask(&w, &check(Kind::IgnoredTest, test)).gated, BTreeSet::from([Level::Branch]));
        // bad: the flag in another workspace
        let other = good.replace("cd apps/dyno", "cd apps/spotter");
        let w = world(&[(".gitea/workflows/ci.yml", &other)], &[], &files);
        assert!(ask(&w, &check(Kind::IgnoredTest, test)).gated.is_empty());
    }

    #[test]
    fn cfg_attr_debug_assertions_counts_only_in_a_release_run() {
        let files = [("crates/Cargo.toml", "[workspace]\n"), ("crates/a/tests/it.rs", "")];
        let mut c = check(Kind::IgnoredTest, "crates/a/tests/it.rs::heavy");
        c.detail = Some("debug_assertions".into());
        let debug = "on:\n  pull_request:\njobs:\n  j:\n    runs-on: ubuntu-latest\n    steps:\n      - run: cargo nextest run\n        working-directory: crates\n";
        assert!(ask(&world(&[(".gitea/workflows/ci.yml", debug)], &[], &files), &c).gated.is_empty());
        let release = debug.replace("cargo nextest run", "cargo nextest run --release");
        assert!(
            !ask(&world(&[(".gitea/workflows/ci.yml", &release)], &[], &files), &c)
                .gated
                .is_empty()
        );
    }

    #[test]
    fn env_gated_tests_need_the_variable_set_where_they_run() {
        let files = [("apps/dyno/Cargo.toml", "[workspace]\n"), ("apps/dyno/crates/x/tests/vm.rs", "")];
        let mut c = check(Kind::EnvGatedTest, "apps/dyno/crates/x/tests/vm.rs::vm_only");
        c.detail = Some("PADDOCK_VM".into());
        let without = "on:\n  pull_request:\njobs:\n  j:\n    runs-on: ubuntu-latest\n    steps:\n      - run: cargo test\n        working-directory: apps/dyno\n";
        assert!(
            ask(&world(&[(".gitea/workflows/ci.yml", without)], &[], &files), &c)
                .gated
                .is_empty()
        );
        let with = without.replace(
            "      - run: cargo test",
            "      - env:\n          PADDOCK_VM: '1'\n        run: cargo test",
        );
        assert!(!ask(&world(&[(".gitea/workflows/ci.yml", &with)], &[], &files), &c).gated.is_empty());
        let inline = without.replace("cargo test", "PADDOCK_VM=1 cargo test");
        assert!(
            !ask(&world(&[(".gitea/workflows/ci.yml", &inline)], &[], &files), &c)
                .gated
                .is_empty()
        );
    }

    #[test]
    fn a_typescript_skip_on_process_env_needs_that_variable_set() {
        let files = [("ui/package.json", "{}"), ("ui/e2e/contact-sheet.spec.ts", "")];
        let mut c = check(Kind::EnvGatedTest, "ui/e2e/contact-sheet.spec.ts::sheet");
        c.detail = Some("SHOTS_DIR".into());
        let plain = "on:\n  pull_request:\njobs:\n  j:\n    runs-on: ubuntu-latest\n    steps:\n      - run: npx playwright test\n        working-directory: ui\n";
        // bad: a Playwright line in the package that does not set the variable
        assert!(ask(&world(&[(".gitea/workflows/ci.yml", plain)], &[], &files), &c).gated.is_empty());
        // bad: a skip whose variable could not be named is never satisfied
        let mut unnamed = c.clone();
        unnamed.detail = None;
        assert!(
            ask(&world(&[(".gitea/workflows/ci.yml", plain)], &[], &files), &unnamed)
                .gated
                .is_empty()
        );
        // good: the variable set on the step
        let with = plain.replace("      - run: npx", "      - env:\n          SHOTS_DIR: out\n        run: npx");
        assert!(!ask(&world(&[(".gitea/workflows/ci.yml", &with)], &[], &files), &c).gated.is_empty());
    }

    #[test]
    fn an_unresolvable_line_that_mentions_the_check_is_reported() {
        let wf = "on:\n  pull_request:\njobs:\n  a:\n    runs-on: ubuntu-latest\n    steps:\n      - working-directory: ${{ steps.s.outputs.dir }}\n        run: node conformance/bin/paddock-conformance.mjs --app paddock --root . --only C13\n";
        let w = world(&[(".gitea/workflows/ci.yml", wf)], &[], &[]);
        let mut c = check(Kind::Conformance, "C13");
        c.scope = Scope::Root;
        let g = ask(&w, &c);
        assert!(g.gated.is_empty());
        assert_eq!(g.unresolved.len(), 1, "{g:?}");
    }

    #[test]
    fn a_secret_in_an_argument_does_not_blind_the_model() {
        let wf = "on:\n  pull_request:\njobs:\n  a:\n    runs-on: ubuntu-latest\n    steps:\n      - run: node conformance/bin/paddock-conformance.mjs --app paddock --root . --only C13 --token ${{ secrets.X }}\n";
        let w = world(&[(".gitea/workflows/ci.yml", wf)], &[], &[]);
        let mut c = check(Kind::Conformance, "C13");
        c.scope = Scope::Root;
        assert_eq!(ask(&w, &c).gated, BTreeSet::from([Level::Branch]));
    }

    #[test]
    fn c100_itself_is_gated_by_its_line_and_a_warn_line_is_report_only() {
        let wf = "on:\n  pull_request:\njobs:\n  c:\n    runs-on: ubuntu-latest\n    steps:\n      - run: ./dev gates check\n";
        let mut c = check(Kind::DevCheck, "C100");
        c.scope = Scope::Root;
        assert_eq!(
            ask(&world(&[(".gitea/workflows/ci.yml", wf)], &[], &[]), &c).gated,
            BTreeSet::from([Level::Branch])
        );
        let warn = wf.replace("gates check", "gates check --warn");
        let g = ask(&world(&[(".gitea/workflows/ci.yml", &warn)], &[], &[]), &c);
        assert!(g.gated.is_empty() && !g.report_only.is_empty());
    }
}
