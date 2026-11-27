//! Declared test reads (ADR 0116 §2, user decision 122): a crate lists the files its tests read, as globs relative to the repository
//! root, in `[package.metadata.paddock] test-reads` of its `Cargo.toml` (read through `cargo metadata`,
//! [`paddock_repo_graph::parse_test_reads`]), as Bazel's `data`, Nx's and Turborepo's `inputs` do.
//!
//! The tracked files a declaration matches become one node per crate (`declared:<crate folder>`) with a `reads` edge from the crate,
//! so the crate's input hash, and the hash of every crate that depends on it, holds their content, and a change to one of them
//! reaches the crate in a plan. A glob that matches no tracked file is a finding ([`Declared::problems`]): the test cache refuses the
//! crate until the declaration is true again.

use paddock_repo_graph::model::glob;
use paddock_repo_graph::model::nodes::{Edge, EdgeKind, Model, ModelNode, NodeKind};
use std::collections::BTreeMap;

/// What one crate declares, against the tracked files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    /// The globs, as written in the manifest.
    pub globs: Vec<String>,
    /// The tracked files any glob matches, sorted.
    pub matched: Vec<String>,
    /// The globs that match no tracked file.
    pub empty: Vec<String>,
}

impl Declared {
    /// What is wrong with the declaration, one line each: a glob that matches no tracked file. A declaration with a problem does not
    /// cover a blind read. An empty list is a true declaration for a crate whose blind reads touch no repository file (a test double
    /// that prints its working folder); the manifest says why in a comment.
    pub fn problems(&self) -> Vec<String> {
        self.empty
            .iter()
            .map(|g| format!("`test-reads` glob `{g}` matches no tracked file"))
            .collect()
    }
}

/// Whether the repository-root-relative glob `g` matches the tracked file `f`. Always anchored at the root: a glob without a slash
/// names a root file, not a file of that name anywhere.
pub fn matches(g: &str, f: &str) -> bool {
    !g.is_empty() && glob::matches(&format!("/{}", g.trim_start_matches('/')), f)
}

/// Every declaration against the tracked `files`, by crate folder.
pub fn resolve(files: &[String], declared: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, Declared> {
    declared
        .iter()
        .map(|(dir, globs)| {
            let mut matched: Vec<String> = files.iter().filter(|f| globs.iter().any(|g| matches(g, f))).cloned().collect();
            matched.sort();
            let empty = globs.iter().filter(|g| !files.iter().any(|f| matches(g, f))).cloned().collect();
            (
                dir.clone(),
                Declared {
                    globs: globs.clone(),
                    matched,
                    empty,
                },
            )
        })
        .collect()
}

/// The id of the node that owns a crate's declared files.
pub fn node_id(dir: &str) -> String {
    format!("declared:{dir}")
}

/// Adds a node per declaring crate owning its matched files outside its own folder (its own files are its code already) and a `reads`
/// edge from the crate. A crate the model does not have is left out.
pub fn add_to_model(model: &mut Model, declared: &BTreeMap<String, Declared>) {
    for (dir, d) in declared {
        let crate_id = format!("crate:{dir}");
        if model.node(&crate_id).is_none() {
            continue;
        }
        let own = format!("{dir}/");
        let files: Vec<&str> = d.matched.iter().filter(|f| !f.starts_with(&own)).map(String::as_str).collect();
        let id = node_id(dir);
        model.add_node(ModelNode::new(&id, NodeKind::Project, &files));
        model.add_edge(Edge::new(crate_id, id, EdgeKind::Reads));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn decl(v: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
        v.iter()
            .map(|(d, g)| (d.to_string(), g.iter().map(|s| s.to_string()).collect()))
            .collect()
    }

    #[test]
    fn globs_are_anchored_at_the_repository_root() {
        assert!(matches("docs/*.toml", "docs/a.toml"));
        assert!(matches("/docs/a.toml", "docs/a.toml"), "a leading slash is the same root");
        assert!(matches("conformance/ci/**", "conformance/ci/x/y.sh"));
        assert!(
            !matches("a.toml", "docs/a.toml"),
            "a glob without a slash is not a name anywhere in the tree"
        );
        assert!(!matches("", "docs/a.toml"), "an empty glob matches nothing");
    }

    #[test]
    fn resolve_lists_what_matches_and_names_the_globs_that_match_nothing() {
        let tracked = files(&["docs/a.toml", "docs/b.md", "crates/x/src/lib.rs"]);
        let got = resolve(&tracked, &decl(&[("crates/x", &["docs/*.toml", "gone/**"])]));
        let d = &got["crates/x"];
        assert_eq!(d.matched, files(&["docs/a.toml"]));
        assert_eq!(d.empty, files(&["gone/**"]));
        assert_eq!(
            d.problems(),
            vec!["`test-reads` glob `gone/**` matches no tracked file".to_string()]
        );
        // Control: the same declaration without the stale glob has no problem.
        assert!(
            resolve(&tracked, &decl(&[("crates/x", &["docs/*.toml"])]))["crates/x"]
                .problems()
                .is_empty()
        );
        assert!(
            resolve(&tracked, &decl(&[("crates/x", &[])]))["crates/x"].problems().is_empty(),
            "an empty list says the blind reads touch no repository file"
        );
    }

    #[test]
    fn the_node_owns_the_files_outside_the_crate_and_the_crate_reads_it() {
        let mut m = Model::new();
        m.add_node(ModelNode::new("crate:crates/x", NodeKind::Project, &["crates/x/"]));
        let tracked = files(&["docs/a.toml", "crates/x/tests/f.txt"]);
        add_to_model(
            &mut m,
            &resolve(
                &tracked,
                &decl(&[
                    ("crates/x", &["docs/*.toml", "crates/x/tests/*"]),
                    ("crates/gone", &["docs/*.toml"]),
                ]),
            ),
        );
        assert_eq!(
            m.node("declared:crates/x").unwrap().files,
            files(&["docs/a.toml"]),
            "its own folder's files are its code already"
        );
        assert!(m.targets("crate:crates/x", EdgeKind::Reads).contains(&"declared:crates/x"));
        assert!(
            m.node("declared:crates/gone").is_none(),
            "a crate the model does not have is left out"
        );
    }
}
