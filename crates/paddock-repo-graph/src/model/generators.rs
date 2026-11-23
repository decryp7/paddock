//! Generators as nodes (ADR 0117 §5, WP-7): each entry of the generator registry is a [`NodeKind::Generator`] node with an
//! `input` edge to every model node it reads, so what makes a generator due is a query on the graph and not a hand list of paths.
//!
//! - [`due`]: the generators a change reaches: through the reverse closure over the edges from the nodes it reads (a script, a
//!   workflow, a crate, with everything they import, run or depend on), through the hand paths for the inputs no edge names, and
//!   through the generator's own outputs (a hand edit of a generated file is stale until regenerated).
//! - [`stale_after_merge`]: the generators a merge of two sides leaves stale: those both sides reach, or one side reaches by its
//!   inputs while the other changed its outputs. Neither side regenerated against the other's inputs.
//!
//! A node a generator reads that the model lacks fails closed: the generator is due on every change, and the reason names it.

use super::glob;
use super::nodes::{Edge, EdgeKind, Model, ModelNode, NodeKind};
use std::collections::BTreeMap;

/// One registry entry as the model sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorDecl {
    /// The registry id.
    pub id: String,
    /// Model node ids it reads: the graph derives everything these need (imports, runs, dependencies).
    pub reads: Vec<String>,
    /// Globs of inputs no edge names (a lockfile, a data file); the hand list, narrowed to what the graph cannot derive.
    pub paths: Vec<String>,
    /// Globs of the files it writes.
    pub outputs: Vec<String>,
}

/// The node id of a generator.
pub fn node_id(id: &str) -> String {
    format!("generator:{id}")
}

/// Why a generator is due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    /// The registry id.
    pub id: String,
    /// One line per reason (`input script:..`, `path ..`, `output ..`, `reads X: not in the model`).
    pub why: Vec<String>,
}

/// Adds a generator node per declaration, with an `input` edge to each node it reads. The reads the model lacks are returned as
/// `(generator id, node id)`: the caller reports them and [`due`] treats the generator as always due.
pub fn add_generators(model: &mut Model, decls: &[GeneratorDecl]) -> Vec<(String, String)> {
    let mut missing = Vec::new();
    for d in decls {
        let id = node_id(&d.id);
        model.add_node(ModelNode::new(&id, NodeKind::Generator, &[]));
        for r in &d.reads {
            if model.node(r).is_none() {
                missing.push((d.id.clone(), r.clone()));
            }
            model.add_edge(Edge::new(&id, r, EdgeKind::Input));
        }
    }
    missing
}

/// The generators `changed` makes due, in declaration order. `model` must hold the generator nodes ([`add_generators`]).
pub fn due(model: &Model, decls: &[GeneratorDecl], changed: &[String]) -> Vec<Due> {
    let (hits, _) = model.reach(changed);
    decls
        .iter()
        .filter_map(|d| {
            let mut why = Vec::new();
            if let Some(h) = hits.get(&node_id(&d.id)) {
                why.extend(h.why.iter().map(|w| w.to_string()));
            }
            for f in changed {
                if d.paths.iter().any(|g| glob::matches(g, f)) {
                    why.push(format!("path {f}"));
                }
                if d.outputs.iter().any(|g| glob::matches(g, f)) {
                    why.push(format!("output {f}"));
                }
            }
            for r in d.reads.iter().filter(|r| model.node(r).is_none()) {
                why.push(format!("reads {r}: not in the model (fail closed)"));
            }
            (!why.is_empty()).then(|| Due {
                id: d.id.clone(),
                why,
            })
        })
        .collect()
}

/// Whether any of `changed` is a file `d` writes.
fn writes_any(d: &GeneratorDecl, changed: &[String]) -> bool {
    changed
        .iter()
        .any(|f| d.outputs.iter().any(|g| glob::matches(g, f)))
}

