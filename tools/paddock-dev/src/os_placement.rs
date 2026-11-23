//! OS placement derived from the code (ADR 0117 §7, WP-6): which packages and modules of a workspace hold code that behaves
//! differently per operating system, read through the repository graph's `os` edges ([`paddock_repo_graph::model::os`]) with rustc's own
//! lexer ([`crate::check::lex::rust`]). The planner ([`crate::host_tests::read`]) and the conformance check C155 both read the
//! placement here, so they cannot disagree about what the code says (ER-22).
//!
//! The unit is a workspace: `crates`, `apps/<app>` or a one-crate workspace such as `tools/paddock-dev`, whose tests are selected by
//! test-name prefix, so it is judged module by module.

use crate::check::lex::blank_ranges;
use crate::check::lex::rust::{comment_ranges, mask, without_test_items};
use crate::host_tests::Derived;
use paddock_repo_graph::model::extract::{BulkRead, build};
use paddock_repo_graph::model::nodes::Model;
use paddock_repo_graph::model::os::{OsExtractor, RustLex, Signal, WINDOWS_MARK, marks, non_member, select_os};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::LazyLock;

/// rustc's own lexer behind the graph's port.
pub struct RustcLex;

impl RustLex for RustcLex {
    fn mask(&self, text: &str) -> String {
        mask(text)
    }

    fn uncommented(&self, text: &str) -> String {
        blank_ranges(text, &comment_ranges(text))
    }

    fn production(&self, masked: &str) -> String {
        without_test_items(masked)
    }
}

/// The files of a repository read from disk: a path that is not there is `Ok(None)`; one that cannot be read or is not text is an
/// error (fail closed).
pub struct DiskBulk(pub PathBuf);

impl BulkRead for DiskBulk {
    fn read_all(&self, paths: &[String]) -> Vec<paddock_repo_graph::model::extract::Read> {
        paths
            .iter()
            .map(|p| match std::fs::read_to_string(self.0.join(p)) {
                Ok(t) => Ok(Some(t)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(format!("{p}: {e}")),
            })
            .collect()
    }
}

static PACKAGE_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^name\s*=\s*"([^"]+)""#).unwrap_or_else(|e| panic!("{e}")));

/// A workspace's placement with what explains it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Placement {
    /// What the planner and C155 read.
    pub derived: Derived,
    /// The folder of each member package.
    pub dirs: BTreeMap<String, String>,
    /// The packages and modules that hold Windows code themselves (not through a module that calls them).
    pub windows: BTreeSet<String>,
    /// Why a package or module is host code: the marks that reach it, each with the node that holds it (`os:cfg` at `crate:crates/x`).
    pub why: BTreeMap<String, Vec<(String, String)>>,
}

/// `(package name, folder)` of every member package under `root` (a `Cargo.toml` with a `[package]` that is not a vendored, docs or
/// fuzz crate and has no `[workspace]` of its own).
fn member_packages(root: &str, files: &[String], texts: &BTreeMap<&str, &str>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for f in files.iter().filter(|f| f.rsplit('/').next() == Some("Cargo.toml")) {
        let Some(text) = texts.get(f.as_str()) else { continue };
        // A one-crate workspace is its own `[workspace]` and is still a package of itself.
        let own_root = f.trim_end_matches("Cargo.toml").trim_end_matches('/') == root;
        if !text.contains("[package]") || (non_member(f, text) && !own_root) {
            continue;
        }
        let after = &text[text.find("[package]").unwrap_or(0)..];
        if let Some(name) = PACKAGE_NAME.captures(after).and_then(|c| c.get(1)) {
            out.push((
                name.as_str().to_string(),
                f.trim_end_matches("Cargo.toml").trim_end_matches('/').to_string(),
            ));
        }
    }
    out
}

/// The placement of the workspace at `root` (`crates`, `apps/dyno`, `tools/paddock-dev`) from the tracked `files`, read through
/// `read` in bulk (the manifests once more, for the package names). A file that cannot be read is an error (fail closed: a
/// placement read from part of the code would take tests off a host).
pub fn derive(root: &str, files: &[String], read: &dyn BulkRead) -> Result<Placement, String> {
    derive_without(root, files, read, &[])
}

