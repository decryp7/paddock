//! The thin edge of C100: reads the checkout into the pure core's inputs.

use super::ci::Sources;
use std::fs;
use std::path::Path;

fn read(p: &Path) -> Result<String, String> {
    fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn entries(dir: &Path) -> Vec<std::fs::DirEntry> {
    let mut v: Vec<_> = fs::read_dir(dir).map(|d| d.flatten().collect()).unwrap_or_default();
    v.sort_by_key(|e| e.file_name());
    v
}

/// Reads `.gitea/workflows`, `.gitea/actions/*/action.yml` and `apps/*/paddock.ci.json` below `root`.
pub fn load_sources(root: &Path) -> Result<Sources, String> {
    let mut s = Sources::default();
    for e in entries(&root.join(".gitea/workflows")) {
        let name = e.file_name().to_string_lossy().to_string();
        if name.ends_with(".yml") || name.ends_with(".yaml") {
            s.workflows.insert(format!(".gitea/workflows/{name}"), read(&e.path())?);
        }
    }
    for e in entries(&root.join(".gitea/actions")) {
        let name = e.file_name().to_string_lossy().to_string();
        for f in ["action.yml", "action.yaml"] {
            let p = e.path().join(f);
            if p.is_file() {
                s.actions.insert(format!(".gitea/actions/{name}"), read(&p)?);
            }
        }
    }
    for e in entries(&root.join("apps")) {
        let name = e.file_name().to_string_lossy().to_string();
        let p = e.path().join("paddock.ci.json");
        if p.is_file() {
            s.contracts.insert(name, read(&p)?);
        }
    }
    for project in crate::ci_run::contract::ROOT_PROJECTS {
        let p = root.join(project).join("paddock.ci.json");
        if p.is_file() {
            s.root_contracts.insert(project.to_string(), read(&p)?);
        }
    }
    Ok(s)
}

use super::register::Version;
use super::tree::Tree;
use crate::input_hash::{Exec, Real};
use std::path::PathBuf;

/// The tracked files of a checkout, read from disk on demand.
pub struct GitTree {
    root: PathBuf,
    names: Vec<String>,
}

impl GitTree {
    /// Lists the tracked files with `git ls-files`.
    pub fn open(root: &Path) -> Result<GitTree, String> {
        let out = Real.run("git", &["ls-files", "-z"], root, None).map_err(|e| e.message)?;
        let mut names: Vec<String> = out.split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect();
        names.sort();
        names.dedup();
        Ok(GitTree {
            root: root.to_path_buf(),
            names,
        })
    }
}

impl Tree for GitTree {
    fn files(&self) -> &[String] {
        &self.names
    }

    fn read(&self, path: &str) -> Option<String> {
        fs::read_to_string(self.root.join(path)).ok()
    }
}

/// `Some(::error:: line)` when `root` is a sparse checkout that lacks folders the gates read (local runs only: CI checks out sparse
/// on purpose). One implementation, `conformance/src/util/full-checkout.mjs`, which also serves conformance and `dev plan`.
pub fn sparse_problem(root: &Path) -> Option<String> {
    let script = "conformance/src/util/full-checkout.mjs";
    if !root.join(script).is_file() {
        return sparse_checkout_on(root).then(|| MISSING_CONFORMANCE.to_string());
    }
    match Real.run("node", &[script, "."], root, None) {
        Ok(_) => None,
        Err(e) => error_line(&e.message),
    }
}

const MISSING_CONFORMANCE: &str = "::error::conformance/ is not in this checkout (a sparse checkout without it), and the gates run it. Fix: run `git sparse-checkout disable` for a full checkout; new worktrees from `./dev worktree add` are full checkouts.";

fn sparse_checkout_on(root: &Path) -> bool {
    std::env::var("GITHUB_ACTIONS").map_or(true, |v| v != "true")
        && Real
            .run("git", &["config", "--get", "core.sparseCheckout"], root, None)
            .is_ok_and(|v| v.trim() == "true")
}

/// The `::error::` line inside a failed run's message (tool name and exit text around it dropped), if there is one.
pub fn error_line(message: &str) -> Option<String> {
    message.find("::error::").map(|i| message[i..].trim().to_string())
}

/// `paddock-conformance` output (`list --json`, `commands --json`) through node. A failure that printed an `::error::` line returns
/// just that line; the node hint is added only when node could not be started or printed nothing useful.
///
/// `list` is the JavaScript host's list plus the checks only the Rust host has (pair P3, ADR 0022 amendment 2026-11-19: `./dev
/// conformance list` holds them, their source is `tools/paddock-dev/src/check/c<n>.rs`): C100 counts them as checks like any other.
pub fn conformance_json(root: &Path, sub: &str) -> Result<String, String> {
    if !root.join("conformance/bin/paddock-conformance.mjs").is_file() {
        return Err(MISSING_CONFORMANCE.to_string());
    }
    let js = Real
        .run("node", &["conformance/bin/paddock-conformance.mjs", sub, "--json"], root, None)
        .map_err(|e| error_line(&e.message).unwrap_or_else(|| format!("{} (node must be on PATH)", e.message)))?;
    if sub != "list" {
        return Ok(js);
    }
    Ok(with_rust_checks(
        &js,
        &crate::check::conformance::list_checks(&crate::check::registry()),
    ))
}

/// `js` (the JavaScript host's `list --json`) with the entries of `rust` whose id it lacks, in id order. Text that is not a JSON
/// array is returned as it is: the reader names that problem.
fn with_rust_checks(js: &str, rust: &serde_json::Value) -> String {
    let Ok(serde_json::Value::Array(mut all)) = serde_json::from_str::<serde_json::Value>(js) else {
        return js.to_string();
    };
    let have: std::collections::BTreeSet<String> = all.iter().filter_map(|c| c["id"].as_str().map(str::to_string)).collect();
    all.extend(
        rust.as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["id"].as_str().is_some_and(|id| !have.contains(id)))
            .cloned(),
    );
    let number = |c: &serde_json::Value| {
        c["id"]
            .as_str()
            .and_then(|i| i.trim_start_matches('C').parse::<u32>().ok())
            .unwrap_or(u32::MAX)
    };
    all.sort_by_key(number);
    serde_json::Value::Array(all).to_string()
}