/// The generators a merge of side `a` and side `b` (each the files that side changed since the merge base) leaves stale: both
/// sides reach it, or one side reaches it and the other changed one of its outputs.
pub fn stale_after_merge(
    model: &Model,
    decls: &[GeneratorDecl],
    a: &[String],
    b: &[String],
) -> Vec<Due> {
    let on_a: BTreeMap<String, Due> = due(model, decls, a)
        .into_iter()
        .map(|d| (d.id.clone(), d))
        .collect();
    let on_b: BTreeMap<String, Due> = due(model, decls, b)
        .into_iter()
        .map(|d| (d.id.clone(), d))
        .collect();
    decls
        .iter()
        .filter_map(|d| {
            let (x, y) = (on_a.get(&d.id), on_b.get(&d.id));
            let mut why = Vec::new();
            match (x, y) {
                (Some(x), Some(y)) => {
                    why.extend(x.why.iter().map(|w| format!("a: {w}")));
                    why.extend(y.why.iter().map(|w| format!("b: {w}")));
                }
                (Some(x), None) if writes_any(d, b) => {
                    why.extend(x.why.iter().map(|w| format!("a: {w}")));
                    why.push("b changed its output".to_string());
                }
                (None, Some(y)) if writes_any(d, a) => {
                    why.extend(y.why.iter().map(|w| format!("b: {w}")));
                    why.push("a changed its output".to_string());
                }
                _ => {}
            }
            (!why.is_empty()).then(|| Due {
                id: d.id.clone(),
                why,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    /// `script:a` imports `script:b`; the generator reads `script:a`, so a change to either file makes it due.
    fn world(with_import: bool) -> (Model, Vec<GeneratorDecl>) {
        let mut m = Model::new();
        m.add_node(ModelNode::new(
            "script:a",
            NodeKind::Script,
            &["tools/a.mjs"],
        ));
        m.add_node(ModelNode::new(
            "script:b",
            NodeKind::Script,
            &["tools/lib/b.mjs"],
        ));
        m.add_node(ModelNode::new(
            "script:c",
            NodeKind::Script,
            &["tools/c.mjs"],
        ));
        if with_import {
            m.add_edge(Edge::new("script:a", "script:b", EdgeKind::Imports));
        }
        let decls = vec![GeneratorDecl {
            id: "notices".into(),
            reads: s(&["script:a"]),
            paths: s(&["data/*.lock"]),
            outputs: s(&["out/NOTICES.md"]),
        }];
        assert!(add_generators(&mut m, &decls).is_empty());
        (m, decls)
    }

    fn ids(d: &[Due]) -> Vec<String> {
        d.iter().map(|x| x.id.clone()).collect()
    }

    #[test]
    fn a_change_to_a_module_a_read_script_imports_makes_the_generator_due() {
        let (m, d) = world(true);
        let got = due(&m, &d, &s(&["tools/lib/b.mjs"]));
        assert_eq!(ids(&got), ["notices"]);
        assert!(
            got[0].why.iter().any(|w| w.contains("input script:a")),
            "{:?}",
            got[0].why
        );
        assert!(
            due(&m, &d, &s(&["tools/c.mjs"])).is_empty(),
            "an unrelated script reaches nothing"
        );
    }

    #[test]
    fn control_without_the_import_edge_the_module_no_longer_reaches_the_generator() {
        let (m, d) = world(false);
        assert!(due(&m, &d, &s(&["tools/lib/b.mjs"])).is_empty());
    }

    #[test]
    fn hand_paths_and_outputs_still_mark_a_generator_and_control_removing_them_does_not() {
        let (m, mut d) = world(true);
        assert_eq!(ids(&due(&m, &d, &s(&["data/x.lock"]))), ["notices"]);
        assert_eq!(
            ids(&due(&m, &d, &s(&["out/NOTICES.md"]))),
            ["notices"],
            "a hand edit of an output"
        );
        d[0].paths.clear();
        d[0].outputs.clear();
        assert!(due(&m, &d, &s(&["data/x.lock", "out/NOTICES.md"])).is_empty());
    }

    #[test]
    fn a_read_the_model_lacks_fails_closed_and_is_named() {
        let mut m = Model::new();
        let d = vec![GeneratorDecl {
            id: "g".into(),
            reads: s(&["script:gone"]),
            paths: s(&["x"]),
            outputs: Vec::new(),
        }];
        let missing = add_generators(&mut m, &d);
        assert_eq!(missing, [("g".to_string(), "script:gone".to_string())]);
        let got = due(&m, &d, &s(&["anything.txt"]));
        assert_eq!(ids(&got), ["g"], "due on every change");
        assert!(got[0].why[0].contains("not in the model"));
    }

    #[test]
    fn a_merge_is_stale_for_what_both_sides_reach_or_one_reaches_while_the_other_changed_the_output()
     {
        let (m, d) = world(true);
        let got = stale_after_merge(&m, &d, &s(&["tools/a.mjs"]), &s(&["tools/lib/b.mjs"]));
        assert_eq!(
            ids(&got),
            ["notices"],
            "each side changed an input, neither regenerated against the other"
        );
        assert!(
            stale_after_merge(&m, &d, &s(&["tools/a.mjs"]), &s(&["tools/c.mjs"])).is_empty(),
            "one side only: its own regeneration holds"
        );
        let out = stale_after_merge(&m, &d, &s(&["tools/a.mjs"]), &s(&["out/NOTICES.md"]));
        assert!(
            out[0]
                .why
                .iter()
                .any(|w| w.contains("output out/NOTICES.md")),
            "{out:?}"
        );
        // control: without the import edge the second side's input change no longer reaches the generator
        let (m2, d2) = world(false);
        assert!(
            stale_after_merge(&m2, &d2, &s(&["tools/a.mjs"]), &s(&["tools/lib/b.mjs"])).is_empty()
        );
    }
}