/// [`derive`] with some kinds of signal left out: the measurement of what one signal (`std::process::Command`) decides.
pub fn derive_without(root: &str, files: &[String], read: &dyn BulkRead, ignoring: &[Signal]) -> Result<Placement, String> {
    let prefix = format!("{}/", root.trim_end_matches('/'));
    let mine: Vec<String> = files.iter().filter(|f| f.starts_with(&prefix)).cloned().collect();
    let manifests: Vec<String> = mine
        .iter()
        .filter(|f| f.rsplit('/').next() == Some("Cargo.toml"))
        .cloned()
        .collect();
    let manifest_texts = paddock_repo_graph::model::extract::texts_of("os", read, &manifests).map_err(|e| e.to_string())?;
    let texts: BTreeMap<&str, &str> = manifest_texts.iter().map(|(f, t)| (f.as_str(), t.as_str())).collect();
    let packages = member_packages(root.trim_end_matches('/'), &manifests, &texts);
    // A one-crate workspace (the root folder is itself a package) is judged module by module: its tests are selected by test-name prefix.
    let module_crates: Vec<String> = packages
        .iter()
        .filter(|(_, d)| d == root.trim_end_matches('/'))
        .map(|(_, d)| d.clone())
        .collect();
    let mut extractor = OsExtractor::new(&RustcLex, &module_crates);
    for s in ignoring {
        extractor = extractor.ignoring(*s);
    }
    let adapters: Vec<Box<dyn paddock_repo_graph::model::extract::Extractor>> = vec![Box::new(extractor)];
    let built = build(&mine, read, &adapters);
    if let Some(e) = built.errors.first() {
        return Err(format!("the OS placement of {root} cannot be derived: {e}"));
    }
    Ok(placement(&built.model, &packages, &module_crates))
}

fn placement(model: &Model, packages: &[(String, String)], module_crates: &[String]) -> Placement {
    let mut out = Placement::default();
    for (name, dir) in packages {
        out.derived.known_packages.insert(name.clone());
        out.dirs.insert(name.clone(), dir.clone());
        let id = format!("crate:{dir}");
        if select_os(model, &id, &[]).len() > 1 {
            out.derived.packages.insert(name.clone());
            out.why.insert(name.clone(), marks(model, &id, &[]).into_iter().collect());
        }
        if own_windows(model, &id) {
            out.windows.insert(name.clone());
        }
    }
    for dir in module_crates {
        let prefix = format!("module:{dir}::");
        for n in model.nodes().filter(|n| n.id.starts_with(&prefix)) {
            let module = format!("{}::", &n.id[prefix.len()..]);
            out.derived.known_modules.insert(module.clone());
            if own_windows(model, &n.id) {
                out.windows.insert(module.clone());
            }
            if select_os(model, &n.id, &[]).len() > 1 {
                out.derived.modules.insert(module.clone());
                out.why.insert(module, marks(model, &n.id, &[]).into_iter().collect());
            }
        }
    }
    out
}

/// Whether the node itself (not a caller of it) holds Windows code.
fn own_windows(model: &Model, id: &str) -> bool {
    model
        .targets(id, paddock_repo_graph::model::nodes::EdgeKind::Os)
        .contains(&WINDOWS_MARK)
}

/// The workspaces whose contract declares `hostTests`, with the contract's path: the shared crates, paddock-dev and each app.
pub fn workspaces(files: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (root, contract) in [
        ("crates", "crates/paddock.ci.json"),
        ("tools/paddock-dev", "tools/paddock-dev/paddock.ci.json"),
    ] {
        if files.iter().any(|f| f == contract) {
            out.push((root.to_string(), contract.to_string()));
        }
    }
    let apps: BTreeSet<&str> = files
        .iter()
        .filter_map(|f| f.strip_prefix("apps/")?.strip_suffix("/paddock.ci.json"))
        .collect();
    out.extend(apps.into_iter().map(|a| (format!("apps/{a}"), format!("apps/{a}/paddock.ci.json"))));
    out
}
