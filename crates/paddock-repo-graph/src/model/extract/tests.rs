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
            let targets = format!(
                r#"[{{"kind":["lib"],"name":"{n}","src_path":"/r/{dir}/src/lib.rs"}},{{"kind":["test"],"name":"it","src_path":"/r/{dir}/tests/it/main.rs"}},{{"kind":["custom-build"],"name":"build-script-build","src_path":"/r/{dir}/build.rs"}}]"#,
                n = name.replace('-', "_")
            );
            format!(
                r#"{{"name":"{name}","manifest_path":"/r/{dir}/Cargo.toml","dependencies":[{}],"targets":{targets}}}"#,
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
        rust_lex: None,
        module_crates: &[],
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
        rust_lex: None,
        module_crates: &[],
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
        rust_lex: None,
        module_crates: &[],
    };
    let names: Vec<&str> = extractors(&ports).iter().map(|a| a.name()).collect();
    assert_eq!(names, ["npm"]);
}

#[test]
fn cargo_emits_test_targets_their_root_files_and_helpers_with_tests_edges() {
    let m = repo(true, false)
        .with("crates/paddock-proc/tests/it/main.rs", "mod helper;")
        .with("crates/paddock-proc/tests/it/helper.rs", "")
        .with("crates/paddock-proc/src/lib.rs", "");
    let b = built(&m);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(
        b.model.node("nextest:paddock-proc::it").map(|n| n.kind),
        Some(NodeKind::TestTarget)
    );
    assert_eq!(
        b.model.node("nextest:paddock-proc").map(|n| n.kind),
        Some(NodeKind::TestTarget),
        "the library's unit tests"
    );
    assert!(
        b.model
            .node("nextest:paddock-proc::build-script-build")
            .is_none(),
        "a build script is no test target"
    );
    assert_eq!(
        edge(
            &b,
            "nextest:paddock-proc::it",
            "test:crates/paddock-proc/tests/it/main.rs"
        ),
        Some(EdgeKind::Tests)
    );
    assert_eq!(
        edge(
            &b,
            "nextest:paddock-proc::it",
            "test:crates/paddock-proc/tests/it/helper.rs"
        ),
        Some(EdgeKind::Tests)
    );
    assert!(
        b.model
            .node("test:crates/paddock-proc/src/lib.rs")
            .is_none(),
        "a source file is code, not a test file"
    );
    let (hits, _) = b
        .model
        .reach(&["crates/paddock-proc/tests/it/helper.rs".to_string()]);
    assert_eq!(hits["nextest:paddock-proc::it"].reach, Reach::Test);
    // Positive control: without the crates workspace no metadata, no target, and the helper is no test file.
    let c = built(&repo(false, false).with("crates/paddock-proc/tests/it/helper.rs", ""));
    assert!(
        c.model
            .node("test:crates/paddock-proc/tests/it/helper.rs")
            .is_none()
    );
}

#[test]
fn a_failed_read_is_an_extract_error_and_an_absent_file_is_not() {
    // Failing first (P1-5): a failed read was turned into "absent" and the adapter skipped the file.
    let m = MemoryRead::new()
        .with("crates/Cargo.toml", "[workspace]\n")
        .failing("ui/package.json", "I/O error");
    let t = texts_of(
        "x",
        &m,
        &["crates/Cargo.toml".to_string(), "no/such".to_string()],
    )
    .unwrap();
    assert_eq!(
        t.keys().collect::<Vec<_>>(),
        ["crates/Cargo.toml"],
        "absent is left out, no error"
    );
    let e = texts_of("x", &m, &["ui/package.json".to_string()]).unwrap_err();
    assert_eq!(
        (e.extractor, e.file.as_str(), e.reason.as_str()),
        ("x", "ui/package.json", "I/O error")
    );
    // The adapters report it (control: the same file readable gives no error).
    let b = built(&m);
    assert!(
        b.errors.iter().any(|e| e.extractor == "npm"
            && e.file == "ui/package.json"
            && e.reason == "I/O error"),
        "{:?}",
        b.errors
    );
    assert!(
        built(&MemoryRead::new().with("ui/package.json", r#"{"name":"@d/ui"}"#))
            .errors
            .is_empty()
    );
}

#[test]
fn the_cargo_adapter_counts_its_metadata_calls_and_stays_within_its_budget() {
    // extract.rs's `Counting` was written and never read; the count is now asserted against the declared budget.
    let m = repo(true, false);
    let cargo = CargoExtractor::new(&FakeCargo, "/r");
    let files = m.paths();
    let mine: Vec<String> = files
        .iter()
        .filter(|f| cargo.claims().iter().any(|c| f.ends_with(c)))
        .cloned()
        .collect();
    cargo.extract(&mine, &m).unwrap();
    assert_eq!(
        cargo.metadata_calls(),
        1,
        "one workspace root is asked, not its members"
    );
    assert!(cargo.metadata_calls() <= cargo.spawn_budget(&mine));
    // Control: a repository with no workspace asks nothing, so the counter is reset per run, not accumulated.
    cargo.extract(&[], &MemoryRead::new()).unwrap();
    assert_eq!(cargo.metadata_calls(), 0);
}
