//! The registry as nodes of the repository model (ADR 0117 §5, WP-7): `due` and `stale_after_merge` are queries on the graph, and
//! what a generator reads (`reads` in `docs/generators.toml`) is followed through the model's `imports`, `runs` and `depends` edges
//! instead of being typed again as paths. The hand `paths` stay for what no edge names (a lockfile, a data file) and are always
//! part of the answer, so moving an input from `paths` to `reads` can only be checked, never silently dropped (the test of
//! `tests/it/generators.rs` walks every tracked file an earlier path list named).

use super::registry::{Generator, Registry};
use crate::plan::RepoGraph;
use crate::repo_model::{World, build_world};
use paddock_repo_graph::History;
use paddock_repo_graph::model::generators::{self as nodes, Due, GeneratorDecl};
use std::collections::BTreeSet;
use std::path::Path;

/// The registry entries as the model's declarations.
pub fn decls(reg: &Registry) -> Vec<GeneratorDecl> {
    reg.generators.iter().map(decl).collect()
}

fn decl(g: &Generator) -> GeneratorDecl {
    GeneratorDecl {
        id: g.id.clone(),
        reads: g.reads.clone(),
        paths: g.paths.clone(),
        outputs: g.outputs.clone(),
    }
}

/// The world with the registry's generators as nodes, and the reads the model lacks (`generator`, `node`).
pub fn with_generators(world: &mut World, reg: &Registry) -> Vec<(String, String)> {
    nodes::add_generators(&mut world.model, &decls(reg))
}

/// The generators `changed` makes due through the graph (reads), the hand paths and the outputs.
pub fn due(world: &World, reg: &Registry, changed: &[String]) -> Vec<Due> {
    nodes::due(&world.model, &decls(reg), changed)
}

/// The generators a merge of two sides leaves stale (see [`nodes::stale_after_merge`]).
pub fn stale_after_merge(world: &World, reg: &Registry, a: &[String], b: &[String]) -> Vec<Due> {
    nodes::stale_after_merge(&world.model, &decls(reg), a, b)
}

/// The ids due for `changed` through the model of `root`'s tree. `Err` says why the model could not be built; the caller then
/// treats every generator as due (a check that could not look is never a pass, ER-33).
pub fn due_ids(root: &Path, history: &dyn History, reg: &Registry, changed: &[String]) -> Result<BTreeSet<String>, String> {
    let graph: RepoGraph = crate::plan::read_graph(root, history).map_err(|e| e.to_string())?;
    let mut world = build_world(root, history, &graph, None)?;
    with_generators(&mut world, reg);
    Ok(due(&world, reg, changed).into_iter().map(|d| d.id).collect())
}
