//! The train replay of the migration net (ADR 0117 §8, review P1-6): every non-merge member commit of a range (trains 15 to 20)
//! planned against its parent by the rule tables and by the model, and every difference listed with its category. A narrowing no
//! category explains is a defect; [`unlisted`] names them, and the golden test fails on any.
//!
//! The model is built once from the tree the replay runs on ([`super::build_world`]); each commit is then a change set asked of it
//! ([`super::reach_in`]), with the texts before and after read at the commit and its parent. The rule tables answer through
//! [`crate::plan::plan_on_graph`] on the same graph.

use super::{World, compare_targets, reach_in};
use crate::plan::{RepoGraph, Request, plan_on_graph};
use paddock_repo_graph::History;
use serde_json::{Value, json};
use std::path::Path;

/// How many reasons a golden keeps per difference.
const WHY_SHOWN: usize = 3;

/// One member commit and the parent it is planned against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The commit.
    pub sha: String,
    /// Its first parent.
    pub parent: String,
}

/// The non-merge commits of `range` (`<from>..<to>`), oldest first, each with its first parent. One `git rev-list`.
pub fn members(root: &Path, range: &str) -> Result<Vec<Member>, String> {
    if range.starts_with('-') || !range.contains("..") {
        return Err(format!("the range {range:?} is not <from>..<to>"));
    }
    let mut cmd = crate::git_env::git();
    cmd.arg("-C")
        .arg(root)
        .args(["rev-list", "--no-merges", "--parents", "--reverse", range]);
    let out = cmd.output().map_err(|e| format!("git rev-list: {e}"))?;
    if !out.status.success() {
        return Err(format!("git rev-list {range}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            let sha = w.next()?;
            let parent = w.next()?; // a root commit has none and is not a change
            Some(Member {
                sha: sha.to_string(),
                parent: parent.to_string(),
            })
        })
        .collect())
}

/// Why a difference between the rule tables and the model is allowed (ADR 0117 §8's list), or that it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    /// The model runs at least as much (a widening, the safe direction).
    Wider,
    /// 1: `.gitea/**/*.md` is documentation, not global.
    GiteaDocs,
    /// 2, 6 (G4, G5, G6): a mention is not a use, a workflow edit reaches the jobs whose text changed, an action or script reaches
    /// its users as plumbing.
    Workflow,
    /// 3: a test-only reach selects the reached test targets, and does not reach the dependents of the code it tests.
    TestOnly,
    /// 4, 6 (G8): `conformance/` reaches the conformance job and each app's conformance step, not every app's tests.
    ConformanceSplit,
    /// 6 (G2): `.gitattributes` narrowed to the attributes that matter.
    AttributesG2,
    /// 6 (G3): a root folder is declared owned or not built; an unknown root file fails closed, a "not built" one reaches nothing.
    RootFolderG3,
    /// 6 (G9): an npm dependent gives the UI half, a Cargo edge the Rust half; the other half is not run.
    HalfSplit,
    /// A generated file (written by a shared crate's test) reaches the project that writes it, not that project's dependents: the
    /// declared input has `adds`, and no source of the crate changed.
    DeclaredInput,
    /// A defect of WP-3's `selection`, pinned until it is fixed: a `release` reach selects the installers only, so an app that
    /// declares no smoke set loses its test suite. ADR 0117 §5: never less than declared.
    ReleaseInstallersOnly,
    /// A narrowing nothing explains: a defect until it is analysed.
    Unlisted,
}

impl Category {
    /// The word in the golden.
    pub fn name(self) -> &'static str {
        match self {
            Category::Wider => "wider",
            Category::GiteaDocs => "1-gitea-docs",
            Category::Workflow => "2-6-workflow-g4-g5-g6",
            Category::TestOnly => "3-test-only",
            Category::ConformanceSplit => "4-conformance-split-g8",
            Category::AttributesG2 => "6-g2-attributes",
            Category::RootFolderG3 => "6-g3-root-folder",
            Category::HalfSplit => "6-g9-half-split",
            Category::DeclaredInput => "6-generated-input",
            Category::ReleaseInstallersOnly => "DEFECT-release-installers-only",
            Category::Unlisted => "UNLISTED",
        }
    }
}

/// One project where the model and the rule tables differ on a commit.
#[derive(Debug, Clone, PartialEq)]
pub struct Diff {
    /// The project id.
    pub project: String,
    /// What the rule tables run (`null`: nothing).
    pub rules: Option<String>,
    /// What the model selects.
    pub model: Option<String>,
    /// The model runs less than the rule tables.
    pub narrowing: bool,
    /// Why it is allowed.
    pub category: Category,
    /// The targets the model no longer runs.
    pub dropped: Vec<String>,
    /// The rule tables' reasons for running the project (a few).
    pub why_rules: Vec<String>,
    /// The model's reasons (a few).
    pub why_model: Vec<String>,
}

