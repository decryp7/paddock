//! The input hash of a node (ADR 0117 §5, review P1-8; the prerequisite of ADR 0116's test-result cache): the text a node's
//! content address is the git blob id of. Pure: the caller reads the git object ids, the compiler lines and the environment and
//! passes them in ([`HashInputs`]); this module decides which nodes and files belong to a node and in what order they are written.
//!
//! What it holds, in order (a change to any line changes the address):
//! 1. the node's own files and every file its `reads`, `imports`, `path-ref` and `input` edges name, as `git ls-tree` lines;
//! 2. the hashes of the nodes it depends on;
//! 3. the build-script environment (the variables a build script of the closure declares with `rerun-if-env-changed`);
//! 4. the contract's command lines for its phases, and the nextest, Vitest and Playwright configuration files that select and run it;
//! 5. the CI image and the OS;
//! 6. the toolchain lines of the tool's own hash (`rustc -vV`, the hashed environment variables and the cargo config digests).

use super::nodes::{EdgeKind, Model};
use std::collections::BTreeSet;

/// The edges a node needs its target's *files* through (what it reads), as opposed to `depends`, whose target contributes its hash.
const FILE_EDGES: [EdgeKind; 5] = [
    EdgeKind::Reads,
    EdgeKind::Imports,
    EdgeKind::PathRef,
    EdgeKind::Input,
    EdgeKind::ReadsScript,
];

/// Everything one node's hash is made of.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HashInputs {
    /// The node.
    pub node: String,
    /// `git ls-tree` lines of the node's own files and the files its file edges name (sorted by the caller's paths).
    pub tree: String,
    /// The nodes it depends on with their hashes, sorted by id.
    pub deps: Vec<(String, String)>,
    /// The build-script variables that are set, `NAME=value`, sorted.
    pub build_env: Vec<String>,
    /// The contract's command lines for the node's phases, as the contract states them.
    pub contract: Vec<String>,
    /// `git ls-tree` lines of the nextest, Vitest and Playwright configuration files that select and run the node.
    pub runner_configs: String,
    /// The CI image: the tag and the digest or version files that pin it.
    pub image: String,
    /// The operating system.
    pub os: String,
    /// The toolchain lines of the tool's own hash.
    pub toolchain: String,
}

/// The canonical text of the inputs: one headed section per part, so no part can pass for another.
pub fn canonical(h: &HashInputs) -> String {
    let mut out = format!("paddock-input-hash 1\nnode {}\n", h.node);
    let mut section = |name: &str, lines: &[String]| {
        out.push_str(&format!("[{name}]\n"));
        for l in lines {
            out.push_str(l.trim_end_matches('\n'));
            out.push('\n');
        }
    };
    section(
        "tree",
        &h.tree.lines().map(str::to_string).collect::<Vec<_>>(),
    );
    section(
        "deps",
        &h.deps
            .iter()
            .map(|(id, hash)| format!("{id} {hash}"))
            .collect::<Vec<_>>(),
    );
    section("build-env", &h.build_env);
    section("contract", &h.contract);
    section(
        "runner-configs",
        &h.runner_configs
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>(),
    );
    section("image", std::slice::from_ref(&h.image));
    section("os", std::slice::from_ref(&h.os));
    section(
        "toolchain",
        &h.toolchain.lines().map(str::to_string).collect::<Vec<_>>(),
    );
    out
}

/// The nodes `id` depends on (`depends` edges, transitively): each one's hash is part of `id`'s. Sorted, without `id`.
pub fn dependencies(model: &Model, id: &str) -> Vec<String> {
    closure(model, id, &[EdgeKind::Depends])
}

/// The nodes whose files are part of `id`'s: itself and everything its file edges reach, transitively over the file edges and
/// `depends` (a dependency's own files are in the dependency's hash, not here: only a read of one counts). Sorted.
pub fn read_set(model: &Model, id: &str) -> Vec<String> {
    let mut set: BTreeSet<String> = closure(model, id, &FILE_EDGES).into_iter().collect();
    set.insert(id.to_string());
    set.into_iter().collect()
}

fn closure(model: &Model, id: &str, kinds: &[EdgeKind]) -> Vec<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut todo = vec![id.to_string()];
    while let Some(n) = todo.pop() {
        for k in kinds {
            for t in model.targets(&n, *k) {
                if t != id && seen.insert(t.to_string()) {
                    todo.push(t.to_string());
                }
            }
        }
    }
    seen.into_iter().collect()
}

/// The files the nodes own, as paths for `git ls-tree` (a folder entry loses its trailing slash). Sorted, without duplicates.
pub fn files_of(model: &Model, ids: &[String]) -> Vec<String> {
    let set: BTreeSet<String> = ids
        .iter()
        .filter_map(|i| model.node(i))
        .flat_map(|n| n.files.iter().map(|f| f.trim_end_matches('/').to_string()))
        .collect();
    set.into_iter().collect()
}