/// The register at a revision, if the revision and the file exist.
pub fn register_at(root: &Path, rev: &str) -> Option<String> {
    Real.run("git", &["show", &format!("{rev}:docs/gates.toml")], root, None).ok()
}

/// The merge base of `base` and HEAD (or `base` itself when there is none).
fn merge_base(root: &Path, base: &str) -> String {
    Real.run("git", &["merge-base", base, "HEAD"], root, None)
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| base.to_string())
}

/// The history of `docs/gates.toml` that date moves are judged against (§3.1), oldest first: every commit of the register along the
/// first parent of the merge base with `base` (a merge into main is one version, carrying the messages of what it brings in), then one more version for HEAD whose message is the messages of all the branch's
/// commits to the register together. A branch that creates the register, or moves several dates over several commits, is judged by
/// its net effect and the trailers anywhere in its commits; the commits already on the base are judged one by one.
pub fn register_history(root: &Path, base: &str) -> Result<Vec<Version>, String> {
    let mb = merge_base(root, base);
    let list = Real
        .run(
            "git",
            &["log", "--first-parent", "--reverse", "--format=%H", &mb, "--", "docs/gates.toml"],
            root,
            None,
        )
        .map_err(|e| e.message)?;
    let mut out: Vec<Version> = Vec::new();
    for sha in list.lines() {
        // A commit that deleted the register has no version.
        let Some(text) = register_at(root, sha) else { continue };
        let mut message = Real
            .run("git", &["log", "-1", "--format=%B", sha], root, None)
            .map_err(|e| format!("git log {sha}: {}", e.message))?;
        // A merge is one version: it carries the messages of the register commits it brings in (`sha^1..sha`).
        if Real
            .run("git", &["rev-parse", "--verify", "-q", &format!("{sha}^2")], root, None)
            .is_ok()
        {
            let brought = Real
                .run(
                    "git",
                    &["log", "--format=%B", &format!("{sha}^1..{sha}"), "--", "docs/gates.toml"],
                    root,
                    None,
                )
                .map_err(|e| format!("git log {sha}^1..{sha}: {}", e.message))?;
            message.push('\n');
            message.push_str(&brought);
        }
        out.push(Version {
            sha: sha.to_string(),
            message,
            text,
        });
    }
    if !out.is_empty() {
        let messages = Real
            .run(
                "git",
                &["log", "--format=%B", &format!("{mb}..HEAD"), "--", "docs/gates.toml"],
                root,
                None,
            )
            .map_err(|e| e.message)?;
        if let (false, Some(text)) = (messages.trim().is_empty(), register_at(root, "HEAD")) {
            out.push(Version {
                sha: "HEAD".into(),
                message: messages,
                text,
            });
        }
    }
    Ok(out)
}