/// A replayed commit.
#[derive(Debug, Clone, PartialEq)]
pub struct Replayed {
    /// The member.
    pub member: Member,
    /// The commit's subject.
    pub subject: String,
    /// The files it changed.
    pub changed: Vec<String>,
    /// What went wrong asking either planner (the commit counts as an unlisted difference).
    pub error: Option<String>,
    /// The differences.
    pub diffs: Vec<Diff>,
    /// How long the rule tables and the model each took for this commit (ER-27: the replay logs where its time goes).
    pub took: (std::time::Duration, std::time::Duration),
}

/// How many commits are planned at once. The rule tables dominate a replay (measured 2026-11-23, 392 commits: 157.6 s of them against
/// 15.5 s for the model, one thread, `docs/benchmarks.md`), and they are git reads, so a few workers help; more than four would only
/// crowd the other tests of a run (ER-48).
pub const WORKERS: usize = 4;

/// Replays `members` on a built world, in `WORKERS` threads, results in the order of `members`. The rule tables and the model both
/// see the tree the graph and the world were read from.
pub fn replay(
    root: &Path,
    history: &(dyn History + Sync),
    graph: &RepoGraph,
    world: &World,
    members: &[Member],
    subject_of: &(dyn Fn(&str) -> String + Sync),
) -> Vec<Replayed> {
    let one = |m: &Member| replay_one(root, history, graph, world, m, subject_of);
    let workers = WORKERS.min(members.len().max(1));
    let mut out: Vec<Option<Replayed>> = vec![None; members.len()];
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|w| {
                let one = &one;
                s.spawn(move || {
                    members
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| i % workers == w)
                        .map(|(i, m)| (i, one(m)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for h in handles {
            for (i, r) in h.join().unwrap_or_default() {
                out[i] = Some(r);
            }
        }
    });
    members
        .iter()
        .zip(out)
        .map(|(m, r)| {
            r.unwrap_or_else(|| {
                failed(
                    m,
                    String::new(),
                    Vec::new(),
                    "the worker thread of this commit panicked".to_string(),
                )
            })
        })
        .collect()
}

fn replay_one(
    root: &Path,
    history: &(dyn History + Sync),
    graph: &RepoGraph,
    world: &World,
    m: &Member,
    subject_of: &(dyn Fn(&str) -> String + Sync),
) -> Replayed {
    let req = Request {
        files: Vec::new(),
        all: false,
        explain: true,
        root: None,
        base: Some(m.parent.clone()),
        head: m.sha.clone(),
    };
    let subject = subject_of(&m.sha);
    let changed = match super::changed_files(history, &req) {
        Ok(c) => c,
        Err(e) => return failed(m, subject, Vec::new(), e),
    };
    let t0 = std::time::Instant::now();
    let plan = match plan_on_graph(root, history, &req, graph) {
        Ok(p) => p,
        Err(e) => return failed(m, subject, changed, format!("the rule tables: {e}")),
    };
    let old = t0.elapsed();
    let t1 = std::time::Instant::now();
    let reached = reach_in(world, root, &req, &changed, graph);
    let c = compare_targets(&plan, &reached, graph, world);
    let diffs = diffs_of(&c, &changed);
    Replayed {
        member: m.clone(),
        subject,
        changed,
        error: None,
        diffs,
        took: (old, t1.elapsed()),
    }
}

/// The differences of a [`compare_targets`] answer for a change, each with its category: the one reader of the comparison for the
/// replay and for the per-rule goldens.
pub fn diffs_of(c: &Value, changed: &[String]) -> Vec<Diff> {
    c["differences"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| {
            let project = d["project"].as_str().unwrap_or_default().to_string();
            let narrowing = c["dropped"].as_array().is_some_and(|l| l.iter().any(|p| p == &json!(project)));
            let strings = |v: &Value| -> Vec<String> {
                v.as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|t| t.as_str().map(str::to_string))
                    .collect()
            };
            Diff {
                rules: d["rules"].as_str().map(str::to_string),
                model: d["model"].as_str().map(str::to_string),
                narrowing,
                category: categorise(&project, narrowing, changed, d),
                dropped: strings(&c["droppedTargets"][&project]),
                why_rules: strings(&d["whyRules"]).into_iter().take(WHY_SHOWN).collect(),
                why_model: strings(&d["whyModel"]).into_iter().take(WHY_SHOWN).collect(),
                project,
            }
        })
        .collect()
}

fn failed(m: &Member, subject: String, changed: Vec<String>, why: String) -> Replayed {
    Replayed {
        member: m.clone(),
        subject,
        changed,
        error: Some(why),
        diffs: Vec::new(),
        took: Default::default(),
    }
}

/// Whether a path is test code (a spec, a test folder, a stand-in under `tests/`): a change to it reaches the tests that run it and
/// nothing that depends on the code it tests.
pub fn is_test_file(f: &str) -> bool {
    const SUFFIXES: [&str; 7] = [".spec.ts", ".test.ts", ".test.mjs", ".test.js", ".test.cjs", ".e2e.ts", "_test.rs"];
    const FOLDERS: [&str; 6] = ["/e2e/", "/tests/", "/test/", "/selftest/", "/fixtures/", "/testing/"];
    SUFFIXES.iter().any(|s| f.ends_with(s))
        || FOLDERS.iter().any(|d| f.contains(d))
        || f.starts_with("conformance/test/")
        || f.starts_with("tests/")
}

/// Whether a changed file is documentation or a register read by prose checks: it never explains a reach by itself.
fn is_prose(f: &str) -> bool {
    f.ends_with(".md") || f.starts_with("docs/")
}

/// The category one reason of the rule tables falls under (the reason is how the old planner reached the project), if any.
fn reason_category(reason: &str, changed: &[String], half: bool) -> Option<Category> {
    let tests_only = || changed.iter().filter(|f| !is_prose(f)).all(|f| is_test_file(f));
    let generated_only = || changed.iter().filter(|f| !is_prose(f)).all(|f| f.contains("/generated/"));
    if let Some(file) = reason.strip_prefix("global ") {
        return match file {
            f if f.starts_with(".gitea/") && f.ends_with(".md") => Some(Category::GiteaDocs),
            f if f.starts_with(".gitea/") => Some(Category::Workflow),
            ".gitattributes" => Some(Category::AttributesG2),
            _ => None,
        };
    }
    if reason.starts_with("conformance conformance/") || reason.starts_with("its checks run paddock-conformance") {
        return Some(Category::ConformanceSplit);
    }
    if reason.starts_with("unknown root file ") {
        return Some(Category::RootFolderG3);
    }
    if reason.contains(" used by ") || reason.contains(", job ") || reason.starts_with("its workflow ") {
        return Some(Category::Workflow);
    }
    if reason.starts_with("npm ") && reason.ends_with("src-tauri") {
        return Some(Category::HalfSplit);
    }
    if reason.starts_with("cargo ") || reason.starts_with("npm ") {
        return if tests_only() {
            Some(Category::TestOnly)
        } else if generated_only() {
            Some(Category::DeclaredInput)
        } else {
            // An edge reaches the half it belongs to: where the model keeps that half, the other is the G9 split.
            half.then_some(Category::HalfSplit)
        };
    }
    if !reason.contains(' ') && is_test_file(reason) {
        return Some(Category::TestOnly);
    }
    None
}

/// The category of one difference: `Wider` when the model runs more; for a narrowing, the §8 entry the change falls under. A
/// narrowing is explained only when every reason the rule tables gave is (a reason nothing lists keeps it `Unlisted`).
fn categorise(_project: &str, narrowing: bool, changed: &[String], d: &Value) -> Category {
    if !narrowing {
        return Category::Wider;
    }
    let model = d["model"].as_str();
    if model == Some("release") {
        return Category::ReleaseInstallersOnly;
    }
    // The model runs one half where the rule tables ran both: a path under the project gave "all", an edge gives its own half.
    let half = d["rules"].as_str() == Some("all") && matches!(model, Some("rust" | "ui"));
    let reasons: Vec<&str> = d["whyRules"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if reasons.is_empty() {
        return Category::Unlisted;
    }
    let cats: Vec<Option<Category>> = reasons
        .iter()
        .map(|r| reason_category(r, changed, half).or_else(|| (half && !r.contains(' ')).then_some(Category::HalfSplit)))
        .collect();
    if cats.iter().any(Option::is_none) {
        return Category::Unlisted;
    }
    cats.into_iter().flatten().max().unwrap_or(Category::Unlisted)
}

/// The narrowings pinned as defects of WP-3's selection (they must shrink to none; the golden holds them).
pub fn defects(r: &[Replayed]) -> usize {
    r.iter()
        .flat_map(|c| &c.diffs)
        .filter(|d| d.category == Category::ReleaseInstallersOnly)
        .count()
}

/// Every narrowing no category explains, and every commit a planner could not answer.
pub fn unlisted(r: &[Replayed]) -> Vec<String> {
    let mut out = Vec::new();
    for c in r {
        if let Some(e) = &c.error {
            out.push(format!("{} {}: {e}", &c.member.sha[..9], c.subject));
        }
        for d in c.diffs.iter().filter(|d| d.category == Category::Unlisted) {
            out.push(format!(
                "{} {}: {} ran {:?} and the model selects {:?}, dropping {:?}",
                &c.member.sha[..9],
                c.subject,
                d.project,
                d.rules,
                d.model,
                d.dropped
            ));
        }
    }
    out
}

/// The replay as the golden records it: per commit the changed-file count and the differences with their category and drops.
pub fn to_json(r: &[Replayed]) -> Value {
    Value::Array(
        r.iter()
            .map(|c| {
                json!({"commit": c.member.sha, "subject": c.subject, "files": c.changed.len(), "error": c.error,
                       "changed": if c.diffs.is_empty() { Vec::new() } else { c.changed.iter().take(12).cloned().collect() },
                       "differences": c.diffs.iter().map(|d| json!({"project": d.project, "rules": d.rules, "model": d.model,
                           "category": d.category.name(), "dropped": d.dropped, "whyRules": d.why_rules, "whyModel": d.why_model})).collect::<Vec<_>>()})
            })
            .collect(),
    )
}
