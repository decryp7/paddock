//! The registry's adapters with their positive controls (ADR 0117 §8): remove the source line of an edge and the edge, and with it
//! the reach, disappears.

use super::*;
use crate::model::Reach;

struct FakeCargo;

impl CargoMetadata for FakeCargo {
    fn metadata(&self, manifest: &str) -> Result<String, GraphError> {
        let pkg = |name: &str, dir: &str, deps: &[&str]| {
            let deps: Vec<String> = deps
                .iter()
                .map(|d| format!(r#"{{"name":"x","path":"/r/{d}"}}"#))
                .collect();
            format!(
                r#"{{"name":"{name}","manifest_path":"/r/{dir}/Cargo.toml","dependencies":[{}]}}"#,
                deps.join(",")
            )
        };
        match manifest {
            "crates/Cargo.toml" => Ok(format!(
                r#"{{"packages":[{},{}]}}"#,
                pkg("paddock-proc", "crates/paddock-proc", &[]),
                pkg(
                    "paddock-watch",
                    "crates/paddock-watch",
                    &["crates/paddock-proc"]
                )
            )),
            m => Err(GraphError::Metadata {
                manifest: m.to_string(),
                reason: "no such workspace".to_string(),
            }),
        }
    }
}

fn repo(watch_deps_on_proc: bool, frontend_dist: bool) -> MemoryRead {
    let mut m = MemoryRead::new()
        .with("crates/Cargo.toml", "[workspace]\nmembers = [\"*\"]\n")
        .with("crates/paddock-proc/Cargo.toml", "[package]\n")
        .with("crates/paddock-watch/Cargo.toml", "[package]\n")
        .with("ui/package.json", r#"{"name":"@d/ui"}"#)
        .with(
            "apps/x/app/package.json",
            r#"{"name":"x","dependencies":{"@d/ui":"1"}}"#,
        );
    if frontend_dist {
        m = m.with(
            "apps/y/src-tauri/tauri.conf.json",
            r#"{"build":{"frontendDist":"../../../ui/dist"}}"#,
        );
    }
    if !watch_deps_on_proc {
        // The fake's answer is fixed; the control drops the edge by dropping the crates workspace root line instead.
        m = m.with("crates/Cargo.toml", "# no workspace\n");
    }
    m
}

fn built(m: &MemoryRead) -> Built {
    let ports = Ports {
        cargo: Some(&FakeCargo),
        root: "/r",
    };
    build(&m.paths(), m, &extractors(&ports))
}

fn edge(b: &Built, from: &str, to: &str) -> Option<EdgeKind> {
    b.model
        .edges()
        .find(|e| e.from == from && e.to == to)
        .map(|e| e.kind)
}

#[test]
fn cargo_emits_crate_nodes_and_depends_edges_and_the_control_removes_them() {
    let b = built(&repo(true, false));
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(
        b.model.node("crate:crates/paddock-proc").map(|n| n.kind),
        Some(NodeKind::Project)
    );
    assert_eq!(
        edge(
            &b,
            "crate:crates/paddock-watch",
            "crate:crates/paddock-proc"
        ),
        Some(EdgeKind::Depends)
    );
    let (hits, _) = b
        .model
        .reach(&["crates/paddock-proc/src/lib.rs".to_string()]);
    assert_eq!(hits["crate:crates/paddock-watch"].reach, Reach::Code);
    // Positive control: without the workspace root line no metadata is asked, the nodes and the edge are gone, nothing is reached.
    let c = built(&repo(false, false));
    assert!(c.model.node("crate:crates/paddock-proc").is_none());
    assert!(
        c.model
            .reach(&["crates/paddock-proc/src/lib.rs".to_string()])
            .0
            .is_empty()
    );
}

#[test]
fn npm_emits_depends_and_path_ref_edges_and_the_control_removes_the_path_ref() {
    let b = built(&repo(true, true));
    assert_eq!(
        edge(&b, "npm:apps/x/app", "npm:ui"),
        Some(EdgeKind::Depends)
    );
    assert_eq!(
        edge(&b, "npm:apps/y/src-tauri", "npm:ui"),
        Some(EdgeKind::PathRef)
    );
    let (hits, _) = b.model.reach(&["ui/src/a.ts".to_string()]);
    assert!(hits.contains_key("npm:apps/y/src-tauri"));
    // Positive control: without `frontendDist` the shell is no node and is not reached.
    let c = built(&repo(true, false));
    assert!(edge(&c, "npm:apps/y/src-tauri", "npm:ui").is_none());
    assert!(
        !c.model
            .reach(&["ui/src/a.ts".to_string()])
            .0
            .contains_key("npm:apps/y/src-tauri")
    );
}

#[test]
fn every_adapter_reads_through_one_bulk_call_and_declares_its_spawns() {
    let m = repo(true, true);
    let rec = Recording::new(&m);
    let ports = Ports {
        cargo: Some(&FakeCargo),
        root: "/r",
    };
    let adapters = extractors(&ports);
    let b = build(&m.paths(), &rec, &adapters);
    assert_eq!(
        rec.calls.borrow().len(),
        adapters.len(),
        "one bulk read per adapter: {:?}",
        rec.calls.borrow()
    );
    // Three Cargo.toml files claimed: the bound; one is a root and is asked.
    assert_eq!(b.spawns, 3);
}

#[test]
fn an_adapter_that_cannot_read_reports_the_file() {
    let m = MemoryRead::new()
        .with("apps/z/Cargo.toml", "[workspace]\n")
        .with("ui/package.json", "{ not json");
    let b = built(&m);
    let names: Vec<(&str, &str)> = b
        .errors
        .iter()
        .map(|e| (e.extractor, e.file.as_str()))
        .collect();
    assert_eq!(
        names,
        [("cargo", "apps/z/Cargo.toml"), ("npm", "ui/package.json")]
    );
}

#[test]
fn the_registry_leaves_cargo_out_without_its_port() {
    let ports = Ports {
        cargo: None,
        root: "/r",
    };
    let names: Vec<&str> = extractors(&ports).iter().map(|a| a.name()).collect();
    assert_eq!(names, ["npm"]);
}