/// Whether the checkout is shallow (`git rev-parse --is-shallow-repository`).
pub fn is_shallow(root: &Path) -> bool {
    Real.run("git", &["rev-parse", "--is-shallow-repository"], root, None)
        .is_ok_and(|o| o.trim() == "true")
}

/// What the planner said per file, or why it could not.
type Answers = Result<std::collections::BTreeMap<String, std::collections::BTreeSet<String>>, String>;

/// The names of ci.yml's plan job outputs a plan turns on (what C100 compares with a check's planned lines): `crates`, `ui`,
/// `eslint`, `conformance`, `tools`, `icons`, `paddock_dev`, `c99` and `any_app`.
pub fn outputs_of(plan: &serde_json::Value) -> std::collections::BTreeSet<String> {
    let on = |k: &str| plan[k].as_bool().unwrap_or(false);
    let mut out: std::collections::BTreeSet<String> = ["crates", "ui", "eslint", "conformance", "tools", "icons"]
        .into_iter()
        .filter(|k| on(k))
        .map(String::from)
        .collect();
    for (key, name) in [("paddockDev", "paddock_dev"), ("c99", "c99")] {
        if on(key) {
            out.insert(name.into());
        }
    }
    if plan["apps"].as_array().is_some_and(|a| !a.is_empty()) {
        out.insert("any_app".into());
    }
    out
}

/// The planner's answers from `./dev plan` itself, in this process: the graphs are read once (`cargo metadata` of every
/// workspace), then one plan per file, with no workflow narrowing and no file content (a change of the file alone).
pub struct RustPlanner {
    root: PathBuf,
    answers: std::cell::RefCell<Option<Answers>>,
}

impl RustPlanner {
    /// A planner over the repository at `root`.
    pub fn new(root: &Path) -> RustPlanner {
        RustPlanner {
            root: root.to_path_buf(),
            answers: std::cell::RefCell::new(None),
        }
    }
}

impl super::gating::Planner for RustPlanner {
    fn prefetch(&self, files: &[String]) {
        use crate::plan::{Options, affected, conformance_apps, per_machine_apps, read_graph};
        let started = std::time::Instant::now();
        let history = paddock_repo_graph::GitHistory::new(&self.root, std::time::Duration::from_secs(120));
        let run = read_graph(&self.root, &history)
            .map_err(|e| {
                format!(
                    "the planner could not read the graphs for {} files after {} ms: {e}",
                    files.len(),
                    started.elapsed().as_millis()
                )
            })
            .and_then(|graph| {
                let naming = |f: &str| crate::plan_repo::files_naming_generated(&self.root, f);
                let opts = Options {
                    crates_naming: Some(&naming),
                    per_machine_apps: per_machine_apps(&self.root),
                    conformance_apps: conformance_apps(&self.root),
                    ..Options::default()
                };
                files
                    .iter()
                    .map(|f| {
                        affected(std::slice::from_ref(f), &graph, &opts)
                            .map(|plan| (f.clone(), outputs_of(&plan)))
                            .map_err(|e| format!("the planner could not plan {f}: {e}"))
                    })
                    .collect()
            });
        *self.answers.borrow_mut() = Some(run);
    }

    fn failure(&self) -> Option<String> {
        self.answers.borrow().as_ref().and_then(|r| r.as_ref().err().cloned())
    }

    fn marks(&self, file: &str) -> Option<std::collections::BTreeSet<String>> {
        match self.answers.borrow().as_ref()? {
            Ok(m) => m.get(file).cloned(),
            Err(_) => None,
        }
    }
}

/// The planner `dev gates check` and `dev gates list` ask (one constructor for both, so a test can see which planner it is).
pub fn planner(root: &Path) -> RustPlanner {
    RustPlanner::new(root)
}

