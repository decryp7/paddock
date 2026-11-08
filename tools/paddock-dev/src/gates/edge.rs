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

/// `paddock-conformance` output (`list --json`, `commands --json`) through node.
pub fn conformance_json(root: &Path, sub: &str) -> Result<String, String> {
    Real.run("node", &["conformance/bin/paddock-conformance.mjs", sub, "--json"], root, None)
        .map_err(|e| format!("{} (node must be on PATH)", e.message))
}

/// The register at a revision, if the revision and the file exist.
pub fn register_at(root: &Path, rev: &str) -> Option<String> {
    Real.run("git", &["show", &format!("{rev}:docs/gates.toml")], root, None).ok()
}

/// The history of `docs/gates.toml`, oldest first. A shallow clone gives the part it has.
pub fn register_history(root: &Path) -> Result<Vec<Version>, String> {
    let list = Real
        .run("git", &["log", "--reverse", "--format=%H", "--", "docs/gates.toml"], root, None)
        .map_err(|e| e.message)?;
    Ok(list
        .lines()
        .filter_map(|sha| {
            let text = register_at(root, sha)?;
            let message = Real.run("git", &["log", "-1", "--format=%B", sha], root, None).ok()?;
            Some(Version {
                sha: sha.to_string(),
                message,
                text,
            })
        })
        .collect())
}

/// Whether the checkout is shallow (`git rev-parse --is-shallow-repository`).
pub fn is_shallow(root: &Path) -> bool {
    Real.run("git", &["rev-parse", "--is-shallow-repository"], root, None)
        .is_ok_and(|o| o.trim() == "true")
}

/// What the planner said per file, or why it could not.
type Answers = Result<std::collections::BTreeMap<String, std::collections::BTreeSet<String>>, String>;

/// The planner's answers, from `scripts/affected-marks.mjs` (one node run for every file asked about).
pub struct NodePlanner {
    root: PathBuf,
    answers: std::cell::RefCell<Option<Answers>>,
}

impl NodePlanner {
    /// A planner that asks the repository's own `scripts/affected-marks.mjs`.
    pub fn new(root: &Path) -> NodePlanner {
        NodePlanner {
            root: root.to_path_buf(),
            answers: std::cell::RefCell::new(None),
        }
    }

    /// Why the planner could not answer, if it could not.
    pub fn failure(&self) -> Option<String> {
        self.answers.borrow().as_ref().and_then(|r| r.as_ref().err().cloned())
    }
}

impl super::gating::Planner for NodePlanner {
    fn prefetch(&self, files: &[String]) {
        let input = serde_json::Value::from(files.to_vec()).to_string();
        let run = Real
            .run("node", &["scripts/affected-marks.mjs"], &self.root, Some(input.as_bytes()))
            .map_err(|e| format!("scripts/affected-marks.mjs failed: {} (node must be on PATH)", e.message))
            .and_then(|out| {
                serde_json::from_str::<std::collections::BTreeMap<String, Vec<String>>>(&out)
                    .map_err(|e| format!("scripts/affected-marks.mjs printed no JSON: {e}"))
            })
            .map(|m| m.into_iter().map(|(k, v)| (k, v.into_iter().collect())).collect());
        *self.answers.borrow_mut() = Some(run);
    }

    fn marks(&self, file: &str) -> Option<std::collections::BTreeSet<String>> {
        match self.answers.borrow().as_ref()? {
            Ok(m) => m.get(file).cloned(),
            Err(_) => None,
        }
    }
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
    register_at(root, &rev)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &Path, args: &[&str]) {
        Real.run("git", args, root, None)
            .unwrap_or_else(|e| panic!("git {args:?}: {}", e.message));
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
    }
}