/// The environment variables a build script declares it reads: the names after `rerun-if-env-changed=` and the literal arguments
/// of `env::var(..)` / `env::var_os(..)`. A name built at run time cannot be seen; the hash then misses it, and the caller says so
/// in its report. Sorted, without duplicates.
pub fn build_script_vars(text: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        if let Some(i) = line.find("rerun-if-env-changed=") {
            let name: String = line[i + "rerun-if-env-changed=".len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                out.insert(name);
            }
        }
        for call in ["env::var(\"", "env::var_os(\""] {
            let mut rest = line;
            while let Some(i) = rest.find(call) {
                rest = &rest[i + call.len()..];
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() && rest[name.len()..].starts_with('"') {
                    out.insert(name);
                }
            }
        }
    }
    out.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::nodes::{Edge, ModelNode, NodeKind};

    fn model() -> Model {
        let mut m = Model::new();
        m.add_node(ModelNode::new("crate:a", NodeKind::Project, &["a/"]));
        m.add_node(ModelNode::new("crate:b", NodeKind::Project, &["b/"]));
        m.add_node(ModelNode::new("crate:c", NodeKind::Project, &["c/"]));
        m.add_node(ModelNode::new("data:d", NodeKind::Doc, &["d/table.toml"]));
        m.add_edge(Edge::new("crate:a", "crate:b", EdgeKind::Depends));
        m.add_edge(Edge::new("crate:b", "crate:c", EdgeKind::Depends));
        m.add_edge(Edge::new("crate:a", "data:d", EdgeKind::Reads));
        m
    }

    #[test]
    fn dependencies_are_transitive_and_the_read_set_holds_the_node_and_what_it_reads() {
        let m = model();
        assert_eq!(dependencies(&m, "crate:a"), ["crate:b", "crate:c"]);
        assert_eq!(read_set(&m, "crate:a"), ["crate:a", "data:d"]);
        assert_eq!(
            files_of(&m, &read_set(&m, "crate:a")),
            ["a", "d/table.toml"]
        );
    }

    #[test]
    fn control_a_node_with_no_edges_depends_on_and_reads_nothing_but_itself() {
        let mut m = model();
        m = {
            let mut n = Model::new();
            for x in m.nodes() {
                n.add_node(x.clone());
            }
            n
        };
        assert!(dependencies(&m, "crate:a").is_empty());
        assert_eq!(read_set(&m, "crate:a"), ["crate:a"]);
    }

    fn base() -> HashInputs {
        HashInputs {
            node: "crate:a".into(),
            tree: "100644 blob 1111 a/lib.rs\n".into(),
            deps: vec![("crate:b".into(), "bbbb".into())],
            build_env: vec!["PADDOCK_SETUP_PRODUCT=x".into()],
            contract: vec!["cargo nextest run".into()],
            runner_configs: "100644 blob 2222 a/.config/nextest.toml\n".into(),
            image: "paddock-ci:v3 3".into(),
            os: "macos".into(),
            toolchain: "release: 1.100.0\n".into(),
        }
    }

    #[test]
    fn every_part_changes_the_text_and_an_unchanged_input_does_not() {
        let b = canonical(&base());
        assert_eq!(b, canonical(&base()), "deterministic");
        type Edit = fn(&mut HashInputs);
        let edits: [(&str, Edit); 8] = [
            ("tree", |h| h.tree.push_str("100644 blob 3333 a/x.rs\n")),
            ("deps", |h| h.deps[0].1 = "cccc".into()),
            ("build env", |h| {
                h.build_env.push("PADDOCK_SETUP_TEST_VERSION=1".into())
            }),
            ("contract command line", |h| {
                h.contract[0] = "cargo test".into()
            }),
            ("runner config", |h| {
                h.runner_configs = "100644 blob 4444 a/.config/nextest.toml\n".into()
            }),
            ("ci image", |h| h.image = "paddock-ci:v4 4".into()),
            ("os", |h| h.os = "linux".into()),
            ("toolchain", |h| h.toolchain = "release: 1.101.0\n".into()),
        ];
        for (what, edit) in edits {
            let mut h = base();
            edit(&mut h);
            assert_ne!(canonical(&h), b, "{what} must change the hash input");
        }
    }

    #[test]
    fn a_line_cannot_pass_for_a_line_of_another_section() {
        let mut a = base();
        a.build_env = vec!["x".into()];
        a.contract = Vec::new();
        let mut b = base();
        b.build_env = Vec::new();
        b.contract = vec!["x".into()];
        assert_ne!(canonical(&a), canonical(&b));
    }

    #[test]
    fn a_build_script_names_its_variables_by_rerun_and_by_literal_reads() {
        let text = "println!(\"cargo:rerun-if-env-changed=PADDOCK_SETUP_PRODUCT\");\nlet v = std::env::var(\"CARGO_CFG_TARGET_OS\").ok();\nlet w = env::var_os(\"OUT_DIR\");\nlet n = std::env::var(name);\n";
        assert_eq!(
            build_script_vars(text),
            ["CARGO_CFG_TARGET_OS", "OUT_DIR", "PADDOCK_SETUP_PRODUCT"]
        );
        assert!(
            build_script_vars("fn main() {}").is_empty(),
            "control: no read, no variable"
        );
    }
}
