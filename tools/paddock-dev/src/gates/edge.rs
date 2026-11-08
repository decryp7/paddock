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
pub fn register_history(root: &Path) -> Vec<Version> {
    let Ok(list) = Real.run("git", &["log", "--reverse", "--format=%H", "--", "docs/gates.toml"], root, None) else {
        return Vec::new();
    };
    list.lines()
        .filter_map(|sha| {
            let text = register_at(root, sha)?;
            let message = Real.run("git", &["log", "-1", "--format=%B", sha], root, None).ok()?;
            Some(Version {
                sha: sha.to_string(),
                message,
                text,
            })
        })
        .collect()
}

/// Whether the checkout is shallow (`git rev-parse --is-shallow-repository`).
pub fn is_shallow(root: &Path) -> bool {
    Real.run("git", &["rev-parse", "--is-shallow-repository"], root, None)
        .is_ok_and(|o| o.trim() == "true")
}
