//! The members of the `crates` workspace and what depends on what, read from `cargo metadata --no-deps` (ER-42: the one accessor
//! of the member list; a folder name is not a package name and not every folder under `crates/` is a member).

use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use paddock_repo_graph::{CargoCli, CargoMetadata};

/// One workspace member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The package name (what `-p` takes).
    pub name: String,
    /// Its folder, repository-relative with `/` and no trailing one (`crates/paddock-proc`).
    pub dir: String,
    /// The names of the packages it depends on (normal, dev and build), members or not.
    pub deps: Vec<String>,
}

/// Parses one `cargo metadata --no-deps` JSON text; manifest paths are made relative to `root` (the absolute repository root).
pub fn parse_members(metadata: &str, root: &str) -> Result<Vec<Member>, String> {
    let v: Value = serde_json::from_str(metadata).map_err(|e| format!("cargo metadata is not JSON: {e}"))?;
    let packages = v["packages"].as_array().ok_or("cargo metadata has no \"packages\" list")?;
    let root = root.replace('\\', "/");
    let root = root.trim_end_matches('/');
    let mut out = Vec::new();
    for p in packages {
        let name = p["name"].as_str().ok_or("a package of cargo metadata has no name")?;
        let manifest = p["manifest_path"]
            .as_str()
            .ok_or("a package of cargo metadata has no manifest_path")?
            .replace('\\', "/");
        let dir = manifest.rsplit_once('/').map_or("", |(d, _)| d);
        let rel = dir
            .strip_prefix(root)
            .map(|r| r.trim_start_matches('/'))
            .ok_or_else(|| format!("the manifest of {name} ({manifest}) is outside {root}"))?;
        let deps = p["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| d["name"].as_str().map(str::to_string))
            .collect();
        out.push(Member {
            name: name.to_string(),
            dir: rel.to_string(),
            deps,
        });
    }
    Ok(out)
}

/// The members of workspace `ws` (repository-relative) under `root`, or why cargo could not say.
pub fn read_members(root: &Path, ws: &str) -> Result<Vec<Member>, String> {
    crate::test_cost::charge(crate::test_cost::Class::Graph, "pre_push::members::read_members")?;
    let json = CargoCli::new(root, Duration::from_secs(120))
        .metadata(&format!("{ws}/Cargo.toml"))
        .map_err(|e| format!("cargo metadata for {ws} failed: {e}"))?;
    parse_members(&json, &root.to_string_lossy())
}

/// The packages whose doctests a change can break: the members holding a changed file and everything in the workspace that
/// depends on them (a public API change breaks a dependent's example). None when a changed file under `ws` is in no member (the
/// workspace manifest, its lockfile and cargo config, a deleted or renamed crate's folder): the whole workspace is reached.
pub fn reached(members: &[Member], ws: &str, files: &[String]) -> Option<BTreeSet<String>> {
    let prefix = format!("{ws}/");
    let mut touched = BTreeSet::new();
    for f in files.iter().filter(|f| f.starts_with(&prefix)) {
        let m = members
            .iter()
            .filter(|m| f.strip_prefix(m.dir.as_str()).is_some_and(|r| r.starts_with('/')))
            .max_by_key(|m| m.dir.len())?;
        touched.insert(m.name.clone());
    }
    let mut all = touched;
    loop {
        let more: Vec<String> = members
            .iter()
            .filter(|m| !all.contains(&m.name) && m.deps.iter().any(|d| all.contains(d)))
            .map(|m| m.name.clone())
            .collect();
        if more.is_empty() {
            return Some(all);
        }
        all.extend(more);
    }
}
