//! Each reader of a register under docs/ refuses an unknown key (ER-42; the typed structs of `register_schema` behind them): the real
//! file is read as it is (the positive control: the reader accepts it) and again with one key no table has.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::repo_root;
use paddock_dev::{escape, gates, generators, ipc_endpoints, locks_guard, repo_inputs, run_scope};

fn text(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap()
}

fn with_unknown(rel: &str) -> String {
    format!("zz_unknown_key = 1\n{}", text(rel))
}

/// The real file with one key no table has, right after the first `[[table]]` header: inside a table, not at the top.
fn with_unknown_in(rel: &str, table: &str) -> String {
    let original = text(rel);
    let header = format!("[[{table}]]\n");
    let at = original.find(&header).unwrap_or_else(|| panic!("{rel} has a [[{table}]]")) + header.len();
    format!("{}zz_unknown_key = 1\n{}", &original[..at], &original[at..])
}

#[test]
fn the_escape_register_reader() {
    assert!(escape::register::parse(&text("docs/escape-register.toml")).is_ok());
    let e = escape::register::parse(&with_unknown("docs/escape-register.toml")).unwrap_err();
    assert!(e.contains("zz_unknown_key"), "{e}");
    let e = escape::register::parse(&with_unknown_in("docs/escape-register.toml", "table")).unwrap_err();
    assert!(e.contains("zz_unknown_key"), "inside [[table]]: {e}");
}

#[test]
fn the_gate_register_reader() {
    let (_, ok) = gates::register::parse(&text("docs/gates.toml"));
    let ok: Vec<_> = ok.iter().filter(|e| e.contains("unknown field")).collect();
    assert!(ok.is_empty(), "{ok:?}");
    let (_, errs) = gates::register::parse(&with_unknown("docs/gates.toml"));
    assert!(errs.iter().any(|e| e.contains("zz_unknown_key")), "{errs:?}");
    let (_, errs) = gates::register::parse(&with_unknown_in("docs/gates.toml", "report_only"));
    assert!(
        errs.iter().any(|e| e.contains("zz_unknown_key")),
        "inside [[report_only]]: {errs:?}"
    );
}

#[test]
fn the_generator_registry_reader() {
    assert!(generators::registry::Registry::parse(&text("docs/generators.toml")).is_ok());
    let e = generators::registry::Registry::parse(&with_unknown("docs/generators.toml"))
        .unwrap_err()
        .join("\n");
    assert!(e.contains("zz_unknown_key"), "{e}");
    let e = generators::registry::Registry::parse(&with_unknown_in("docs/generators.toml", "generator"))
        .unwrap_err()
        .join("\n");
    assert!(e.contains("zz_unknown_key"), "inside [[generator]]: {e}");
}

#[test]
fn the_ipc_endpoint_readers() {
    assert!(ipc_endpoints::parse_register(&text("docs/ipc-endpoints.toml")).is_ok());
    assert!(ipc_endpoints::parse_ratchet(&text("docs/ipc-endpoint-ratchet.toml")).is_ok());
    assert!(
        ipc_endpoints::parse_register(&with_unknown("docs/ipc-endpoints.toml"))
            .unwrap_err()
            .contains("zz_unknown_key")
    );
    assert!(
        ipc_endpoints::parse_ratchet(&with_unknown("docs/ipc-endpoint-ratchet.toml"))
            .unwrap_err()
            .contains("zz_unknown_key")
    );
    assert!(
        ipc_endpoints::parse_register(&with_unknown_in("docs/ipc-endpoints.toml", "endpoint"))
            .unwrap_err()
            .contains("zz_unknown_key")
    );
    assert!(
        ipc_endpoints::parse_ratchet(&with_unknown_in("docs/ipc-endpoint-ratchet.toml", "ratchet"))
            .unwrap_err()
            .contains("zz_unknown_key")
    );
}

#[test]
fn the_pin_reader() {
    assert!(locks_guard::parse_pins(&text("docs/shared-crate-features.toml")).is_ok());
    assert!(
        locks_guard::parse_pins(&with_unknown("docs/shared-crate-features.toml"))
            .unwrap_err()
            .contains("zz_unknown_key")
    );
}

#[test]
fn the_repository_graph_reader() {
    assert!(repo_inputs::planner(&text("docs/repo-graph.toml")).is_ok());
    assert!(
        repo_inputs::planner(&with_unknown("docs/repo-graph.toml"))
            .unwrap_err()
            .contains("zz_unknown_key")
    );
    assert!(
        repo_inputs::planner(&with_unknown_in("docs/repo-graph.toml", "input"))
            .unwrap_err()
            .contains("zz_unknown_key")
    );
}

#[test]
fn the_scope_register_reader() {
    // The register holds no producer yet: the answer is `None`, not an error; an unknown key is the error.
    assert!(run_scope::recorded(&text("docs/ci-scope-first-runs.toml"), "ci.yml").is_ok());
    assert!(
        run_scope::recorded(&with_unknown("docs/ci-scope-first-runs.toml"), "ci.yml")
            .unwrap_err()
            .contains("zz_unknown_key")
    );
}

#[test]
fn the_maps_refuse_a_nested_value_of_the_wrong_shape() {
    // The pins' `[features.<crate>]` and the scope register's `[first_green]` are maps whose keys are data; their shape is the check.
    let pins = locks_guard::parse_pins("watch = []\n[features.x]\n\"apps/a\" = \"not a list\"\n").unwrap_err();
    assert!(pins.contains("docs/shared-crate-features.toml"), "{pins}");
    let scope = run_scope::recorded("[first_green]\n\"ci.yml\" = \"nine\"\n", "ci.yml").unwrap_err();
    assert!(scope.contains("docs/ci-scope-first-runs.toml"), "{scope}");
}