/// The register at the merge base of `base` and HEAD, so that a branch is compared with the commit it started from and a growing
/// ratchet is caught before the merge (on main itself the base is HEAD, and the nightly has nothing to compare). Falls back to the
/// register at `base` itself when there is no merge base.
pub fn base_register(root: &Path, base: &str) -> Option<String> {
    let rev = Real
        .run("git", &["merge-base", base, "HEAD"], root, None)
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| base.to_string());
    register_at(root, &rev).or_else(|| {
        // A revision that exists but has no register: the branch creates it, so there is no list that could have grown.
        Real.run("git", &["rev-parse", "--verify", "-q", &format!("{rev}^{{commit}}")], root, None)
            .ok()
            .map(|_| "version = 1\n".to_string())
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_rust_hosts_checks_join_the_list_and_the_javascript_one_wins_a_shared_id() {
        let js = r#"[{"id":"C06","file":"a.mjs"},{"id":"C139","file":"b.mjs"}]"#;
        let rust = serde_json::json!([{"id": "C39", "file": "c39.rs"}, {"id": "C139", "file": "c139.rs"}]);
        let merged: serde_json::Value = serde_json::from_str(&super::with_rust_checks(js, &rust)).unwrap();
        let ids: Vec<(&str, &str)> = merged
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c["id"].as_str().unwrap(), c["file"].as_str().unwrap()))
            .collect();
        assert_eq!(ids, [("C06", "a.mjs"), ("C39", "c39.rs"), ("C139", "b.mjs")]);
        assert_eq!(super::with_rust_checks("not json", &rust), "not json");
    }

    use super::*;

    fn git(root: &Path, args: &[&str]) {
        Real.run("git", args, root, None)
            .unwrap_or_else(|e| panic!("git {args:?}: {}", e.message));
    }

    fn entry(review: &str) -> String {
        format!(
            "version = 1\n[[not_gated]]\nid = \"X\"\nreason = \"r\"\nowner = \"o\"\nsince = 2026-11-08\nreview_by = {review}\napproved = \"coordinator\"\n"
        )
    }

    #[test]
    fn a_checkout_without_conformance_is_told_to_disable_the_sparse_checkout_not_given_a_node_error() {
        let dir = tempfile::tempdir().unwrap();
        let e = conformance_json(dir.path(), "list").unwrap_err();
        assert!(
            e.contains("::error::") && e.contains("conformance/") && e.contains("git sparse-checkout disable"),
            "{e}"
        );
        assert!(!e.contains("node must be on PATH"), "{e}");
    }

    #[test]
    fn main_judges_each_merge_as_one_version_and_a_direct_move_without_a_trailer_fails() {
        use crate::gates::register::date_history;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let init = [
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ];
        let git_c = |args: &[&str]| git(root, &[&init[..], args].concat());
        let write = |review: &str| {
            std::fs::create_dir_all(root.join("docs")).unwrap();
            std::fs::write(root.join("docs/gates.toml"), entry(review)).unwrap();
        };
        git(root, &["init", "-q", "-b", "main"]);
        write("2026-12-06");
        git(root, &["add", "."]);
        git_c(&["commit", "-q", "-m", "feat: register"]);
        // a branch moves the date twice (the first commit alone has no trailer; the net move has one)
        git(root, &["checkout", "-q", "-b", "topic"]);
        write("2026-12-20");
        git_c(&["commit", "-q", "-am", "docs: re-plan"]);
        write("2027-01-07");
        git_c(&[
            "commit",
            "-q",
            "-am",
            "docs: re-plan again",
            "-m",
            "Gates-date: X 2026-12-06 -> 2027-01-07; approved: coordinator",
        ]);
        git(root, &["checkout", "-q", "main"]);
        git_c(&["merge", "-q", "--no-ff", "-m", "Merge topic", "topic"]);
        // on main the merge is ONE version carrying the messages it brings in
        let h = register_history(root, "main").unwrap();
        assert_eq!(h.len(), 2, "{:?}", h.iter().map(|v| &v.message).collect::<Vec<_>>());
        assert!(date_history(&h).is_empty(), "{:?}", date_history(&h));
        // a later direct commit on main that moves the date without a trailer is caught
        write("2027-01-06");
        git_c(&["commit", "-q", "-am", "docs: quiet move"]);
        let h = register_history(root, "main").unwrap();
        assert_eq!(h.len(), 3);
        assert!(
            date_history(&h).iter().any(|p| p.message.contains("without a `Gates-date:")),
            "{:?}",
            date_history(&h)
        );
    }

    #[test]
    fn a_trailer_of_a_later_branch_commit_is_still_a_trailer_in_the_joined_messages() {
        use crate::gates::register::date_history;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let init = [
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ];
        let git_c = |args: &[&str]| git(root, &[&init[..], args].concat());
        let write = |text: String| {
            std::fs::create_dir_all(root.join("docs")).unwrap();
            std::fs::write(root.join("docs/gates.toml"), text).unwrap();
        };
        git(root, &["init", "-q", "-b", "main"]);
        write(entry("2026-12-06"));
        git(root, &["add", "."]);
        git_c(&["commit", "-q", "-m", "feat: register"]);
        git(root, &["checkout", "-q", "-b", "topic"]);
        // an earlier commit of the branch touches the register without moving a date, the later one moves it with its trailer
        write(format!("{}# note\n", entry("2026-12-06")));
        git_c(&["commit", "-q", "-am", "docs: a note"]);
        write(format!("{}# note\n", entry("2026-12-20")));
        git_c(&[
            "commit",
            "-q",
            "-am",
            "docs: re-plan",
            "-m",
            "Gates-date: X 2026-12-06 -> 2026-12-20; approved: coordinator",
        ]);
        let h = register_history(root, "main").unwrap();
        assert!(
            date_history(&h).is_empty(),
            "{:?} in {:?}",
            date_history(&h),
            h.last().map(|v| &v.message)
        );
    }

    #[test]
    fn a_branch_is_compared_with_its_merge_base_not_with_the_moved_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let init = [
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ];
        let commit = |msg: &str| git(root, &[&init[..], &["commit", "-q", "-am", msg]].concat());
        git(root, &["init", "-q", "-b", "main"]);
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/gates.toml"), "version = 1\n# base\n").unwrap();
        git(root, &["add", "."]);
        commit("base");
        git(root, &["checkout", "-q", "-b", "topic"]);
        // main moves on after the branch was cut
        git(root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("docs/gates.toml"), "version = 1\n# main moved\n").unwrap();
        commit("main moves");
        git(root, &["checkout", "-q", "topic"]);
        // good: the merge base is the commit the branch started from
        assert_eq!(base_register(root, "main").unwrap(), "version = 1\n# base\n");
        // bad (what the plain base would give): main's later text
        assert_eq!(register_at(root, "main").unwrap(), "version = 1\n# main moved\n");
        // no such revision: nothing to compare with, the caller reports "not measured"
        assert!(base_register(root, "nope").is_none());
        // a revision without a register (the branch creates it): an empty register, nothing could have grown
        git(root, &["checkout", "-q", "--orphan", "bare"]);
        git(root, &["rm", "-rfq", "."]);
        std::fs::write(root.join("x"), "x").unwrap();
        git(root, &["add", "x"]);
        git(root, &[&init[..], &["commit", "-q", "-m", "bare"]].concat());
        assert_eq!(base_register(root, "bare").as_deref(), Some("version = 1\n"));
        git(root, &["checkout", "-q", "topic"]);
        // the history: the base's commits one by one, then HEAD with the messages of the branch's commits together
        std::fs::write(root.join("docs/gates.toml"), "version = 1\n# topic one\n").unwrap();
        commit("first move");
        std::fs::write(root.join("docs/gates.toml"), "version = 1\n# topic two\n").unwrap();
        git(
            root,
            &[
                &init[..],
                &[
                    "commit",
                    "-q",
                    "-am",
                    "second move",
                    "-m",
                    "Gates-date: x 2026-01-01 -> 2026-02-01; approved: coordinator",
                ],
            ]
            .concat(),
        );
        let h = register_history(root, "main").unwrap();
        assert_eq!(h.len(), 2, "{h:?}");
        assert_eq!(h[0].text, "version = 1\n# base\n");
        assert_eq!(h[1].sha, "HEAD");
        assert!(h[1].message.contains("first move") && h[1].message.contains("Gates-date: x 2026-01-01"));
        // a branch that creates the register has no history to judge
        assert!(register_history(root, "nope").is_err() || register_history(root, "nope").unwrap().is_empty());
    }

    #[test]
    fn the_outputs_of_a_plan_are_the_plan_job_names() {
        let plan = serde_json::json!({"crates": true, "ui": false, "tools": true, "paddockDev": true, "c99": false, "ciImage": true, "apps": ["x"]});
        let got: Vec<String> = outputs_of(&plan).into_iter().collect();
        assert_eq!(got, ["any_app", "crates", "paddock_dev", "tools"]);
        assert!(outputs_of(&serde_json::json!({"apps": []})).is_empty());
    }
}
